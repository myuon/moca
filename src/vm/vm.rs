use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::vm::threads::{Channel, ThreadSpawner};
use crate::vm::{Chunk, Function, GcRef, Heap, Op, Value};

#[cfg(all(target_arch = "aarch64", feature = "jit"))]
use crate::jit::compiler::{CompiledCode, CompiledLoop, JitCompiler};
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
use crate::jit::compiler_x86_64::{CompiledCode, CompiledLoop, JitCompiler};
#[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
use crate::jit::marshal::{JitCallContext, JitContext, JitReturn, JitValue};

/// A call frame for the VM.
#[derive(Debug)]
struct Frame {
    /// Index into the function table (usize::MAX for main)
    func_index: usize,
    /// Program counter
    pc: usize,
    /// Base index into the stack for locals
    stack_base: usize,
}

/// Exception handler frame.
#[derive(Debug)]
struct TryFrame {
    /// Stack depth when try block started
    stack_depth: usize,
    /// Call frame depth when try block started
    frame_depth: usize,
    /// PC to jump to for catch handler
    handler_pc: usize,
    /// Function index when try started
    func_index: usize,
}

/// GC statistics.
#[derive(Debug, Clone, Default)]
pub struct VmGcStats {
    pub cycles: usize,
    pub total_pause_us: u64,
    pub max_pause_us: u64,
}

/// Opcode execution profile data.
#[derive(Debug, Clone, Default)]
pub struct OpcodeProfile {
    /// Execution counts per opcode name
    pub counts: HashMap<&'static str, u64>,
}

impl OpcodeProfile {
    /// Get total number of executed instructions.
    pub fn total_instructions(&self) -> u64 {
        self.counts.values().sum()
    }

    /// Get sorted entries by count (descending).
    pub fn sorted_by_count(&self) -> Vec<(&'static str, u64)> {
        let mut entries: Vec<_> = self.counts.iter().map(|(&k, &v)| (k, v)).collect();
        entries.sort_by(|a, b| b.1.cmp(&a.1));
        entries
    }
}

/// The moca virtual machine.
pub struct VM {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    heap: Heap,
    try_frames: Vec<TryFrame>,
    /// Function call counters for JIT (index matches Chunk::functions)
    call_counts: Vec<u32>,
    /// Whether JIT compilation is enabled
    jit_enabled: bool,
    /// JIT threshold
    jit_threshold: u32,
    /// Whether to trace JIT events
    trace_jit: bool,
    /// GC statistics
    gc_stats: VmGcStats,
    /// Thread spawner for managing spawned threads
    thread_spawner: ThreadSpawner,
    /// Channels for inter-thread communication (id -> channel)
    channels: Vec<Arc<Channel<Value>>>,
    /// JIT compiled functions (only on AArch64 with jit feature)
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    jit_functions: HashMap<usize, CompiledCode>,
    /// JIT compiled functions (only on x86-64 with jit feature)
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    jit_functions: HashMap<usize, CompiledCode>,
    /// Number of JIT compilations performed
    jit_compile_count: usize,
    /// Output stream for print statements (stdout)
    output: Box<dyn Write>,
    /// Output stream for stderr
    stderr: Box<dyn Write>,
    /// File descriptor table for open files (fd >= 3)
    file_descriptors: HashMap<i64, File>,
    /// Socket descriptor table for TCP connections (fd >= 3)
    socket_descriptors: HashMap<i64, TcpStream>,
    /// Pending socket fds (created by socket() but not yet connected)
    pending_sockets: HashSet<i64>,
    /// Listener descriptor table for TCP servers (fd >= 3)
    listener_descriptors: HashMap<i64, TcpListener>,
    /// Next available file descriptor
    next_fd: i64,
    /// Command-line arguments passed to the script
    cli_args: Vec<String>,
    /// Whether opcode profiling is enabled
    profile_opcodes: bool,
    /// Opcode execution counts for profiling
    opcode_profile: OpcodeProfile,
    /// String constant cache: maps string index to heap reference
    /// Once a string constant is allocated, it's cached here for reuse.
    string_cache: Vec<Option<GcRef>>,
    /// Loop iteration counters for hot loop detection.
    /// Key: (function_index, backward_jump_pc), Value: iteration count
    loop_counts: HashMap<(usize, usize), u32>,
    /// JIT compiled loops (only on AArch64 with jit feature)
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    jit_loops: HashMap<(usize, usize), CompiledLoop>,
    /// JIT compiled loops (only on x86-64 with jit feature)
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    jit_loops: HashMap<(usize, usize), CompiledLoop>,
}

impl VM {
    pub fn new() -> Self {
        Self::new_with_config(None, true, Box::new(io::stdout()), Box::new(io::stderr()))
    }

    /// Create a VM with a custom output stream.
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self::new_with_config(None, true, output, Box::new(io::stderr()))
    }

    /// Create a new VM with custom heap configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    pub fn new_with_heap_config(heap_limit: Option<usize>, gc_enabled: bool) -> Self {
        Self::new_with_config(
            heap_limit,
            gc_enabled,
            Box::new(io::stdout()),
            Box::new(io::stderr()),
        )
    }

    /// Create a new VM with full configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    /// * `output` - Output stream for print statements (stdout)
    /// * `stderr` - Output stream for stderr
    pub fn new_with_config(
        heap_limit: Option<usize>,
        gc_enabled: bool,
        output: Box<dyn Write>,
        stderr: Box<dyn Write>,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
            heap: Heap::new_with_config(heap_limit, gc_enabled),
            try_frames: Vec::new(),
            call_counts: Vec::new(),
            jit_enabled: true,
            jit_threshold: 1000,
            trace_jit: false,
            gc_stats: VmGcStats::default(),
            thread_spawner: ThreadSpawner::new(),
            channels: Vec::new(),
            #[cfg(all(target_arch = "aarch64", feature = "jit"))]
            jit_functions: HashMap::new(),
            #[cfg(all(target_arch = "x86_64", feature = "jit"))]
            jit_functions: HashMap::new(),
            jit_compile_count: 0,
            output,
            stderr,
            file_descriptors: HashMap::new(),
            socket_descriptors: HashMap::new(),
            pending_sockets: HashSet::new(),
            listener_descriptors: HashMap::new(),
            next_fd: 3, // fd 0, 1, 2 are reserved for stdin, stdout, stderr
            cli_args: Vec::new(),
            profile_opcodes: false,
            opcode_profile: OpcodeProfile::default(),
            string_cache: Vec::new(),
            loop_counts: HashMap::new(),
            #[cfg(all(target_arch = "aarch64", feature = "jit"))]
            jit_loops: HashMap::new(),
            #[cfg(all(target_arch = "x86_64", feature = "jit"))]
            jit_loops: HashMap::new(),
        }
    }

    /// Initialize string constant cache for a chunk.
    fn init_string_cache(&mut self, chunk: &Chunk) {
        self.string_cache = vec![None; chunk.strings.len()];
    }

    /// Get or allocate a string constant.
    /// Returns the cached reference if available, otherwise allocates and caches.
    fn get_or_alloc_string(&mut self, idx: usize, chunk: &Chunk) -> Result<GcRef, String> {
        // Check cache first
        if let Some(Some(r)) = self.string_cache.get(idx) {
            return Ok(*r);
        }

        // Allocate and cache
        let s = chunk.strings.get(idx).cloned().unwrap_or_default();
        let r = self.heap.alloc_string(s)?;

        // Store in cache
        if idx < self.string_cache.len() {
            self.string_cache[idx] = Some(r);
        }

        Ok(r)
    }

    /// Get the string cache base pointer for JIT access.
    /// Returns pointer to the first element of the cache Vec.
    pub fn string_cache_ptr(&self) -> *const Option<GcRef> {
        self.string_cache.as_ptr()
    }

    /// Configure JIT settings.
    pub fn set_jit_config(&mut self, enabled: bool, threshold: u32, trace: bool) {
        self.jit_enabled = enabled;
        self.jit_threshold = threshold;
        self.trace_jit = trace;
    }

    /// Enable or disable opcode profiling.
    pub fn set_profile_opcodes(&mut self, enabled: bool) {
        self.profile_opcodes = enabled;
    }

    /// Get opcode execution profile.
    pub fn opcode_profile(&self) -> &OpcodeProfile {
        &self.opcode_profile
    }

    /// Record an opcode execution for profiling.
    /// This is public so JIT helpers can also record their operations.
    #[inline]
    pub fn record_opcode(&mut self, name: &'static str) {
        if self.profile_opcodes {
            *self.opcode_profile.counts.entry(name).or_insert(0) += 1;
        }
    }

    /// Set command-line arguments for the script.
    pub fn set_cli_args(&mut self, args: Vec<String>) {
        self.cli_args = args;
    }

    /// Get the number of command-line arguments.
    pub fn cli_argc(&self) -> usize {
        self.cli_args.len()
    }

    /// Get a command-line argument by index.
    pub fn cli_argv(&self, index: usize) -> &str {
        self.cli_args.get(index).map(|s| s.as_str()).unwrap_or("")
    }

    /// Get all command-line arguments.
    pub fn cli_args(&self) -> &[String] {
        &self.cli_args
    }

    /// Get GC statistics.
    pub fn gc_stats(&self) -> &VmGcStats {
        &self.gc_stats
    }

    /// Get immutable reference to the heap.
    pub fn heap(&self) -> &Heap {
        &self.heap
    }

    /// Get mutable reference to the heap.
    pub fn heap_mut(&mut self) -> &mut Heap {
        &mut self.heap
    }

    /// Initialize call counts for a chunk.
    fn init_call_counts(&mut self, chunk: &Chunk) {
        self.call_counts = vec![0; chunk.functions.len()];
    }

    /// Increment call count and check if function should be JIT compiled.
    fn should_jit_compile(&mut self, func_index: usize, func_name: &str) -> bool {
        if !self.jit_enabled {
            return false;
        }

        if func_index >= self.call_counts.len() {
            return false;
        }

        self.call_counts[func_index] += 1;

        if self.call_counts[func_index] == self.jit_threshold {
            if self.trace_jit {
                eprintln!(
                    "[JIT] Hot function detected: {} (calls: {})",
                    func_name, self.jit_threshold
                );
            }
            return true;
        }

        false
    }

    /// Check if a loop should be JIT compiled based on iteration count.
    /// Returns true when the loop reaches the hot threshold and JIT is enabled.
    fn should_jit_compile_loop(&self, func_index: usize, back_jump_pc: usize) -> bool {
        if !self.jit_enabled {
            return false;
        }
        let key = (func_index, back_jump_pc);
        if let Some(&count) = self.loop_counts.get(&key) {
            count == self.jit_threshold
        } else {
            false
        }
    }

    /// Check if a loop has already been JIT compiled.
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn is_loop_jit_compiled(&self, func_index: usize, back_jump_pc: usize) -> bool {
        self.jit_enabled && self.jit_loops.contains_key(&(func_index, back_jump_pc))
    }

    /// Check if a loop has already been JIT compiled.
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn is_loop_jit_compiled(&self, func_index: usize, back_jump_pc: usize) -> bool {
        self.jit_enabled && self.jit_loops.contains_key(&(func_index, back_jump_pc))
    }

    /// Compile a function to native code (AArch64 with jit feature only).
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn jit_compile_function(&mut self, func: &Function, func_index: usize) {
        if self.jit_functions.contains_key(&func_index) {
            return; // Already compiled
        }

        let compiler = JitCompiler::new();
        match compiler.compile(func, func_index) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Compiled function '{}' ({} bytes)",
                        func.name,
                        compiled.memory.size()
                    );
                }
                self.jit_functions.insert(func_index, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!("[JIT] Failed to compile '{}': {}", func.name, e);
                }
            }
        }
    }

    /// Check if a function has been JIT compiled (AArch64 with jit feature only).
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn is_jit_compiled(&self, func_index: usize) -> bool {
        self.jit_functions.contains_key(&func_index)
    }

    /// Compile a function to native code (x86-64 with jit feature only).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn jit_compile_function(&mut self, func: &Function, func_index: usize) {
        if self.jit_functions.contains_key(&func_index) {
            return; // Already compiled
        }

        let compiler = JitCompiler::new();
        match compiler.compile(func, func_index) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Compiled function '{}' ({} bytes)",
                        func.name,
                        compiled.memory.size()
                    );
                }
                self.jit_functions.insert(func_index, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!("[JIT] Failed to compile '{}': {}", func.name, e);
                }
            }
        }
    }

    /// Check if a function has been JIT compiled (x86-64 with jit feature only).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn is_jit_compiled(&self, func_index: usize) -> bool {
        self.jit_functions.contains_key(&func_index)
    }

    /// Compile a hot loop to native code (x86-64 with jit feature only).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn jit_compile_loop(
        &mut self,
        func: &Function,
        func_index: usize,
        loop_start_pc: usize,
        loop_end_pc: usize,
    ) {
        let key = (func_index, loop_end_pc);
        if self.jit_loops.contains_key(&key) {
            return; // Already compiled
        }

        if self.trace_jit {
            eprintln!(
                "[JIT] Hot loop detected in '{}' at PC {}..{} (iterations: {})",
                func.name, loop_start_pc, loop_end_pc, self.jit_threshold
            );
        }

        let compiler = JitCompiler::new();
        let jit_compiled_funcs: std::collections::HashSet<usize> =
            self.jit_functions.keys().copied().collect();
        match compiler.compile_loop(func, loop_start_pc, loop_end_pc, &jit_compiled_funcs) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Compiled loop in '{}' PC {}..{} ({} bytes)",
                        func.name,
                        loop_start_pc,
                        loop_end_pc,
                        compiled.memory.size()
                    );
                }
                self.jit_loops.insert(key, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Failed to compile loop in '{}' PC {}..{}: {}",
                        func.name, loop_start_pc, loop_end_pc, e
                    );
                }
            }
        }
    }

    /// Compile a hot loop to native code (AArch64 with jit feature only).
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn jit_compile_loop(
        &mut self,
        func: &Function,
        func_index: usize,
        loop_start_pc: usize,
        loop_end_pc: usize,
    ) {
        let key = (func_index, loop_end_pc);
        if self.jit_loops.contains_key(&key) {
            return; // Already compiled
        }

        if self.trace_jit {
            eprintln!(
                "[JIT] Hot loop detected in '{}' at PC {}..{} (iterations: {})",
                func.name, loop_start_pc, loop_end_pc, self.jit_threshold
            );
        }

        let compiler = JitCompiler::new();
        let jit_compiled_funcs: std::collections::HashSet<usize> =
            self.jit_functions.keys().copied().collect();
        match compiler.compile_loop(func, loop_start_pc, loop_end_pc, &jit_compiled_funcs) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Compiled loop in '{}' PC {}..{} ({} bytes)",
                        func.name,
                        loop_start_pc,
                        loop_end_pc,
                        compiled.memory.size()
                    );
                }
                self.jit_loops.insert(key, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT] Failed to compile loop in '{}' PC {}..{}: {}",
                        func.name, loop_start_pc, loop_end_pc, e
                    );
                }
            }
        }
    }

    /// Execute a JIT compiled loop (x86-64 with jit feature only).
    ///
    /// Returns the PC to continue from after the loop (loop_end_pc + 1).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn execute_jit_loop(
        &mut self,
        func_index: usize,
        loop_end_pc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<usize, String> {
        let key = (func_index, loop_end_pc);

        // Get the entry point first to avoid borrow conflicts
        let (entry, loop_end): (
            unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn,
            usize,
        ) = {
            let compiled = self.jit_loops.get(&key).unwrap();
            (unsafe { compiled.entry_point() }, compiled.loop_end_pc)
        };

        // Create JIT context with locals
        let locals_count = func.locals_count;
        let mut ctx = JitContext::new(locals_count);

        // Copy current locals from VM to JIT context
        let frame = self.frames.last().unwrap();
        let stack_base = frame.stack_base;
        for i in 0..locals_count {
            if stack_base + i < self.stack.len() {
                ctx.set_local(i, JitValue::from_value(&self.stack[stack_base + i]));
            }
        }

        // Set up JitCallContext for runtime calls from JIT code
        let mut call_ctx = JitCallContext {
            vm: self as *mut VM as *mut u8,
            chunk: chunk as *const Chunk as *const u8,
            call_helper: jit_call_helper,
            push_string_helper: jit_push_string_helper,
            array_len_helper: jit_array_len_helper,
            syscall_helper: jit_syscall_helper,
            heap_base: self.heap.memory_base_ptr(),
            string_cache: self.string_cache.as_ptr() as *const u64,
            string_cache_len: self.string_cache.len() as u64,
        };

        // Execute the JIT loop code
        let _result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                ctx.stack,
                ctx.locals,
            )
        };

        if self.trace_jit {
            eprintln!("[JIT] Executed loop in '{}' PC ..{}", func.name, loop_end);
        }

        // Copy locals back from JIT context to VM
        let frame = self.frames.last().unwrap();
        let stack_base = frame.stack_base;
        for i in 0..locals_count {
            if stack_base + i < self.stack.len() {
                let jit_val = ctx.get_local(i);
                self.stack[stack_base + i] = jit_val.to_value();
            }
        }

        // Return the PC to continue from (after the loop)
        Ok(loop_end + 1)
    }

    /// Execute a JIT compiled loop (AArch64 with jit feature only).
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn execute_jit_loop(
        &mut self,
        func_index: usize,
        loop_end_pc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<usize, String> {
        let key = (func_index, loop_end_pc);

        // Get the entry point first to avoid borrow conflicts
        let (entry, loop_end): (
            unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn,
            usize,
        ) = {
            let compiled = self.jit_loops.get(&key).unwrap();
            (unsafe { compiled.entry_point() }, compiled.loop_end_pc)
        };

        // Create JIT context with locals
        let locals_count = func.locals_count;
        let mut ctx = JitContext::new(locals_count);

        // Copy current locals from VM to JIT context
        let frame = self.frames.last().unwrap();
        let stack_base = frame.stack_base;
        for i in 0..locals_count {
            if stack_base + i < self.stack.len() {
                ctx.set_local(i, JitValue::from_value(&self.stack[stack_base + i]));
            }
        }

        // Set up JitCallContext for runtime calls from JIT code
        let mut call_ctx = JitCallContext {
            vm: self as *mut VM as *mut u8,
            chunk: chunk as *const Chunk as *const u8,
            call_helper: jit_call_helper,
            push_string_helper: jit_push_string_helper,
            array_len_helper: jit_array_len_helper,
            syscall_helper: jit_syscall_helper,
            heap_base: self.heap.memory_base_ptr(),
            string_cache: self.string_cache.as_ptr() as *const u64,
            string_cache_len: self.string_cache.len() as u64,
        };

        // Execute the JIT loop code
        let _result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                ctx.stack,
                ctx.locals,
            )
        };

        if self.trace_jit {
            eprintln!("[JIT] Executed loop in '{}' PC ..{}", func.name, loop_end);
        }

        // Copy locals back from JIT context to VM
        let frame = self.frames.last().unwrap();
        let stack_base = frame.stack_base;
        for i in 0..locals_count {
            if stack_base + i < self.stack.len() {
                let jit_val = ctx.get_local(i);
                self.stack[stack_base + i] = jit_val.to_value();
            }
        }

        // Return the PC to continue from (after the loop)
        Ok(loop_end + 1)
    }

    /// Execute a JIT compiled function (x86-64 with jit feature only).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn execute_jit_function(
        &mut self,
        func_index: usize,
        argc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<Value, String> {
        // Get the entry point first to avoid borrow conflicts
        let entry: unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn = {
            let compiled = self.jit_functions.get(&func_index).unwrap();
            unsafe { compiled.entry_point() }
        };

        // Create JIT context with locals
        let locals_count = func.locals_count;
        let mut ctx = JitContext::new(locals_count);

        // Pop arguments from VM stack and push to JIT stack (in reverse order)
        let args: Vec<Value> = (0..argc)
            .map(|_| self.stack.pop().unwrap())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Set up arguments as locals (arguments are the first locals)
        for (i, arg) in args.iter().enumerate() {
            ctx.set_local(i, JitValue::from_value(arg));
        }

        // Set up JitCallContext for runtime calls from JIT code
        let mut call_ctx = JitCallContext {
            vm: self as *mut VM as *mut u8,
            chunk: chunk as *const Chunk as *const u8,
            call_helper: jit_call_helper,
            push_string_helper: jit_push_string_helper,
            array_len_helper: jit_array_len_helper,
            syscall_helper: jit_syscall_helper,
            heap_base: self.heap.memory_base_ptr(),
            string_cache: self.string_cache.as_ptr() as *const u64,
            string_cache_len: self.string_cache.len() as u64,
        };

        // Execute the JIT code
        // Pass call context, stack and locals pointers
        let result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                ctx.stack,
                ctx.locals,
            )
        };

        if self.trace_jit {
            eprintln!(
                "[JIT] Executed function '{}', result: tag={}, payload={}",
                func.name, result.tag, result.payload
            );
        }

        // Convert return value to VM Value
        Ok(result.to_value())
    }

    /// Execute a JIT compiled function (AArch64 with jit feature only).
    ///
    /// AArch64 ABI: Arguments passed in x0-x2, return value in x0/x1.
    /// Function signature: fn(vm_ctx: *mut u8, stack: *mut JitValue, locals: *mut JitValue) -> JitReturn
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn execute_jit_function(
        &mut self,
        func_index: usize,
        argc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<Value, String> {
        // Get the entry point first to avoid borrow conflicts
        let entry: unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn = {
            let compiled = self.jit_functions.get(&func_index).unwrap();
            unsafe { compiled.entry_point() }
        };

        // Create JIT context with locals
        let locals_count = func.locals_count;
        let mut ctx = JitContext::new(locals_count);

        // Pop arguments from VM stack and push to JIT stack (in reverse order)
        let args: Vec<Value> = (0..argc)
            .map(|_| self.stack.pop().unwrap())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        // Set up arguments as locals (arguments are the first locals)
        for (i, arg) in args.iter().enumerate() {
            ctx.set_local(i, JitValue::from_value(arg));
        }

        // Set up JitCallContext for runtime calls from JIT code
        let mut call_ctx = JitCallContext {
            vm: self as *mut VM as *mut u8,
            chunk: chunk as *const Chunk as *const u8,
            call_helper: jit_call_helper,
            push_string_helper: jit_push_string_helper,
            array_len_helper: jit_array_len_helper,
            syscall_helper: jit_syscall_helper,
            heap_base: self.heap.memory_base_ptr(),
            string_cache: self.string_cache.as_ptr() as *const u64,
            string_cache_len: self.string_cache.len() as u64,
        };

        // Execute the JIT code
        // Pass call context, stack and locals pointers
        let result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                ctx.stack,
                ctx.locals,
            )
        };

        if self.trace_jit {
            eprintln!(
                "[JIT] Executed function '{}', result: tag={}, payload={}",
                func.name, result.tag, result.payload
            );
        }

        // Convert return value to VM Value
        Ok(result.to_value())
    }

    /// Get the number of JIT compilations performed.
    pub fn jit_compile_count(&self) -> usize {
        self.jit_compile_count
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<(), String> {
        // Initialize call counts for JIT
        self.init_call_counts(chunk);
        // Initialize string constant cache
        self.init_string_cache(chunk);

        // Start with main
        self.frames.push(Frame {
            func_index: usize::MAX, // Marker for main
            pc: 0,
            stack_base: 0,
        });

        loop {
            // Check if GC should run
            if self.heap.should_gc() {
                self.collect_garbage();
            }

            let frame = self.frames.last_mut().unwrap();
            let func = if frame.func_index == usize::MAX {
                &chunk.main
            } else {
                &chunk.functions[frame.func_index]
            };

            if frame.pc >= func.code.len() {
                // End of function without explicit return
                break;
            }

            let op = func.code[frame.pc].clone();
            frame.pc += 1;

            // Profile opcode execution if enabled
            if self.profile_opcodes {
                *self.opcode_profile.counts.entry(op.name()).or_insert(0) += 1;
            }

            let result = self.execute_op(op, chunk);
            match result {
                Ok(ControlFlow::Continue) => {}
                Ok(ControlFlow::Return) => {
                    if self.frames.is_empty() {
                        break;
                    }
                }
                Ok(ControlFlow::Exit) => break,
                Err(e) => {
                    // Try to handle exception
                    if !self.handle_exception(e.clone(), chunk)? {
                        return Err(e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Run a chunk and return the result value (used for thread execution).
    pub fn run_and_get_result(&mut self, chunk: &Chunk) -> Result<Value, String> {
        // Start with main
        self.frames.push(Frame {
            func_index: usize::MAX, // Marker for main
            pc: 0,
            stack_base: 0,
        });

        let mut result = Value::Null;

        loop {
            // Check if GC should run
            if self.heap.should_gc() {
                self.collect_garbage();
            }

            let frame = self.frames.last_mut().unwrap();
            let func = if frame.func_index == usize::MAX {
                &chunk.main
            } else {
                &chunk.functions[frame.func_index]
            };

            if frame.pc >= func.code.len() {
                // End of function without explicit return
                break;
            }

            let op = func.code[frame.pc].clone();
            frame.pc += 1;

            // Profile opcode execution if enabled
            if self.profile_opcodes {
                *self.opcode_profile.counts.entry(op.name()).or_insert(0) += 1;
            }

            let control = self.execute_op(op, chunk);
            match control {
                Ok(ControlFlow::Continue) => {}
                Ok(ControlFlow::Return) => {
                    if self.frames.is_empty() {
                        // Main returned - capture the return value from stack
                        result = self.stack.pop().unwrap_or(Value::Null);
                        break;
                    }
                }
                Ok(ControlFlow::Exit) => {
                    // Capture the return value before exiting
                    result = self.stack.pop().unwrap_or(Value::Null);
                    break;
                }
                Err(e) => {
                    // Try to handle exception
                    if !self.handle_exception(e.clone(), chunk)? {
                        return Err(e);
                    }
                }
            }
        }

        Ok(result)
    }

    fn execute_op(&mut self, op: Op, chunk: &Chunk) -> Result<ControlFlow, String> {
        match op {
            // ========================================
            // Constants
            // ========================================
            Op::I32Const(n) => {
                // For booleans: 0 = false, 1 = true
                // For other values: treat as i64
                match n {
                    0 => self.stack.push(Value::Bool(false)),
                    1 => self.stack.push(Value::Bool(true)),
                    _ => self.stack.push(Value::I64(n as i64)),
                }
            }
            Op::I64Const(n) => {
                self.stack.push(Value::I64(n));
            }
            Op::F32Const(f) => {
                self.stack.push(Value::F64(f as f64));
            }
            Op::F64Const(f) => {
                self.stack.push(Value::F64(f));
            }
            Op::RefNull => {
                self.stack.push(Value::Null);
            }
            Op::StringConst(idx) => {
                let r = self.get_or_alloc_string(idx, chunk)?;
                self.stack.push(Value::Ref(r));
            }

            // ========================================
            // Local Variables
            // ========================================
            Op::LocalGet(slot) => {
                let frame = self.frames.last().unwrap();
                let index = frame.stack_base + slot;
                let value = self.stack.get(index).copied().unwrap_or(Value::Null);
                self.stack.push(value);
            }
            Op::LocalSet(slot) => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let frame = self.frames.last().unwrap();
                let index = frame.stack_base + slot;

                // Ensure stack is large enough
                while self.stack.len() <= index {
                    self.stack.push(Value::Null);
                }

                // Write barrier: capture old value before overwriting
                let old_value = self.stack[index];
                self.write_barrier(old_value);

                self.stack[index] = value;
            }

            // ========================================
            // Stack Manipulation
            // ========================================
            Op::Drop => {
                self.stack.pop();
            }
            Op::Dup => {
                let value = self.stack.last().copied().ok_or("stack underflow")?;
                self.stack.push(value);
            }
            Op::Pick(n) => {
                let len = self.stack.len();
                if n >= len {
                    return Err("stack underflow".to_string());
                }
                let value = self.stack[len - 1 - n];
                self.stack.push(value);
            }
            Op::PickDyn => {
                let depth_val = self.stack.pop().ok_or("stack underflow")?;
                let depth = depth_val
                    .as_i64()
                    .ok_or("runtime error: PickDyn requires integer depth")?
                    as usize;
                let len = self.stack.len();
                if depth >= len {
                    return Err("stack underflow".to_string());
                }
                let value = self.stack[len - 1 - depth];
                self.stack.push(value);
            }

            // ========================================
            // i32 Arithmetic
            // ========================================
            Op::I32Add => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::I64((a.wrapping_add(b)) as i64));
            }
            Op::I32Sub => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::I64((a.wrapping_sub(b)) as i64));
            }
            Op::I32Mul => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::I64((a.wrapping_mul(b)) as i64));
            }
            Op::I32DivS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::I64((a / b) as i64));
            }
            Op::I32RemS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::I64((a % b) as i64));
            }
            Op::I32Eqz => {
                let a = self.stack.pop().ok_or("stack underflow")?;
                self.stack.push(Value::Bool(!a.is_truthy()));
            }

            // ========================================
            // i64 Arithmetic
            // ========================================
            Op::I64Add => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.add(a, b)?;
                self.stack.push(result);
            }
            Op::I64Sub => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.sub(a, b)?;
                self.stack.push(result);
            }
            Op::I64Mul => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.mul(a, b)?;
                self.stack.push(result);
            }
            Op::I64DivS => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.div(a, b)?;
                self.stack.push(result);
            }
            Op::I64RemS => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::I64(a % b));
            }
            Op::I64Neg => {
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = match a {
                    Value::I64(n) => Value::I64(-n),
                    Value::F64(f) => Value::F64(-f),
                    _ => return Err("runtime error: cannot negate non-numeric value".to_string()),
                };
                self.stack.push(result);
            }

            // ========================================
            // f32 Arithmetic
            // ========================================
            Op::F32Add => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::F64((a + b) as f64));
            }
            Op::F32Sub => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::F64((a - b) as f64));
            }
            Op::F32Mul => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::F64((a * b) as f64));
            }
            Op::F32Div => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                if b == 0.0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::F64((a / b) as f64));
            }
            Op::F32Neg => {
                let a = self.pop_float()? as f32;
                self.stack.push(Value::F64((-a) as f64));
            }

            // ========================================
            // f64 Arithmetic
            // ========================================
            Op::F64Add => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::F64(a + b));
            }
            Op::F64Sub => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::F64(a - b));
            }
            Op::F64Mul => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::F64(a * b));
            }
            Op::F64Div => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                if b == 0.0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::F64(a / b));
            }
            Op::F64Neg => {
                let a = self.pop_float()?;
                self.stack.push(Value::F64(-a));
            }

            // ========================================
            // i32 Comparison
            // ========================================
            Op::I32Eq => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a == b));
            }
            Op::I32Ne => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a != b));
            }
            Op::I32LtS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a < b));
            }
            Op::I32LeS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a <= b));
            }
            Op::I32GtS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a > b));
            }
            Op::I32GeS => {
                let b = self.pop_int()? as i32;
                let a = self.pop_int()? as i32;
                self.stack.push(Value::Bool(a >= b));
            }

            // ========================================
            // i64 Comparison
            // ========================================
            Op::I64Eq => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }
            Op::I64Ne => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = !self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }
            Op::I64LtS => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? < 0;
                self.stack.push(Value::Bool(result));
            }
            Op::I64LeS => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? <= 0;
                self.stack.push(Value::Bool(result));
            }
            Op::I64GtS => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? > 0;
                self.stack.push(Value::Bool(result));
            }
            Op::I64GeS => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? >= 0;
                self.stack.push(Value::Bool(result));
            }

            // ========================================
            // f32 Comparison
            // ========================================
            Op::F32Eq => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a == b));
            }
            Op::F32Ne => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a != b));
            }
            Op::F32Lt => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a < b));
            }
            Op::F32Le => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a <= b));
            }
            Op::F32Gt => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a > b));
            }
            Op::F32Ge => {
                let b = self.pop_float()? as f32;
                let a = self.pop_float()? as f32;
                self.stack.push(Value::Bool(a >= b));
            }

            // ========================================
            // f64 Comparison
            // ========================================
            Op::F64Eq => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a == b));
            }
            Op::F64Ne => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a != b));
            }
            Op::F64Lt => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a < b));
            }
            Op::F64Le => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a <= b));
            }
            Op::F64Gt => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a > b));
            }
            Op::F64Ge => {
                let b = self.pop_float()?;
                let a = self.pop_float()?;
                self.stack.push(Value::Bool(a >= b));
            }

            // ========================================
            // Ref Comparison
            // ========================================
            Op::RefEq => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }
            Op::RefIsNull => {
                let a = self.stack.pop().ok_or("stack underflow")?;
                self.stack.push(Value::Bool(a == Value::Null));
            }

            // ========================================
            // Type Conversion
            // ========================================
            Op::I32WrapI64 => {
                let a = self.pop_int()?;
                self.stack.push(Value::I64((a as i32) as i64));
            }
            Op::I64ExtendI32S => {
                let a = self.pop_int()? as i32;
                self.stack.push(Value::I64(a as i64));
            }
            Op::I64ExtendI32U => {
                let a = self.pop_int()? as u32;
                self.stack.push(Value::I64(a as i64));
            }
            Op::F64ConvertI64S => {
                let a = self.pop_int()?;
                self.stack.push(Value::F64(a as f64));
            }
            Op::I64TruncF64S => {
                let a = self.pop_float()?;
                self.stack.push(Value::I64(a as i64));
            }
            Op::F64ConvertI32S => {
                let a = self.pop_int()? as i32;
                self.stack.push(Value::F64(a as f64));
            }
            Op::F32ConvertI32S => {
                let a = self.pop_int()? as i32;
                self.stack.push(Value::F64((a as f32) as f64));
            }
            Op::F32ConvertI64S => {
                let a = self.pop_int()?;
                self.stack.push(Value::F64((a as f32) as f64));
            }
            Op::I32TruncF32S => {
                let a = self.pop_float()? as f32;
                self.stack.push(Value::I64((a as i32) as i64));
            }
            Op::I32TruncF64S => {
                let a = self.pop_float()?;
                self.stack.push(Value::I64((a as i32) as i64));
            }
            Op::I64TruncF32S => {
                let a = self.pop_float()? as f32;
                self.stack.push(Value::I64(a as i64));
            }
            Op::F32DemoteF64 => {
                let a = self.pop_float()?;
                self.stack.push(Value::F64((a as f32) as f64));
            }
            Op::F64PromoteF32 => {
                let a = self.pop_float()? as f32;
                self.stack.push(Value::F64(a as f64));
            }
            Op::Jmp(target) => {
                // Get frame info without holding mutable borrow
                let (current_pc, func_index) = {
                    let frame = self.frames.last().unwrap();
                    (frame.pc.saturating_sub(1), frame.func_index) // PC was already incremented
                };

                // Detect backward branch (loop)
                if target < current_pc {
                    let key = (func_index, current_pc);
                    let count = self.loop_counts.entry(key).or_insert(0);
                    *count += 1;

                    // Loop range: start_pc = target, end_pc = current_pc
                    let loop_start_pc = target;
                    let loop_end_pc = current_pc;

                    // Check if this loop should be JIT compiled
                    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
                    {
                        if self.should_jit_compile_loop(func_index, current_pc) {
                            let func = if func_index == usize::MAX {
                                &chunk.main
                            } else {
                                &chunk.functions[func_index]
                            };
                            self.jit_compile_loop(func, func_index, loop_start_pc, loop_end_pc);
                        }
                    }

                    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
                    {
                        if self.should_jit_compile_loop(func_index, current_pc) {
                            let func = if func_index == usize::MAX {
                                &chunk.main
                            } else {
                                &chunk.functions[func_index]
                            };
                            self.jit_compile_loop(func, func_index, loop_start_pc, loop_end_pc);
                        }
                    }

                    // Execute JIT compiled loop if available
                    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
                    {
                        if self.is_loop_jit_compiled(func_index, current_pc) {
                            let func = if func_index == usize::MAX {
                                &chunk.main
                            } else {
                                &chunk.functions[func_index]
                            };
                            let next_pc =
                                self.execute_jit_loop(func_index, current_pc, func, chunk)?;
                            let frame = self.frames.last_mut().unwrap();
                            frame.pc = next_pc;
                            return Ok(ControlFlow::Continue);
                        }
                    }

                    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
                    {
                        if self.is_loop_jit_compiled(func_index, current_pc) {
                            let func = if func_index == usize::MAX {
                                &chunk.main
                            } else {
                                &chunk.functions[func_index]
                            };
                            let next_pc =
                                self.execute_jit_loop(func_index, current_pc, func, chunk)?;
                            let frame = self.frames.last_mut().unwrap();
                            frame.pc = next_pc;
                            return Ok(ControlFlow::Continue);
                        }
                    }
                }

                // Update PC (fallback to interpreter)
                let frame = self.frames.last_mut().unwrap();
                frame.pc = target;
            }
            Op::BrIfFalse(target) => {
                let cond = self.stack.pop().ok_or("stack underflow")?;
                if !cond.is_truthy() {
                    let frame = self.frames.last_mut().unwrap();
                    frame.pc = target;
                }
            }
            Op::BrIf(target) => {
                let cond = self.stack.pop().ok_or("stack underflow")?;
                if cond.is_truthy() {
                    let frame = self.frames.last_mut().unwrap();
                    frame.pc = target;
                }
            }
            Op::Call(func_index, argc) => {
                let func = &chunk.functions[func_index];

                if argc != func.arity {
                    return Err(format!(
                        "runtime error: function '{}' expects {} arguments, got {}",
                        func.name, func.arity, argc
                    ));
                }

                // Check if we should JIT compile this function
                #[cfg(all(target_arch = "x86_64", feature = "jit"))]
                {
                    if self.should_jit_compile(func_index, &func.name) {
                        self.jit_compile_function(func, func_index);
                    }

                    // If JIT compiled, execute via JIT
                    if self.is_jit_compiled(func_index) {
                        let result = self.execute_jit_function(func_index, argc, func, chunk)?;
                        self.stack.push(result);
                        return Ok(ControlFlow::Continue);
                    }
                }

                #[cfg(all(target_arch = "aarch64", feature = "jit"))]
                {
                    if self.should_jit_compile(func_index, &func.name) {
                        self.jit_compile_function(func, func_index);
                    }

                    // If JIT compiled, execute via JIT
                    if self.is_jit_compiled(func_index) {
                        let result = self.execute_jit_function(func_index, argc, func, chunk)?;
                        self.stack.push(result);
                        return Ok(ControlFlow::Continue);
                    }
                }

                // Fall back to interpreter
                let new_stack_base = self.stack.len() - argc;

                self.frames.push(Frame {
                    func_index,
                    pc: 0,
                    stack_base: new_stack_base,
                });
            }
            Op::Ret => {
                let return_value = self.stack.pop().unwrap_or(Value::Null);

                let frame = self.frames.pop().unwrap();

                if self.frames.is_empty() {
                    // Push the return value back so run_and_get_result can retrieve it
                    self.stack.push(return_value);
                    return Ok(ControlFlow::Exit);
                }

                // Clean up the stack (remove locals and arguments)
                self.stack.truncate(frame.stack_base);

                // Push return value
                self.stack.push(return_value);

                return Ok(ControlFlow::Return);
            }
            Op::ArrayLen => {
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val
                    .as_ref()
                    .ok_or("runtime error: expected array or string")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;

                let len = obj.slots.len() as i64;
                self.stack.push(Value::I64(len));
            }
            Op::TypeOf => {
                use crate::vm::heap::ObjectKind;
                let value = self.stack.pop().ok_or("stack underflow")?;
                let type_name = match &value {
                    Value::I64(_) => "int",
                    Value::F64(_) => "float",
                    Value::Bool(_) => "bool",
                    Value::Null => "nil",
                    Value::Ref(r) => {
                        if let Some(obj) = self.heap.get(*r) {
                            match obj.kind {
                                ObjectKind::String => "string",
                                ObjectKind::Array => "array",
                                ObjectKind::Slots => "slots",
                            }
                        } else {
                            "unknown"
                        }
                    }
                };
                let r = self.heap.alloc_string(type_name.to_string())?;
                self.stack.push(Value::Ref(r));
            }
            Op::ToString => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let s = self.value_to_string(&value)?;
                let r = self.heap.alloc_string(s)?;
                self.stack.push(Value::Ref(r));
            }
            Op::ParseInt => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let r = value
                    .as_ref()
                    .ok_or("runtime error: parse_int expects string")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                let s = obj.slots_to_string();
                let n: i64 = s
                    .trim()
                    .parse()
                    .map_err(|_| format!("runtime error: cannot parse '{}' as int", s))?;
                self.stack.push(Value::I64(n));
            }
            Op::StrLen => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let r = value
                    .as_ref()
                    .ok_or("runtime error: str_len expects string")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                // Length is the number of slots
                let len = obj.slots.len() as i64;
                self.stack.push(Value::I64(len));
            }
            Op::Throw => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let msg = self.value_to_string(&value)?;
                return Err(format!("runtime error: {}", msg));
            }
            Op::TryBegin(handler_pc) => {
                let frame = self.frames.last().unwrap();
                self.try_frames.push(TryFrame {
                    stack_depth: self.stack.len(),
                    frame_depth: self.frames.len(),
                    handler_pc,
                    func_index: frame.func_index,
                });
            }
            Op::TryEnd => {
                self.try_frames.pop();
            }
            Op::PrintDebug => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let s = self.value_to_string(&value)?;
                writeln!(self.output, "{}", s).map_err(|e| format!("io error: {}", e))?;
                // print returns the value it printed (for expression statements)
                self.stack.push(value);
            }
            Op::GcHint(_bytes) => {
                // Hint about upcoming allocation - might trigger GC
                if self.heap.should_gc() {
                    self.collect_garbage();
                }
            }

            // Thread operations
            Op::ThreadSpawn(func_index) => {
                // Clone the chunk for the new thread
                let chunk_clone = chunk.clone();

                // Spawn a new thread that creates a VM and runs the function
                let thread_id = self.thread_spawner.spawn(move || {
                    let mut vm = VM::new();

                    // Create a wrapper main that calls the target function and captures return
                    // The wrapper just calls the function and returns its result
                    let wrapper_main = Function {
                        name: "__thread_main__".to_string(),
                        arity: 0,
                        locals_count: 1, // To store return value
                        code: vec![
                            Op::Call(func_index, 0), // Call the target function (must be 0-arity)
                            Op::Ret,                 // Return the result
                        ],
                        stackmap: None,
                        local_types: vec![],
                    };

                    let thread_chunk = Chunk {
                        functions: chunk_clone.functions.clone(),
                        main: wrapper_main,
                        strings: chunk_clone.strings.clone(),
                        debug: None,
                    };

                    match vm.run_and_get_result(&thread_chunk) {
                        Ok(result) => result,
                        Err(_e) => Value::Null,
                    }
                });

                // Push the thread handle ID as the result
                self.stack.push(Value::I64(thread_id as i64));
            }
            Op::ChannelCreate => {
                // Create a new channel and return [sender_id, receiver_id]
                // For simplicity, we use the same id for both (same underlying channel)
                let channel = Channel::new();
                let id = self.channels.len();
                self.channels.push(channel);

                // Create slots with [sender_id, receiver_id] layout
                let arr = self
                    .heap
                    .alloc_slots(vec![Value::I64(id as i64), Value::I64(id as i64)])?;
                self.stack.push(Value::Ref(arr));
            }
            Op::ChannelSend => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let channel_id = self.pop_int()? as usize;

                let channel = self
                    .channels
                    .get(channel_id)
                    .ok_or_else(|| format!("runtime error: channel {} not found", channel_id))?
                    .clone();

                channel
                    .send(value)
                    .map_err(|_| "runtime error: channel closed")?;
            }
            Op::ChannelRecv => {
                let channel_id = self.pop_int()? as usize;

                let channel = self
                    .channels
                    .get(channel_id)
                    .ok_or_else(|| format!("runtime error: channel {} not found", channel_id))?
                    .clone();

                let value = channel.recv().unwrap_or(Value::Null);
                self.stack.push(value);
            }
            Op::ThreadJoin => {
                let thread_id = self.pop_int()? as usize;

                let result = self.thread_spawner.join(thread_id)?;
                self.stack.push(result);
            }

            // Heap slot operations
            Op::HeapAlloc(n) => {
                let mut slots = Vec::with_capacity(n);
                for _ in 0..n {
                    slots.push(self.stack.pop().ok_or("stack underflow")?);
                }
                slots.reverse();
                let r = self.heap.alloc_slots(slots)?;
                self.stack.push(Value::Ref(r));
            }
            Op::HeapLoad(offset) => {
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                if offset >= obj.slots.len() {
                    return Err(format!(
                        "runtime error: slot index {} out of bounds",
                        offset
                    ));
                }
                self.stack.push(obj.slots[offset]);
            }
            Op::HeapStore(offset) => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                self.heap.write_slot(r, offset, value).map_err(|e| {
                    format!("runtime error: slot index {} out of bounds ({})", offset, e)
                })?;
            }
            Op::HeapLoadDyn => {
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;

                if index < 0 || index as usize >= obj.slots.len() {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                self.stack.push(obj.slots[index as usize]);
            }
            Op::HeapStoreDyn => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                if index < 0 {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                self.heap
                    .write_slot(r, index as usize, value)
                    .map_err(|e| format!("runtime error: {}", e))?;
            }
            Op::HeapAllocDyn => {
                // Pop size from stack, then pop that many elements as initial values
                let size_val = self.stack.pop().ok_or("stack underflow")?;
                let size = size_val
                    .as_i64()
                    .ok_or("runtime error: HeapAllocDyn requires integer size")?
                    as usize;
                // Pop 'size' elements from stack (they were pushed in order, so reverse)
                let mut slots = Vec::with_capacity(size);
                for _ in 0..size {
                    slots.push(self.stack.pop().ok_or("stack underflow")?);
                }
                slots.reverse();
                let r = self.heap.alloc_slots(slots)?;
                self.stack.push(Value::Ref(r));
            }
            Op::HeapAllocDynSimple => {
                // Pop size from stack, allocate that many null-initialized slots
                let size_val = self.stack.pop().ok_or("stack underflow")?;
                let size = size_val
                    .as_i64()
                    .ok_or("runtime error: HeapAllocDynSimple requires integer size")?
                    as usize;
                let slots = vec![Value::Null; size];
                let r = self.heap.alloc_slots(slots)?;
                self.stack.push(Value::Ref(r));
            }
            Op::Syscall(syscall_num, argc) => {
                // Collect arguments from stack
                let mut args = Vec::with_capacity(argc);
                for _ in 0..argc {
                    args.push(self.stack.pop().ok_or("stack underflow")?);
                }
                args.reverse(); // Arguments were popped in reverse order

                let result = self.handle_syscall(syscall_num, &args)?;
                self.stack.push(result);
            }
            Op::Argc => {
                let count = self.cli_args.len() as i64;
                self.stack.push(Value::I64(count));
            }
            Op::Argv => {
                let idx = match self.stack.pop() {
                    Some(Value::I64(i)) => i as usize,
                    _ => return Err("argv: expected integer index".to_string()),
                };
                let arg = self.cli_argv(idx).to_string();
                let r = self.heap.alloc_string(arg)?;
                self.stack.push(Value::Ref(r));
            }
            Op::Args => {
                // Create an array of all CLI arguments
                let mut slots = Vec::with_capacity(self.cli_args.len());
                for arg in self.cli_args.clone() {
                    let r = self.heap.alloc_string(arg)?;
                    slots.push(Value::Ref(r));
                }
                let arr_ref = self.heap.alloc_slots(slots)?;
                self.stack.push(Value::Ref(arr_ref));
            }
        }

        Ok(ControlFlow::Continue)
    }

    fn add(&mut self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a + b)),
            (Value::F64(a), Value::F64(b)) => Ok(Value::F64(a + b)),
            (Value::I64(a), Value::F64(b)) => Ok(Value::F64(a as f64 + b)),
            (Value::F64(a), Value::I64(b)) => Ok(Value::F64(a + b as f64)),
            (Value::Ref(a), Value::Ref(b)) => {
                // String concatenation
                let a_obj = self.heap.get(a).ok_or("runtime error: invalid reference")?;
                let b_obj = self.heap.get(b).ok_or("runtime error: invalid reference")?;

                let a_str = a_obj.slots_to_string();
                let b_str = b_obj.slots_to_string();
                let result = format!("{}{}", a_str, b_str);
                let r = self.heap.alloc_string(result)?;
                Ok(Value::Ref(r))
            }
            _ => Err("runtime error: cannot add these types".to_string()),
        }
    }

    fn sub(&self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a - b)),
            (Value::F64(a), Value::F64(b)) => Ok(Value::F64(a - b)),
            (Value::I64(a), Value::F64(b)) => Ok(Value::F64(a as f64 - b)),
            (Value::F64(a), Value::I64(b)) => Ok(Value::F64(a - b as f64)),
            _ => Err("runtime error: cannot subtract these types".to_string()),
        }
    }

    fn mul(&self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a * b)),
            (Value::F64(a), Value::F64(b)) => Ok(Value::F64(a * b)),
            (Value::I64(a), Value::F64(b)) => Ok(Value::F64(a as f64 * b)),
            (Value::F64(a), Value::I64(b)) => Ok(Value::F64(a * b as f64)),
            _ => Err("runtime error: cannot multiply these types".to_string()),
        }
    }

    fn div(&self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => {
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                Ok(Value::I64(a / b))
            }
            (Value::F64(a), Value::F64(b)) => {
                if b == 0.0 {
                    return Err("runtime error: division by zero".to_string());
                }
                Ok(Value::F64(a / b))
            }
            (Value::I64(a), Value::F64(b)) => {
                if b == 0.0 {
                    return Err("runtime error: division by zero".to_string());
                }
                Ok(Value::F64(a as f64 / b))
            }
            (Value::F64(a), Value::I64(b)) => {
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                Ok(Value::F64(a / b as f64))
            }
            _ => Err("runtime error: cannot divide these types".to_string()),
        }
    }

    fn compare(&self, a: &Value, b: &Value) -> Result<i32, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(a.cmp(b) as i32),
            (Value::F64(a), Value::F64(b)) => Ok(a.partial_cmp(b).map(|o| o as i32).unwrap_or(0)),
            (Value::I64(a), Value::F64(b)) => {
                let a = *a as f64;
                Ok(a.partial_cmp(b).map(|o| o as i32).unwrap_or(0))
            }
            (Value::F64(a), Value::I64(b)) => {
                let b = *b as f64;
                Ok(a.partial_cmp(&b).map(|o| o as i32).unwrap_or(0))
            }
            _ => Err("runtime error: cannot compare these types".to_string()),
        }
    }

    fn values_equal(&self, a: &Value, b: &Value) -> bool {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => a == b,
            (Value::F64(a), Value::F64(b)) => a == b,
            (Value::I64(a), Value::F64(b)) => (*a as f64) == *b,
            (Value::F64(a), Value::I64(b)) => *a == (*b as f64),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Ref(a_ref), Value::Ref(b_ref)) => {
                // Compare by content for strings
                let a_obj = self.heap.get(*a_ref);
                let b_obj = self.heap.get(*b_ref);

                match (a_obj, b_obj) {
                    (Some(a), Some(b)) => {
                        // Compare strings by content
                        a.slots_to_string() == b.slots_to_string()
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    fn value_to_string(&self, value: &Value) -> Result<String, String> {
        match value {
            Value::I64(n) => Ok(n.to_string()),
            Value::F64(f) => {
                if f.fract() == 0.0 {
                    Ok(format!("{}.0", f))
                } else {
                    Ok(f.to_string())
                }
            }
            Value::Bool(b) => Ok(b.to_string()),
            Value::Null => Ok("nil".to_string()),
            Value::Ref(r) => {
                let obj = self
                    .heap
                    .get(*r)
                    .ok_or("runtime error: invalid reference")?;
                // Try to interpret as string first
                // Only treat as string if all slots are printable Unicode characters
                // (not control characters 0-31, except tab/newline/carriage return)
                let is_printable_string = !obj.slots.is_empty()
                    && obj.slots.iter().all(|v| {
                        if let Some(c) = v.as_i64() {
                            if let Some(ch) = char::from_u32(c as u32) {
                                // Allow printable chars, tab, newline, carriage return
                                ch >= ' ' || ch == '\t' || ch == '\n' || ch == '\r'
                            } else {
                                false
                            }
                        } else {
                            false
                        }
                    });
                if is_printable_string {
                    // Interpret as string
                    let chars: String = obj
                        .slots
                        .iter()
                        .filter_map(|v| v.as_i64())
                        .filter_map(|c| char::from_u32(c as u32))
                        .collect();
                    Ok(chars)
                } else {
                    // Interpret as array/struct - show all elements
                    let mut parts = Vec::new();
                    for elem in obj.slots.iter() {
                        parts.push(self.value_to_string(elem)?);
                    }
                    Ok(format!("[{}]", parts.join(", ")))
                }
            }
        }
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        let value = self.stack.pop().ok_or("stack underflow")?;
        value.as_i64().ok_or_else(|| "expected integer".to_string())
    }

    fn pop_float(&mut self) -> Result<f64, String> {
        let value = self.stack.pop().ok_or("stack underflow")?;
        match value {
            Value::F64(f) => Ok(f),
            Value::I64(i) => Ok(i as f64),
            _ => Err("expected float".to_string()),
        }
    }

    fn handle_exception(&mut self, error: String, _chunk: &Chunk) -> Result<bool, String> {
        // Look for a try frame that can handle this exception
        while let Some(try_frame) = self.try_frames.pop() {
            // Unwind call stack to the try frame's depth
            while self.frames.len() > try_frame.frame_depth {
                self.frames.pop();
            }

            // Restore stack to the try frame's depth
            self.stack.truncate(try_frame.stack_depth);

            // Push the error message as a string
            let error_ref = self.heap.alloc_string(error.clone())?;
            self.stack.push(Value::Ref(error_ref));

            // Jump to the handler
            if let Some(frame) = self.frames.last_mut()
                && frame.func_index == try_frame.func_index
            {
                frame.pc = try_frame.handler_pc;
                return Ok(true);
            }
        }

        // No handler found
        Ok(false)
    }

    /// Write barrier for GC - called before overwriting a reference.
    ///
    /// For stop-the-world GC, this is a no-op. When concurrent GC is enabled,
    /// this implements the SATB (Snapshot-At-The-Beginning) barrier to ensure
    /// the old value is not lost during concurrent marking.
    ///
    /// This barrier must be called at:
    /// - SETL: before storing to a local variable
    /// - SETF: before storing to an object field
    #[inline]
    fn write_barrier(&self, _old_value: Value) {
        // No-op for stop-the-world GC.
        // When concurrent GC is integrated, this will call:
        // self.concurrent_gc.write_barrier(old_value);
    }

    fn collect_garbage(&mut self) {
        // Collect all roots from the stack
        let mut roots: Vec<Value> = self.stack.clone();

        // Add string cache references as roots
        for r in self.string_cache.iter().flatten() {
            roots.push(Value::Ref(*r));
        }

        self.heap.collect(&roots);
    }

    /// Handle syscall instructions
    /// Syscall numbers:
    /// - 1: write(fd, buf, count) -> bytes_written
    /// - 2: open(path, flags) -> fd
    /// - 3: close(fd) -> 0 on success
    /// - 4: read(fd, count) -> string (heap ref) or error
    fn handle_syscall(&mut self, syscall_num: usize, args: &[Value]) -> Result<Value, String> {
        // Syscall numbers
        const SYSCALL_WRITE: usize = 1;
        const SYSCALL_OPEN: usize = 2;
        const SYSCALL_CLOSE: usize = 3;
        const SYSCALL_READ: usize = 4;
        const SYSCALL_SOCKET: usize = 5;
        const SYSCALL_CONNECT: usize = 6;
        const SYSCALL_BIND: usize = 7;
        const SYSCALL_LISTEN: usize = 8;
        const SYSCALL_ACCEPT: usize = 9;

        // Error codes (negative return values)
        const EBADF: i64 = -1; // Bad file descriptor
        const ENOENT: i64 = -2; // No such file or directory
        const EACCES: i64 = -3; // Permission denied
        const ECONNREFUSED: i64 = -4; // Connection refused
        const ETIMEDOUT: i64 = -5; // Connection timed out
        const EAFNOSUPPORT: i64 = -6; // Address family not supported
        const ESOCKTNOSUPPORT: i64 = -7; // Socket type not supported
        const EADDRINUSE: i64 = -8; // Address already in use

        // Open flags (Linux-compatible values)
        const O_RDONLY: i64 = 0;
        const O_WRONLY: i64 = 1;
        const O_CREAT: i64 = 64;
        const O_TRUNC: i64 = 512;

        // Socket constants (Linux-compatible values)
        const AF_INET: i64 = 2;
        const SOCK_STREAM: i64 = 1;

        match syscall_num {
            SYSCALL_OPEN => {
                if args.len() != 2 {
                    return Err(format!(
                        "open syscall expects 2 arguments, got {}",
                        args.len()
                    ));
                }

                // Get path string
                let path_ref = match &args[0] {
                    Value::Ref(r) => *r,
                    _ => return Err("open: path must be a string".to_string()),
                };
                let heap_obj = self
                    .heap
                    .get(path_ref)
                    .ok_or_else(|| "open: invalid reference".to_string())?;
                let path = heap_obj.slots_to_string();

                // Get flags
                let flags = args[1]
                    .as_i64()
                    .ok_or_else(|| "open: flags must be an integer".to_string())?;

                // Build OpenOptions based on flags
                let mut options = OpenOptions::new();

                // O_RDONLY (0) means read-only if O_WRONLY is not set
                if flags & O_WRONLY != 0 {
                    options.write(true);
                } else {
                    // O_RDONLY: read-only mode
                    options.read(true);
                }
                let _ = O_RDONLY; // suppress unused warning
                if flags & O_CREAT != 0 {
                    options.create(true);
                }
                if flags & O_TRUNC != 0 {
                    options.truncate(true);
                }

                // Try to open the file
                match options.open(&path) {
                    Ok(file) => {
                        let fd = self.next_fd;
                        self.next_fd += 1;
                        self.file_descriptors.insert(fd, file);
                        Ok(Value::I64(fd))
                    }
                    Err(e) => {
                        // Map IO errors to our error codes
                        let error_code = match e.kind() {
                            std::io::ErrorKind::NotFound => ENOENT,
                            std::io::ErrorKind::PermissionDenied => EACCES,
                            _ => EBADF,
                        };
                        Ok(Value::I64(error_code))
                    }
                }
            }
            SYSCALL_CLOSE => {
                if args.len() != 1 {
                    return Err(format!(
                        "close syscall expects 1 argument, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "close: fd must be an integer".to_string())?;

                // Cannot close stdin/stdout/stderr
                if fd <= 2 {
                    return Ok(Value::I64(EBADF));
                }

                // Remove from fd table (File/TcpStream/TcpListener is dropped automatically)
                let closed = self.file_descriptors.remove(&fd).is_some()
                    || self.socket_descriptors.remove(&fd).is_some()
                    || self.pending_sockets.remove(&fd)
                    || self.listener_descriptors.remove(&fd).is_some();

                if closed {
                    Ok(Value::I64(0)) // Success
                } else {
                    Ok(Value::I64(EBADF)) // Invalid fd
                }
            }
            SYSCALL_WRITE => {
                if args.len() != 3 {
                    return Err(format!(
                        "write syscall expects 3 arguments, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "write: fd must be an integer".to_string())?;
                let buf_ref = match &args[1] {
                    Value::Ref(r) => *r,
                    _ => return Err("write: buf must be a string".to_string()),
                };
                let count = args[2]
                    .as_i64()
                    .ok_or_else(|| "write: count must be an integer".to_string())?;

                // Get the string from heap
                let heap_obj = self
                    .heap
                    .get(buf_ref)
                    .ok_or_else(|| "write: invalid reference".to_string())?;
                let buf_str = heap_obj.slots_to_string();

                // Calculate actual bytes to write
                let buf_bytes = buf_str.as_bytes();
                let actual_count = (count as usize).min(buf_bytes.len());
                let bytes_to_write = &buf_bytes[..actual_count];

                // Write to the appropriate output based on fd
                let result = if fd == 1 {
                    // stdout
                    self.output
                        .write_all(bytes_to_write)
                        .map(|_| actual_count as i64)
                        .unwrap_or(EBADF)
                } else if fd == 2 {
                    // stderr
                    self.stderr
                        .write_all(bytes_to_write)
                        .map(|_| actual_count as i64)
                        .unwrap_or(EBADF)
                } else if let Some(file) = self.file_descriptors.get_mut(&fd) {
                    // File from fd table
                    file.write_all(bytes_to_write)
                        .map(|_| actual_count as i64)
                        .unwrap_or(EBADF)
                } else if let Some(socket) = self.socket_descriptors.get_mut(&fd) {
                    // Socket from socket_descriptors table
                    socket
                        .write_all(bytes_to_write)
                        .map(|_| actual_count as i64)
                        .unwrap_or(EBADF)
                } else {
                    // Invalid fd
                    EBADF
                };

                Ok(Value::I64(result))
            }
            SYSCALL_READ => {
                if args.len() != 2 {
                    return Err(format!(
                        "read syscall expects 2 arguments, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "read: fd must be an integer".to_string())?;
                let count = args[1]
                    .as_i64()
                    .ok_or_else(|| "read: count must be an integer".to_string())?;

                // Validate arguments
                if fd <= 2 || count < 0 {
                    return Ok(Value::I64(EBADF));
                }

                // Read up to count bytes from file or socket
                let mut buffer = vec![0u8; count as usize];
                let bytes_read = if let Some(file) = self.file_descriptors.get_mut(&fd) {
                    match file.read(&mut buffer) {
                        Ok(n) => n,
                        Err(_) => return Ok(Value::I64(EBADF)),
                    }
                } else if let Some(socket) = self.socket_descriptors.get_mut(&fd) {
                    match socket.read(&mut buffer) {
                        Ok(n) => n,
                        Err(_) => return Ok(Value::I64(EBADF)),
                    }
                } else {
                    return Ok(Value::I64(EBADF));
                };

                // Truncate buffer to actual bytes read
                buffer.truncate(bytes_read);

                // Convert to string (assuming UTF-8)
                let content = match String::from_utf8(buffer) {
                    Ok(s) => s,
                    Err(e) => {
                        // Fall back to lossy conversion for non-UTF8 data
                        String::from_utf8_lossy(&e.into_bytes()).into_owned()
                    }
                };

                // Allocate string on heap and return reference
                let heap_ref = self.heap.alloc_string(content)?;
                Ok(Value::Ref(heap_ref))
            }
            SYSCALL_SOCKET => {
                if args.len() != 2 {
                    return Err(format!(
                        "socket syscall expects 2 arguments, got {}",
                        args.len()
                    ));
                }

                let domain = args[0]
                    .as_i64()
                    .ok_or_else(|| "socket: domain must be an integer".to_string())?;
                let sock_type = args[1]
                    .as_i64()
                    .ok_or_else(|| "socket: type must be an integer".to_string())?;

                // Only support AF_INET (2)
                if domain != AF_INET {
                    return Ok(Value::I64(EAFNOSUPPORT));
                }

                // Only support SOCK_STREAM (1) for TCP
                if sock_type != SOCK_STREAM {
                    return Ok(Value::I64(ESOCKTNOSUPPORT));
                }

                // Allocate fd and mark as pending socket
                let fd = self.next_fd;
                self.next_fd += 1;
                self.pending_sockets.insert(fd);

                Ok(Value::I64(fd))
            }
            SYSCALL_CONNECT => {
                if args.len() != 3 {
                    return Err(format!(
                        "connect syscall expects 3 arguments, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "connect: fd must be an integer".to_string())?;

                // Get host string
                let host_ref = match &args[1] {
                    Value::Ref(r) => *r,
                    _ => return Err("connect: host must be a string".to_string()),
                };
                let heap_obj = self
                    .heap
                    .get(host_ref)
                    .ok_or_else(|| "connect: invalid reference".to_string())?;
                let host = heap_obj.slots_to_string();

                let port = args[2]
                    .as_i64()
                    .ok_or_else(|| "connect: port must be an integer".to_string())?;

                // Check fd is a pending socket
                if !self.pending_sockets.remove(&fd) {
                    return Ok(Value::I64(EBADF));
                }

                // Try to connect
                let addr = format!("{}:{}", host, port);
                match TcpStream::connect(&addr) {
                    Ok(stream) => {
                        self.socket_descriptors.insert(fd, stream);
                        Ok(Value::I64(0)) // Success
                    }
                    Err(e) => {
                        // Map IO errors to our error codes
                        let error_code = match e.kind() {
                            std::io::ErrorKind::ConnectionRefused => ECONNREFUSED,
                            std::io::ErrorKind::TimedOut => ETIMEDOUT,
                            std::io::ErrorKind::NotFound => ENOENT,
                            std::io::ErrorKind::PermissionDenied => EACCES,
                            _ => ECONNREFUSED, // Default to connection refused
                        };
                        Ok(Value::I64(error_code))
                    }
                }
            }
            SYSCALL_BIND => {
                if args.len() != 3 {
                    return Err(format!(
                        "bind syscall expects 3 arguments, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "bind: fd must be an integer".to_string())?;

                // Get host string
                let host_ref = match &args[1] {
                    Value::Ref(r) => *r,
                    _ => return Err("bind: host must be a string".to_string()),
                };
                let heap_obj = self
                    .heap
                    .get(host_ref)
                    .ok_or_else(|| "bind: invalid reference".to_string())?;
                let host = heap_obj.slots_to_string();

                let port = args[2]
                    .as_i64()
                    .ok_or_else(|| "bind: port must be an integer".to_string())?;

                // Check fd is a pending socket
                if !self.pending_sockets.remove(&fd) {
                    return Ok(Value::I64(EBADF));
                }

                // Try to bind (creates TcpListener)
                let addr = format!("{}:{}", host, port);
                match TcpListener::bind(&addr) {
                    Ok(listener) => {
                        self.listener_descriptors.insert(fd, listener);
                        Ok(Value::I64(0)) // Success
                    }
                    Err(e) => {
                        // Map IO errors to our error codes
                        let error_code = match e.kind() {
                            std::io::ErrorKind::AddrInUse => EADDRINUSE,
                            std::io::ErrorKind::PermissionDenied => EACCES,
                            _ => EBADF,
                        };
                        Ok(Value::I64(error_code))
                    }
                }
            }
            SYSCALL_LISTEN => {
                if args.len() != 2 {
                    return Err(format!(
                        "listen syscall expects 2 arguments, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "listen: fd must be an integer".to_string())?;

                let _backlog = args[1]
                    .as_i64()
                    .ok_or_else(|| "listen: backlog must be an integer".to_string())?;

                // Check fd is a valid listener (already listening after bind in Rust)
                if self.listener_descriptors.contains_key(&fd) {
                    Ok(Value::I64(0)) // Success - already listening
                } else {
                    Ok(Value::I64(EBADF)) // Not a valid listener
                }
            }
            SYSCALL_ACCEPT => {
                if args.len() != 1 {
                    return Err(format!(
                        "accept syscall expects 1 argument, got {}",
                        args.len()
                    ));
                }

                let fd = args[0]
                    .as_i64()
                    .ok_or_else(|| "accept: fd must be an integer".to_string())?;

                // Get the listener
                let listener = match self.listener_descriptors.get(&fd) {
                    Some(l) => l,
                    None => return Ok(Value::I64(EBADF)),
                };

                // Accept a connection
                match listener.accept() {
                    Ok((stream, _addr)) => {
                        let client_fd = self.next_fd;
                        self.next_fd += 1;
                        self.socket_descriptors.insert(client_fd, stream);
                        Ok(Value::I64(client_fd))
                    }
                    Err(_) => Ok(Value::I64(EBADF)),
                }
            }
            _ => Err(format!("unknown syscall: {}", syscall_num)),
        }
    }
}

/// JIT call helper function.
/// This is called from JIT code when executing a Call instruction.
/// It executes the target function via the VM and returns the result.
#[cfg(feature = "jit")]
unsafe extern "C" fn jit_call_helper(
    ctx: *mut JitCallContext,
    func_index: u64,
    argc: u64,
    args: *const JitValue,
) -> JitReturn {
    // SAFETY: ctx, vm, and chunk pointers are valid for the duration of this call
    // as they are set up by execute_jit_function before calling JIT code.
    let ctx_ref = unsafe { &mut *ctx };
    let vm = unsafe { &mut *(ctx_ref.vm as *mut VM) };
    let chunk = unsafe { &*(ctx_ref.chunk as *const Chunk) };

    // Record opcode for profiling (JIT path)
    vm.record_opcode("Call");

    let func_index = func_index as usize;
    let argc = argc as usize;

    let func = &chunk.functions[func_index];

    // Check arity
    if argc != func.arity {
        // Return nil on error (could improve error handling)
        return JitReturn { tag: 3, payload: 0 }; // TAG_NIL
    }

    // Check if we should JIT compile this function (increments call count)
    if vm.should_jit_compile(func_index, &func.name) {
        vm.jit_compile_function(func, func_index);
    }

    // FAST PATH: If target function is JIT compiled, call directly with stack allocation
    // This avoids heap allocations and VM stack operations for recursive JIT calls.
    // Combine is_jit_compiled check and entry point lookup into single HashMap access.
    if let Some(compiled) = vm.jit_functions.get(&func_index) {
        // Get entry point directly from the compiled code
        let entry: unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn =
            unsafe { compiled.entry_point() };

        // Use stack-allocated locals (fixed size, supports up to 64 locals)
        const MAX_LOCALS: usize = 64;
        let mut locals = [JitValue { tag: 3, payload: 0 }; MAX_LOCALS];

        // Copy arguments to locals (arguments are the first `argc` locals)
        for (i, local) in locals.iter_mut().take(argc).enumerate() {
            *local = unsafe { *args.add(i) };
        }

        // Use stack-allocated value stack
        let mut stack = [JitValue { tag: 0, payload: 0 }; 256];

        // Call JIT function directly with stack-allocated buffers
        let result = unsafe {
            entry(
                ctx as *mut u8, // Same context - recursive calls use same call_helper
                stack.as_mut_ptr(),
                locals.as_mut_ptr(),
            )
        };

        // Only check trace_jit flag if enabled (avoid branch in hot path when disabled)
        #[cfg(debug_assertions)]
        if vm.trace_jit {
            eprintln!(
                "[JIT] Executed function '{}', result: tag={}, payload={}",
                func.name, result.tag, result.payload
            );
        }

        return result;
    }

    // SLOW PATH: Execute via interpreter
    // Push arguments to VM stack
    for i in 0..argc {
        let jit_val = unsafe { *args.add(i) };
        vm.stack.push(jit_val.to_value());
    }

    {
        // Execute via interpreter: push frame and run until return
        // Track the frame depth BEFORE pushing our frame, so we know when to stop
        let starting_frame_depth = vm.frames.len();

        let new_stack_base = vm.stack.len() - argc;
        vm.frames.push(Frame {
            func_index,
            pc: 0,
            stack_base: new_stack_base,
        });

        // Run until the function returns (when frame depth returns to starting level)
        #[allow(clippy::while_let_loop)]
        loop {
            let frame = match vm.frames.last_mut() {
                Some(f) => f,
                None => break,
            };

            let current_func = if frame.func_index == usize::MAX {
                &chunk.main
            } else {
                &chunk.functions[frame.func_index]
            };

            if frame.pc >= current_func.code.len() {
                // End of function
                break;
            }

            let op = current_func.code[frame.pc].clone();
            frame.pc += 1;

            match vm.execute_op(op, chunk) {
                Ok(ControlFlow::Continue) => {}
                Ok(ControlFlow::Return) => {
                    // Check if we returned from our target function by checking frame depth
                    // This correctly handles nested calls to the same function
                    if vm.frames.len() <= starting_frame_depth {
                        break;
                    }
                }
                Ok(ControlFlow::Exit) => break,
                Err(_) => {
                    return JitReturn { tag: 3, payload: 0 }; // TAG_NIL on error
                }
            }
        }

        // Get return value from stack
        let result = vm.stack.pop().unwrap_or(Value::Null);
        let jit_result = JitValue::from_value(&result);
        JitReturn {
            tag: jit_result.tag,
            payload: jit_result.payload,
        }
    }
}

/// JIT push string helper function.
/// Allocates a string constant on the heap and returns a Ref.
#[cfg(feature = "jit")]
unsafe extern "C" fn jit_push_string_helper(
    ctx: *mut JitCallContext,
    string_index: u64,
) -> JitReturn {
    let ctx_ref = unsafe { &mut *ctx };
    let vm = unsafe { &mut *(ctx_ref.vm as *mut VM) };
    let chunk = unsafe { &*(ctx_ref.chunk as *const Chunk) };

    // Record opcode for profiling (JIT path)
    vm.record_opcode("PushString");

    let idx = string_index as usize;

    // Use get_or_alloc_string which handles caching
    match vm.get_or_alloc_string(idx, chunk) {
        Ok(r) => JitReturn {
            tag: 4, // TAG_PTR
            payload: r.index as u64,
        },
        Err(_) => JitReturn {
            tag: 3, // TAG_NIL
            payload: 0,
        },
    }
}

/// JIT array/string length helper function.
/// Returns the length of an array or string.
#[cfg(feature = "jit")]
unsafe extern "C" fn jit_array_len_helper(ctx: *mut JitCallContext, ref_index: u64) -> JitReturn {
    let ctx_ref = unsafe { &mut *ctx };
    let vm = unsafe { &mut *(ctx_ref.vm as *mut VM) };

    // Record opcode for profiling (JIT path)
    vm.record_opcode("ArrayLen");

    let gc_ref = GcRef {
        index: ref_index as usize,
    };

    match vm.heap.get(gc_ref) {
        Some(obj) => JitReturn {
            tag: 0, // TAG_INT
            payload: obj.slots.len() as u64,
        },
        None => JitReturn {
            tag: 0, // TAG_INT
            payload: 0,
        },
    }
}

/// JIT syscall helper function.
/// Executes a syscall via the VM and returns the result.
#[cfg(feature = "jit")]
unsafe extern "C" fn jit_syscall_helper(
    ctx: *mut JitCallContext,
    syscall_num: u64,
    argc: u64,
    args: *const JitValue,
) -> JitReturn {
    let ctx_ref = unsafe { &mut *ctx };
    let vm = unsafe { &mut *(ctx_ref.vm as *mut VM) };

    // Record opcode for profiling (JIT path)
    vm.record_opcode("Syscall");

    let argc = argc as usize;

    // Convert JitValue args to VM Values
    let mut vm_args = Vec::with_capacity(argc);
    for i in 0..argc {
        let jit_val = unsafe { *args.add(i) };
        vm_args.push(jit_val.to_value());
    }

    // Call the syscall handler
    match vm.handle_syscall(syscall_num as usize, &vm_args) {
        Ok(result) => {
            let jit_result = JitValue::from_value(&result);
            JitReturn {
                tag: jit_result.tag,
                payload: jit_result.payload,
            }
        }
        Err(_) => JitReturn {
            tag: 3, // TAG_NIL
            payload: 0,
        },
    }
}

enum ControlFlow {
    Continue,
    Return,
    Exit,
}

impl Default for VM {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_code(ops: Vec<Op>) -> Result<Vec<Value>, String> {
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops,
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![],
            debug: None,
        };

        let mut vm = VM::new();
        vm.run(&chunk)?;
        Ok(vm.stack)
    }

    fn run_code_with_strings(ops: Vec<Op>, strings: Vec<String>) -> Result<Vec<Value>, String> {
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 0,
                code: ops,
                stackmap: None,
                local_types: vec![],
            },
            strings,
            debug: None,
        };

        let mut vm = VM::new();
        vm.run(&chunk)?;
        Ok(vm.stack)
    }

    #[test]
    fn test_push_int() {
        let stack = run_code(vec![Op::I64Const(42)]).unwrap();
        assert_eq!(stack, vec![Value::I64(42)]);
    }

    #[test]
    fn test_push_float() {
        let stack = run_code(vec![Op::F64Const(3.14)]).unwrap();
        assert_eq!(stack, vec![Value::F64(3.14)]);
    }

    #[test]
    fn test_push_nil() {
        let stack = run_code(vec![Op::RefNull]).unwrap();
        assert_eq!(stack, vec![Value::Null]);
    }

    #[test]
    fn test_add() {
        let stack = run_code(vec![Op::I64Const(1), Op::I64Const(2), Op::I64Add]).unwrap();
        assert_eq!(stack, vec![Value::I64(3)]);
    }

    #[test]
    fn test_add_float() {
        let stack = run_code(vec![Op::F64Const(1.5), Op::F64Const(2.5), Op::F64Add]).unwrap();
        assert_eq!(stack, vec![Value::F64(4.0)]);
    }

    #[test]
    fn test_comparison() {
        let stack = run_code(vec![Op::I64Const(1), Op::I64Const(2), Op::I64LtS]).unwrap();
        assert_eq!(stack, vec![Value::Bool(true)]);
    }

    #[test]
    fn test_division_by_zero() {
        let result = run_code(vec![Op::I64Const(1), Op::I64Const(0), Op::I64DivS]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("division by zero"));
    }

    #[test]
    fn test_locals() {
        let stack = run_code(vec![Op::I64Const(42), Op::LocalSet(0), Op::LocalGet(0)]).unwrap();
        assert_eq!(stack, vec![Value::I64(42), Value::I64(42)]);
    }

    #[test]
    fn test_conditional_jump() {
        // if false, skip push 1, else push 2
        let stack = run_code(vec![
            Op::I32Const(0), // false
            Op::BrIfFalse(4),
            Op::I64Const(1),
            Op::Jmp(5),
            Op::I64Const(2),
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(2)]);
    }

    #[test]
    fn test_array_operations() {
        // HeapAlloc takes slots from stack: [e0, e1, e2] -> creates Slots object
        // Length is now slots.len(), no length prefix
        let stack = run_code(vec![
            Op::I64Const(1),  // element 0
            Op::I64Const(2),  // element 1
            Op::I64Const(3),  // element 2
            Op::HeapAlloc(3), // 3 elements
            Op::ArrayLen,
        ])
        .unwrap();
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0], Value::I64(3));
    }

    #[test]
    fn test_string_operations() {
        // Test string concatenation using Op::I64Add
        let stack = run_code_with_strings(
            vec![Op::StringConst(0), Op::StringConst(1), Op::I64Add],
            vec!["Hello, ".to_string(), "World!".to_string()],
        )
        .unwrap();
        assert_eq!(stack.len(), 1);
        // The result should be a string pointer
        assert!(stack[0].is_ref());
    }

    #[test]
    fn test_write_barrier_setl() {
        // Test that LocalSet correctly calls write barrier when overwriting references.
        // In stop-the-world GC the barrier is a no-op, but this verifies the code path.
        //
        // This test:
        // 1. Stores an array in local 0
        // 2. Overwrites local 0 with a new array (triggers write barrier)
        // 3. Verifies execution completes successfully
        let result = run_code(vec![
            // Allocate array [elem] and store in local 0
            Op::I64Const(1), // element
            Op::HeapAlloc(1),
            Op::LocalSet(0),
            // Allocate another array [elem]
            Op::I64Const(2), // element
            Op::HeapAlloc(1),
            // Overwrite local 0 (triggers write barrier, old value was array ref)
            Op::LocalSet(0),
            // Get local 0 to verify it's still a valid reference
            Op::LocalGet(0),
            Op::ArrayLen, // If we can get length, it's a valid array
        ]);

        assert!(
            result.is_ok(),
            "LocalSet write barrier test failed: {:?}",
            result
        );
        // The last value should be the array length (1 element)
        let stack = result.unwrap();
        assert!(stack.iter().any(|v| *v == Value::I64(1)));
    }

    #[test]
    fn test_syscall_write_invalid_fd() {
        // Test writing to invalid fd returns EBADF (-1)
        let stack = run_code_with_strings(
            vec![
                Op::I64Const(99),   // invalid fd
                Op::StringConst(0), // buffer
                Op::I64Const(5),    // count
                Op::Syscall(1, 3),  // syscall_write
            ],
            vec!["hello".to_string()],
        )
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_close_invalid_fd() {
        // Test closing invalid fd returns EBADF (-1)
        let stack = run_code(vec![
            Op::I64Const(99),  // invalid fd
            Op::Syscall(3, 1), // syscall_close
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_close_reserved_fd() {
        // Test closing reserved fd (stdin/stdout/stderr) returns EBADF
        let stack = run_code(vec![
            Op::I64Const(1),   // stdout
            Op::Syscall(3, 1), // syscall_close
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_open_write_close() {
        use std::io::Read;

        // Create a temporary file path
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("moca_test_syscall.txt");
        let path_str = temp_path.to_str().unwrap().to_string();

        // Clean up any existing file
        let _ = std::fs::remove_file(&temp_path);

        // O_WRONLY | O_CREAT | O_TRUNC = 1 | 64 | 512 = 577
        let flags = 1 | 64 | 512;

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 1,
                code: vec![
                    // fd = open(path, flags)
                    Op::StringConst(0),  // path
                    Op::I64Const(flags), // flags
                    Op::Syscall(2, 2),   // syscall_open
                    Op::LocalSet(0),     // store fd in local 0
                    // write(fd, "hello", 5)
                    Op::LocalGet(0),    // fd
                    Op::StringConst(1), // buffer
                    Op::I64Const(5),    // count
                    Op::Syscall(1, 3),  // syscall_write
                    Op::Drop,           // discard write result
                    // close(fd)
                    Op::LocalGet(0),   // fd
                    Op::Syscall(3, 1), // syscall_close
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![path_str.clone(), "hello".to_string()],
            debug: None,
        };

        let mut vm = VM::new();
        let result = vm.run(&chunk);
        assert!(result.is_ok(), "syscall test failed: {:?}", result);

        // Verify file contents
        let mut contents = String::new();
        let mut file = std::fs::File::open(&temp_path).expect("file should exist");
        file.read_to_string(&mut contents).unwrap();
        assert_eq!(contents, "hello");

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_syscall_read_invalid_fd() {
        // Test reading from invalid fd returns EBADF (-1)
        let stack = run_code(vec![
            Op::I64Const(99),  // invalid fd
            Op::I64Const(10),  // count
            Op::Syscall(4, 2), // syscall_read
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_read_reserved_fd() {
        // Test reading from reserved fd (stdout) returns EBADF
        let stack = run_code(vec![
            Op::I64Const(1),   // stdout
            Op::I64Const(10),  // count
            Op::Syscall(4, 2), // syscall_read
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_read_from_file() {
        use std::io::Write;

        // Create a temporary file with content
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("moca_test_read.txt");
        let path_str = temp_path.to_str().unwrap().to_string();

        // Write content to file using Rust
        {
            let mut file = std::fs::File::create(&temp_path).unwrap();
            file.write_all(b"hello world").unwrap();
        }

        // O_RDONLY = 0
        let flags = 0i64;

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 2,
                code: vec![
                    // fd = open(path, O_RDONLY)
                    Op::StringConst(0),  // path
                    Op::I64Const(flags), // flags
                    Op::Syscall(2, 2),   // syscall_open
                    Op::LocalSet(0),     // store fd at stack[0]
                    // content = read(fd, 100)
                    Op::LocalGet(0),   // push fd from stack[0]
                    Op::I64Const(100), // count
                    Op::Syscall(4, 2), // syscall_read -> returns string ref
                    Op::LocalSet(1),   // store content at stack[1]
                    // close(fd)
                    Op::LocalGet(0),   // push fd
                    Op::Syscall(3, 1), // syscall_close
                    Op::Drop,          // discard close result
                    // return content
                    Op::LocalGet(1), // push content ref
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![path_str.clone()],
            debug: None,
        };

        let mut vm = VM::new();
        let result = vm.run(&chunk);
        assert!(result.is_ok(), "syscall read test failed: {:?}", result);

        // Find the content ref in the stack (last Ref value)
        let content_ref = vm
            .stack
            .iter()
            .rev()
            .find_map(|v| {
                if let Value::Ref(r) = v {
                    Some(*r)
                } else {
                    None
                }
            })
            .expect("Expected to find a Ref value in stack");
        let content = vm.heap.get(content_ref).unwrap().slots_to_string();
        assert_eq!(content, "hello world");

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_syscall_read_partial() {
        use std::io::Write;

        // Create a temporary file with content
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("moca_test_read_partial.txt");
        let path_str = temp_path.to_str().unwrap().to_string();

        // Write content to file using Rust
        {
            let mut file = std::fs::File::create(&temp_path).unwrap();
            file.write_all(b"hello world").unwrap();
        }

        // O_RDONLY = 0
        let flags = 0i64;

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 2,
                code: vec![
                    // fd = open(path, O_RDONLY)
                    Op::StringConst(0),  // path
                    Op::I64Const(flags), // flags
                    Op::Syscall(2, 2),   // syscall_open
                    Op::LocalSet(0),     // store fd at stack[0]
                    // content = read(fd, 5) - only read first 5 bytes
                    Op::LocalGet(0),   // push fd
                    Op::I64Const(5),   // count
                    Op::Syscall(4, 2), // syscall_read -> returns string ref
                    Op::LocalSet(1),   // store content at stack[1]
                    // close(fd)
                    Op::LocalGet(0),   // push fd
                    Op::Syscall(3, 1), // syscall_close
                    Op::Drop,          // discard close result
                    // return content
                    Op::LocalGet(1), // push content ref
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec![path_str.clone()],
            debug: None,
        };

        let mut vm = VM::new();
        let result = vm.run(&chunk);
        assert!(
            result.is_ok(),
            "syscall read partial test failed: {:?}",
            result
        );

        // Find the content ref in the stack (last Ref value)
        let content_ref = vm
            .stack
            .iter()
            .rev()
            .find_map(|v| {
                if let Value::Ref(r) = v {
                    Some(*r)
                } else {
                    None
                }
            })
            .expect("Expected to find a Ref value in stack");
        let content = vm.heap.get(content_ref).unwrap().slots_to_string();
        assert_eq!(content, "hello");

        // Clean up
        let _ = std::fs::remove_file(&temp_path);
    }

    #[test]
    fn test_syscall_socket_valid() {
        // socket(AF_INET=2, SOCK_STREAM=1) should return fd >= 3
        let stack = run_code(vec![
            Op::I64Const(2),   // AF_INET
            Op::I64Const(1),   // SOCK_STREAM
            Op::Syscall(5, 2), // syscall_socket
        ])
        .unwrap();
        assert_eq!(stack.len(), 1);
        let fd = stack[0].as_i64().unwrap();
        assert!(fd >= 3, "socket fd should be >= 3, got {}", fd);
    }

    #[test]
    fn test_syscall_socket_invalid_domain() {
        // socket(999, SOCK_STREAM=1) should return EAFNOSUPPORT (-6)
        let stack = run_code(vec![
            Op::I64Const(999), // Invalid domain
            Op::I64Const(1),   // SOCK_STREAM
            Op::Syscall(5, 2), // syscall_socket
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-6)]); // EAFNOSUPPORT
    }

    #[test]
    fn test_syscall_socket_invalid_type() {
        // socket(AF_INET=2, 999) should return ESOCKTNOSUPPORT (-7)
        let stack = run_code(vec![
            Op::I64Const(2),   // AF_INET
            Op::I64Const(999), // Invalid socket type
            Op::Syscall(5, 2), // syscall_socket
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-7)]); // ESOCKTNOSUPPORT
    }

    #[test]
    fn test_syscall_connect_invalid_fd() {
        // connect(999, "example.com", 80) should return EBADF (-1)
        let stack = run_code_with_strings(
            vec![
                Op::I64Const(999),  // Invalid fd
                Op::StringConst(0), // host
                Op::I64Const(80),   // port
                Op::Syscall(6, 3),  // syscall_connect
            ],
            vec!["example.com".to_string()],
        )
        .unwrap();
        assert_eq!(stack, vec![Value::I64(-1)]); // EBADF
    }

    #[test]
    fn test_syscall_close_pending_socket() {
        // socket() then close() should work
        let stack = run_code(vec![
            Op::I64Const(2),   // AF_INET
            Op::I64Const(1),   // SOCK_STREAM
            Op::Syscall(5, 2), // syscall_socket -> fd
            Op::LocalSet(0),   // store fd
            Op::LocalGet(0),   // push fd
            Op::Syscall(3, 1), // syscall_close
        ])
        .unwrap();
        // Last value should be 0 (success)
        assert!(stack.iter().any(|v| *v == Value::I64(0)));
    }

    #[test]
    fn test_syscall_http_get_local_server() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;

        // Start a simple HTTP server on a random port
        let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind");
        let port = listener.local_addr().unwrap().port();

        let server_handle = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                // Read request
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);

                // Send HTTP response
                let response =
                    "HTTP/1.0 200 OK\r\nContent-Type: text/plain\r\n\r\nHello from test server!";
                let _ = stream.write_all(response.as_bytes());
            }
        });

        // Give server time to start
        thread::sleep(std::time::Duration::from_millis(50));

        // E2E test: socket -> connect -> write(HTTP GET) -> read -> close
        let http_request = format!("GET / HTTP/1.0\r\nHost: 127.0.0.1:{}\r\n\r\n", port);
        let request_len = http_request.len() as i64;

        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 2,
                code: vec![
                    // fd = socket(AF_INET=2, SOCK_STREAM=1)
                    Op::I64Const(2),   // AF_INET
                    Op::I64Const(1),   // SOCK_STREAM
                    Op::Syscall(5, 2), // syscall_socket
                    Op::LocalSet(0),   // store fd at local 0
                    // connect(fd, "127.0.0.1", port)
                    Op::LocalGet(0),           // push fd
                    Op::StringConst(0),        // host = "127.0.0.1"
                    Op::I64Const(port as i64), // port
                    Op::Syscall(6, 3),         // syscall_connect
                    Op::Drop,                  // discard connect result
                    // write(fd, request, len)
                    Op::LocalGet(0),           // push fd
                    Op::StringConst(1),        // request string
                    Op::I64Const(request_len), // count
                    Op::Syscall(1, 3),         // syscall_write
                    Op::Drop,                  // discard write result
                    // response = read(fd, 4096)
                    Op::LocalGet(0),    // push fd
                    Op::I64Const(4096), // count
                    Op::Syscall(4, 2),  // syscall_read
                    Op::LocalSet(1),    // store response at local 1
                    // close(fd)
                    Op::LocalGet(0),   // push fd
                    Op::Syscall(3, 1), // syscall_close
                    Op::Drop,          // discard close result
                    // return response
                    Op::LocalGet(1), // push response ref
                ],
                stackmap: None,
                local_types: vec![],
            },
            strings: vec!["127.0.0.1".to_string(), http_request],
            debug: None,
        };

        let mut vm = VM::new();
        let result = vm.run(&chunk);
        assert!(result.is_ok(), "HTTP GET test failed: {:?}", result);

        // Find the response ref in the stack
        let response_ref = vm
            .stack
            .iter()
            .rev()
            .find_map(|v| {
                if let Value::Ref(r) = v {
                    Some(*r)
                } else {
                    None
                }
            })
            .expect("Expected to find a Ref value in stack");

        let response = vm.heap.get(response_ref).unwrap().slots_to_string();

        // Response should contain HTTP status and our test message
        assert!(
            response.contains("HTTP/1.0 200 OK"),
            "Response should contain HTTP status: {}",
            response
        );
        assert!(
            response.contains("Hello from test server!"),
            "Response should contain test message: {}",
            response
        );

        // Wait for server thread to finish
        let _ = server_handle.join();
    }
}
