use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;

use crate::vm::threads::{Channel, ThreadSpawner};
use crate::vm::{Chunk, Function, GcRef, Heap, Op, Value, ValueType};

#[cfg(all(target_arch = "aarch64", feature = "jit"))]
use crate::jit::compiler::{CompiledCode, CompiledLoop};
#[cfg(all(target_arch = "aarch64", feature = "jit"))]
use crate::jit::compiler_microop::MicroOpJitCompiler;
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
use crate::jit::compiler_microop_x86_64::MicroOpJitCompiler;
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
use crate::jit::compiler_x86_64::{CompiledCode, CompiledLoop};
#[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
use crate::jit::function_table::JitFunctionTable;
#[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
use crate::jit::marshal::{JitCallContext, JitReturn, JitValue};

/// A call frame for the VM.
#[derive(Debug)]
struct Frame {
    /// Index into the function table (usize::MAX for main)
    func_index: usize,
    /// Program counter
    pc: usize,
    /// Base index into the stack for locals
    stack_base: usize,
    /// For MicroOp interpreter: caller's vreg index for return value.
    /// None for the old interpreter or when return value is not captured.
    ret_vreg: Option<usize>,
    /// Minimum valid stack length for this frame (= stack_base + locals + temps).
    /// Pops below this are "stack underflow".
    stack_floor: usize,
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
    /// Function table for JIT direct call dispatch
    #[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
    jit_function_table: JitFunctionTable,
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
    /// Whether to use the MicroOp interpreter instead of the stack-based interpreter
    use_microop: bool,
    /// Pre-allocated type descriptor heap references (indexed by type descriptor table index)
    type_descriptor_refs: Vec<GcRef>,
    /// Pre-allocated interface descriptor heap references (indexed by interface descriptor table index)
    interface_descriptor_refs: Vec<GcRef>,
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
            #[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
            jit_function_table: JitFunctionTable::new(0),
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
            use_microop: true,
            type_descriptor_refs: Vec::new(),
            interface_descriptor_refs: Vec::new(),
        }
    }

    /// Pre-allocate interface descriptor heap objects from the chunk's interface descriptor table.
    /// Layout: [iface_name, method_count]
    fn init_interface_descriptors(&mut self, chunk: &Chunk) -> Result<(), String> {
        self.interface_descriptor_refs = Vec::with_capacity(chunk.interface_descriptors.len());
        for id in &chunk.interface_descriptors {
            let name_ref = self.heap.alloc_string(id.name.clone())?;
            let slots = vec![
                Value::Ref(name_ref),                     // slot 0: iface_name
                Value::I64(id.method_names.len() as i64), // slot 1: method_count
            ];
            let gc_ref = self.heap.alloc_slots(slots)?;
            self.interface_descriptor_refs.push(gc_ref);
        }
        Ok(())
    }

    /// Pre-allocate type descriptor heap objects from the chunk's type descriptor table.
    /// Layout: [tag_id, type_name, field_count, ...field_names, ...field_type_desc_refs,
    ///          aux_count, ...aux_type_desc_refs,
    ///          vtable_count, ...iface_desc_ref, vtable_ref pairs]
    fn init_type_descriptors(&mut self, chunk: &Chunk) -> Result<(), String> {
        // Build tag_name -> index map for resolving field type references
        let tag_to_idx: std::collections::HashMap<&str, usize> = chunk
            .type_descriptors
            .iter()
            .enumerate()
            .map(|(i, td)| (td.tag_name.as_str(), i))
            .collect();

        // Pass 1: Allocate all type descriptor heap objects with placeholder nils for type refs
        self.type_descriptor_refs = Vec::with_capacity(chunk.type_descriptors.len());
        for td in &chunk.type_descriptors {
            let n = td.field_names.len();
            let m = td.aux_type_tags.len();
            let v = td.vtables.len();
            // field_names + field_type_desc_refs + aux_count + aux_type_desc_refs
            // + vtable_count + (iface_desc_ref + vtable_ref) * v
            let type_info_slots = 3 + n + n + 1 + m + 1 + 2 * v;
            let mut slots = Vec::with_capacity(type_info_slots);

            // slot 0: tag_id (string pool index as i64)
            let tag_idx = chunk
                .strings
                .iter()
                .position(|s| s == &td.tag_name)
                .unwrap_or(0);
            slots.push(Value::I64(tag_idx as i64));

            // slot 1: type_name (allocated string)
            let name_ref = self.heap.alloc_string(td.tag_name.clone())?;
            slots.push(Value::Ref(name_ref));

            // slot 2: field_count
            slots.push(Value::I64(n as i64));

            // slot 3..3+n: field_names (allocated strings)
            for field_name in &td.field_names {
                let field_ref = self.heap.alloc_string(field_name.clone())?;
                slots.push(Value::Ref(field_ref));
            }

            // slot 3+n..3+2n: placeholder nils for field type descriptor refs
            for _ in 0..n {
                slots.push(Value::Null);
            }

            // slot 3+2n: aux_count
            slots.push(Value::I64(m as i64));

            // slot 3+2n+1..3+2n+1+m: placeholder nils for aux type descriptor refs
            for _ in 0..m {
                slots.push(Value::Null);
            }

            // slot BASE: vtable_count
            slots.push(Value::I64(v as i64));

            // slot BASE+1..: placeholder nils for (iface_desc_ref, vtable_ref) pairs
            for _ in 0..v {
                slots.push(Value::Null); // iface_desc_ref placeholder
                slots.push(Value::Null); // vtable_ref placeholder
            }

            let gc_ref = self.heap.alloc_slots(slots)?;
            self.type_descriptor_refs.push(gc_ref);
        }

        // Pass 2: Fill in field type descriptor refs, aux type descriptor refs, and vtable refs
        for (i, td) in chunk.type_descriptors.iter().enumerate() {
            let n = td.field_names.len();
            let m = td.aux_type_tags.len();
            // Fill field type desc refs
            for (j, ft_tag) in td.field_type_tags.iter().enumerate() {
                if let Some(&ft_idx) = tag_to_idx.get(ft_tag.as_str()) {
                    let ft_ref = self.type_descriptor_refs[ft_idx];
                    let slot_index = 3 + n + j;
                    self.heap.write_slot(
                        self.type_descriptor_refs[i],
                        slot_index,
                        Value::Ref(ft_ref),
                    )?;
                }
            }
            // Fill aux type desc refs
            for (j, aux_tag) in td.aux_type_tags.iter().enumerate() {
                if let Some(&aux_idx) = tag_to_idx.get(aux_tag.as_str()) {
                    let aux_ref = self.type_descriptor_refs[aux_idx];
                    let slot_index = 3 + 2 * n + 1 + j;
                    self.heap.write_slot(
                        self.type_descriptor_refs[i],
                        slot_index,
                        Value::Ref(aux_ref),
                    )?;
                }
            }
            // Fill vtable entries
            let vtable_base = 3 + 2 * n + 1 + m + 1;
            for (j, (iface_idx, func_indices)) in td.vtables.iter().enumerate() {
                // Set iface_desc_ref
                if let Some(&iface_ref) = self.interface_descriptor_refs.get(*iface_idx) {
                    self.heap.write_slot(
                        self.type_descriptor_refs[i],
                        vtable_base + 2 * j,
                        Value::Ref(iface_ref),
                    )?;
                }
                // Allocate vtable heap object with func_indices
                let vtable_slots: Vec<Value> = func_indices
                    .iter()
                    .map(|&fi| Value::I64(fi as i64))
                    .collect();
                let vtable_ref = self.heap.alloc_slots(vtable_slots)?;
                self.heap.write_slot(
                    self.type_descriptor_refs[i],
                    vtable_base + 2 * j + 1,
                    Value::Ref(vtable_ref),
                )?;
            }
        }

        Ok(())
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
    /// Uses MicroOp-based JIT compiler which takes register-based IR as input.
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn jit_compile_function(&mut self, func: &Function, func_index: usize) {
        if self.jit_functions.contains_key(&func_index) {
            return; // Already compiled
        }

        // Convert to MicroOp IR first
        use super::microop_converter;
        let converted = microop_converter::convert(func);

        let compiler = MicroOpJitCompiler::new();
        match compiler.compile(&converted, func.locals_count, func_index) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT/MicroOp] Compiled function '{}' ({} bytes)",
                        func.name,
                        compiled.memory.size()
                    );
                }
                // Update function table with entry point for direct call dispatch
                let entry: unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn =
                    unsafe { compiled.entry_point() };
                self.jit_function_table.update(
                    func_index,
                    entry as usize as u64,
                    compiled.total_regs,
                );
                self.jit_functions.insert(func_index, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!("[JIT/MicroOp] Failed to compile '{}': {}", func.name, e);
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
    /// Uses MicroOp-based JIT compiler which takes register-based IR as input.
    /// Frame layout: unboxed, 8B per VReg slot (payload only).
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn jit_compile_function(&mut self, func: &Function, func_index: usize) {
        if self.jit_functions.contains_key(&func_index) {
            return; // Already compiled
        }

        // Convert to MicroOp IR first
        use super::microop_converter;
        let converted = microop_converter::convert(func);

        let compiler = MicroOpJitCompiler::new();
        match compiler.compile(&converted, func.locals_count, func_index) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT/MicroOp] Compiled function '{}' ({} bytes)",
                        func.name,
                        compiled.memory.size()
                    );
                }
                // Update function table with entry point for direct call dispatch
                let entry: unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn =
                    unsafe { compiled.entry_point() };
                self.jit_function_table.update(
                    func_index,
                    entry as usize as u64,
                    compiled.total_regs,
                );
                self.jit_functions.insert(func_index, compiled);
                self.jit_compile_count += 1;
            }
            Err(e) => {
                if self.trace_jit {
                    eprintln!("[JIT/MicroOp] Failed to compile '{}': {}", func.name, e);
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
    /// Uses MicroOp-based JIT compiler which takes register-based IR as input.
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

        // Convert to MicroOp IR
        use super::microop_converter;
        let converted = microop_converter::convert(func);

        // Map Op PCs to MicroOp PCs
        let loop_start_microop = converted.pc_map[loop_start_pc];
        let loop_end_microop = converted.pc_map[loop_end_pc];

        if self.trace_jit {
            eprintln!(
                "[JIT/MicroOp] Hot loop detected in '{}' at Op PC {}..{} (MicroOp PC {}..{}, iterations: {})",
                func.name,
                loop_start_pc,
                loop_end_pc,
                loop_start_microop,
                loop_end_microop,
                self.jit_threshold
            );
        }

        let compiler = MicroOpJitCompiler::new();
        match compiler.compile_loop(
            &converted,
            func.locals_count,
            func_index,
            loop_start_microop,
            loop_end_microop,
            loop_start_pc,
            loop_end_pc,
        ) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT/MicroOp] Compiled loop in '{}' Op PC {}..{} ({} bytes)",
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
                        "[JIT/MicroOp] Failed to compile loop in '{}' Op PC {}..{}: {}",
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

        // Convert to MicroOp IR
        use super::microop_converter;
        let converted = microop_converter::convert(func);

        // Map Op PCs to MicroOp PCs
        let loop_start_microop = converted.pc_map[loop_start_pc];
        let loop_end_microop = converted.pc_map[loop_end_pc];

        if self.trace_jit {
            eprintln!(
                "[JIT/MicroOp] Hot loop detected in '{}' at Op PC {}..{} (MicroOp PC {}..{}, iterations: {})",
                func.name,
                loop_start_pc,
                loop_end_pc,
                loop_start_microop,
                loop_end_microop,
                self.jit_threshold
            );
        }

        let compiler = MicroOpJitCompiler::new();
        match compiler.compile_loop(
            &converted,
            func.locals_count,
            func_index,
            loop_start_microop,
            loop_end_microop,
            loop_start_pc,
            loop_end_pc,
        ) {
            Ok(compiled) => {
                if self.trace_jit {
                    eprintln!(
                        "[JIT/MicroOp] Compiled loop in '{}' Op PC {}..{} ({} bytes)",
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
                        "[JIT/MicroOp] Failed to compile loop in '{}' Op PC {}..{}: {}",
                        func.name, loop_start_pc, loop_end_pc, e
                    );
                }
            }
        }
    }

    /// Convert a u64 payload back to a VM Value using type information.
    #[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
    fn payload_to_value(payload: u64, ty: ValueType) -> Value {
        match ty {
            ValueType::I32 => Value::I64(payload as i32 as i64),
            ValueType::I64 => Value::I64(payload as i64),
            ValueType::F32 | ValueType::F64 => Value::F64(f64::from_bits(payload)),
            ValueType::Ref => {
                if payload == 0 {
                    Value::Null
                } else {
                    Value::Ref(GcRef {
                        index: payload as usize,
                    })
                }
            }
        }
    }

    /// Execute a JIT compiled loop (x86-64 with jit feature only).
    /// MicroOp JIT uses unboxed frames (8B/slot, payload only).
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

        let (entry, loop_end, total_regs): (
            unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn,
            usize,
            usize,
        ) = {
            let compiled = self.jit_loops.get(&key).unwrap();
            (
                unsafe { compiled.entry_point() },
                compiled.loop_end_pc,
                compiled.total_regs,
            )
        };

        let frame_regs = if total_regs > 0 {
            total_regs
        } else {
            func.locals_count
        };

        // Allocate frame with shadow tag space (payload + shadow, 8B per slot each)
        let mut jit_frame = vec![0u64; frame_regs * 2];

        // Copy VRegs from VM stack to JIT frame (payload only)
        let vm_frame = self.frames.last().unwrap();
        let stack_base = vm_frame.stack_base;
        for (i, slot) in jit_frame
            .iter_mut()
            .enumerate()
            .take(frame_regs.min(func.locals_count))
        {
            if stack_base + i < self.stack.len() {
                *slot = JitValue::from_value(&self.stack[stack_base + i]).payload;
            }
        }

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

            heap_alloc_dyn_simple_helper: jit_heap_alloc_dyn_simple_helper,
            // heap_alloc_typed_helper removed
            jit_function_table: self.jit_function_table.base_ptr(),
        };

        let _result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                jit_frame.as_mut_ptr(),
                jit_frame.as_mut_ptr(), // unused
            )
        };

        if self.trace_jit {
            eprintln!("[JIT] Executed loop in '{}' PC ..{}", func.name, loop_end);
        }

        // Copy locals back from JIT frame to VM stack using type info
        let vm_frame = self.frames.last().unwrap();
        let stack_base = vm_frame.stack_base;
        for (i, &payload) in jit_frame.iter().enumerate().take(func.locals_count) {
            if stack_base + i < self.stack.len() {
                let ty = func.local_types.get(i).copied().unwrap_or(ValueType::I64);
                self.stack[stack_base + i] = Self::payload_to_value(payload, ty);
            }
        }

        Ok(loop_end + 1)
    }

    /// Execute a JIT compiled loop (AArch64 with jit feature only).
    /// MicroOp JIT uses unboxed frames (8B/slot, payload only).
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn execute_jit_loop(
        &mut self,
        func_index: usize,
        loop_end_pc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<usize, String> {
        let key = (func_index, loop_end_pc);

        let (entry, loop_end, total_regs): (
            unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn,
            usize,
            usize,
        ) = {
            let compiled = self.jit_loops.get(&key).unwrap();
            (
                unsafe { compiled.entry_point() },
                compiled.loop_end_pc,
                compiled.total_regs,
            )
        };

        let frame_size = if total_regs > 0 {
            total_regs
        } else {
            func.locals_count
        };

        // Allocate frame: payload + shadow tags (8B per slot, doubled for shadow area)
        let mut jit_frame = vec![0u64; frame_size * 2];

        // Copy VRegs from VM stack to JIT frame (payload only)
        let vm_frame = self.frames.last().unwrap();
        let stack_base = vm_frame.stack_base;
        for (i, slot) in jit_frame
            .iter_mut()
            .enumerate()
            .take(frame_size.min(func.locals_count))
        {
            if stack_base + i < self.stack.len() {
                *slot = JitValue::from_value(&self.stack[stack_base + i]).payload;
            }
        }

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

            heap_alloc_dyn_simple_helper: jit_heap_alloc_dyn_simple_helper,
            // heap_alloc_typed_helper removed
            jit_function_table: self.jit_function_table.base_ptr(),
        };

        let _result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                jit_frame.as_mut_ptr(),
                jit_frame.as_mut_ptr(), // unused
            )
        };

        if self.trace_jit {
            eprintln!("[JIT] Executed loop in '{}' PC ..{}", func.name, loop_end);
        }

        // Copy locals back from JIT frame to VM stack using type info
        let vm_frame = self.frames.last().unwrap();
        let stack_base = vm_frame.stack_base;
        for (i, &payload) in jit_frame.iter().enumerate().take(func.locals_count) {
            if stack_base + i < self.stack.len() {
                let ty = func.local_types.get(i).copied().unwrap_or(ValueType::I64);
                self.stack[stack_base + i] = Self::payload_to_value(payload, ty);
            }
        }

        Ok(loop_end + 1)
    }

    /// Execute a JIT compiled function (x86-64 with jit feature only).
    ///
    /// MicroOp-based JIT: RDI=ctx, RSI=frame_base (u64 payload array, 8B/slot), RDX=unused.
    /// Frame layout: VReg(0..locals_count) = locals, VReg(locals_count..) = temps.
    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
    fn execute_jit_function(
        &mut self,
        func_index: usize,
        argc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<Value, String> {
        // Get the entry point and total_regs to avoid borrow conflicts
        let (entry, total_regs): (
            unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn,
            usize,
        ) = {
            let compiled = self.jit_functions.get(&func_index).unwrap();
            (unsafe { compiled.entry_point() }, compiled.total_regs)
        };

        // Allocate frame: total_regs * 2 slots (payload + shadow tags, 8 bytes per slot)
        let frame_regs = if total_regs > 0 {
            total_regs
        } else {
            func.locals_count
        };
        let mut frame = vec![0u64; frame_regs * 2];

        // Pop arguments from VM stack and write payloads to frame
        let args: Vec<Value> = (0..argc)
            .map(|_| self.stack.pop().unwrap())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        for (i, arg) in args.iter().enumerate() {
            frame[i] = JitValue::from_value(arg).payload;
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

            heap_alloc_dyn_simple_helper: jit_heap_alloc_dyn_simple_helper,
            // heap_alloc_typed_helper removed
            jit_function_table: self.jit_function_table.base_ptr(),
        };

        // Execute the JIT code
        let result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                frame.as_mut_ptr(),
                frame.as_mut_ptr(), // unused
            )
        };

        if self.trace_jit {
            eprintln!(
                "[JIT] Executed function '{}', result: tag={}, payload={}",
                func.name, result.tag, result.payload
            );
        }

        // Convert return value to VM Value (tag+payload from return registers)
        Ok(result.to_value())
    }

    /// Execute a JIT compiled function (AArch64 with jit feature only).
    ///
    /// MicroOp-based JIT: x0=ctx, x1=frame_base (u64 payload array, 8B/slot), x2=unused.
    /// Frame layout: VReg(0..locals_count) = locals, VReg(locals_count..) = temps.
    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
    fn execute_jit_function(
        &mut self,
        func_index: usize,
        argc: usize,
        func: &Function,
        chunk: &Chunk,
    ) -> Result<Value, String> {
        // Get the entry point and total_regs to avoid borrow conflicts
        let (entry, total_regs): (
            unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn,
            usize,
        ) = {
            let compiled = self.jit_functions.get(&func_index).unwrap();
            (unsafe { compiled.entry_point() }, compiled.total_regs)
        };

        // Allocate frame: total_regs slots (8 bytes per slot, payload only)
        let frame_size = if total_regs > 0 {
            total_regs
        } else {
            func.locals_count
        };
        let mut frame = vec![0u64; frame_size];

        // Pop arguments from VM stack and write payloads to frame
        let args: Vec<Value> = (0..argc)
            .map(|_| self.stack.pop().unwrap())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        for (i, arg) in args.iter().enumerate() {
            frame[i] = JitValue::from_value(arg).payload;
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

            heap_alloc_dyn_simple_helper: jit_heap_alloc_dyn_simple_helper,
            // heap_alloc_typed_helper removed
            jit_function_table: self.jit_function_table.base_ptr(),
        };

        // Execute the JIT code
        let result: JitReturn = unsafe {
            entry(
                &mut call_ctx as *mut JitCallContext as *mut u8,
                frame.as_mut_ptr(),
                frame.as_mut_ptr(), // unused
            )
        };

        if self.trace_jit {
            eprintln!(
                "[JIT] Executed function '{}', result: tag={}, payload={}",
                func.name, result.tag, result.payload
            );
        }

        // Convert return value to VM Value (tag+payload from return registers)
        Ok(result.to_value())
    }

    /// Get the number of JIT compilations performed.
    pub fn jit_compile_count(&self) -> usize {
        self.jit_compile_count
    }

    /// Enable or disable the MicroOp interpreter.
    pub fn set_use_microop(&mut self, enabled: bool) {
        self.use_microop = enabled;
    }

    pub fn run(&mut self, chunk: &Chunk) -> Result<(), String> {
        if self.use_microop {
            return self.run_microop(chunk);
        }

        // Initialize call counts for JIT
        self.init_call_counts(chunk);
        // Initialize string constant cache
        self.init_string_cache(chunk);
        // Initialize interface and type descriptor heap objects
        self.init_interface_descriptors(chunk)?;
        self.init_type_descriptors(chunk)?;
        // Initialize JIT function table
        #[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
        {
            self.jit_function_table = JitFunctionTable::new(chunk.functions.len());
        }

        // Start with main
        self.frames.push(Frame {
            func_index: usize::MAX, // Marker for main
            pc: 0,
            stack_base: 0,
            ret_vreg: None,
            stack_floor: 0,
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
        // Initialize interface and type descriptors for this chunk
        self.init_interface_descriptors(chunk)?;
        self.init_type_descriptors(chunk)?;

        // Start with main
        self.frames.push(Frame {
            func_index: usize::MAX, // Marker for main
            pc: 0,
            stack_base: 0,
            ret_vreg: None,
            stack_floor: 0,
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

    /// Run the VM using the MicroOp interpreter.
    ///
    /// Converts each function's Op bytecode to MicroOps lazily (on first call),
    /// caches the result, and executes using register-based MicroOps with
    /// Raw fallback for unconverted operations.
    fn run_microop(&mut self, chunk: &Chunk) -> Result<(), String> {
        use super::microop::{CmpCond, ConvertedFunction, MicroOp};
        use super::microop_converter;

        // Initialize (same as run())
        self.init_call_counts(chunk);
        self.init_string_cache(chunk);
        self.init_interface_descriptors(chunk)?;
        self.init_type_descriptors(chunk)?;
        #[cfg(all(any(target_arch = "aarch64", target_arch = "x86_64"), feature = "jit"))]
        {
            self.jit_function_table = JitFunctionTable::new(chunk.functions.len());
        }

        // Lazy conversion cache: indexed by func_index
        let mut func_cache: Vec<Option<ConvertedFunction>> = vec![None; chunk.functions.len()];
        let main_converted = microop_converter::convert(&chunk.main);

        // Push main frame with register file space
        let main_regs = chunk.main.locals_count + main_converted.temps_count;
        self.frames.push(Frame {
            func_index: usize::MAX,
            pc: 0,
            stack_base: 0,
            ret_vreg: None,
            stack_floor: main_regs,
        });
        self.stack.resize(main_regs, Value::Null);

        loop {
            // GC check
            if self.heap.should_gc() {
                self.collect_garbage();
            }

            // Get current frame info
            let func_index = self.frames.last().unwrap().func_index;
            let pc = self.frames.last().unwrap().pc;

            // Get converted function
            let converted = if func_index == usize::MAX {
                &main_converted
            } else {
                func_cache[func_index]
                    .get_or_insert_with(|| microop_converter::convert(&chunk.functions[func_index]))
            };

            // Check for end of code
            if pc >= converted.micro_ops.len() {
                break;
            }

            // Fetch and advance PC
            let mop = converted.micro_ops[pc].clone();
            self.frames.last_mut().unwrap().pc = pc + 1;

            // Dispatch
            match mop {
                MicroOp::Jmp {
                    target,
                    old_pc,
                    old_target,
                } => {
                    // Detect backward branch (loop) for JIT
                    if old_target < old_pc {
                        let key = (func_index, old_pc);
                        let count = self.loop_counts.entry(key).or_insert(0);
                        *count += 1;

                        let loop_start_pc = old_target;
                        let loop_end_pc = old_pc;

                        #[cfg(all(target_arch = "x86_64", feature = "jit"))]
                        {
                            if self.should_jit_compile_loop(func_index, old_pc) {
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
                            if self.should_jit_compile_loop(func_index, old_pc) {
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
                            if self.is_loop_jit_compiled(func_index, old_pc) {
                                let func = if func_index == usize::MAX {
                                    &chunk.main
                                } else {
                                    &chunk.functions[func_index]
                                };
                                let next_old_pc =
                                    self.execute_jit_loop(func_index, old_pc, func, chunk)?;
                                // Map returned Op PC back to MicroOp PC
                                self.frames.last_mut().unwrap().pc = converted.pc_map[next_old_pc];
                                continue;
                            }
                        }

                        #[cfg(all(target_arch = "aarch64", feature = "jit"))]
                        {
                            if self.is_loop_jit_compiled(func_index, old_pc) {
                                let func = if func_index == usize::MAX {
                                    &chunk.main
                                } else {
                                    &chunk.functions[func_index]
                                };
                                let next_old_pc =
                                    self.execute_jit_loop(func_index, old_pc, func, chunk)?;
                                // Map returned Op PC back to MicroOp PC
                                self.frames.last_mut().unwrap().pc = converted.pc_map[next_old_pc];
                                continue;
                            }
                        }
                    }

                    self.frames.last_mut().unwrap().pc = target;
                }
                MicroOp::BrIf { cond, target } => {
                    let frame = self.frames.last().unwrap();
                    let val = self.stack[frame.stack_base + cond.0];
                    if val.is_truthy() {
                        self.frames.last_mut().unwrap().pc = target;
                    }
                }
                MicroOp::BrIfFalse { cond, target } => {
                    let frame = self.frames.last().unwrap();
                    let val = self.stack[frame.stack_base + cond.0];
                    if !val.is_truthy() {
                        self.frames.last_mut().unwrap().pc = target;
                    }
                }
                MicroOp::Call {
                    func_id,
                    ref args,
                    ret,
                } => {
                    let callee_func = &chunk.functions[func_id];
                    let caller_stack_base = self.frames.last().unwrap().stack_base;

                    // JIT path: compile and execute hot functions via JIT
                    #[cfg(all(target_arch = "x86_64", feature = "jit"))]
                    {
                        if self.should_jit_compile(func_id, &callee_func.name) {
                            self.jit_compile_function(callee_func, func_id);
                        }
                        if self.is_jit_compiled(func_id) {
                            // Push args onto operand stack for JIT (it pops them)
                            for arg in args.iter() {
                                self.stack.push(self.stack[caller_stack_base + arg.0]);
                            }
                            let result =
                                self.execute_jit_function(func_id, args.len(), callee_func, chunk)?;
                            // Store return value in caller's ret vreg
                            if let Some(ret_v) = ret {
                                let sb = self.frames.last().unwrap().stack_base;
                                self.stack[sb + ret_v.0] = result;
                            }
                            continue;
                        }
                    }
                    #[cfg(all(target_arch = "aarch64", feature = "jit"))]
                    {
                        if self.should_jit_compile(func_id, &callee_func.name) {
                            self.jit_compile_function(callee_func, func_id);
                        }
                        if self.is_jit_compiled(func_id) {
                            for arg in args.iter() {
                                self.stack.push(self.stack[caller_stack_base + arg.0]);
                            }
                            let result =
                                self.execute_jit_function(func_id, args.len(), callee_func, chunk)?;
                            if let Some(ret_v) = ret {
                                let sb = self.frames.last().unwrap().stack_base;
                                self.stack[sb + ret_v.0] = result;
                            }
                            continue;
                        }
                    }

                    // MicroOp interpreter path
                    if func_cache[func_id].is_none() {
                        func_cache[func_id] =
                            Some(microop_converter::convert(&chunk.functions[func_id]));
                    }
                    let callee_temps = func_cache[func_id].as_ref().unwrap().temps_count;
                    let callee_regs = callee_func.locals_count + callee_temps;

                    let new_stack_base = self.stack.len();

                    // Allocate register file for callee
                    self.stack.resize(new_stack_base + callee_regs, Value::Null);

                    // Copy args from caller vregs to callee locals
                    for (i, arg) in args.iter().enumerate() {
                        self.stack[new_stack_base + i] = self.stack[caller_stack_base + arg.0];
                    }

                    // Push callee frame
                    self.frames.push(Frame {
                        func_index: func_id,
                        pc: 0,
                        stack_base: new_stack_base,
                        ret_vreg: ret.map(|v| v.0),
                        stack_floor: new_stack_base + callee_regs,
                    });
                }
                MicroOp::Ret { src } => {
                    // Get return value
                    let return_value = match src {
                        Some(vreg) => {
                            let frame = self.frames.last().unwrap();
                            self.stack
                                .get(frame.stack_base + vreg.0)
                                .copied()
                                .unwrap_or(Value::Null)
                        }
                        None => Value::Null,
                    };

                    // Pop callee frame
                    let callee_frame = self.frames.pop().unwrap();

                    if self.frames.is_empty() {
                        // Main returned
                        self.stack.push(return_value);
                        break;
                    }

                    // Truncate stack (remove callee's data)
                    self.stack.truncate(callee_frame.stack_base);

                    // Store return value in caller's ret vreg
                    if let Some(ret_vreg_idx) = callee_frame.ret_vreg {
                        let caller_stack_base = self.frames.last().unwrap().stack_base;
                        self.stack[caller_stack_base + ret_vreg_idx] = return_value;
                    }
                }
                // ========================================
                // Indirect call (register-based)
                // ========================================
                MicroOp::CallIndirect {
                    callee,
                    ref args,
                    ret,
                } => {
                    let caller_stack_base = self.frames.last().unwrap().stack_base;
                    let closure_val = self.stack[caller_stack_base + callee.0];
                    let closure_ref = closure_val
                        .as_ref()
                        .ok_or("runtime error: CallIndirect expects a callable reference")?;

                    let closure_obj = self
                        .heap
                        .get(closure_ref)
                        .ok_or("runtime error: invalid callable reference")?;

                    let func_index = closure_obj.slots[0]
                        .as_i64()
                        .ok_or("runtime error: callable slot 0 must be func_index")?
                        as usize;

                    let callee_func = &chunk.functions[func_index];

                    // Convert and cache if needed
                    if func_cache.len() <= func_index {
                        func_cache.resize(func_index + 1, None);
                    }
                    if func_cache[func_index].is_none() {
                        func_cache[func_index] =
                            Some(microop_converter::convert(&chunk.functions[func_index]));
                    }
                    let callee_temps = func_cache[func_index].as_ref().unwrap().temps_count;
                    let callee_regs = callee_func.locals_count + callee_temps;

                    let new_stack_base = self.stack.len();

                    // Allocate register file for callee
                    self.stack.resize(new_stack_base + callee_regs, Value::Null);

                    // Slot 0: closure_ref, slots 1..: user args
                    self.stack[new_stack_base] = closure_val;
                    for (i, arg) in args.iter().enumerate() {
                        self.stack[new_stack_base + 1 + i] = self.stack[caller_stack_base + arg.0];
                    }

                    self.frames.push(Frame {
                        func_index,
                        pc: 0,
                        stack_base: new_stack_base,
                        ret_vreg: ret.map(|v| v.0),
                        stack_floor: new_stack_base + callee_regs,
                    });
                }

                // ========================================
                // Dynamic call by func_index (register-based)
                // ========================================
                MicroOp::CallDynamic {
                    func_idx,
                    ref args,
                    ret,
                } => {
                    let caller_stack_base = self.frames.last().unwrap().stack_base;
                    let func_index = self.stack[caller_stack_base + func_idx.0]
                        .as_i64()
                        .ok_or("runtime error: CallDynamic expects func_index as integer")?
                        as usize;

                    let callee_func = &chunk.functions[func_index];

                    // Convert and cache if needed
                    if func_cache.len() <= func_index {
                        func_cache.resize(func_index + 1, None);
                    }
                    if func_cache[func_index].is_none() {
                        func_cache[func_index] =
                            Some(microop_converter::convert(&chunk.functions[func_index]));
                    }
                    let callee_temps = func_cache[func_index].as_ref().unwrap().temps_count;
                    let callee_regs = callee_func.locals_count + callee_temps;

                    let new_stack_base = self.stack.len();

                    // Allocate register file for callee
                    self.stack.resize(new_stack_base + callee_regs, Value::Null);

                    // Copy args directly (no closure_ref prepended)
                    for (i, arg) in args.iter().enumerate() {
                        self.stack[new_stack_base + i] = self.stack[caller_stack_base + arg.0];
                    }

                    self.frames.push(Frame {
                        func_index,
                        pc: 0,
                        stack_base: new_stack_base,
                        ret_vreg: ret.map(|v| v.0),
                        stack_floor: new_stack_base + callee_regs,
                    });
                }

                // ========================================
                // Heap operations (register-based)
                // ========================================
                MicroOp::HeapLoad { dst, src, offset } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let r = self.stack[sb + src.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    let value = self.heap.read_slot(r, offset).ok_or_else(|| {
                        format!("runtime error: slot index {} out of bounds", offset)
                    })?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = value;
                }
                MicroOp::HeapLoadDyn { dst, obj, idx } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let index = self.stack[sb + idx.0]
                        .as_i64()
                        .ok_or("runtime error: expected integer index")?;
                    let r = self.stack[sb + obj.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    if index < 0 {
                        return Err(format!("runtime error: slot index {} out of bounds", index));
                    }
                    let value = self.heap.read_slot(r, index as usize).ok_or_else(|| {
                        format!("runtime error: slot index {} out of bounds", index)
                    })?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = value;
                }
                MicroOp::HeapStore {
                    dst_obj,
                    offset,
                    src,
                } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let value = self.stack[sb + src.0];
                    let r = self.stack[sb + dst_obj.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    self.heap.write_slot(r, offset, value).map_err(|e| {
                        format!("runtime error: slot index {} out of bounds ({})", offset, e)
                    })?;
                }
                MicroOp::HeapStoreDyn { obj, idx, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let value = self.stack[sb + src.0];
                    let index = self.stack[sb + idx.0]
                        .as_i64()
                        .ok_or("runtime error: expected integer index")?;
                    let r = self.stack[sb + obj.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    if index < 0 {
                        return Err(format!("runtime error: slot index {} out of bounds", index));
                    }
                    self.heap
                        .write_slot(r, index as usize, value)
                        .map_err(|e| {
                            format!("runtime error: slot index {} out of bounds ({})", index, e)
                        })?;
                }
                MicroOp::HeapLoad2 { dst, obj, idx } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let index = self.stack[sb + idx.0]
                        .as_i64()
                        .ok_or("runtime error: expected integer index")?;
                    let r = self.stack[sb + obj.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    let ptr_val = self
                        .heap
                        .read_slot(r, 0)
                        .ok_or_else(|| "runtime error: slot index 0 out of bounds".to_string())?;
                    let ptr_ref = ptr_val
                        .as_ref()
                        .ok_or("runtime error: expected reference for ptr field")?;
                    if index < 0 {
                        return Err(format!("runtime error: slot index {} out of bounds", index));
                    }
                    let value = self
                        .heap
                        .read_slot(ptr_ref, index as usize)
                        .ok_or_else(|| {
                            format!("runtime error: slot index {} out of bounds", index)
                        })?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = value;
                }
                MicroOp::HeapStore2 { obj, idx, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let value = self.stack[sb + src.0];
                    let index = self.stack[sb + idx.0]
                        .as_i64()
                        .ok_or("runtime error: expected integer index")?;
                    let r = self.stack[sb + obj.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    let ptr_val = self
                        .heap
                        .read_slot(r, 0)
                        .ok_or_else(|| "runtime error: slot index 0 out of bounds".to_string())?;
                    let ptr_ref = ptr_val
                        .as_ref()
                        .ok_or("runtime error: expected reference for ptr field")?;
                    if index < 0 {
                        return Err(format!("runtime error: slot index {} out of bounds", index));
                    }
                    self.heap
                        .write_slot(ptr_ref, index as usize, value)
                        .map_err(|e| {
                            format!("runtime error: slot index {} out of bounds ({})", index, e)
                        })?;
                }
                MicroOp::HeapOffsetRef { dst, src, offset } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let r = self.stack[sb + src.0]
                        .as_ref()
                        .ok_or("runtime error: expected reference")?;
                    let n = self.stack[sb + offset.0]
                        .as_i64()
                        .ok_or("runtime error: expected integer offset")?;
                    if n < 0 {
                        return Err(format!(
                            "runtime error: negative offset {} for HeapOffsetRef",
                            n
                        ));
                    }
                    let new_ref = r.with_added_slot_offset(n as usize);
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Ref(new_ref);
                }

                MicroOp::StackPush { src } => {
                    let frame = self.frames.last().unwrap();
                    let val = self.stack[frame.stack_base + src.0];
                    self.stack.push(val);
                }
                MicroOp::StackPop { dst } => {
                    let val = self.pop_operand()?;
                    let frame = self.frames.last().unwrap();
                    let idx = frame.stack_base + dst.0;
                    // Ensure register file slot exists
                    while self.stack.len() <= idx {
                        self.stack.push(Value::Null);
                    }
                    self.stack[idx] = val;
                }
                MicroOp::StringConst { dst, idx } => {
                    let r = self.get_or_alloc_string(idx, chunk)?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Ref(r);
                }
                MicroOp::TypeDescLoad { dst, idx } => {
                    let gc_ref = self
                        .type_descriptor_refs
                        .get(idx)
                        .ok_or_else(|| format!("invalid type descriptor index: {}", idx))?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Ref(*gc_ref);
                }
                MicroOp::InterfaceDescLoad { dst, idx } => {
                    let gc_ref = self
                        .interface_descriptor_refs
                        .get(idx)
                        .ok_or_else(|| format!("invalid interface descriptor index: {}", idx))?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Ref(*gc_ref);
                }
                MicroOp::VtableLookup {
                    dst,
                    type_info,
                    iface_desc,
                } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let ti_ref = self.stack[sb + type_info.0]
                        .as_ref()
                        .ok_or("runtime error: VtableLookup expects type_info reference")?;
                    let iface_ref = self.stack[sb + iface_desc.0]
                        .as_ref()
                        .ok_or("runtime error: VtableLookup expects iface_desc reference")?;
                    let result = self.vtable_lookup(ti_ref, iface_ref)?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = result;
                }
                MicroOp::HeapAlloc { dst, args } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let slots: Vec<Value> = args.iter().map(|a| self.stack[sb + a.0]).collect();
                    let r = self.heap.alloc_slots(slots)?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Ref(r);
                }
                MicroOp::HeapAllocDynSimple { dst, size } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let size_val = self.stack[sb + size.0]
                        .as_i64()
                        .ok_or("runtime error: HeapAllocDynSimple requires integer size")?
                        as usize;
                    let slots = vec![Value::Null; size_val];
                    let r = self.heap.alloc_slots(slots)?;
                    self.stack[sb + dst.0] = Value::Ref(r);
                }
                MicroOp::Raw { op } => {
                    // Profile if enabled
                    if self.profile_opcodes {
                        *self.opcode_profile.counts.entry(op.name()).or_insert(0) += 1;
                    }

                    match self.execute_op(op, chunk) {
                        Ok(ControlFlow::Continue) => {}
                        Ok(_) => {
                            // Control flow ops should never be Raw
                            // (converter ensures this)
                        }
                        Err(e) => {
                            if !self.handle_exception(e.clone(), chunk)? {
                                return Err(e);
                            }
                        }
                    }
                }

                // ========================================
                // Move / Constants
                // ========================================
                MicroOp::Mov { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = self.stack[sb + src.0];
                }
                MicroOp::ConstI64 { dst, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::I64(imm);
                }
                MicroOp::ConstI32 { dst, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let val = match imm {
                        0 => Value::Bool(false),
                        1 => Value::Bool(true),
                        _ => Value::I64(imm as i64),
                    };
                    self.stack[sb + dst.0] = val;
                }
                MicroOp::ConstF64 { dst, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::F64(imm);
                }
                MicroOp::ConstF32 { dst, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::F64(imm as f64);
                }
                MicroOp::RefNull { dst } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Null;
                }

                // ========================================
                // i64 ALU
                // ========================================
                MicroOp::AddI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = self.add(va, vb)?;
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = result;
                }
                MicroOp::AddI64Imm { dst, a, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va.wrapping_add(imm));
                }
                MicroOp::SubI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = self.sub(va, vb)?;
                    self.stack[sb + dst.0] = result;
                }
                MicroOp::MulI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = self.mul(va, vb)?;
                    self.stack[sb + dst.0] = result;
                }
                MicroOp::DivI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = self.div(va, vb)?;
                    self.stack[sb + dst.0] = result;
                }
                MicroOp::RemI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    if vb == 0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack[sb + dst.0] = Value::I64(va % vb);
                }
                MicroOp::NegI64 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(-v);
                }
                MicroOp::AndI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va & vb);
                }
                MicroOp::OrI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va | vb);
                }
                MicroOp::XorI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va ^ vb);
                }
                MicroOp::ShlI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va.wrapping_shl(vb as u32 & 63));
                }
                MicroOp::ShlI64Imm { dst, a, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va.wrapping_shl(imm as u32 & 63));
                }
                MicroOp::ShrI64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va >> (vb as u32 & 63));
                }
                MicroOp::ShrI64Imm { dst, a, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(va >> (imm as u32 & 63));
                }
                MicroOp::ShrU64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(((va as u64) >> (vb as u32 & 63)) as i64);
                }
                MicroOp::ShrU64Imm { dst, a, imm } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64(((va as u64) >> (imm as u32 & 63)) as i64);
                }
                MicroOp::UMul128Hi { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")?;
                    let result = ((va as u64 as u128) * (vb as u64 as u128)) >> 64;
                    self.stack[sb + dst.0] = Value::I64(result as i64);
                }

                // ========================================
                // i32 ALU
                // ========================================
                MicroOp::AddI32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::I64((va + vb) as i64);
                }
                MicroOp::SubI32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::I64((va - vb) as i64);
                }
                MicroOp::MulI32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::I64((va * vb) as i64);
                }
                MicroOp::DivI32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    if vb == 0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack[sb + dst.0] = Value::I64((va / vb) as i64);
                }
                MicroOp::RemI32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    if vb == 0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack[sb + dst.0] = Value::I64((va % vb) as i64);
                }
                MicroOp::EqzI32 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0];
                    self.stack[sb + dst.0] = Value::Bool(!v.is_truthy());
                }

                // ========================================
                // f64 ALU
                // ========================================
                MicroOp::AddF64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")?;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::F64(va + vb);
                }
                MicroOp::SubF64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")?;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::F64(va - vb);
                }
                MicroOp::MulF64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")?;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::F64(va * vb);
                }
                MicroOp::DivF64 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")?;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")?;
                    if vb == 0.0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack[sb + dst.0] = Value::F64(va / vb);
                }
                MicroOp::NegF64 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::F64(-v);
                }

                // ========================================
                // f32 ALU
                // ========================================
                MicroOp::AddF32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")? as f32;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::F64((va + vb) as f64);
                }
                MicroOp::SubF32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")? as f32;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::F64((va - vb) as f64);
                }
                MicroOp::MulF32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")? as f32;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::F64((va * vb) as f64);
                }
                MicroOp::DivF32 { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")? as f32;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")? as f32;
                    if vb == 0.0 {
                        return Err("runtime error: division by zero".to_string());
                    }
                    self.stack[sb + dst.0] = Value::F64((va / vb) as f64);
                }
                MicroOp::NegF32 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::F64((-v) as f64);
                }

                // ========================================
                // Comparisons
                // ========================================
                MicroOp::CmpI64 { dst, a, b, cond } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = match cond {
                        CmpCond::Eq => self.values_equal(&va, &vb),
                        CmpCond::Ne => !self.values_equal(&va, &vb),
                        CmpCond::LtS => self.compare(&va, &vb)? < 0,
                        CmpCond::LeS => self.compare(&va, &vb)? <= 0,
                        CmpCond::GtS => self.compare(&va, &vb)? > 0,
                        CmpCond::GeS => self.compare(&va, &vb)? >= 0,
                    };
                    let sb = self.frames.last().unwrap().stack_base;
                    self.stack[sb + dst.0] = Value::Bool(result);
                }
                MicroOp::CmpI64Imm { dst, a, imm, cond } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")?;
                    let result = match cond {
                        CmpCond::Eq => va == imm,
                        CmpCond::Ne => va != imm,
                        CmpCond::LtS => va < imm,
                        CmpCond::LeS => va <= imm,
                        CmpCond::GtS => va > imm,
                        CmpCond::GeS => va >= imm,
                    };
                    self.stack[sb + dst.0] = Value::Bool(result);
                }
                MicroOp::CmpI32 { dst, a, b, cond } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_i64().ok_or("expected integer")? as i32;
                    let vb = self.stack[sb + b.0].as_i64().ok_or("expected integer")? as i32;
                    let result = match cond {
                        CmpCond::Eq => va == vb,
                        CmpCond::Ne => va != vb,
                        CmpCond::LtS => va < vb,
                        CmpCond::LeS => va <= vb,
                        CmpCond::GtS => va > vb,
                        CmpCond::GeS => va >= vb,
                    };
                    self.stack[sb + dst.0] = Value::Bool(result);
                }
                MicroOp::CmpF64 { dst, a, b, cond } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")?;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")?;
                    let result = match cond {
                        CmpCond::Eq => va == vb,
                        CmpCond::Ne => va != vb,
                        CmpCond::LtS => va < vb,
                        CmpCond::LeS => va <= vb,
                        CmpCond::GtS => va > vb,
                        CmpCond::GeS => va >= vb,
                    };
                    self.stack[sb + dst.0] = Value::Bool(result);
                }
                MicroOp::CmpF32 { dst, a, b, cond } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0].as_f64().ok_or("expected float")? as f32;
                    let vb = self.stack[sb + b.0].as_f64().ok_or("expected float")? as f32;
                    let result = match cond {
                        CmpCond::Eq => va == vb,
                        CmpCond::Ne => va != vb,
                        CmpCond::LtS => va < vb,
                        CmpCond::LeS => va <= vb,
                        CmpCond::GtS => va > vb,
                        CmpCond::GeS => va >= vb,
                    };
                    self.stack[sb + dst.0] = Value::Bool(result);
                }

                // ========================================
                // Type Conversions
                // ========================================
                MicroOp::I32WrapI64 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::I64((v as i32) as i64);
                }
                MicroOp::I64ExtendI32S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::I64(v as i64);
                }
                MicroOp::I64ExtendI32U { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::I64((v as u32) as i64);
                }
                MicroOp::F64ConvertI64S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::F64(v as f64);
                }
                MicroOp::I64TruncF64S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::I64(v as i64);
                }
                MicroOp::F64ConvertI32S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::F64(v as f64);
                }
                MicroOp::F32ConvertI32S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")? as i32;
                    self.stack[sb + dst.0] = Value::F64((v as f32) as f64);
                }
                MicroOp::F32ConvertI64S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_i64().ok_or("expected integer")?;
                    self.stack[sb + dst.0] = Value::F64((v as f32) as f64);
                }
                MicroOp::I32TruncF32S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::I64((v as i32) as i64);
                }
                MicroOp::I32TruncF64S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::I64((v as i32) as i64);
                }
                MicroOp::I64TruncF32S { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::I64(v as i64);
                }
                MicroOp::F32DemoteF64 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::F64((v as f32) as f64);
                }
                MicroOp::F64PromoteF32 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0].as_f64().ok_or("expected float")? as f32;
                    self.stack[sb + dst.0] = Value::F64(v as f64);
                }
                MicroOp::F64ReinterpretAsI64 { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let f = self.stack[sb + src.0].as_f64().ok_or("expected float")?;
                    self.stack[sb + dst.0] = Value::I64(i64::from_ne_bytes(f.to_ne_bytes()));
                }

                // ========================================
                // Ref operations
                // ========================================
                MicroOp::RefEq { dst, a, b } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let va = self.stack[sb + a.0];
                    let vb = self.stack[sb + b.0];
                    let result = self.values_equal(&va, &vb);
                    self.stack[sb + dst.0] = Value::Bool(result);
                }
                MicroOp::RefIsNull { dst, src } => {
                    let sb = self.frames.last().unwrap().stack_base;
                    let v = self.stack[sb + src.0];
                    self.stack[sb + dst.0] = Value::Bool(v.is_null());
                }
            }
        }

        Ok(())
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
            Op::I64And => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack.push(Value::I64(a & b));
            }
            Op::I64Or => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack.push(Value::I64(a | b));
            }
            Op::I64Xor => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack.push(Value::I64(a ^ b));
            }
            Op::I64Shl => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack.push(Value::I64(a.wrapping_shl(b as u32 & 63)));
            }
            Op::I64ShrS => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack.push(Value::I64(a >> (b as u32 & 63)));
            }
            Op::I64ShrU => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                self.stack
                    .push(Value::I64(((a as u64) >> (b as u32 & 63)) as i64));
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
            Op::F64ReinterpretAsI64 => {
                let f = self.pop_float()?;
                self.stack
                    .push(Value::I64(i64::from_ne_bytes(f.to_ne_bytes())));
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
                    ret_vreg: None,
                    stack_floor: 0,
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
            Op::UMul128Hi => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                let result = ((a as u64 as u128) * (b as u64 as u128)) >> 64;
                self.stack.push(Value::I64(result as i64));
            }
            Op::TypeOf => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let tag = match value {
                    Value::I64(_) => 0,
                    Value::F64(_) => 1,
                    Value::Bool(_) => 2,
                    Value::Null => 3,
                    Value::Ref(_) => 4,
                };
                self.stack.push(Value::I64(tag));
            }
            Op::HeapSize => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let r = value
                    .as_ref()
                    .ok_or("runtime error: __heap_size expects reference")?;
                let size = self
                    .heap
                    .slot_count(r)
                    .ok_or("runtime error: invalid reference in __heap_size")?;
                self.stack.push(Value::I64(size as i64));
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
                        type_descriptors: chunk_clone.type_descriptors.clone(),
                        interface_descriptors: chunk_clone.interface_descriptors.clone(),
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
            // HeapAllocArray removed — use HeapAlloc instead
            Op::HeapLoad(offset) => {
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let value = self
                    .heap
                    .read_slot(r, offset)
                    .ok_or_else(|| format!("runtime error: slot index {} out of bounds", offset))?;
                self.stack.push(value);
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

                if index < 0 {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                let value = self
                    .heap
                    .read_slot(r, index as usize)
                    .ok_or_else(|| format!("runtime error: slot index {} out of bounds", index))?;
                self.stack.push(value);
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
            Op::HeapLoad2 => {
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let ptr_val = self
                    .heap
                    .read_slot(r, 0)
                    .ok_or("runtime error: slot index 0 out of bounds")?;
                let ptr_ref = ptr_val
                    .as_ref()
                    .ok_or("runtime error: expected reference for ptr field")?;
                if index < 0 {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                let value = self
                    .heap
                    .read_slot(ptr_ref, index as usize)
                    .ok_or_else(|| format!("runtime error: slot index {} out of bounds", index))?;
                self.stack.push(value);
            }
            Op::HeapStore2 => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let ptr_val = self
                    .heap
                    .read_slot(r, 0)
                    .ok_or("runtime error: slot index 0 out of bounds")?;
                let ptr_ref = ptr_val
                    .as_ref()
                    .ok_or("runtime error: expected reference for ptr field")?;
                if index < 0 {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                self.heap
                    .write_slot(ptr_ref, index as usize, value)
                    .map_err(|e| format!("runtime error: {}", e))?;
            }
            Op::HeapOffsetRef => {
                let offset = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                if offset < 0 {
                    return Err(format!(
                        "runtime error: negative offset {} for HeapOffsetRef",
                        offset
                    ));
                }
                let new_ref = r.with_added_slot_offset(offset as usize);
                self.stack.push(Value::Ref(new_ref));
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
            // HeapAllocString removed — use HeapAlloc(2) instead
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

            // Indirect call: closure_ref is passed as slot 0, function body reads captures via HeapLoad
            Op::CallIndirect(argc) => {
                // Stack layout: [..., closure_ref, arg0, arg1, ..., arg_{argc-1}]
                let stack_len = self.stack.len();
                if stack_len < argc + 1 {
                    return Err("stack underflow in CallIndirect".to_string());
                }

                // Pop argc args off the stack
                let args_start = stack_len - argc;
                let args: Vec<Value> = self.stack[args_start..].to_vec();
                self.stack.truncate(args_start);

                // Pop the closure reference
                let closure_val = self.stack.pop().ok_or("stack underflow in CallIndirect")?;
                let closure_ref = closure_val
                    .as_ref()
                    .ok_or("runtime error: CallIndirect expects a callable reference")?;

                // Read func_index from heap slot 0
                let closure_obj = self
                    .heap
                    .get(closure_ref)
                    .ok_or("runtime error: invalid callable reference")?;
                let func_index = closure_obj.slots[0]
                    .as_i64()
                    .ok_or("runtime error: callable slot 0 must be func_index (integer)")?
                    as usize;

                let func = &chunk.functions[func_index];
                let expected_arity = func.arity; // 1 (closure_ref) + user args
                let total_args = 1 + argc;

                if total_args != expected_arity {
                    return Err(format!(
                        "runtime error: function '{}' expects {} arguments (1 closure_ref + {} params), got 1 + {} args",
                        func.name,
                        expected_arity,
                        expected_arity - 1,
                        argc
                    ));
                }

                // Slot 0: closure_ref, slots 1..: user args
                let new_stack_base = self.stack.len();
                self.stack.push(closure_val);
                for arg in args {
                    self.stack.push(arg);
                }

                self.frames.push(Frame {
                    func_index,
                    pc: 0,
                    stack_base: new_stack_base,
                    ret_vreg: None,
                    stack_floor: 0,
                });
            }

            // ========================================
            // Dynamic call by func_index on stack
            // ========================================
            Op::CallDynamic(argc) => {
                // Stack: [..., func_index, arg0, arg1, ..., arg_{argc-1}]
                let stack_len = self.stack.len();
                if stack_len < argc + 1 {
                    return Err("stack underflow in CallDynamic".to_string());
                }

                // Pop argc args
                let args_start = stack_len - argc;
                let args: Vec<Value> = self.stack[args_start..].to_vec();
                self.stack.truncate(args_start);

                // Pop func_index
                let func_index_val = self.stack.pop().ok_or("stack underflow in CallDynamic")?;
                let func_index = func_index_val
                    .as_i64()
                    .ok_or("runtime error: CallDynamic expects func_index as integer")?
                    as usize;

                let func = &chunk.functions[func_index];
                if func.arity != argc {
                    return Err(format!(
                        "runtime error: function '{}' expects {} arguments, got {}",
                        func.name, func.arity, argc
                    ));
                }

                let new_stack_base = self.stack.len();
                for arg in args {
                    self.stack.push(arg);
                }

                self.frames.push(Frame {
                    func_index,
                    pc: 0,
                    stack_base: new_stack_base,
                    ret_vreg: None,
                    stack_floor: 0,
                });
            }

            // ========================================
            // Type Descriptor
            // ========================================
            Op::TypeDescLoad(idx) => {
                let gc_ref = self
                    .type_descriptor_refs
                    .get(idx)
                    .ok_or_else(|| format!("invalid type descriptor index: {}", idx))?;
                self.stack.push(Value::Ref(*gc_ref));
            }

            // ========================================
            // Interface Descriptor
            // ========================================
            Op::InterfaceDescLoad(idx) => {
                let gc_ref = self
                    .interface_descriptor_refs
                    .get(idx)
                    .ok_or_else(|| format!("invalid interface descriptor index: {}", idx))?;
                self.stack.push(Value::Ref(*gc_ref));
            }

            // ========================================
            // Vtable Lookup
            // ========================================
            Op::VtableLookup => {
                // Stack: [..., type_info_ref, iface_desc_ref]
                let iface_val = self.stack.pop().ok_or("stack underflow in VtableLookup")?;
                let ti_val = self.stack.pop().ok_or("stack underflow in VtableLookup")?;
                let ti_ref = ti_val
                    .as_ref()
                    .ok_or("runtime error: VtableLookup expects type_info reference")?;
                let iface_ref = iface_val
                    .as_ref()
                    .ok_or("runtime error: VtableLookup expects iface_desc reference")?;

                let result = self.vtable_lookup(ti_ref, iface_ref)?;
                self.stack.push(result);
            }
        }

        Ok(ControlFlow::Continue)
    }

    /// Look up an interface vtable in a type_info heap object.
    /// Walks the vtable entries comparing iface_desc_ref by pointer equality.
    /// Returns vtable_ref (Value::Ref) if found, or Value::Null if not.
    fn vtable_lookup(&self, ti_ref: GcRef, iface_ref: GcRef) -> Result<Value, String> {
        let ti_obj = self
            .heap
            .get(ti_ref)
            .ok_or("runtime error: invalid type_info reference")?;
        let n = ti_obj.slots[2]
            .as_i64()
            .ok_or("runtime error: type_info slot 2 must be field_count")? as usize;
        let aux_slot = 3 + 2 * n;
        let m = ti_obj.slots[aux_slot]
            .as_i64()
            .ok_or("runtime error: type_info aux_count slot must be integer")?
            as usize;
        let vtable_base = aux_slot + 1 + m;
        let v = ti_obj.slots[vtable_base]
            .as_i64()
            .ok_or("runtime error: type_info vtable_count slot must be integer")?
            as usize;
        for j in 0..v {
            let entry_iface = &ti_obj.slots[vtable_base + 1 + 2 * j];
            if let Some(entry_ref) = entry_iface.as_ref()
                && entry_ref == iface_ref
            {
                return Ok(ti_obj.slots[vtable_base + 1 + 2 * j + 1]);
            }
        }
        Ok(Value::Null)
    }

    fn add(&mut self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a.wrapping_add(b))),
            (Value::F64(a), Value::F64(b)) => Ok(Value::F64(a + b)),
            (Value::I64(a), Value::F64(b)) => Ok(Value::F64(a as f64 + b)),
            (Value::F64(a), Value::I64(b)) => Ok(Value::F64(a + b as f64)),
            (Value::Ref(a), Value::Ref(b)) => {
                // String concatenation fallback for cases where codegen
                // couldn't statically detect Ref+Ref (e.g. array indexing)
                let a_obj = self.heap.get(a).ok_or("runtime error: invalid reference")?;
                let b_obj = self.heap.get(b).ok_or("runtime error: invalid reference")?;
                let a_data_ref = a_obj.slots[0]
                    .as_ref()
                    .ok_or("runtime error: invalid string ptr")?;
                let b_data_ref = b_obj.slots[0]
                    .as_ref()
                    .ok_or("runtime error: invalid string ptr")?;
                let a_data = self
                    .heap
                    .get(a_data_ref)
                    .ok_or("runtime error: invalid string data")?;
                let b_data = self
                    .heap
                    .get(b_data_ref)
                    .ok_or("runtime error: invalid string data")?;
                let a_str = a_data.slots_to_string();
                let b_str = b_data.slots_to_string();
                let result = format!("{}{}", a_str, b_str);
                let r = self.heap.alloc_string(result)?;
                Ok(Value::Ref(r))
            }
            _ => Err("runtime error: cannot add these types".to_string()),
        }
    }

    fn sub(&self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a.wrapping_sub(b))),
            (Value::F64(a), Value::F64(b)) => Ok(Value::F64(a - b)),
            (Value::I64(a), Value::F64(b)) => Ok(Value::F64(a as f64 - b)),
            (Value::F64(a), Value::I64(b)) => Ok(Value::F64(a - b as f64)),
            _ => Err("runtime error: cannot subtract these types".to_string()),
        }
    }

    fn mul(&self, a: Value, b: Value) -> Result<Value, String> {
        match (a, b) {
            (Value::I64(a), Value::I64(b)) => Ok(Value::I64(a.wrapping_mul(b))),
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
                // Reference identity comparison.
                // String equality is handled by _string_eq in prelude.
                a_ref.index == b_ref.index
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

                // Structural heuristic for 2-slot [Ref, I64] objects:
                // Try to detect string vs array by checking if the data array
                // contains only printable characters.
                if obj.slots.len() == 2
                    && let (Some(data_ref), Some(len)) =
                        (obj.slots[0].as_ref(), obj.slots[1].as_i64())
                {
                    let len_usize = len as usize;
                    // Check if data looks like a string (non-empty, all printable chars)
                    let mut is_string = len_usize > 0;
                    let mut chars = String::new();
                    for i in 0..len_usize {
                        match self.heap.read_slot(data_ref, i) {
                            Some(Value::I64(c)) => {
                                if let Some(ch) = char::from_u32(c as u32) {
                                    // Accept printable chars + common whitespace
                                    if ch >= ' ' || ch == '\n' || ch == '\r' || ch == '\t' {
                                        chars.push(ch);
                                    } else {
                                        is_string = false;
                                        break;
                                    }
                                } else {
                                    is_string = false;
                                    break;
                                }
                            }
                            _ => {
                                is_string = false;
                                break;
                            }
                        }
                    }
                    // Empty 2-slot objects and string-like data → display as string
                    if is_string {
                        return Ok(chars);
                    }
                    // Otherwise display as array
                    let mut parts = Vec::new();
                    for i in 0..len_usize {
                        if let Some(elem) = self.heap.read_slot(data_ref, i) {
                            parts.push(self.value_to_string(&elem)?);
                        }
                    }
                    return Ok(format!("[{}]", parts.join(", ")));
                }

                // Fallback: show all elements as array/struct
                let mut parts = Vec::new();
                for elem in obj.slots.iter() {
                    parts.push(self.value_to_string(elem)?);
                }
                Ok(format!("[{}]", parts.join(", ")))
            }
        }
    }

    /// Convert a heap GcRef (String struct [ptr, len]) to a Rust String.
    /// Follows the ptr to the data array and reads character slots.
    fn ref_to_rust_string(&self, r: GcRef) -> Result<String, String> {
        let obj = self
            .heap
            .get(r)
            .ok_or("runtime error: invalid string reference")?;
        let data_ref = obj.slots[0]
            .as_ref()
            .ok_or("runtime error: invalid string ptr")?;
        let data = self
            .heap
            .get(data_ref)
            .ok_or("runtime error: invalid string data")?;
        Ok(data.slots_to_string())
    }

    /// Pop a value from the operand stack, respecting the register file boundary.
    fn pop_operand(&mut self) -> Result<Value, String> {
        let floor = self.frames.last().map_or(0, |f| f.stack_floor);
        if self.stack.len() <= floor {
            return Err("stack underflow".to_string());
        }
        Ok(self.stack.pop().unwrap())
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

        // Add type descriptor references as roots
        for r in &self.type_descriptor_refs {
            roots.push(Value::Ref(*r));
        }

        // Add interface descriptor references as roots
        for r in &self.interface_descriptor_refs {
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
    /// - 10: time() -> epoch seconds
    /// - 11: time_nanos() -> epoch nanoseconds
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
        const SYSCALL_TIME: usize = 10;
        const SYSCALL_TIME_NANOS: usize = 11;

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
                let path = self.ref_to_rust_string(path_ref)?;

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

                // Get the string from heap (String struct: [ptr, len])
                let buf_str = self.ref_to_rust_string(buf_ref)?;

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
                let host = self.ref_to_rust_string(host_ref)?;

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
                let host = self.ref_to_rust_string(host_ref)?;

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
            SYSCALL_TIME => {
                if !args.is_empty() {
                    return Err(format!(
                        "time syscall expects 0 arguments, got {}",
                        args.len()
                    ));
                }

                let duration = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("time syscall failed: {}", e))?;
                Ok(Value::I64(duration.as_secs() as i64))
            }
            SYSCALL_TIME_NANOS => {
                if !args.is_empty() {
                    return Err(format!(
                        "time_nanos syscall expects 0 arguments, got {}",
                        args.len()
                    ));
                }

                let duration = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_err(|e| format!("time_nanos syscall failed: {}", e))?;
                Ok(Value::I64(duration.as_nanos() as i64))
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
    if let Some(compiled) = vm.jit_functions.get(&func_index) {
        // AArch64: unboxed frame (8B per slot, payload only)
        #[cfg(target_arch = "aarch64")]
        {
            let entry: unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn =
                unsafe { compiled.entry_point() };

            const MAX_LOCALS: usize = 64;
            let mut frame = [0u64; MAX_LOCALS];

            for (i, slot) in frame.iter_mut().take(argc).enumerate() {
                *slot = unsafe { (*args.add(i)).payload };
            }

            let result = unsafe { entry(ctx as *mut u8, frame.as_mut_ptr(), frame.as_mut_ptr()) };

            #[cfg(debug_assertions)]
            if vm.trace_jit {
                eprintln!(
                    "[JIT] Executed function '{}', result: tag={}, payload={}",
                    func.name, result.tag, result.payload
                );
            }

            ctx_ref.heap_base = vm.heap.memory_base_ptr();
            return result;
        }

        // x86_64: unboxed frame (8B per slot, payload + shadow tags)
        #[cfg(target_arch = "x86_64")]
        {
            let entry: unsafe extern "C" fn(*mut u8, *mut u64, *mut u64) -> JitReturn =
                unsafe { compiled.entry_point() };

            const MAX_LOCALS: usize = 128; // 64 payloads + 64 shadow tags
            let mut frame = [0u64; MAX_LOCALS];

            for (i, slot) in frame.iter_mut().take(argc).enumerate() {
                *slot = unsafe { (*args.add(i)).payload };
            }

            let result = unsafe { entry(ctx as *mut u8, frame.as_mut_ptr(), frame.as_mut_ptr()) };

            #[cfg(debug_assertions)]
            if vm.trace_jit {
                eprintln!(
                    "[JIT] Executed function '{}', result: tag={}, payload={}",
                    func.name, result.tag, result.payload
                );
            }

            ctx_ref.heap_base = vm.heap.memory_base_ptr();
            return result;
        }
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
            ret_vreg: None,
            stack_floor: 0,
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
        // Update heap_base in case the called function grew the heap
        ctx_ref.heap_base = vm.heap.memory_base_ptr();
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

/// JIT HeapAllocDynSimple helper function.
/// Allocates `size` null-initialized slots on the heap.
#[cfg(feature = "jit")]
unsafe extern "C" fn jit_heap_alloc_dyn_simple_helper(
    ctx: *mut JitCallContext,
    size: u64,
) -> JitReturn {
    let ctx_ref = unsafe { &mut *ctx };
    let vm = unsafe { &mut *(ctx_ref.vm as *mut VM) };

    vm.record_opcode("HeapAllocDynSimple");

    let size = size as usize;
    let slots = vec![Value::Null; size];
    match vm.heap.alloc_slots(slots) {
        Ok(r) => {
            ctx_ref.heap_base = vm.heap.memory_base_ptr();
            JitReturn {
                tag: 4, // TAG_PTR
                payload: r.index as u64,
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let mut vm = VM::new();
        // Use the stack-based interpreter for unit tests that check vm.stack directly
        vm.set_use_microop(false);
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
            debug: None,
        };

        let mut vm = VM::new();
        // Use the stack-based interpreter for unit tests that check vm.stack directly
        vm.set_use_microop(false);
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
        // Array<T> struct layout: [ptr, len]
        // ptr points to data array, len is the element count
        let stack = run_code(vec![
            Op::I64Const(1),  // element 0
            Op::I64Const(2),  // element 1
            Op::I64Const(3),  // element 2
            Op::HeapAlloc(3), // data array with 3 elements
            Op::I64Const(3),  // length value
            Op::HeapAlloc(2), // Array<T> struct [ptr, len]
            Op::HeapLoad(1),  // read len field
        ])
        .unwrap();
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0], Value::I64(3));
    }

    #[test]
    fn test_string_operations() {
        // Test that StringConst produces a valid string reference
        let stack =
            run_code_with_strings(vec![Op::StringConst(0)], vec!["Hello".to_string()]).unwrap();
        assert_eq!(stack.len(), 1);
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
            Op::HeapLoad(0), // If we can read slot 0, it's a valid reference
        ]);

        assert!(
            result.is_ok(),
            "LocalSet write barrier test failed: {:?}",
            result
        );
        // The last value should be the element we stored (2)
        let stack = result.unwrap();
        assert!(stack.iter().any(|v| *v == Value::I64(2)));
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
        let content = vm.ref_to_rust_string(content_ref).unwrap();
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
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
        let content = vm.ref_to_rust_string(content_ref).unwrap();
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
            type_descriptors: vec![],
            interface_descriptors: vec![],
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

        let response = vm.ref_to_rust_string(response_ref).unwrap();

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
