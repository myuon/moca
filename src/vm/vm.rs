use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

use crate::vm::threads::{Channel, ThreadSpawner};
use crate::vm::{Chunk, Function, Heap, Op, Value};

#[cfg(all(target_arch = "aarch64", feature = "jit"))]
use crate::jit::compiler::{CompiledCode, JitCompiler};
#[cfg(all(target_arch = "x86_64", feature = "jit"))]
use crate::jit::compiler_x86_64::{CompiledCode, JitCompiler};
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

/// The moca virtual machine.
pub struct VM {
    stack: Vec<Value>,
    frames: Vec<Frame>,
    heap: Heap,
    try_frames: Vec<TryFrame>,
    /// Function call counters for JIT (index matches Chunk::functions)
    call_counts: Vec<u32>,
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
    /// Output stream for print statements
    output: Box<dyn Write>,
}

impl VM {
    pub fn new() -> Self {
        Self::new_with_config(None, true, Box::new(io::stdout()))
    }

    /// Create a VM with a custom output stream.
    pub fn with_output(output: Box<dyn Write>) -> Self {
        Self::new_with_config(None, true, output)
    }

    /// Create a new VM with custom heap configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    pub fn new_with_heap_config(heap_limit: Option<usize>, gc_enabled: bool) -> Self {
        Self::new_with_config(heap_limit, gc_enabled, Box::new(io::stdout()))
    }

    /// Create a new VM with full configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    /// * `output` - Output stream for print statements
    pub fn new_with_config(
        heap_limit: Option<usize>,
        gc_enabled: bool,
        output: Box<dyn Write>,
    ) -> Self {
        Self {
            stack: Vec::with_capacity(1024),
            frames: Vec::with_capacity(64),
            heap: Heap::new_with_config(heap_limit, gc_enabled),
            try_frames: Vec::new(),
            call_counts: Vec::new(),
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
        }
    }

    /// Configure JIT settings.
    pub fn set_jit_config(&mut self, threshold: u32, trace: bool) {
        self.jit_threshold = threshold;
        self.trace_jit = trace;
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
            Op::PushInt(n) => {
                self.stack.push(Value::I64(n));
            }
            Op::PushFloat(f) => {
                self.stack.push(Value::F64(f));
            }
            Op::PushTrue => {
                self.stack.push(Value::Bool(true));
            }
            Op::PushFalse => {
                self.stack.push(Value::Bool(false));
            }
            Op::PushNull => {
                self.stack.push(Value::Null);
            }
            Op::PushString(idx) => {
                let s = chunk.strings.get(idx).cloned().unwrap_or_default();
                let r = self.heap.alloc_string(s)?;
                self.stack.push(Value::Ref(r));
            }
            Op::Pop => {
                self.stack.pop();
            }
            Op::Dup => {
                let value = self.stack.last().copied().ok_or("stack underflow")?;
                self.stack.push(value);
            }
            Op::GetL(slot) => {
                let frame = self.frames.last().unwrap();
                let index = frame.stack_base + slot;
                let value = self.stack.get(index).copied().unwrap_or(Value::Null);
                self.stack.push(value);
            }
            Op::SetL(slot) => {
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
            Op::Add => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.add(a, b)?;
                self.stack.push(result);
            }
            Op::Sub => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.sub(a, b)?;
                self.stack.push(result);
            }
            Op::Mul => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.mul(a, b)?;
                self.stack.push(result);
            }
            Op::Div => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.div(a, b)?;
                self.stack.push(result);
            }
            Op::Mod => {
                let b = self.pop_int()?;
                let a = self.pop_int()?;
                if b == 0 {
                    return Err("runtime error: division by zero".to_string());
                }
                self.stack.push(Value::I64(a % b));
            }
            Op::Neg => {
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = match a {
                    Value::I64(n) => Value::I64(-n),
                    Value::F64(f) => Value::F64(-f),
                    _ => return Err("runtime error: cannot negate non-numeric value".to_string()),
                };
                self.stack.push(result);
            }
            Op::Eq => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }
            Op::Ne => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = !self.values_equal(&a, &b);
                self.stack.push(Value::Bool(result));
            }
            Op::Lt => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? < 0;
                self.stack.push(Value::Bool(result));
            }
            Op::Le => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? <= 0;
                self.stack.push(Value::Bool(result));
            }
            Op::Gt => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? > 0;
                self.stack.push(Value::Bool(result));
            }
            Op::Ge => {
                let b = self.stack.pop().ok_or("stack underflow")?;
                let a = self.stack.pop().ok_or("stack underflow")?;
                let result = self.compare(&a, &b)? >= 0;
                self.stack.push(Value::Bool(result));
            }
            Op::Not => {
                let a = self.stack.pop().ok_or("stack underflow")?;
                self.stack.push(Value::Bool(!a.is_truthy()));
            }
            Op::Jmp(target) => {
                let frame = self.frames.last_mut().unwrap();
                frame.pc = target;
            }
            Op::JmpIfFalse(target) => {
                let cond = self.stack.pop().ok_or("stack underflow")?;
                if !cond.is_truthy() {
                    let frame = self.frames.last_mut().unwrap();
                    frame.pc = target;
                }
            }
            Op::JmpIfTrue(target) => {
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

                let len = if let Some(arr) = obj.as_array() {
                    arr.elements.len() as i64
                } else if let Some(s) = obj.as_string() {
                    s.value.chars().count() as i64
                } else if let Some(slots) = obj.as_slots() {
                    // For fixed arrays (Slots), slot[0] contains the length
                    // Note: For vectors, use vec_len() which reads slots[1]
                    slots
                        .slots
                        .first()
                        .and_then(|v| v.as_i64())
                        .ok_or("runtime error: invalid slots length")?
                } else {
                    return Err("runtime error: len expects array or string".to_string());
                };
                self.stack.push(Value::I64(len));
            }
            Op::New(n) => {
                let mut fields = HashMap::new();
                for _ in 0..n {
                    let value = self.stack.pop().ok_or("stack underflow")?;
                    let key = self.stack.pop().ok_or("stack underflow")?;

                    // Key should be a string
                    let key_ref = key
                        .as_ref()
                        .ok_or("runtime error: object key must be a string")?;
                    let key_obj = self
                        .heap
                        .get(key_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let key_str = key_obj
                        .as_string()
                        .ok_or("runtime error: object key must be a string")?;
                    fields.insert(key_str.value.clone(), value);
                }
                let r = self.heap.alloc_object_map(fields)?;
                self.stack.push(Value::Ref(r));
            }
            Op::GetF(str_idx) => {
                let obj = self.stack.pop().ok_or("stack underflow")?;
                let r = obj.as_ref().ok_or("runtime error: expected object")?;

                let field_name = chunk.strings.get(str_idx).cloned().unwrap_or_default();

                let heap_obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                let obj = heap_obj
                    .as_object()
                    .ok_or("runtime error: expected object")?;

                let value = obj.fields.get(&field_name).copied().unwrap_or(Value::Null);
                self.stack.push(value);
            }
            Op::SetF(str_idx) => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let obj = self.stack.pop().ok_or("stack underflow")?;
                let r = obj.as_ref().ok_or("runtime error: expected object")?;

                let field_name = chunk.strings.get(str_idx).cloned().unwrap_or_default();

                // Write barrier: get old value first (immutable borrow)
                let old_value = {
                    let heap_obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                    let obj = heap_obj
                        .as_object()
                        .ok_or("runtime error: expected object")?;
                    obj.fields.get(&field_name).copied().unwrap_or(Value::Null)
                };
                self.write_barrier(old_value);

                // Now do the mutable update
                let heap_obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let obj = heap_obj
                    .as_object_mut()
                    .ok_or("runtime error: expected object")?;

                obj.fields.insert(field_name, value);
            }
            Op::TypeOf => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let type_name = match &value {
                    Value::I64(_) => "int",
                    Value::F64(_) => "float",
                    Value::Bool(_) => "bool",
                    Value::Null => "nil",
                    Value::Ref(r) => {
                        if let Some(obj) = self.heap.get(*r) {
                            match obj.obj_type() {
                                super::ObjectType::String => "string",
                                super::ObjectType::Array => "array",
                                super::ObjectType::Object => "object",
                                super::ObjectType::Slots => "array", // Slots is used for arrays and vectors
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
                let s = obj
                    .as_string()
                    .ok_or("runtime error: parse_int expects string")?;
                let n: i64 = s
                    .value
                    .trim()
                    .parse()
                    .map_err(|_| format!("runtime error: cannot parse '{}' as int", s.value))?;
                self.stack.push(Value::I64(n));
            }
            Op::StrLen => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let r = value
                    .as_ref()
                    .ok_or("runtime error: str_len expects string")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;
                let s = obj
                    .as_string()
                    .ok_or("runtime error: str_len expects string")?;
                let len = s.value.chars().count() as i64;
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

                // Create slots with [len=2, sender_id, receiver_id] layout
                let arr = self.heap.alloc_slots(vec![
                    Value::I64(2),
                    Value::I64(id as i64),
                    Value::I64(id as i64),
                ])?;
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
            Op::AllocHeap(n) => {
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
                let slots = obj
                    .as_slots()
                    .ok_or("runtime error: expected slots object")?;
                if offset >= slots.slots.len() {
                    return Err(format!(
                        "runtime error: slot index {} out of bounds",
                        offset
                    ));
                }
                self.stack.push(slots.slots[offset]);
            }
            Op::HeapStore(offset) => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let slots = obj
                    .as_slots_mut()
                    .ok_or("runtime error: expected slots object")?;
                if offset >= slots.slots.len() {
                    return Err(format!(
                        "runtime error: slot index {} out of bounds",
                        offset
                    ));
                }
                slots.slots[offset] = value;
            }
            Op::HeapLoadDyn => {
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;

                // Support Slots and String types
                if let Some(slots) = obj.as_slots() {
                    if index < 0 || index as usize >= slots.slots.len() {
                        return Err(format!("runtime error: slot index {} out of bounds", index));
                    }
                    self.stack.push(slots.slots[index as usize]);
                } else if let Some(s) = obj.as_string() {
                    // String indexing: return ASCII code at index (adjusted for +1 offset from codegen)
                    let actual_index = index - 1; // codegen adds +1, so subtract it back
                    let bytes = s.value.as_bytes();
                    if actual_index < 0 || actual_index as usize >= bytes.len() {
                        return Err(format!(
                            "runtime error: string index {} out of bounds (length {})",
                            actual_index,
                            bytes.len()
                        ));
                    }
                    let byte_value = bytes[actual_index as usize] as i64;
                    self.stack.push(Value::I64(byte_value));
                } else {
                    return Err("runtime error: expected slots or string".to_string());
                }
            }
            Op::HeapStoreDyn => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val.as_ref().ok_or("runtime error: expected reference")?;
                let obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let slots = obj
                    .as_slots_mut()
                    .ok_or("runtime error: expected slots object")?;
                if index < 0 || index as usize >= slots.slots.len() {
                    return Err(format!("runtime error: slot index {} out of bounds", index));
                }
                slots.slots[index as usize] = value;
            }
            Op::VectorPush => {
                // Push value to vector: [vec, value] -> []
                // Vector is Slots with layout: [ptr, len, cap]
                let value = self.stack.pop().ok_or("stack underflow")?;
                let vec_val = self.stack.pop().ok_or("stack underflow")?;
                let vec_ref = vec_val
                    .as_ref()
                    .ok_or("runtime error: expected vector reference")?;

                // Get vector header: slots[0]=ptr, slots[1]=len, slots[2]=cap
                let (data_ptr, len, cap) = {
                    let vec_obj = self
                        .heap
                        .get(vec_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = vec_obj
                        .as_slots()
                        .ok_or("runtime error: expected slots (vector header)")?;
                    let ptr = slots.slots[0].as_ref();
                    let len = slots.slots[1]
                        .as_i64()
                        .ok_or("runtime error: invalid vector length")?;
                    let cap = slots.slots[2]
                        .as_i64()
                        .ok_or("runtime error: invalid vector capacity")?;
                    (ptr, len, cap)
                };

                if len >= cap {
                    // Need to grow: new capacity is max(8, cap * 2)
                    let new_cap = if cap == 0 { 8 } else { cap * 2 };

                    // Allocate new data storage
                    let new_data: Vec<Value> = vec![Value::Null; new_cap as usize];
                    let new_data_ref = self.heap.alloc_slots(new_data)?;

                    // Copy old data to new storage if exists
                    if let Some(old_ptr) = data_ptr {
                        for i in 0..len as usize {
                            let old_val = {
                                let old_obj = self
                                    .heap
                                    .get(old_ptr)
                                    .ok_or("runtime error: invalid reference")?;
                                let old_slots =
                                    old_obj.as_slots().ok_or("runtime error: expected slots")?;
                                old_slots.slots[i]
                            };
                            let new_obj = self
                                .heap
                                .get_mut(new_data_ref)
                                .ok_or("runtime error: invalid reference")?;
                            let new_slots = new_obj
                                .as_slots_mut()
                                .ok_or("runtime error: expected slots")?;
                            new_slots.slots[i] = old_val;
                        }
                    }

                    // Store the new value
                    {
                        let new_obj = self
                            .heap
                            .get_mut(new_data_ref)
                            .ok_or("runtime error: invalid reference")?;
                        let new_slots = new_obj
                            .as_slots_mut()
                            .ok_or("runtime error: expected slots")?;
                        new_slots.slots[len as usize] = value;
                    }

                    // Update vector header: slots[0]=ptr, slots[1]=len, slots[2]=cap
                    let vec_obj = self
                        .heap
                        .get_mut(vec_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = vec_obj
                        .as_slots_mut()
                        .ok_or("runtime error: expected slots")?;
                    slots.slots[0] = Value::Ref(new_data_ref);
                    slots.slots[1] = Value::I64(len + 1);
                    slots.slots[2] = Value::I64(new_cap);
                } else {
                    // Has space, just store the value
                    let data_ptr =
                        data_ptr.ok_or("runtime error: vector has capacity but no data")?;
                    {
                        let data_obj = self
                            .heap
                            .get_mut(data_ptr)
                            .ok_or("runtime error: invalid reference")?;
                        let slots = data_obj
                            .as_slots_mut()
                            .ok_or("runtime error: expected slots")?;
                        slots.slots[len as usize] = value;
                    }

                    // Update length: slots[1] = len + 1
                    let vec_obj = self
                        .heap
                        .get_mut(vec_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = vec_obj
                        .as_slots_mut()
                        .ok_or("runtime error: expected slots")?;
                    slots.slots[1] = Value::I64(len + 1);
                }
            }
            Op::VectorPop => {
                // Pop value from vector: [vec] -> [value]
                // Vector is Slots with layout: [ptr, len, cap]
                let vec_val = self.stack.pop().ok_or("stack underflow")?;
                let vec_ref = vec_val
                    .as_ref()
                    .ok_or("runtime error: expected vector reference")?;

                let (data_ptr, len) = {
                    let vec_obj = self
                        .heap
                        .get(vec_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = vec_obj
                        .as_slots()
                        .ok_or("runtime error: expected slots (vector header)")?;
                    let ptr = slots.slots[0]
                        .as_ref()
                        .ok_or("runtime error: vector has no data")?;
                    let len = slots.slots[1]
                        .as_i64()
                        .ok_or("runtime error: invalid vector length")?;
                    if len == 0 {
                        return Err("runtime error: cannot pop from empty vector".to_string());
                    }
                    (ptr, len)
                };

                // Get the value
                let value = {
                    let data_obj = self
                        .heap
                        .get(data_ptr)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = data_obj.as_slots().ok_or("runtime error: expected slots")?;
                    slots.slots[(len - 1) as usize]
                };

                // Update length: slots[1] = len - 1
                {
                    let vec_obj = self
                        .heap
                        .get_mut(vec_ref)
                        .ok_or("runtime error: invalid reference")?;
                    let slots = vec_obj
                        .as_slots_mut()
                        .ok_or("runtime error: expected slots")?;
                    slots.slots[1] = Value::I64(len - 1);
                }

                self.stack.push(value);
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

                if let (Some(a_str), Some(b_str)) = (a_obj.as_string(), b_obj.as_string()) {
                    let result = format!("{}{}", a_str.value, b_str.value);
                    let r = self.heap.alloc_string(result)?;
                    return Ok(Value::Ref(r));
                }

                Err("runtime error: cannot add these types".to_string())
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
                        if let (Some(a_str), Some(b_str)) = (a.as_string(), b.as_string()) {
                            return a_str.value == b_str.value;
                        }
                        // For arrays and objects, compare by reference
                        a_ref.index == b_ref.index
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
                match obj {
                    super::HeapObject::String(s) => Ok(s.value.clone()),
                    super::HeapObject::Array(a) => {
                        let mut parts = Vec::new();
                        for elem in &a.elements {
                            parts.push(self.value_to_string(elem)?);
                        }
                        Ok(format!("[{}]", parts.join(", ")))
                    }
                    super::HeapObject::Object(o) => {
                        let mut parts = Vec::new();
                        for (k, v) in &o.fields {
                            parts.push(format!("{}: {}", k, self.value_to_string(v)?));
                        }
                        Ok(format!("{{{}}}", parts.join(", ")))
                    }
                    super::HeapObject::Slots(s) => {
                        // Slots: slot[0] is length, slot[1..] are elements (for fixed arrays)
                        // Note: For vectors (Slots[ptr, len, cap]), this will show raw values
                        let mut parts = Vec::new();
                        for elem in s.slots.iter().skip(1) {
                            parts.push(self.value_to_string(elem)?);
                        }
                        Ok(format!("[{}]", parts.join(", ")))
                    }
                }
            }
        }
    }

    fn pop_int(&mut self) -> Result<i64, String> {
        let value = self.stack.pop().ok_or("stack underflow")?;
        value.as_i64().ok_or_else(|| "expected integer".to_string())
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
        let roots: Vec<Value> = self.stack.clone();
        self.heap.collect(&roots);
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
        for i in 0..argc {
            locals[i] = unsafe { *args.add(i) };
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
        let stack = run_code(vec![Op::PushInt(42)]).unwrap();
        assert_eq!(stack, vec![Value::I64(42)]);
    }

    #[test]
    fn test_push_float() {
        let stack = run_code(vec![Op::PushFloat(3.14)]).unwrap();
        assert_eq!(stack, vec![Value::F64(3.14)]);
    }

    #[test]
    fn test_push_nil() {
        let stack = run_code(vec![Op::PushNull]).unwrap();
        assert_eq!(stack, vec![Value::Null]);
    }

    #[test]
    fn test_add() {
        let stack = run_code(vec![Op::PushInt(1), Op::PushInt(2), Op::Add]).unwrap();
        assert_eq!(stack, vec![Value::I64(3)]);
    }

    #[test]
    fn test_add_float() {
        let stack = run_code(vec![Op::PushFloat(1.5), Op::PushFloat(2.5), Op::Add]).unwrap();
        assert_eq!(stack, vec![Value::F64(4.0)]);
    }

    #[test]
    fn test_comparison() {
        let stack = run_code(vec![Op::PushInt(1), Op::PushInt(2), Op::Lt]).unwrap();
        assert_eq!(stack, vec![Value::Bool(true)]);
    }

    #[test]
    fn test_division_by_zero() {
        let result = run_code(vec![Op::PushInt(1), Op::PushInt(0), Op::Div]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("division by zero"));
    }

    #[test]
    fn test_locals() {
        let stack = run_code(vec![Op::PushInt(42), Op::SetL(0), Op::GetL(0)]).unwrap();
        assert_eq!(stack, vec![Value::I64(42), Value::I64(42)]);
    }

    #[test]
    fn test_conditional_jump() {
        // if false, skip push 1, else push 2
        let stack = run_code(vec![
            Op::PushFalse,
            Op::JmpIfFalse(4),
            Op::PushInt(1),
            Op::Jmp(5),
            Op::PushInt(2),
        ])
        .unwrap();
        assert_eq!(stack, vec![Value::I64(2)]);
    }

    #[test]
    fn test_array_operations() {
        // AllocHeap takes slots from stack: [len, e1, e2, e3] -> creates Slots object
        let stack = run_code(vec![
            Op::PushInt(3),   // length
            Op::PushInt(1),   // element 0
            Op::PushInt(2),   // element 1
            Op::PushInt(3),   // element 2
            Op::AllocHeap(4), // 1 len + 3 elements
            Op::ArrayLen,
        ])
        .unwrap();
        assert_eq!(stack.len(), 1);
        assert_eq!(stack[0], Value::I64(3));
    }

    #[test]
    fn test_string_operations() {
        // Test string concatenation using Op::Add
        let stack = run_code_with_strings(
            vec![Op::PushString(0), Op::PushString(1), Op::Add],
            vec!["Hello, ".to_string(), "World!".to_string()],
        )
        .unwrap();
        assert_eq!(stack.len(), 1);
        // The result should be a string pointer
        assert!(stack[0].is_ref());
    }

    #[test]
    fn test_write_barrier_setl() {
        // Test that SetL correctly calls write barrier when overwriting references.
        // In stop-the-world GC the barrier is a no-op, but this verifies the code path.
        //
        // This test:
        // 1. Stores an array in local 0
        // 2. Overwrites local 0 with a new array (triggers write barrier)
        // 3. Verifies execution completes successfully
        let result = run_code(vec![
            // Allocate array [len=1, elem] and store in local 0
            Op::PushInt(1), // length
            Op::PushInt(1), // element
            Op::AllocHeap(2),
            Op::SetL(0),
            // Allocate another array [len=1, elem]
            Op::PushInt(1), // length
            Op::PushInt(2), // element
            Op::AllocHeap(2),
            // Overwrite local 0 (triggers write barrier, old value was array ref)
            Op::SetL(0),
            // Get local 0 to verify it's still a valid reference
            Op::GetL(0),
            Op::ArrayLen, // If we can get length, it's a valid array
        ]);

        assert!(
            result.is_ok(),
            "SetL write barrier test failed: {:?}",
            result
        );
        // The last value should be the array length (1 element)
        let stack = result.unwrap();
        assert!(stack.iter().any(|v| *v == Value::I64(1)));
    }

    #[test]
    fn test_write_barrier_setf() {
        // Test that SetF correctly calls write barrier when overwriting object fields.
        // In stop-the-world GC the barrier is a no-op, but this verifies the code path.
        //
        // SetF stack order: [object, value] with value on top
        // Pop order: value first, then object
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "__main__".to_string(),
                arity: 0,
                locals_count: 1,
                code: vec![
                    // Create object { x: 1 }
                    Op::PushString(0), // "x"
                    Op::PushInt(1),
                    Op::New(1),
                    Op::SetL(0),
                    // Update object.x = 2 (triggers write barrier)
                    // SetF expects stack: [object, value] (value on top)
                    Op::GetL(0),    // push object
                    Op::PushInt(2), // push value
                    Op::SetF(0),    // str_idx 0 = "x", stores 2 in object.x
                    // Get the updated field to verify
                    Op::GetL(0),
                    Op::GetF(0),
                ],
                stackmap: None,
            },
            strings: vec!["x".to_string()],
            debug: None,
        };

        let mut vm = VM::new();
        let result = vm.run(&chunk);
        assert!(
            result.is_ok(),
            "SetF write barrier test failed: {:?}",
            result
        );

        // The last pushed value should be the updated field value (2)
        assert!(
            vm.stack.iter().any(|v| *v == Value::I64(2)),
            "Expected to find updated value 2 in stack"
        );
    }
}
