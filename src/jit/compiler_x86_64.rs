//! JIT compiler for moca bytecode on x86-64.
//!
//! This module implements a baseline JIT compiler that translates moca bytecode
//! to x86-64 machine code using a template-based approach.

use super::codebuf::CodeBuffer;
use super::memory::ExecutableMemory;
use super::x86_64::{Cond, Reg, X86_64Assembler};
use crate::vm::{Function, Op, ValueType};
use std::collections::{HashMap, HashSet};

/// Value tag constants for JIT code.
/// Values are represented as 128-bit (tag: u64, payload: u64).
pub mod value_tags {
    pub const TAG_INT: u64 = 0;
    pub const TAG_FLOAT: u64 = 1;
    pub const TAG_BOOL: u64 = 2;
    pub const TAG_NIL: u64 = 3;
    pub const TAG_PTR: u64 = 4;
}

/// Register conventions for moca JIT on x86-64.
///
/// Following System V AMD64 ABI with moca-specific assignments:
/// - R12: VM context pointer (callee-saved)
/// - R13: Value stack pointer (callee-saved)
/// - R14: Locals base pointer (callee-saved)
/// - R15: Constants pool pointer (callee-saved)
/// - RAX, RCX, RDX, RSI, RDI, R8-R11: Temporaries and function arguments
pub mod regs {
    use super::Reg;

    pub const VM_CTX: Reg = Reg::R12;
    pub const VSTACK: Reg = Reg::R13;
    pub const LOCALS: Reg = Reg::R14;
    pub const CONSTS: Reg = Reg::R15;

    // Temporaries
    pub const TMP0: Reg = Reg::Rax;
    pub const TMP1: Reg = Reg::Rcx;
    pub const TMP2: Reg = Reg::Rdx;
    pub const TMP3: Reg = Reg::Rsi;
    pub const TMP4: Reg = Reg::R8;
    pub const TMP5: Reg = Reg::R9;
}

/// Size of a Value on the stack (128 bits = 16 bytes).
pub const VALUE_SIZE: i32 = 16;

/// Compiled JIT code for a function.
pub struct CompiledCode {
    /// The executable memory containing the compiled code
    pub memory: ExecutableMemory,
    /// Entry point offset within the memory
    pub entry_offset: usize,
    /// Stack map for GC (pc_offset -> bitmap of stack slots with refs)
    pub stack_map: HashMap<usize, Vec<bool>>,
}

impl CompiledCode {
    /// Get the entry point as a function pointer.
    ///
    /// # Safety
    /// The caller must ensure the function signature matches the expected ABI.
    pub unsafe fn entry_point<F>(&self) -> F
    where
        F: Copy,
    {
        unsafe {
            let ptr = self.memory.as_ptr().add(self.entry_offset);
            std::mem::transmute_copy(&ptr)
        }
    }
}

/// Compiled JIT code for a hot loop.
pub struct CompiledLoop {
    /// The executable memory containing the compiled code
    pub memory: ExecutableMemory,
    /// Entry point offset within the memory
    pub entry_offset: usize,
    /// Bytecode PC where the loop starts (backward jump target)
    pub loop_start_pc: usize,
    /// Bytecode PC where the loop ends (backward jump instruction)
    pub loop_end_pc: usize,
    /// Stack map for GC (pc_offset -> bitmap of stack slots with refs)
    pub stack_map: HashMap<usize, Vec<bool>>,
}

impl CompiledLoop {
    /// Get the entry point as a function pointer.
    ///
    /// # Safety
    /// The caller must ensure the function signature matches the expected ABI.
    pub unsafe fn entry_point<F>(&self) -> F
    where
        F: Copy,
    {
        unsafe {
            let ptr = self.memory.as_ptr().add(self.entry_offset);
            std::mem::transmute_copy(&ptr)
        }
    }
}

/// JIT compiler for moca functions on x86-64.
pub struct JitCompiler {
    buf: CodeBuffer,
    /// Labels for jump targets (bytecode pc -> native offset)
    labels: HashMap<usize, usize>,
    /// Forward references for jumps (native_offset, bytecode_target)
    forward_refs: Vec<(usize, usize)>,
    /// Stack map entries being built
    stack_map: HashMap<usize, Vec<bool>>,
    /// Current stack depth (number of values)
    stack_depth: usize,
    /// Index of the function being compiled (for self-recursion detection)
    self_func_index: Option<usize>,
    /// Number of locals for the function being compiled
    self_locals_count: usize,
    /// Type information for local variables (for type specialization)
    local_types: Vec<ValueType>,
    /// Type stack tracking value types during compilation
    type_stack: Vec<ValueType>,
}

impl JitCompiler {
    pub fn new() -> Self {
        Self {
            buf: CodeBuffer::new(),
            labels: HashMap::new(),
            forward_refs: Vec::new(),
            stack_map: HashMap::new(),
            stack_depth: 0,
            self_func_index: None,
            self_locals_count: 0,
            local_types: Vec::new(),
            type_stack: Vec::new(),
        }
    }

    /// Compile a function to native code.
    ///
    /// # Arguments
    /// * `func` - The function to compile
    /// * `func_index` - The index of this function (for self-recursion optimization)
    pub fn compile(mut self, func: &Function, func_index: usize) -> Result<CompiledCode, String> {
        // Store function info for self-recursion detection
        self.self_func_index = Some(func_index);
        self.self_locals_count = func.locals_count;
        self.local_types = func.local_types.clone();
        // Emit prologue
        self.emit_prologue(func);

        // Record entry point after prologue
        let entry_offset = 0; // Prologue is at the start

        // Pre-compute jump targets for peephole safety check
        let jump_targets: HashSet<usize> = func
            .code
            .iter()
            .filter_map(|op| match op {
                Op::Jmp(t) | Op::BrIfFalse(t) | Op::BrIf(t) => Some(*t),
                _ => None,
            })
            .collect();

        // Compile each instruction with peephole optimization
        let mut pc = 0;
        while pc < func.code.len() {
            // Record label for this bytecode PC
            self.labels.insert(pc, self.buf.len());

            let op = &func.code[pc];

            // Peephole optimization: fuse comparison + conditional jump
            let next_pc = pc + 1;
            if next_pc < func.code.len()
                && !jump_targets.contains(&next_pc)
                && let Some(cmp_cond) = Self::get_cmp_cond(op)
            {
                let next_op = &func.code[next_pc];
                match next_op {
                    Op::BrIfFalse(target) => {
                        self.pop2_types(); // cmp pops 2, jmpif pops the bool
                        self.emit_fused_cmp_jmp(cmp_cond, *target, true)?;
                        self.labels.insert(next_pc, self.buf.len());
                        pc += 2;
                        continue;
                    }
                    Op::BrIf(target) => {
                        self.pop2_types();
                        self.emit_fused_cmp_jmp(cmp_cond, *target, false)?;
                        self.labels.insert(next_pc, self.buf.len());
                        pc += 2;
                        continue;
                    }
                    _ => {}
                }
            }

            // Compile the operation
            self.compile_op(op, pc)?;
            pc += 1;
        }

        // Patch forward references
        self.patch_forward_refs();

        // Emit epilogue label for returns
        self.labels.insert(func.code.len(), self.buf.len());
        self.emit_epilogue();

        // Get the raw code bytes
        let code = self.buf.into_code();

        // Allocate executable memory
        let mut memory = ExecutableMemory::new(code.len())
            .map_err(|e| format!("Failed to allocate executable memory: {}", e))?;

        // Copy code to executable memory
        memory
            .write(0, &code)
            .map_err(|e| format!("Failed to write code: {}", e))?;

        // Make executable
        memory
            .make_executable()
            .map_err(|e| format!("Failed to make memory executable: {}", e))?;

        Ok(CompiledCode {
            memory,
            entry_offset,
            stack_map: self.stack_map,
        })
    }

    /// Compile a loop to native code.
    ///
    /// # Arguments
    /// * `func` - The function containing the loop
    /// * `loop_start_pc` - Bytecode PC where loop begins (backward jump target)
    /// * `loop_end_pc` - Bytecode PC of the backward jump instruction
    ///
    /// # Returns
    /// * `CompiledLoop` - The compiled loop code
    pub fn compile_loop(
        mut self,
        func: &Function,
        loop_start_pc: usize,
        loop_end_pc: usize,
        jit_compiled_funcs: &HashSet<usize>,
    ) -> Result<CompiledLoop, String> {
        // Check for unsupported operations in the loop
        // Call instructions are only allowed if the target function is already JIT compiled
        for pc in loop_start_pc..=loop_end_pc {
            if let Some(Op::Call(target_func_index, _)) = func.code.get(pc)
                && !jit_compiled_funcs.contains(target_func_index)
            {
                return Err(format!(
                    "Loop contains Call to non-JIT-compiled function {}",
                    target_func_index
                ));
            }
        }

        // Store function info
        self.self_locals_count = func.locals_count;
        self.local_types = func.local_types.clone();

        // Emit prologue (same as function)
        self.emit_prologue(func);

        // Record entry point after prologue
        let entry_offset = 0;

        // Pre-compute jump targets within the loop for peephole safety check.
        // If an instruction is a jump target, we cannot fuse it with its predecessor.
        let jump_targets: HashSet<usize> = (loop_start_pc..=loop_end_pc)
            .filter_map(|check_pc| match func.code.get(check_pc) {
                Some(Op::Jmp(t)) | Some(Op::BrIfFalse(t)) | Some(Op::BrIf(t)) => Some(*t),
                _ => None,
            })
            .collect();

        // Compile instructions from loop_start_pc to loop_end_pc (inclusive)
        let mut pc = loop_start_pc;
        while pc <= loop_end_pc {
            if pc >= func.code.len() {
                return Err(format!(
                    "Loop PC {} out of bounds (function has {} instructions)",
                    pc,
                    func.code.len()
                ));
            }

            let op = &func.code[pc];

            // Record label for this bytecode PC
            self.labels.insert(pc, self.buf.len());

            // Peephole optimization: fuse comparison + conditional jump
            let next_pc = pc + 1;
            if next_pc <= loop_end_pc
                && !jump_targets.contains(&next_pc)
                && let Some(cmp_cond) = Self::get_cmp_cond(op)
            {
                let next_op = &func.code[next_pc];
                match next_op {
                    Op::BrIfFalse(target) if *target > loop_end_pc => {
                        // Fused comparison + loop exit
                        self.pop2_types();
                        self.emit_fused_cmp_jmp(cmp_cond, func.code.len(), true)?;
                        self.labels.insert(next_pc, self.buf.len());
                        pc += 2;
                        continue;
                    }
                    Op::BrIfFalse(target) => {
                        // Fused comparison + internal branch
                        self.pop2_types();
                        self.emit_fused_cmp_jmp(cmp_cond, *target, true)?;
                        self.labels.insert(next_pc, self.buf.len());
                        pc += 2;
                        continue;
                    }
                    Op::BrIf(target) => {
                        // Fused comparison + conditional jump
                        self.pop2_types();
                        self.emit_fused_cmp_jmp(cmp_cond, *target, false)?;
                        self.labels.insert(next_pc, self.buf.len());
                        pc += 2;
                        continue;
                    }
                    _ => {}
                }
            }

            // Non-fused path
            match op {
                Op::BrIfFalse(target) if *target > loop_end_pc => {
                    // This is a loop exit condition - jump to epilogue
                    self.type_stack.pop();
                    self.emit_loop_exit_check(func.code.len())?;
                }
                Op::Jmp(target) if *target == loop_start_pc => {
                    // This is the backward branch - jump to loop start
                    self.emit_jmp(loop_start_pc)?;
                }
                _ => {
                    // Regular instruction
                    self.compile_op(op, pc)?;
                }
            }

            pc += 1;
        }

        // Emit epilogue label (for loop exit) - must be before patch_forward_refs
        // so the exit jump target is available for patching
        self.labels.insert(func.code.len(), self.buf.len());
        self.emit_epilogue();

        // Patch forward references (including loop exit jump)
        self.patch_forward_refs();

        // Get the raw code bytes
        let code = self.buf.into_code();

        // Allocate executable memory
        let mut memory = ExecutableMemory::new(code.len())
            .map_err(|e| format!("Failed to allocate executable memory: {}", e))?;

        // Copy code to executable memory
        memory
            .write(0, &code)
            .map_err(|e| format!("Failed to write code: {}", e))?;

        // Make executable
        memory
            .make_executable()
            .map_err(|e| format!("Failed to make memory executable: {}", e))?;

        Ok(CompiledLoop {
            memory,
            entry_offset,
            loop_start_pc,
            loop_end_pc,
            stack_map: self.stack_map,
        })
    }

    /// Emit code to check loop exit condition and jump to epilogue.
    /// This pops the condition value and jumps to epilogue if false.
    fn emit_loop_exit_check(&mut self, exit_label: usize) -> Result<(), String> {
        // Pop condition value (tag is at VSTACK-16, payload at VSTACK-8)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        }
        self.stack_depth = self.stack_depth.saturating_sub(1);

        // Load payload (boolean value)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VSTACK, 8);
        }

        // Test if false (0)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.test_rr(regs::TMP0, regs::TMP0);
        }

        // Record forward reference for conditional jump
        let jmp_offset = self.buf.len();
        self.forward_refs.push((jmp_offset, exit_label));

        // JE (jump if zero/false) to exit - placeholder offset
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0);
        }

        Ok(())
    }

    /// Emit function prologue.
    fn emit_prologue(&mut self, _func: &Function) {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Save frame pointer
        asm.push(Reg::Rbp);
        asm.mov_rr(Reg::Rbp, Reg::Rsp);

        // Save callee-saved registers
        asm.push(Reg::Rbx);
        asm.push(Reg::R12);
        asm.push(Reg::R13);
        asm.push(Reg::R14);
        asm.push(Reg::R15);

        // Initialize moca registers from arguments (System V AMD64 ABI)
        // RDI = VM context, RSI = value stack, RDX = locals base
        asm.mov_rr(regs::VM_CTX, Reg::Rdi);
        asm.mov_rr(regs::VSTACK, Reg::Rsi);
        asm.mov_rr(regs::LOCALS, Reg::Rdx);

        self.stack_depth = 0;
    }

    /// Emit function epilogue.
    fn emit_epilogue(&mut self) {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Restore callee-saved registers
        asm.pop(Reg::R15);
        asm.pop(Reg::R14);
        asm.pop(Reg::R13);
        asm.pop(Reg::R12);
        asm.pop(Reg::Rbx);

        // Restore frame pointer
        asm.pop(Reg::Rbp);

        // Return
        asm.ret();
    }

    /// Compile a single bytecode operation with type tracking.
    fn compile_op(&mut self, op: &Op, _pc: usize) -> Result<(), String> {
        match op {
            Op::I64Const(n) => {
                self.type_stack.push(ValueType::I64);
                self.emit_push_int(*n)
            }
            Op::I32Const(n) => {
                self.type_stack.push(ValueType::I32);
                self.emit_push_int(*n as i64)
            }
            Op::F64Const(f) => {
                self.type_stack.push(ValueType::F64);
                self.emit_push_float(*f)
            }
            Op::F32Const(f) => {
                self.type_stack.push(ValueType::F32);
                self.emit_push_float(*f as f64)
            }
            Op::RefNull => {
                self.type_stack.push(ValueType::Ref);
                self.emit_push_nil()
            }
            Op::Drop => {
                self.type_stack.pop();
                self.emit_pop()
            }

            Op::LocalGet(idx) => {
                self.type_stack.push(self.local_type(*idx));
                self.emit_load_local(*idx)
            }
            Op::LocalSet(idx) => {
                self.type_stack.pop();
                self.emit_store_local(*idx)
            }

            Op::I64Add => {
                let (a, b) = self.pop2_types();
                if a == ValueType::I64 && b == ValueType::I64 {
                    self.type_stack.push(ValueType::I64);
                    self.emit_add_int()
                } else {
                    self.type_stack.push(ValueType::I64);
                    self.emit_add()
                }
            }
            Op::I64Sub => {
                let (a, b) = self.pop2_types();
                if a == ValueType::I64 && b == ValueType::I64 {
                    self.type_stack.push(ValueType::I64);
                    self.emit_sub_int()
                } else {
                    self.type_stack.push(ValueType::I64);
                    self.emit_sub()
                }
            }
            Op::I64Mul => {
                let (a, b) = self.pop2_types();
                if a == ValueType::I64 && b == ValueType::I64 {
                    self.type_stack.push(ValueType::I64);
                    self.emit_mul_int()
                } else {
                    self.type_stack.push(ValueType::I64);
                    self.emit_mul()
                }
            }
            Op::I64DivS => {
                let (a, b) = self.pop2_types();
                if a == ValueType::I64 && b == ValueType::I64 {
                    self.type_stack.push(ValueType::I64);
                    self.emit_div_int()
                } else {
                    self.type_stack.push(ValueType::I64);
                    self.emit_div()
                }
            }

            Op::I64LtS => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::L)
            }
            Op::I64LeS => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::Le)
            }
            Op::I64GtS => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::G)
            }
            Op::I64GeS => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::Ge)
            }
            Op::I64Eq => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_eq()
            }
            Op::I64Ne => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_ne()
            }

            Op::Jmp(target) => self.emit_jmp(*target),
            Op::BrIfFalse(target) => {
                self.type_stack.pop();
                self.emit_jmp_if_false(*target)
            }
            Op::BrIf(target) => {
                self.type_stack.pop();
                self.emit_jmp_if_true(*target)
            }

            Op::Ret => {
                self.type_stack.pop();
                self.emit_ret()
            }

            Op::Call(func_index, argc) => {
                for _ in 0..*argc {
                    self.type_stack.pop();
                }
                self.type_stack.push(ValueType::I64);
                self.emit_call(*func_index, *argc)
            }

            Op::StringConst(idx) => {
                self.type_stack.push(ValueType::Ref);
                self.emit_push_string(*idx)
            }
            Op::Syscall(syscall_num, argc) => {
                for _ in 0..*argc {
                    self.type_stack.pop();
                }
                self.type_stack.push(ValueType::I64);
                self.emit_syscall(*syscall_num, *argc)
            }
            Op::I64Neg => {
                // Neg preserves the type
                self.emit_neg()
            }

            // F64 arithmetic operations
            Op::F64Add => {
                self.pop2_types();
                self.type_stack.push(ValueType::F64);
                self.emit_add()
            }
            Op::F64Sub => {
                self.pop2_types();
                self.type_stack.push(ValueType::F64);
                self.emit_sub()
            }
            Op::F64Mul => {
                self.pop2_types();
                self.type_stack.push(ValueType::F64);
                self.emit_mul()
            }
            Op::F64Div => {
                self.pop2_types();
                self.type_stack.push(ValueType::F64);
                self.emit_div()
            }
            Op::F64Neg => {
                // Neg preserves the type
                self.emit_neg()
            }
            // F64 comparison operations (use same integer comparison infrastructure)
            Op::F64Lt => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::L)
            }
            Op::F64Le => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::Le)
            }
            Op::F64Gt => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::G)
            }
            Op::F64Ge => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_cmp_int(Cond::Ge)
            }
            Op::F64Eq => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_eq()
            }
            Op::F64Ne => {
                self.pop2_types();
                self.type_stack.push(ValueType::I32);
                self.emit_ne()
            }

            // Heap operations
            Op::HeapLoad(slot) => {
                self.type_stack.pop();
                self.type_stack.push(ValueType::I64);
                self.emit_heap_load(*slot)
            }
            Op::HeapStore(slot) => {
                self.type_stack.pop();
                self.type_stack.pop();
                self.emit_heap_store(*slot)
            }
            Op::HeapLoadDyn => {
                self.type_stack.pop();
                self.type_stack.pop();
                self.type_stack.push(ValueType::I64);
                self.emit_heap_load_dyn()
            }
            Op::HeapStoreDyn => {
                self.type_stack.pop();
                self.type_stack.pop();
                self.type_stack.pop();
                self.emit_heap_store_dyn()
            }
            Op::HeapLoad2 => {
                self.type_stack.pop();
                self.type_stack.pop();
                self.type_stack.push(ValueType::I64);
                self.emit_heap_load2()
            }
            Op::HeapStore2 => {
                self.type_stack.pop();
                self.type_stack.pop();
                self.type_stack.pop();
                self.emit_heap_store2()
            }

            // Unsupported operations - fail compilation so VM falls back to interpreter
            _ => Err(format!("Unsupported operation for JIT: {:?}", op)),
        }
    }

    /// Push an integer onto the value stack.
    fn emit_push_int(&mut self, n: i64) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Store tag (0 = int)
        asm.mov_ri64(regs::TMP0, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP0);

        // Store value
        asm.mov_ri64(regs::TMP0, n);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);

        // Advance stack pointer
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Push a float onto the value stack.
    fn emit_push_float(&mut self, f: f64) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Store tag (1 = float)
        asm.mov_ri64(regs::TMP0, value_tags::TAG_FLOAT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP0);

        // Store float bits as u64
        let bits = f.to_bits();
        asm.mov_ri64(regs::TMP0, bits as i64);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);

        // Advance stack pointer
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Push a bool onto the value stack.
    fn emit_push_bool(&mut self, b: bool) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Store tag (2 = bool)
        asm.mov_ri64(regs::TMP0, value_tags::TAG_BOOL as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP0);

        // Store value
        asm.mov_ri64(regs::TMP0, if b { 1 } else { 0 });
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);

        // Advance stack pointer
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Push nil onto the value stack.
    fn emit_push_nil(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Store tag (3 = nil)
        asm.mov_ri64(regs::TMP0, value_tags::TAG_NIL as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP0);

        // Store 0 as payload
        asm.xor_rr(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);

        // Advance stack pointer
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Pop a value from the stack (discard).
    fn emit_pop(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Decrement stack pointer
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Load a local variable onto the stack.
    fn emit_load_local(&mut self, idx: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        let offset = (idx as i32) * VALUE_SIZE;

        // Load tag
        asm.mov_rm(regs::TMP0, regs::LOCALS, offset);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP0);

        // Load value
        asm.mov_rm(regs::TMP0, regs::LOCALS, offset + 8);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);

        // Advance stack pointer
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Store top of stack into a local variable.
    fn emit_store_local(&mut self, idx: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        let offset = (idx as i32) * VALUE_SIZE;

        // Decrement stack pointer first (peek and pop)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);

        // Load tag from stack
        asm.mov_rm(regs::TMP0, regs::VSTACK, 0);
        asm.mov_mr(regs::LOCALS, offset, regs::TMP0);

        // Load value from stack
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8);
        asm.mov_mr(regs::LOCALS, offset + 8, regs::TMP0);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Get the type of a local variable for specialization.
    fn local_type(&self, idx: usize) -> ValueType {
        self.local_types.get(idx).copied().unwrap_or(ValueType::I64)
    }

    /// Pop two types from the type stack and return them (a, b).
    fn pop2_types(&mut self) -> (ValueType, ValueType) {
        let b = self.type_stack.pop().unwrap_or(ValueType::I64);
        let a = self.type_stack.pop().unwrap_or(ValueType::I64);
        (a, b)
    }

    /// Int-specialized addition: skip tag checks, directly add payloads.
    fn emit_add_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a
        asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b
        asm.add_rr(regs::TMP0, regs::TMP1);
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        // a's tag slot is already TAG_INT (0), just write result payload
        asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Int-specialized subtraction: skip tag checks, directly subtract payloads.
    fn emit_sub_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a
        asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b
        asm.sub_rr(regs::TMP0, regs::TMP1);
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Int-specialized multiplication: skip tag checks, directly multiply payloads.
    fn emit_mul_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a
        asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b
        asm.imul_rr(regs::TMP0, regs::TMP1);
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Int-specialized division: skip tag checks, use idiv.
    fn emit_div_int(&mut self) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a (dividend)
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b (divisor)
            // idiv divides RDX:RAX by operand, result in RAX
            // Sign-extend RAX into RDX (cqo)
            asm.cqo();
            asm.idiv(regs::TMP1);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0); // RAX = quotient
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer addition: pop two values, push their sum.
    /// Addition (handles both int and float).
    fn emit_add(&mut self) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP2, regs::VSTACK, -VALUE_SIZE); // b_tag
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b_payload
            asm.mov_rm(regs::TMP3, regs::VSTACK, -2 * VALUE_SIZE); // a_tag
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a_payload
            asm.or_rr(regs::TMP2, regs::TMP3);
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let jne_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        // INT path
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.add_rr(regs::TMP0, regs::TMP1);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }
        let jmp_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let float_path = self.buf.len();
        {
            let offset = (float_path as i32) - (jne_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[jne_pos + 2..jne_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // FLOAT path
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8);
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8);
            asm.mov_rm(regs::TMP2, regs::VSTACK, -2 * VALUE_SIZE);
            asm.mov_rm(regs::TMP3, regs::VSTACK, -VALUE_SIZE);
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let a_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let a_is_float_pos = self.buf.len();
        {
            let offset = (a_is_float_pos as i32) - (a_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[a_is_int_pos + 2..a_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_float_done = self.buf.len();
        {
            let offset = (a_float_done as i32) - (a_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[a_conv_done + 1..a_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cmp_ri32(regs::TMP3, 0);
        }
        let b_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(1, regs::TMP1);
        }
        let b_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let b_is_float_pos = self.buf.len();
        {
            let offset = (b_is_float_pos as i32) - (b_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[b_is_int_pos + 2..b_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(1, regs::TMP1);
        }
        let b_float_done = self.buf.len();
        {
            let offset = (b_float_done as i32) - (b_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[b_conv_done + 1..b_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.addsd(0, 1);
            asm.movq_r64_xmm(regs::TMP0, 0);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }
        let end_pos = self.buf.len();
        {
            let offset = (end_pos as i32) - (jmp_end_pos as i32) - 5;
            let code = self.buf.code_mut();
            code[jmp_end_pos + 1..jmp_end_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }
        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Subtraction (handles both int and float).
    fn emit_sub(&mut self) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP2, regs::VSTACK, -VALUE_SIZE);
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8);
            asm.mov_rm(regs::TMP3, regs::VSTACK, -2 * VALUE_SIZE);
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8);
            asm.or_rr(regs::TMP2, regs::TMP3);
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let jne_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        // INT path
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_rr(regs::TMP0, regs::TMP1);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }
        let jmp_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let float_path = self.buf.len();
        {
            let offset = (float_path as i32) - (jne_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[jne_pos + 2..jne_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // FLOAT path
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8);
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8);
            asm.mov_rm(regs::TMP2, regs::VSTACK, -2 * VALUE_SIZE);
            asm.mov_rm(regs::TMP3, regs::VSTACK, -VALUE_SIZE);
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let a_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let a_is_float_pos = self.buf.len();
        {
            let offset = (a_is_float_pos as i32) - (a_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[a_is_int_pos + 2..a_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_float_done = self.buf.len();
        {
            let offset = (a_float_done as i32) - (a_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[a_conv_done + 1..a_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cmp_ri32(regs::TMP3, 0);
        }
        let b_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(1, regs::TMP1);
        }
        let b_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let b_is_float_pos = self.buf.len();
        {
            let offset = (b_is_float_pos as i32) - (b_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[b_is_int_pos + 2..b_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(1, regs::TMP1);
        }
        let b_float_done = self.buf.len();
        {
            let offset = (b_float_done as i32) - (b_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[b_conv_done + 1..b_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.subsd(0, 1);
            asm.movq_r64_xmm(regs::TMP0, 0);
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }
        let end_pos = self.buf.len();
        {
            let offset = (end_pos as i32) - (jmp_end_pos as i32) - 5;
            let code = self.buf.code_mut();
            code[jmp_end_pos + 1..jmp_end_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }
        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Multiplication (handles both int and float).
    fn emit_mul(&mut self) -> Result<(), String> {
        // Load both operands with their tags
        // Stack layout: [..., a_tag, a_payload, b_tag, b_payload] <- VSTACK
        // a is at VSTACK - 2*VALUE_SIZE, b is at VSTACK - VALUE_SIZE
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Load b (top of stack)
            asm.mov_rm(regs::TMP2, regs::VSTACK, -VALUE_SIZE); // b_tag
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b_payload
            // Load a
            asm.mov_rm(regs::TMP3, regs::VSTACK, -2 * VALUE_SIZE); // a_tag
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a_payload

            // Check if both are INT (tag == 0)
            asm.or_rr(regs::TMP2, regs::TMP3); // TMP2 = a_tag | b_tag
            asm.cmp_ri32(regs::TMP2, 0);
        }

        // Jump to float path if not both int
        let jne_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // placeholder
        }

        // INT path: multiply using imul
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.imul_rr(regs::TMP0, regs::TMP1);
            // Pop one value (result stays at top-1 position)
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            // Store result (tag stays INT=0)
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }

        // Jump to end
        let jmp_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder
        }

        // Patch jne to float path
        let float_path = self.buf.len();
        {
            let offset = (float_path as i32) - (jne_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[jne_pos + 2..jne_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }

        // FLOAT path: multiply using SSE
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Reload a and b (tags already checked, we know at least one is FLOAT)
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a_payload
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b_payload
            asm.mov_rm(regs::TMP2, regs::VSTACK, -2 * VALUE_SIZE); // a_tag
            asm.mov_rm(regs::TMP3, regs::VSTACK, -VALUE_SIZE); // b_tag

            // Convert a to float if needed (if a_tag == 0, convert)
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let a_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // jump if a is already float
        }
        // a is int, convert to float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // jump over the "a is float" case
        }
        // Patch jump for "a is already float"
        let a_is_float_pos = self.buf.len();
        {
            let offset = (a_is_float_pos as i32) - (a_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[a_is_int_pos + 2..a_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // a is already float, just move to xmm0
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0); // no-op but keeps consistent
        }
        let a_float_done = self.buf.len();
        {
            let offset = (a_float_done as i32) - (a_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[a_conv_done + 1..a_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }

        // Now convert b to float if needed
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cmp_ri32(regs::TMP3, 0);
        }
        let b_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // jump if b is already float
        }
        // b is int, convert to float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(1, regs::TMP1);
        }
        let b_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        // Patch jump for "b is already float"
        let b_is_float_pos = self.buf.len();
        {
            let offset = (b_is_float_pos as i32) - (b_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[b_is_int_pos + 2..b_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // b is already float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(1, regs::TMP1);
        }
        let b_float_done = self.buf.len();
        {
            let offset = (b_float_done as i32) - (b_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[b_conv_done + 1..b_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }

        // Now a is in xmm0 (bits in TMP0), b is in xmm1
        // Move a to xmm0 and multiply
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.mulsd(0, 1);
            // Move result back to GP register
            asm.movq_r64_xmm(regs::TMP0, 0);
            // Pop one value
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            // Store float result
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }

        // Patch jump to end from int path
        let end_pos = self.buf.len();
        {
            let offset = (end_pos as i32) - (jmp_end_pos as i32) - 5;
            let code = self.buf.code_mut();
            code[jmp_end_pos + 1..jmp_end_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Division (handles both int and float).
    fn emit_div(&mut self) -> Result<(), String> {
        // Load both operands with their tags
        // Stack layout: [..., a_tag, a_payload, b_tag, b_payload] <- VSTACK
        // a is at VSTACK - 2*VALUE_SIZE, b is at VSTACK - VALUE_SIZE
        // Result = a / b
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Load b (top of stack) - divisor
            asm.mov_rm(regs::TMP2, regs::VSTACK, -VALUE_SIZE); // b_tag
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b_payload
            // Load a - dividend
            asm.mov_rm(regs::TMP3, regs::VSTACK, -2 * VALUE_SIZE); // a_tag
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a_payload

            // Check if both are INT (tag == 0)
            asm.or_rr(regs::TMP2, regs::TMP3); // TMP2 = a_tag | b_tag
            asm.cmp_ri32(regs::TMP2, 0);
        }

        // Jump to float path if not both int
        let jne_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // placeholder
        }

        // INT path: divide using idiv
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // TMP0 = a (dividend), TMP1 = b (divisor)
            // For idiv, dividend must be in RDX:RAX
            asm.mov_rr(Reg::Rax, regs::TMP0);
            asm.cqo(); // Sign-extend RAX into RDX:RAX
            asm.idiv(regs::TMP1); // quotient in RAX

            // Pop one value
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            // Store result
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, Reg::Rax);
        }

        // Jump to end
        let jmp_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder
        }

        // Patch jne to float path
        let float_path = self.buf.len();
        {
            let offset = (float_path as i32) - (jne_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[jne_pos + 2..jne_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }

        // FLOAT path: divide using SSE
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Reload a and b
            asm.mov_rm(regs::TMP0, regs::VSTACK, -2 * VALUE_SIZE + 8); // a_payload
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // b_payload
            asm.mov_rm(regs::TMP2, regs::VSTACK, -2 * VALUE_SIZE); // a_tag
            asm.mov_rm(regs::TMP3, regs::VSTACK, -VALUE_SIZE); // b_tag

            // Convert a to float if needed
            asm.cmp_ri32(regs::TMP2, 0);
        }
        let a_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        // a is int, convert to float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let a_is_float_pos = self.buf.len();
        {
            let offset = (a_is_float_pos as i32) - (a_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[a_is_int_pos + 2..a_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // a is already float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_r64_xmm(regs::TMP0, 0);
        }
        let a_float_done = self.buf.len();
        {
            let offset = (a_float_done as i32) - (a_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[a_conv_done + 1..a_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }

        // Convert b to float if needed
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cmp_ri32(regs::TMP3, 0);
        }
        let b_is_int_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0);
        }
        // b is int, convert to float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.cvtsi2sd_xmm_r64(1, regs::TMP1);
        }
        let b_conv_done = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }
        let b_is_float_pos = self.buf.len();
        {
            let offset = (b_is_float_pos as i32) - (b_is_int_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[b_is_int_pos + 2..b_is_int_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }
        // b is already float
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(1, regs::TMP1);
        }
        let b_float_done = self.buf.len();
        {
            let offset = (b_float_done as i32) - (b_conv_done as i32) - 5;
            let code = self.buf.code_mut();
            code[b_conv_done + 1..b_conv_done + 5].copy_from_slice(&offset.to_le_bytes());
        }

        // Divide: xmm0 = xmm0 / xmm1
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.divsd(0, 1);
            // Move result back to GP register
            asm.movq_r64_xmm(regs::TMP0, 0);
            // Pop one value
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            // Store float result
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE, regs::TMP1);
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP0);
        }

        // Patch jump to end from int path
        let end_pos = self.buf.len();
        {
            let offset = (end_pos as i32) - (jmp_end_pos as i32) - 5;
            let code = self.buf.code_mut();
            code[jmp_end_pos + 1..jmp_end_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Map a bytecode comparison Op to its x86-64 condition code.
    fn get_cmp_cond(op: &Op) -> Option<Cond> {
        match op {
            Op::I64LtS => Some(Cond::L),
            Op::I64LeS => Some(Cond::Le),
            Op::I64GtS => Some(Cond::G),
            Op::I64GeS => Some(Cond::Ge),
            Op::I64Eq => Some(Cond::E),
            Op::I64Ne => Some(Cond::Ne),
            _ => None,
        }
    }

    /// Fused compare + conditional jump: pop two values, compare, and jump directly.
    /// Eliminates the intermediate boolean push/pop from separate cmp + jmpif.
    ///
    /// For JmpIfFalse: jumps when comparison is FALSE (inverted condition)
    /// For JmpIfTrue: jumps when comparison is TRUE (original condition)
    fn emit_fused_cmp_jmp(
        &mut self,
        cmp_cond: Cond,
        target_pc: usize,
        invert: bool,
    ) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Pop two operands
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_rm(regs::TMP1, regs::VSTACK, 8); // b (rhs)
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // a (lhs)
            // Compare a vs b
            asm.cmp_rr(regs::TMP0, regs::TMP1);
        }

        // For JmpIfFalse: jump when comparison is FALSE -> invert condition
        // For JmpIfTrue: jump when comparison is TRUE -> use original condition
        let jmp_cond = if invert { cmp_cond.invert() } else { cmp_cond };

        // Record forward reference and emit conditional jump
        let current = self.buf.len();
        self.forward_refs.push((current, target_pc));
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(jmp_cond, 0);
        }

        // Consumed 2 stack values, pushed none
        self.stack_depth = self.stack_depth.saturating_sub(2);
        Ok(())
    }

    /// Integer comparison: pop two values, push bool result.
    fn emit_cmp_int(&mut self, cond: Cond) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop two operands
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP1, regs::VSTACK, 8); // b
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // a

        // Compare a vs b
        asm.cmp_rr(regs::TMP0, regs::TMP1);

        // Set result based on condition (SETcc sets low byte)
        asm.setcc(cond, regs::TMP0);
        // Zero-extend to 64-bit
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);

        // Push bool result
        asm.mov_ri64(regs::TMP1, value_tags::TAG_BOOL as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP1);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Equality comparison.
    fn emit_eq(&mut self) -> Result<(), String> {
        self.emit_cmp_int(Cond::E)
    }

    /// Not-equal comparison.
    fn emit_ne(&mut self) -> Result<(), String> {
        self.emit_cmp_int(Cond::Ne)
    }

    /// Unconditional jump.
    fn emit_jmp(&mut self, target: usize) -> Result<(), String> {
        // Record forward reference if target is ahead
        let current = self.buf.len();
        self.forward_refs.push((current, target));

        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Emit placeholder that will be patched
        asm.jmp_rel32(0);

        Ok(())
    }

    /// Jump if top of stack is false (pop value).
    fn emit_jmp_if_false(&mut self, target: usize) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Pop value and load it
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // value

            // Test if zero (false)
            asm.test_rr(regs::TMP0, regs::TMP0);
        }

        // Record forward reference and emit branch
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::E, 0); // JE (jump if zero)
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Jump if top of stack is true (pop value).
    fn emit_jmp_if_true(&mut self, target: usize) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Pop value and load it
            asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
            asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // value

            // Test if non-zero (true)
            asm.test_rr(regs::TMP0, regs::TMP0);
        }

        // Record forward reference and emit branch
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // JNE (jump if non-zero)
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Return from function.
    fn emit_ret(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop return value from JIT stack into RAX (tag) and RDX (payload)
        // Stack layout: [tag: 8 bytes][payload: 8 bytes]
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(Reg::Rax, regs::VSTACK, 0); // tag
        asm.mov_rm(Reg::Rdx, regs::VSTACK, 8); // payload

        // Restore callee-saved registers and return
        asm.pop(Reg::R15);
        asm.pop(Reg::R14);
        asm.pop(Reg::R13);
        asm.pop(Reg::R12);
        asm.pop(Reg::Rbx);
        asm.pop(Reg::Rbp);
        asm.ret();

        Ok(())
    }

    /// Emit HeapLoad(n): pop ref, load slot n from heap object, push value.
    /// Heap layout: [Header(1w) | Tag0 | Val0 | Tag1 | Val1 | ...]
    /// Address: heap_base + (ref_payload + 1 + 2*n) * 8
    fn emit_heap_load(&mut self, slot: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        // TMP0 = ref payload (heap word offset)
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8);

        // TMP1 = heap_base (from JitCallContext offset 48)
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Calculate address: TMP0 = ref_payload + 1 + 2*slot
        let slot_offset = (1 + 2 * slot) as i32;
        asm.add_ri32(regs::TMP0, slot_offset);
        // TMP0 = TMP0 * 8 (convert word offset to byte offset)
        asm.shl_ri(regs::TMP0, 3);
        // TMP1 = heap_base + byte_offset
        asm.add_rr(regs::TMP1, regs::TMP0);

        // Load tag and payload from heap
        asm.mov_rm(regs::TMP2, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP3, regs::TMP1, 8); // payload

        // Push value to VSTACK
        asm.mov_mr(regs::VSTACK, 0, regs::TMP2); // tag
        asm.mov_mr(regs::VSTACK, 8, regs::TMP3); // payload
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 0; // net: pop 1, push 1
        Ok(())
    }

    /// Emit HeapStore(n): pop value and ref, store value into slot n.
    fn emit_heap_store(&mut self, slot: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop value from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP2, regs::VSTACK, 0); // value tag
        asm.mov_rm(regs::TMP3, regs::VSTACK, 8); // value payload

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // ref payload (heap word offset)

        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Calculate address: heap_base + (ref + 1 + 2*slot) * 8
        let slot_offset = (1 + 2 * slot) as i32;
        asm.add_ri32(regs::TMP0, slot_offset);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);

        // Store tag and payload to heap
        asm.mov_mr(regs::TMP1, 0, regs::TMP2); // tag
        asm.mov_mr(regs::TMP1, 8, regs::TMP3); // payload

        self.stack_depth = self.stack_depth.saturating_sub(2);
        Ok(())
    }

    /// Emit HeapLoadDyn: pop index and ref, load slot at dynamic index, push value.
    fn emit_heap_load_dyn(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop index from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP2, regs::VSTACK, 8); // index payload

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // ref payload (heap word offset)

        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Calculate slot address: ref + 1 + 2*index
        // TMP2 = index * 2
        asm.shl_ri(regs::TMP2, 1);
        // TMP0 = ref + 1
        asm.add_ri32(regs::TMP0, 1);
        // TMP0 = ref + 1 + 2*index
        asm.add_rr(regs::TMP0, regs::TMP2);
        // Convert to byte offset
        asm.shl_ri(regs::TMP0, 3);
        // TMP1 = heap_base + byte_offset
        asm.add_rr(regs::TMP1, regs::TMP0);

        // Load tag and payload
        asm.mov_rm(regs::TMP2, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP3, regs::TMP1, 8); // payload

        // Push value to VSTACK
        asm.mov_mr(regs::VSTACK, 0, regs::TMP2);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP3);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth -= 1; // net: pop 2, push 1
        Ok(())
    }

    /// Emit HeapStoreDyn: pop value, index, and ref, store value at dynamic index.
    fn emit_heap_store_dyn(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop value from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP4, regs::VSTACK, 0); // value tag
        asm.mov_rm(regs::TMP5, regs::VSTACK, 8); // value payload

        // Pop index from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP2, regs::VSTACK, 8); // index payload

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // ref payload

        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Calculate slot address: ref + 1 + 2*index
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);

        // Store tag and payload to heap
        asm.mov_mr(regs::TMP1, 0, regs::TMP4); // tag
        asm.mov_mr(regs::TMP1, 8, regs::TMP5); // payload

        self.stack_depth = self.stack_depth.saturating_sub(3);
        Ok(())
    }

    /// Emit HeapLoad2: pop index and ref, load heap[heap[ref][0]][idx], push value.
    fn emit_heap_load2(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop index from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP2, regs::VSTACK, 8); // index payload

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // ref payload (heap word offset)

        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: Load ptr field (slot 0) from ref
        // ptr_addr = ref + 1 + 2*0 = ref + 1
        asm.add_ri32(regs::TMP0, 1);
        asm.shl_ri(regs::TMP0, 3); // byte offset
        asm.add_rr(regs::TMP0, regs::TMP1); // TMP0 = heap_base + byte_offset
        // TMP0 = ptr payload (the inner array reference)
        asm.mov_rm(regs::TMP0, regs::TMP0, 8); // payload of slot 0

        // Step 2: Load element at dynamic index from ptr
        // slot_addr = ptr + 1 + 2*index
        asm.shl_ri(regs::TMP2, 1); // TMP2 = index * 2
        asm.add_ri32(regs::TMP0, 1); // TMP0 = ptr + 1
        asm.add_rr(regs::TMP0, regs::TMP2); // TMP0 = ptr + 1 + 2*index
        asm.shl_ri(regs::TMP0, 3); // byte offset
        asm.add_rr(regs::TMP0, regs::TMP1); // TMP0 = heap_base + byte_offset

        // Load tag and payload
        asm.mov_rm(regs::TMP2, regs::TMP0, 0); // tag
        asm.mov_rm(regs::TMP3, regs::TMP0, 8); // payload

        // Push value to VSTACK
        asm.mov_mr(regs::VSTACK, 0, regs::TMP2);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP3);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth -= 1; // net: pop 2, push 1
        Ok(())
    }

    /// Emit HeapStore2: pop value, index, and ref, store value at heap[heap[ref][0]][idx].
    fn emit_heap_store2(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop value from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP4, regs::VSTACK, 0); // value tag
        asm.mov_rm(regs::TMP5, regs::VSTACK, 8); // value payload

        // Pop index from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP2, regs::VSTACK, 8); // index payload

        // Pop ref from VSTACK
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // ref payload

        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: Load ptr field (slot 0) from ref
        asm.add_ri32(regs::TMP0, 1);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP0, regs::TMP1); // TMP0 = heap_base + byte_offset
        asm.mov_rm(regs::TMP0, regs::TMP0, 8); // TMP0 = ptr payload

        // Step 2: Store value at dynamic index in ptr
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP0, regs::TMP1); // TMP0 = heap_base + slot_byte_offset

        // Store tag and payload to heap
        asm.mov_mr(regs::TMP0, 0, regs::TMP4);
        asm.mov_mr(regs::TMP0, 8, regs::TMP5);

        self.stack_depth = self.stack_depth.saturating_sub(3);
        Ok(())
    }

    /// Emit a function call.
    /// Uses direct call for self-recursion, otherwise falls back to runtime helper.
    fn emit_call(&mut self, target_func_index: usize, argc: usize) -> Result<(), String> {
        // Check for self-recursion: if calling ourselves, use direct call
        if self.self_func_index == Some(target_func_index) {
            return self.emit_call_self(argc);
        }
        self.emit_call_external(target_func_index, argc)
    }

    /// Emit a direct self-recursive call (optimized path).
    ///
    /// This avoids going through jit_call_helper by:
    /// 1. Allocating new locals on native stack
    /// 2. Copying arguments to new locals
    /// 3. Calling entry point directly (offset 0)
    /// 4. Deallocating locals
    fn emit_call_self(&mut self, argc: usize) -> Result<(), String> {
        let args_offset = (argc as i32) * VALUE_SIZE;
        let locals_size = (self.self_locals_count as i32) * VALUE_SIZE;
        // Allocate new locals on native stack
        // We need locals_count * 16 bytes, but must maintain 16-byte alignment
        // After 3 pushes (24 bytes) + prologue (48 bytes) = 72 bytes from entry RSP-8
        // Total: 72 + 8 = 80 bytes, which is 16-byte aligned
        // Adding locals_size must keep alignment, so round up to multiple of 16
        let aligned_locals_size = ((locals_size + 15) / 16) * 16;
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);

            // Save callee-saved registers
            asm.push(regs::VM_CTX); // R12
            asm.push(regs::VSTACK); // R13
            asm.push(regs::LOCALS); // R14

            if aligned_locals_size > 0 {
                asm.sub_ri32(Reg::Rsp, aligned_locals_size);
            }

            // Copy arguments from VSTACK to new locals (first argc slots)
            // Arguments are at [VSTACK - argc*16, VSTACK)
            // New locals are at [RSP, RSP + locals_size)
            for i in 0..argc {
                let src_offset = -((argc - i) as i32) * VALUE_SIZE; // Relative to VSTACK
                let dst_offset = (i as i32) * VALUE_SIZE; // Relative to RSP

                // Load tag and payload from VSTACK
                asm.mov_rm(regs::TMP0, regs::VSTACK, src_offset); // tag
                asm.mov_rm(regs::TMP1, regs::VSTACK, src_offset + 8); // payload

                // Store to new locals on stack
                asm.mov_mr(Reg::Rsp, dst_offset, regs::TMP0); // tag
                asm.mov_mr(Reg::Rsp, dst_offset + 8, regs::TMP1); // payload
            }

            // DON'T adjust VSTACK here - callee starts at our current VSTACK position
            // This way callee's stack operations don't overwrite our stack values
            // We'll pop the args AFTER the call returns

            // Set up call arguments:
            // RDI = VM_CTX (R12) - same context
            // RSI = VSTACK (R13) - callee starts here (after our args)
            // RDX = new locals (RSP)
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_rr(Reg::Rsi, regs::VSTACK);
            asm.mov_rr(Reg::Rdx, Reg::Rsp);
        }

        // Call self (entry point is at offset 0 from function start)
        // We need to compute the relative offset from current position to entry
        // Since we're in the middle of the function, we need a backward jump
        // Use call rel32 instruction
        let call_site = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.call_rel32(0); // Placeholder, will be patched
        }

        // Calculate offset from end of call instruction to entry (offset 0)
        // call rel32 is 5 bytes, so after emitting, we're at call_site + 5
        // Relative offset = target - (call_site + 5) = 0 - (call_site + 5) = -(call_site + 5)
        let rel_offset = -((call_site + 5) as i32);

        // Patch the call instruction
        let code = self.buf.code_mut();
        code[call_site + 1..call_site + 5].copy_from_slice(&rel_offset.to_le_bytes());

        // Deallocate locals
        if aligned_locals_size > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.add_ri32(Reg::Rsp, aligned_locals_size);
        }

        // Restore saved registers and push return value
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::LOCALS);
            asm.pop(regs::VSTACK);
            asm.pop(regs::VM_CTX);

            // Now pop the arguments that were on our stack before the call
            // VSTACK was restored to its original value (after the args)
            asm.sub_ri32(regs::VSTACK, args_offset);

            // Return value is in RAX (tag) and RDX (payload)
            // Store it where the first arg was, then advance VSTACK by one slot
            asm.mov_mr(regs::VSTACK, 0, Reg::Rax); // store tag
            asm.mov_mr(regs::VSTACK, 8, Reg::Rdx); // store payload
            asm.add_ri32(regs::VSTACK, VALUE_SIZE);
        }

        // Update stack depth: -argc + 1
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

        Ok(())
    }

    /// Emit an external function call via runtime helper.
    ///
    /// Call convention for runtime helper (System V AMD64):
    ///   RDI = ctx (*mut JitCallContext, from VM_CTX register)
    ///   RSI = func_index
    ///   RDX = argc
    ///   RCX = args pointer (points to argc JitValues on our stack)
    ///
    /// The helper returns JitReturn in RAX (tag) and RDX (payload).
    fn emit_call_external(&mut self, func_index: usize, argc: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Arguments are already on the JIT value stack (VSTACK).
        // We need to compute the pointer to the first argument.
        // VSTACK points to the next free slot, so args start at VSTACK - argc * VALUE_SIZE

        let args_offset = (argc as i32) * VALUE_SIZE;

        // Save callee-saved registers that we use and that the helper might clobber
        // when calling into other JIT functions.
        // We push 3 registers for 16-byte stack alignment (6 in prologue + 3 = 9 total = 72 bytes)
        // After return address (8 bytes), 72 + 8 = 80, which is 16-byte aligned.
        asm.push(regs::VM_CTX); // R12 - save our JitCallContext pointer
        asm.push(regs::VSTACK); // R13 - save our value stack pointer (CRITICAL!)
        asm.push(regs::LOCALS); // R14 - save our locals pointer

        // Calculate args pointer: RCX = VSTACK - argc * VALUE_SIZE
        // Note: VSTACK (R13) still has original value, we just pushed a copy
        asm.mov_rr(Reg::Rcx, regs::VSTACK);
        asm.sub_ri32(Reg::Rcx, args_offset);

        // Set up arguments for call_helper:
        // RDI = ctx (VM_CTX register, which is R12)
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);

        // RSI = func_index
        asm.mov_ri64(Reg::Rsi, func_index as i64);

        // RDX = argc
        asm.mov_ri64(Reg::Rdx, argc as i64);

        // RCX = args pointer (already set above)

        // Load the call_helper function pointer from JitCallContext.
        // JitCallContext layout:
        //   offset 0: vm (*mut u8)
        //   offset 8: chunk (*const u8)
        //   offset 16: call_helper (fn pointer)
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 16); // R8 = ctx->call_helper

        // Stack alignment calculation:
        // Entry: RSP = X - 8 (X was 16-aligned, return addr pushed by caller)
        // After prologue (6 pushes): RSP = X - 8 - 48 = X - 56 (8 bytes off alignment)
        // After our 3 pushes: RSP = X - 56 - 24 = X - 80 (16-byte aligned!)
        // So no extra alignment push needed.

        // Call the helper function
        asm.call_r(regs::TMP4);

        // Restore saved registers (in reverse order)
        asm.pop(regs::LOCALS);
        asm.pop(regs::VSTACK);
        asm.pop(regs::VM_CTX);

        // Pop the arguments from JIT stack (they've been consumed)
        // VSTACK is now restored to its original value (after the arguments)
        asm.sub_ri32(regs::VSTACK, args_offset);

        // Push the return value onto the JIT stack
        // Return value is in RAX (tag) and RDX (payload)
        asm.mov_mr(regs::VSTACK, 0, Reg::Rax); // store tag
        asm.mov_mr(regs::VSTACK, 8, Reg::Rdx); // store payload
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        // Update stack depth: -argc + 1 (pop args, push result)
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

        Ok(())
    }

    /// Emit PushString operation.
    /// Calls push_string_helper to allocate string on heap and push Ref to stack.
    /// Emit PushString operation with string constant caching.
    ///
    /// JitCallContext layout:
    /// - offset 56: string_cache (*const Option<GcRef>)
    ///
    /// Option<GcRef> layout (16 bytes):
    /// - offset 0: discriminant (0=None, non-0=Some)
    /// - offset 8: value (GcRef.index if Some)
    fn emit_push_string(&mut self, string_index: usize) -> Result<(), String> {
        // Calculate cache entry address: string_cache + string_index * 16
        // Load string_cache pointer from JitCallContext (offset 56)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VM_CTX, 56); // TMP0 = string_cache

            // Calculate offset and add: TMP0 = string_cache + string_index * 16
            asm.add_ri32(regs::TMP0, (string_index * 16) as i32);

            // Load discriminant (tag) from cache entry
            asm.mov_rm(regs::TMP1, regs::TMP0, 0); // TMP1 = discriminant

            // Check if discriminant is 0 (None) - need to call helper
            asm.test_rr(regs::TMP1, regs::TMP1);
        }

        // JE (jump if equal/zero) to helper - placeholder
        let je_to_helper_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0); // Will patch later
        }

        // === FAST PATH: Cache hit ===
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Load cached ref value from cache entry (offset 8)
            asm.mov_rm(regs::TMP1, regs::TMP0, 8); // TMP1 = cached GcRef.index

            // Push Ref onto JIT stack
            // Store tag (4 = PTR)
            asm.mov_ri64(regs::TMP0, value_tags::TAG_PTR as i64);
            asm.mov_mr(regs::VSTACK, 0, regs::TMP0);
            // Store payload (ref index)
            asm.mov_mr(regs::VSTACK, 8, regs::TMP1);
            asm.add_ri32(regs::VSTACK, VALUE_SIZE);
        }

        // JMP to end (skip slow path) - placeholder
        let jmp_to_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // Will patch later
        }

        // === SLOW PATH: Cache miss - call helper ===
        let helper_start_pos = self.buf.len();

        // Patch JE to jump here
        {
            let offset = (helper_start_pos as i32) - (je_to_helper_pos as i32) - 6; // 6 = size of JE rel32
            let code = self.buf.code_mut();
            code[je_to_helper_pos + 2..je_to_helper_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }

        // Save callee-saved registers and call helper
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::VSTACK);
            asm.push(regs::LOCALS);

            // Set up arguments for push_string_helper:
            // RDI = ctx (VM_CTX register)
            // RSI = string_index
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_ri64(Reg::Rsi, string_index as i64);

            // Load push_string_helper from JitCallContext (offset 24)
            asm.mov_rm(regs::TMP4, regs::VM_CTX, 24);

            // Call the helper
            asm.call_r(regs::TMP4);

            // Restore saved registers
            asm.pop(regs::LOCALS);
            asm.pop(regs::VSTACK);
            asm.pop(regs::VM_CTX);

            // Push the return value onto the JIT stack
            // Return value is in RAX (tag) and RDX (payload)
            asm.mov_mr(regs::VSTACK, 0, Reg::Rax);
            asm.mov_mr(regs::VSTACK, 8, Reg::Rdx);
            asm.add_ri32(regs::VSTACK, VALUE_SIZE);
        }

        // === END ===
        let end_pos = self.buf.len();

        // Patch JMP to jump here
        {
            let offset = (end_pos as i32) - (jmp_to_end_pos as i32) - 5; // 5 = size of JMP rel32
            let code = self.buf.code_mut();
            code[jmp_to_end_pos + 1..jmp_to_end_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }

        self.stack_depth += 1;

        Ok(())
    }

    /// Emit Syscall operation.
    /// Pops argc arguments, calls syscall_helper, pushes result.
    fn emit_syscall(&mut self, syscall_num: usize, argc: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        let args_offset = (argc as i32) * VALUE_SIZE;

        // Save callee-saved registers
        asm.push(regs::VM_CTX);
        asm.push(regs::VSTACK);
        asm.push(regs::LOCALS);

        // Calculate args pointer: R8 = VSTACK - argc * VALUE_SIZE
        asm.mov_rr(Reg::R8, regs::VSTACK);
        asm.sub_ri32(Reg::R8, args_offset);

        // Set up arguments for syscall_helper:
        // RDI = ctx (VM_CTX register)
        // RSI = syscall_num
        // RDX = argc
        // RCX = args pointer
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_ri64(Reg::Rsi, syscall_num as i64);
        asm.mov_ri64(Reg::Rdx, argc as i64);
        asm.mov_rr(Reg::Rcx, Reg::R8);

        // Load syscall_helper from JitCallContext (offset 40)
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 40);

        // Call the helper
        asm.call_r(regs::TMP4);

        // Restore saved registers
        asm.pop(regs::LOCALS);
        asm.pop(regs::VSTACK);
        asm.pop(regs::VM_CTX);

        // Pop the arguments from JIT stack
        asm.sub_ri32(regs::VSTACK, args_offset);

        // Push the return value onto the JIT stack
        asm.mov_mr(regs::VSTACK, 0, Reg::Rax);
        asm.mov_mr(regs::VSTACK, 8, Reg::Rdx);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        // Update stack depth: -argc + 1 (pop args, push result)
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

        Ok(())
    }

    /// Emit Neg operation.
    /// Negates the top value on the stack (int or float).
    fn emit_neg(&mut self) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);

            // Load tag and payload from top of stack
            // Stack layout: [tag: 8 bytes][payload: 8 bytes]
            asm.mov_rm(regs::TMP0, regs::VSTACK, -VALUE_SIZE); // tag
            asm.mov_rm(regs::TMP1, regs::VSTACK, -VALUE_SIZE + 8); // payload

            // Check if it's an int (tag == 0)
            asm.cmp_ri32(regs::TMP0, value_tags::TAG_INT as i32);
        }

        // Record position for conditional jump
        let jne_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(Cond::Ne, 0); // placeholder, will patch
        }

        // INT path: negate using neg instruction
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.neg(regs::TMP1);
            // Store back
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP1);
        }

        // Jump over float path
        let jmp_end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder, will patch
        }

        // Patch the JNE to jump here (float path)
        let float_path_start = self.buf.len();
        {
            let rel_offset = (float_path_start as i32) - (jne_pos as i32) - 6; // Jcc rel32 is 6 bytes
            let code = self.buf.code_mut();
            code[jne_pos + 2..jne_pos + 6].copy_from_slice(&rel_offset.to_le_bytes());
        }

        // FLOAT path: XOR sign bit (bit 63) to negate
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Load sign bit mask: 0x8000000000000000
            asm.mov_ri64(regs::TMP2, 0x8000000000000000u64 as i64);
            // XOR to flip sign bit
            asm.xor_rr(regs::TMP1, regs::TMP2);
            // Store back
            asm.mov_mr(regs::VSTACK, -VALUE_SIZE + 8, regs::TMP1);
        }

        // Patch the JMP to jump here (end)
        let end_pos = self.buf.len();
        {
            let rel_offset = (end_pos as i32) - (jmp_end_pos as i32) - 5; // JMP rel32 is 5 bytes
            let code = self.buf.code_mut();
            code[jmp_end_pos + 1..jmp_end_pos + 5].copy_from_slice(&rel_offset.to_le_bytes());
        }

        // Stack depth unchanged (pop 1, push 1)
        Ok(())
    }

    /// Patch forward references for jumps.
    fn patch_forward_refs(&mut self) {
        for (native_offset, bytecode_target) in &self.forward_refs {
            if let Some(&target_offset) = self.labels.get(bytecode_target) {
                // x86-64 relative offsets are from the END of the instruction
                // JMP rel32: 5 bytes (E9 xx xx xx xx)
                // Jcc rel32: 6 bytes (0F 8x xx xx xx xx)

                let code = self.buf.code_mut();
                let opcode = code[*native_offset];

                let inst_len = if opcode == 0xE9 {
                    5 // JMP rel32
                } else if opcode == 0x0F {
                    6 // Jcc rel32
                } else {
                    continue; // Unknown instruction
                };

                let rel_offset = (target_offset as i32) - (*native_offset as i32) - inst_len;

                // Patch the offset (starts at native_offset + 1 for JMP, +2 for Jcc)
                let offset_pos = if opcode == 0xE9 {
                    *native_offset + 1
                } else {
                    *native_offset + 2
                };

                let bytes = rel_offset.to_le_bytes();
                code[offset_pos] = bytes[0];
                code[offset_pos + 1] = bytes[1];
                code[offset_pos + 2] = bytes[2];
                code[offset_pos + 3] = bytes[3];
            }
        }
    }
}

impl Default for JitCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compiler_creation() {
        let compiler = JitCompiler::new();
        assert_eq!(compiler.stack_depth, 0);
    }

    #[test]
    fn test_compile_simple_function() {
        let func = Function {
            name: "test".to_string(),
            arity: 0,
            locals_count: 1,
            code: vec![Op::I64Const(42), Op::LocalSet(0), Op::LocalGet(0), Op::Ret],
            stackmap: None,
            local_types: vec![],
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func, 0); // func_index = 0 for tests

        // Just verify it compiles without error
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_arithmetic() {
        let func = Function {
            name: "add".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![Op::I64Const(10), Op::I64Const(20), Op::I64Add, Op::Ret],
            stackmap: None,
            local_types: vec![],
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func, 0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_loop() {
        let func = Function {
            name: "loop".to_string(),
            arity: 0,
            locals_count: 1,
            code: vec![
                Op::I64Const(0),  // 0: push 0
                Op::LocalSet(0),  // 1: i = 0
                Op::LocalGet(0),  // 2: push i (loop start)
                Op::I64Const(10), // 3: push 10
                Op::I64LtS,       // 4: i < 10
                Op::BrIfFalse(9), // 5: if false, exit
                Op::LocalGet(0),  // 6: push i
                Op::I64Const(1),  // 7: push 1
                Op::I64Add,       // 8: i + 1
                Op::LocalSet(0),  // 9: i = i + 1  (target of BrIfFalse)
                Op::Jmp(2),       // 10: goto loop start
                Op::Ret,          // 11: return
            ],
            stackmap: None,
            local_types: vec![],
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func, 0);
        assert!(result.is_ok());
    }
}
