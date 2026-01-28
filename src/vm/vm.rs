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
use crate::jit::marshal::{JitContext, JitReturn, JitValue};

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
        match compiler.compile(func) {
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
        match compiler.compile(func) {
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
    ) -> Result<Value, String> {
        let compiled = self.jit_functions.get(&func_index).unwrap();

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

        // Get the function pointer
        // Signature: fn(vm_ctx: *mut u8, vstack: *mut JitValue, locals: *mut JitValue) -> JitReturn
        let entry: unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn =
            unsafe { compiled.entry_point() };

        // Execute the JIT code
        // Pass null for vm_ctx (not used currently), stack and locals pointers
        let result: JitReturn = unsafe { entry(std::ptr::null_mut(), ctx.stack, ctx.locals) };

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
    ) -> Result<Value, String> {
        let compiled = self.jit_functions.get(&func_index).unwrap();

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

        // Get the function pointer
        // Signature: fn(vm_ctx: *mut u8, vstack: *mut JitValue, locals: *mut JitValue) -> JitReturn
        let entry: unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn =
            unsafe { compiled.entry_point() };

        // Execute the JIT code
        // SAFETY: The compiled code follows AArch64 calling convention.
        // vm_ctx is null (not used currently), stack and locals are valid pointers.
        let result: JitReturn = unsafe { entry(std::ptr::null_mut(), ctx.stack, ctx.locals) };

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
            Op::AllocArray(n) => {
                let mut elements = Vec::with_capacity(n);
                for _ in 0..n {
                    elements.push(self.stack.pop().ok_or("stack underflow")?);
                }
                elements.reverse();
                let r = self.heap.alloc_array(elements)?;
                self.stack.push(Value::Ref(r));
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
                } else {
                    return Err("runtime error: len expects array or string".to_string());
                };
                self.stack.push(Value::I64(len));
            }
            Op::ArrayGet => {
                let index = self.pop_int()?;
                let val = self.stack.pop().ok_or("stack underflow")?;
                let r = val
                    .as_ref()
                    .ok_or("runtime error: expected array or string")?;
                let obj = self.heap.get(r).ok_or("runtime error: invalid reference")?;

                if let Some(arr) = obj.as_array() {
                    if index < 0 || index as usize >= arr.elements.len() {
                        return Err(format!(
                            "runtime error: array index {} out of bounds (length {})",
                            index,
                            arr.elements.len()
                        ));
                    }
                    let value = arr.elements[index as usize];
                    self.stack.push(value);
                } else if let Some(s) = obj.as_string() {
                    let bytes = s.value.as_bytes();
                    if index < 0 || index as usize >= bytes.len() {
                        return Err(format!(
                            "runtime error: string index {} out of bounds (length {})",
                            index,
                            bytes.len()
                        ));
                    }
                    let byte_value = bytes[index as usize] as i64;
                    self.stack.push(Value::I64(byte_value));
                } else {
                    return Err("runtime error: expected array or string".to_string());
                }
            }
            Op::ArraySet => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let index = self.pop_int()?;
                let arr = self.stack.pop().ok_or("stack underflow")?;
                let r = arr.as_ref().ok_or("runtime error: expected array")?;

                let obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let arr = obj.as_array_mut().ok_or("runtime error: expected array")?;

                if index < 0 || index as usize >= arr.elements.len() {
                    return Err(format!(
                        "runtime error: array index {} out of bounds (length {})",
                        index,
                        arr.elements.len()
                    ));
                }

                arr.elements[index as usize] = value;
            }
            Op::ArrayPush => {
                let value = self.stack.pop().ok_or("stack underflow")?;
                let arr = self.stack.pop().ok_or("stack underflow")?;
                let r = arr.as_ref().ok_or("runtime error: expected array")?;

                let obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let arr = obj.as_array_mut().ok_or("runtime error: expected array")?;
                arr.elements.push(value);
            }
            Op::ArrayPop => {
                let arr = self.stack.pop().ok_or("stack underflow")?;
                let r = arr.as_ref().ok_or("runtime error: expected array")?;

                let obj = self
                    .heap
                    .get_mut(r)
                    .ok_or("runtime error: invalid reference")?;
                let arr = obj.as_array_mut().ok_or("runtime error: expected array")?;

                let value = arr
                    .elements
                    .pop()
                    .ok_or("runtime error: cannot pop from empty array")?;
                self.stack.push(value);
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
            Op::Print => {
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

                // Create an array with [id, id] (sender and receiver share the channel)
                let arr = self
                    .heap
                    .alloc_array(vec![Value::I64(id as i64), Value::I64(id as i64)])?;
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
        let stack = run_code(vec![
            Op::PushInt(1),
            Op::PushInt(2),
            Op::PushInt(3),
            Op::AllocArray(3),
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
            // Allocate array and store in local 0
            Op::PushInt(1),
            Op::AllocArray(1),
            Op::SetL(0),
            // Allocate another array
            Op::PushInt(2),
            Op::AllocArray(1),
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
