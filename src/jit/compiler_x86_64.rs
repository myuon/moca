//! JIT compiler for moca bytecode on x86-64.
//!
//! This module implements a baseline JIT compiler that translates moca bytecode
//! to x86-64 machine code using a template-based approach.

use super::codebuf::CodeBuffer;
use super::memory::ExecutableMemory;
use super::x86_64::{Cond, Reg, X86_64Assembler};
use crate::vm::{Function, Op};
use std::collections::HashMap;

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
}

impl JitCompiler {
    pub fn new() -> Self {
        Self {
            buf: CodeBuffer::new(),
            labels: HashMap::new(),
            forward_refs: Vec::new(),
            stack_map: HashMap::new(),
            stack_depth: 0,
        }
    }

    /// Compile a function to native code.
    pub fn compile(mut self, func: &Function) -> Result<CompiledCode, String> {
        // Emit prologue
        self.emit_prologue(func);

        // Record entry point after prologue
        let entry_offset = 0; // Prologue is at the start

        // Compile each instruction
        for (pc, op) in func.code.iter().enumerate() {
            // Record label for this bytecode PC
            self.labels.insert(pc, self.buf.len());

            // Compile the operation
            self.compile_op(op, pc)?;
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

    /// Compile a single bytecode operation.
    fn compile_op(&mut self, op: &Op, _pc: usize) -> Result<(), String> {
        match op {
            Op::PushInt(n) => self.emit_push_int(*n),
            Op::PushFloat(f) => self.emit_push_float(*f),
            Op::PushTrue => self.emit_push_bool(true),
            Op::PushFalse => self.emit_push_bool(false),
            Op::PushNull => self.emit_push_nil(),
            Op::Pop => self.emit_pop(),

            Op::GetL(idx) => self.emit_load_local(*idx),
            Op::SetL(idx) => self.emit_store_local(*idx),

            Op::Add => self.emit_add_int(),
            Op::Sub => self.emit_sub_int(),
            Op::Mul => self.emit_mul_int(),
            Op::Div => self.emit_div_int(),

            Op::Lt => self.emit_cmp_int(Cond::L),
            Op::Le => self.emit_cmp_int(Cond::Le),
            Op::Gt => self.emit_cmp_int(Cond::G),
            Op::Ge => self.emit_cmp_int(Cond::Ge),
            Op::Eq => self.emit_eq(),
            Op::Ne => self.emit_ne(),

            Op::Jmp(target) => self.emit_jmp(*target),
            Op::JmpIfFalse(target) => self.emit_jmp_if_false(*target),
            Op::JmpIfTrue(target) => self.emit_jmp_if_true(*target),

            Op::Ret => self.emit_ret(),

            Op::Call(func_index, argc) => self.emit_call(*func_index, *argc),

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

    /// Integer addition: pop two values, push their sum.
    fn emit_add_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop second operand (b)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP1, regs::VSTACK, 8); // b value

        // Pop first operand (a)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8); // a value

        // Add
        asm.add_rr(regs::TMP0, regs::TMP1);

        // Push result with int tag
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP1);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer subtraction: pop two values, push their difference.
    fn emit_sub_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop second operand (b)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP1, regs::VSTACK, 8);

        // Pop first operand (a)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8);

        // Subtract
        asm.sub_rr(regs::TMP0, regs::TMP1);

        // Push result
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP1);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer multiplication.
    fn emit_mul_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop two operands
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP1, regs::VSTACK, 8);
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP0, regs::VSTACK, 8);

        // Multiply
        asm.imul_rr(regs::TMP0, regs::TMP1);

        // Push result
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP1);
        asm.mov_mr(regs::VSTACK, 8, regs::TMP0);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer division.
    fn emit_div_int(&mut self) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Pop two operands (divisor first, then dividend)
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(regs::TMP1, regs::VSTACK, 8); // divisor
        asm.sub_ri32(regs::VSTACK, VALUE_SIZE);
        asm.mov_rm(Reg::Rax, regs::VSTACK, 8); // dividend into RAX

        // Sign-extend RAX into RDX:RAX
        asm.cqo();

        // Divide RDX:RAX by TMP1, quotient in RAX
        asm.idiv(regs::TMP1);

        // Push result (quotient is in RAX)
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::VSTACK, 0, regs::TMP1);
        asm.mov_mr(regs::VSTACK, 8, Reg::Rax);
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
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

    /// Emit a function call.
    /// This calls the runtime helper to execute the function via VM.
    ///
    /// Call convention for runtime helper (System V AMD64):
    ///   RDI = ctx (*mut JitCallContext, from VM_CTX register)
    ///   RSI = func_index
    ///   RDX = argc
    ///   RCX = args pointer (points to argc JitValues on our stack)
    ///
    /// The helper returns JitReturn in RAX (tag) and RDX (payload).
    fn emit_call(&mut self, func_index: usize, argc: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);

        // Arguments are already on the JIT value stack (VSTACK).
        // We need to compute the pointer to the first argument.
        // VSTACK points to the next free slot, so args start at VSTACK - argc * VALUE_SIZE

        let args_offset = (argc as i32) * VALUE_SIZE;

        // Save callee-saved registers that we use and that the helper might clobber
        // when calling into other JIT functions.
        // We push 3 registers for 16-byte stack alignment (6 in prologue + 3 = 9 total = 72 bytes)
        // After return address (8 bytes), 72 + 8 = 80, which is 16-byte aligned.
        asm.push(regs::VM_CTX);   // R12 - save our JitCallContext pointer
        asm.push(regs::VSTACK);   // R13 - save our value stack pointer (CRITICAL!)
        asm.push(regs::LOCALS);   // R14 - save our locals pointer

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
        asm.mov_mr(regs::VSTACK, 0, Reg::Rax);  // store tag
        asm.mov_mr(regs::VSTACK, 8, Reg::Rdx);  // store payload
        asm.add_ri32(regs::VSTACK, VALUE_SIZE);

        // Update stack depth: -argc + 1 (pop args, push result)
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

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
            code: vec![Op::PushInt(42), Op::SetL(0), Op::GetL(0), Op::Ret],
            stackmap: None,
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func);

        // Just verify it compiles without error
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_arithmetic() {
        let func = Function {
            name: "add".to_string(),
            arity: 0,
            locals_count: 0,
            code: vec![Op::PushInt(10), Op::PushInt(20), Op::Add, Op::Ret],
            stackmap: None,
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func);
        assert!(result.is_ok());
    }

    #[test]
    fn test_compile_loop() {
        let func = Function {
            name: "loop".to_string(),
            arity: 0,
            locals_count: 1,
            code: vec![
                Op::PushInt(0),    // 0: push 0
                Op::SetL(0),       // 1: i = 0
                Op::GetL(0),       // 2: push i (loop start)
                Op::PushInt(10),   // 3: push 10
                Op::Lt,            // 4: i < 10
                Op::JmpIfFalse(9), // 5: if false, exit
                Op::GetL(0),       // 6: push i
                Op::PushInt(1),    // 7: push 1
                Op::Add,           // 8: i + 1
                Op::SetL(0),       // 9: i = i + 1  (target of JmpIfFalse)
                Op::Jmp(2),        // 10: goto loop start
                Op::Ret,           // 11: return
            ],
            stackmap: None,
        };

        let compiler = JitCompiler::new();
        let result = compiler.compile(&func);
        assert!(result.is_ok());
    }
}
