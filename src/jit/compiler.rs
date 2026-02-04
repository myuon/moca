//! JIT compiler for moca bytecode.
//!
//! This module implements a baseline JIT compiler that translates moca bytecode
//! to native machine code using a template-based approach.

#[cfg(target_arch = "aarch64")]
use super::aarch64::{AArch64Assembler, Cond, Reg};
#[cfg(target_arch = "aarch64")]
use super::codebuf::CodeBuffer;
#[cfg(target_arch = "aarch64")]
use super::memory::ExecutableMemory;
#[cfg(target_arch = "aarch64")]
use crate::vm::{Function, Op};
#[cfg(target_arch = "aarch64")]
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

/// Register conventions for moca JIT on AArch64.
///
/// Following AArch64 calling convention with moca-specific assignments:
/// - x19: VM context pointer (callee-saved)
/// - x20: Value stack pointer (callee-saved)
/// - x21: Locals base pointer (callee-saved)
/// - x22: Constants pool pointer (callee-saved)
/// - x0-x7: Temporaries and function arguments
/// - x9-x15: Additional temporaries
#[cfg(target_arch = "aarch64")]
pub mod regs {
    use super::Reg;

    pub const VM_CTX: Reg = Reg::X19;
    pub const VSTACK: Reg = Reg::X20;
    pub const LOCALS: Reg = Reg::X21;
    pub const CONSTS: Reg = Reg::X22;

    // Temporaries
    pub const TMP0: Reg = Reg::X0;
    pub const TMP1: Reg = Reg::X1;
    pub const TMP2: Reg = Reg::X2;
    pub const TMP3: Reg = Reg::X3;
    pub const TMP4: Reg = Reg::X9;
    pub const TMP5: Reg = Reg::X10;
}

/// Size of a Value on the stack (128 bits = 16 bytes).
pub const VALUE_SIZE: u16 = 16;

/// Compiled JIT code for a function.
#[cfg(target_arch = "aarch64")]
pub struct CompiledCode {
    /// The executable memory containing the compiled code
    pub memory: ExecutableMemory,
    /// Entry point offset within the memory
    pub entry_offset: usize,
    /// Stack map for GC (pc_offset -> bitmap of stack slots with refs)
    pub stack_map: HashMap<usize, Vec<bool>>,
}

#[cfg(target_arch = "aarch64")]
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
#[cfg(target_arch = "aarch64")]
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

#[cfg(target_arch = "aarch64")]
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

/// JIT compiler for moca functions.
#[cfg(target_arch = "aarch64")]
pub struct JitCompiler {
    buf: CodeBuffer,
    /// Labels for jump targets (bytecode pc -> native offset)
    labels: HashMap<usize, usize>,
    /// Forward references for jumps
    forward_refs: Vec<(usize, usize)>, // (native_offset, bytecode_target)
    /// Stack map entries being built
    stack_map: HashMap<usize, Vec<bool>>,
    /// Current stack depth (number of values)
    stack_depth: usize,
    /// Index of the function being compiled (for self-recursion detection)
    self_func_index: usize,
    /// Number of locals in the function being compiled
    self_locals_count: usize,
}

#[cfg(target_arch = "aarch64")]
impl JitCompiler {
    pub fn new() -> Self {
        Self {
            buf: CodeBuffer::new(),
            labels: HashMap::new(),
            forward_refs: Vec::new(),
            stack_map: HashMap::new(),
            stack_depth: 0,
            self_func_index: 0,
            self_locals_count: 0,
        }
    }

    /// Compile a function to native code.
    ///
    /// # Arguments
    /// * `func` - The function to compile
    /// * `func_index` - The index of this function (used for self-recursion optimization)
    pub fn compile(mut self, func: &Function, func_index: usize) -> Result<CompiledCode, String> {
        // Store function info for self-recursion detection
        self.self_func_index = func_index;
        self.self_locals_count = func.locals_count;

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
    ) -> Result<CompiledLoop, String> {
        // Check for unsupported operations in the loop
        // Currently, loops containing Call instructions are not supported
        // due to complex interaction with the JIT call context
        for pc in loop_start_pc..=loop_end_pc {
            if let Some(op) = func.code.get(pc)
                && matches!(op, Op::Call(_, _))
            {
                return Err("Loop contains Call instruction (not yet supported)".to_string());
            }
        }

        // Store function info
        self.self_locals_count = func.locals_count;

        // Emit prologue (same as function)
        self.emit_prologue(func);

        // Record entry point after prologue
        let entry_offset = 0;

        // Compile instructions from loop_start_pc to loop_end_pc (inclusive)
        for pc in loop_start_pc..=loop_end_pc {
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

            // Handle jumps that exit the loop specially
            match op {
                Op::JmpIfFalse(target) if *target > loop_end_pc => {
                    // This is a loop exit condition - jump to epilogue
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
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }
        self.stack_depth = self.stack_depth.saturating_sub(1);

        // Load payload (boolean value)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr_offset(regs::TMP0, regs::VSTACK, 8);
        }

        // Record forward reference for conditional jump
        let jmp_offset = self.buf.len();
        self.forward_refs.push((jmp_offset, exit_label));

        // CBZ (compare and branch if zero) to exit - placeholder offset
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbz(regs::TMP0, 0); // Placeholder, will be patched
        }

        Ok(())
    }

    /// Emit function prologue.
    fn emit_prologue(&mut self, func: &Function) {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Save callee-saved registers and link register
        // stp x29, x30, [sp, #-16]!
        asm.stp_pre(Reg::Fp, Reg::Lr, -16);

        // Save our callee-saved registers
        // stp x19, x20, [sp, #-16]!
        asm.stp_pre(Reg::X19, Reg::X20, -16);
        // stp x21, x22, [sp, #-16]!
        asm.stp_pre(Reg::X21, Reg::X22, -16);

        // Set up frame pointer
        // mov x29, sp
        asm.add_imm(Reg::Fp, Reg::Sp, 0);

        // Initialize moca registers from arguments
        // x0 = VM context, x1 = value stack, x2 = locals base
        asm.mov(regs::VM_CTX, Reg::X0);
        asm.mov(regs::VSTACK, Reg::X1);
        asm.mov(regs::LOCALS, Reg::X2);

        // Allocate space for locals (each local is 16 bytes)
        let locals_size = (func.locals_count * VALUE_SIZE as usize) as u16;
        if locals_size > 0 {
            // Locals are already allocated by caller in locals base area
        }

        self.stack_depth = 0;
    }

    /// Emit function epilogue.
    fn emit_epilogue(&mut self) {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Restore callee-saved registers
        // ldp x21, x22, [sp], #16
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        // ldp x19, x20, [sp], #16
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        // ldp x29, x30, [sp], #16
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);

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

            Op::Add => self.emit_add(),
            Op::Sub => self.emit_sub(),
            Op::Mul => self.emit_mul(),
            Op::Div => self.emit_div(),

            Op::Lt => self.emit_cmp_int(Cond::Lt),
            Op::Le => self.emit_cmp_int(Cond::Le),
            Op::Gt => self.emit_cmp_int(Cond::Gt),
            Op::Ge => self.emit_cmp_int(Cond::Ge),
            Op::Eq => self.emit_eq(),
            Op::Ne => self.emit_ne(),

            Op::Jmp(target) => self.emit_jmp(*target),
            Op::JmpIfFalse(target) => self.emit_jmp_if_false(*target),
            Op::JmpIfTrue(target) => self.emit_jmp_if_true(*target),

            Op::Ret => self.emit_ret(),

            Op::Call(func_index, argc) => {
                if *func_index == self.self_func_index {
                    // Self-recursion: use optimized direct call
                    self.emit_call_self(*argc)
                } else {
                    // External call: use jit_call_helper
                    self.emit_call(*func_index, *argc)
                }
            }

            Op::PushString(idx) => self.emit_push_string(*idx),
            Op::ArrayLen => self.emit_array_len(),
            Op::Syscall(syscall_num, argc) => self.emit_syscall(*syscall_num, *argc),
            Op::Neg => self.emit_neg(),

            // Unsupported operations - fail compilation so VM falls back to interpreter
            _ => Err(format!("Unsupported operation for JIT: {:?}", op)),
        }
    }

    /// Push an integer onto the value stack.
    fn emit_push_int(&mut self, n: i64) -> Result<(), String> {
        // Store tag (0 = int)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(regs::TMP0, value_tags::TAG_INT as u16);
            asm.str(regs::TMP0, regs::VSTACK, 0);
        }

        // Load the immediate value
        self.emit_load_imm64(n);

        // Store value and advance stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        self.stack_depth += 1;
        Ok(())
    }

    /// Load a 64-bit immediate into TMP0.
    fn emit_load_imm64(&mut self, n: i64) {
        let u = n as u64;

        // MOVZ for first 16 bits
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(regs::TMP0, (u & 0xFFFF) as u16);
        }

        // MOVK for remaining bits if needed
        if u > 0xFFFF {
            // MOVK Xd, #imm16, LSL #16
            let inst =
                0xF2A00000 | ((((u >> 16) & 0xFFFF) as u32) << 5) | (regs::TMP0.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF {
            // MOVK Xd, #imm16, LSL #32
            let inst =
                0xF2C00000 | ((((u >> 32) & 0xFFFF) as u32) << 5) | (regs::TMP0.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF_FFFF {
            // MOVK Xd, #imm16, LSL #48
            let inst =
                0xF2E00000 | ((((u >> 48) & 0xFFFF) as u32) << 5) | (regs::TMP0.code() as u32);
            self.buf.emit_u32(inst);
        }
    }

    /// Push a float onto the value stack.
    fn emit_push_float(&mut self, f: f64) -> Result<(), String> {
        // Store tag (1 = float)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(regs::TMP0, value_tags::TAG_FLOAT as u16);
            asm.str(regs::TMP0, regs::VSTACK, 0);
        }

        // Store float bits as u64
        let bits = f.to_bits();
        self.emit_load_imm64(bits as i64);

        // Store value and advance stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        self.stack_depth += 1;
        Ok(())
    }

    /// Push a bool onto the value stack.
    fn emit_push_bool(&mut self, b: bool) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Store tag (2 = bool)
        asm.mov_imm(regs::TMP0, value_tags::TAG_BOOL as u16);
        asm.str(regs::TMP0, regs::VSTACK, 0);

        // Store value
        asm.mov_imm(regs::TMP0, if b { 1 } else { 0 });
        asm.str(regs::TMP0, regs::VSTACK, 8);

        // Advance stack pointer
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Push nil onto the value stack.
    fn emit_push_nil(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Store tag (3 = nil)
        asm.mov_imm(regs::TMP0, value_tags::TAG_NIL as u16);
        asm.str(regs::TMP0, regs::VSTACK, 0);

        // Store 0 as payload
        asm.mov_imm(regs::TMP0, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);

        // Advance stack pointer
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Pop a value from the stack (discard).
    fn emit_pop(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Decrement stack pointer
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Load a local variable onto the stack.
    fn emit_load_local(&mut self, idx: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        let offset = (idx * VALUE_SIZE as usize) as u16;

        // Load tag
        asm.ldr(regs::TMP0, regs::LOCALS, offset);
        asm.str(regs::TMP0, regs::VSTACK, 0);

        // Load value
        asm.ldr(regs::TMP0, regs::LOCALS, offset + 8);
        asm.str(regs::TMP0, regs::VSTACK, 8);

        // Advance stack pointer
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth += 1;
        Ok(())
    }

    /// Store top of stack into a local variable.
    fn emit_store_local(&mut self, idx: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        let offset = (idx * VALUE_SIZE as usize) as u16;

        // Decrement stack pointer first (peek and pop)
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        // Load tag from stack
        asm.ldr(regs::TMP0, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::LOCALS, offset);

        // Load value from stack
        asm.ldr(regs::TMP0, regs::VSTACK, 8);
        asm.str(regs::TMP0, regs::LOCALS, offset + 8);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Helper to emit a binary arithmetic operation that handles both int and float.
    /// `int_op` emits the integer operation using TMP0, TMP1 -> TMP0.
    /// `float_op` emits the float operation using D0, D1 -> D0.
    fn emit_arith_op<F, G>(&mut self, int_op: F, float_op: G) -> Result<(), String>
    where
        F: FnOnce(&mut AArch64Assembler),
        G: FnOnce(&mut AArch64Assembler),
    {
        // Load both operands with tags
        // Stack: [..., a, b] where each is (tag, payload)
        // TMP0 = a_payload, TMP1 = b_payload
        // TMP2 = a_tag, TMP3 = b_tag
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Load b (top of stack)
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
            asm.ldr(regs::TMP3, regs::VSTACK, 0); // b_tag
            asm.ldr(regs::TMP1, regs::VSTACK, 8); // b_payload

            // Load a
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
            asm.ldr(regs::TMP2, regs::VSTACK, 0); // a_tag
            asm.ldr(regs::TMP0, regs::VSTACK, 8); // a_payload

            // Check if both are INT: (a_tag | b_tag) == 0
            asm.orr(regs::TMP4, regs::TMP2, regs::TMP3);
        }

        // Branch to float path if not both INT
        let cbnz_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbnz(regs::TMP4, 0); // placeholder offset
        }

        // INT path: TMP0 = a, TMP1 = b, result in TMP0
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            int_op(&mut asm);
            // Push result with int tag
            asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
            asm.str(regs::TMP1, regs::VSTACK, 0);
            asm.str(regs::TMP0, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // Jump to end
        let b_end_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // placeholder offset
        }

        // FLOAT path
        let float_path_start = self.buf.len();

        // Patch CBNZ to jump here
        {
            let offset = (float_path_start as i32) - (cbnz_pos as i32);
            let code = self.buf.code_mut();
            // CBNZ encoding: bits 23:5 contain imm19 (offset/4)
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let inst = u32::from_le_bytes([
                code[cbnz_pos],
                code[cbnz_pos + 1],
                code[cbnz_pos + 2],
                code[cbnz_pos + 3],
            ]);
            let new_inst = (inst & 0xFF00001F) | (imm19 << 5);
            code[cbnz_pos..cbnz_pos + 4].copy_from_slice(&new_inst.to_le_bytes());
        }

        // Convert a to float if needed (TMP2 = a_tag, TMP0 = a_payload)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbnz(regs::TMP2, 0); // if a_tag != 0, already float
        }
        let a_is_int_pos = self.buf.len() - 4;

        // a is INT, convert to float
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.scvtf_d_x(0, regs::TMP0); // D0 = (double)TMP0
        }
        let a_conv_done_b = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // jump past the float case
        }

        // a is FLOAT
        let a_is_float_pos = self.buf.len();
        {
            // Patch the CBNZ for a_tag
            let offset = (a_is_float_pos as i32) - (a_is_int_pos as i32);
            let code = self.buf.code_mut();
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let inst = u32::from_le_bytes([
                code[a_is_int_pos],
                code[a_is_int_pos + 1],
                code[a_is_int_pos + 2],
                code[a_is_int_pos + 3],
            ]);
            let new_inst = (inst & 0xFF00001F) | (imm19 << 5);
            code[a_is_int_pos..a_is_int_pos + 4].copy_from_slice(&new_inst.to_le_bytes());
        }
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.fmov_d_x(0, regs::TMP0); // D0 = TMP0 (as bits)
        }

        // Patch the branch after a conversion
        let a_conv_merge = self.buf.len();
        {
            let offset = (a_conv_merge as i32) - (a_conv_done_b as i32);
            let code = self.buf.code_mut();
            let imm26 = ((offset / 4) as u32) & 0x03FFFFFF;
            let inst = u32::from_le_bytes([
                code[a_conv_done_b],
                code[a_conv_done_b + 1],
                code[a_conv_done_b + 2],
                code[a_conv_done_b + 3],
            ]);
            let new_inst = (inst & 0xFC000000) | imm26;
            code[a_conv_done_b..a_conv_done_b + 4].copy_from_slice(&new_inst.to_le_bytes());
        }

        // Convert b to float if needed (TMP3 = b_tag, TMP1 = b_payload)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbnz(regs::TMP3, 0); // if b_tag != 0, already float
        }
        let b_is_int_pos = self.buf.len() - 4;

        // b is INT, convert to float
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.scvtf_d_x(1, regs::TMP1); // D1 = (double)TMP1
        }
        let b_conv_done_b = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // jump past the float case
        }

        // b is FLOAT
        let b_is_float_pos = self.buf.len();
        {
            // Patch the CBNZ for b_tag
            let offset = (b_is_float_pos as i32) - (b_is_int_pos as i32);
            let code = self.buf.code_mut();
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let inst = u32::from_le_bytes([
                code[b_is_int_pos],
                code[b_is_int_pos + 1],
                code[b_is_int_pos + 2],
                code[b_is_int_pos + 3],
            ]);
            let new_inst = (inst & 0xFF00001F) | (imm19 << 5);
            code[b_is_int_pos..b_is_int_pos + 4].copy_from_slice(&new_inst.to_le_bytes());
        }
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.fmov_d_x(1, regs::TMP1); // D1 = TMP1 (as bits)
        }

        // Patch the branch after b conversion
        let b_conv_merge = self.buf.len();
        {
            let offset = (b_conv_merge as i32) - (b_conv_done_b as i32);
            let code = self.buf.code_mut();
            let imm26 = ((offset / 4) as u32) & 0x03FFFFFF;
            let inst = u32::from_le_bytes([
                code[b_conv_done_b],
                code[b_conv_done_b + 1],
                code[b_conv_done_b + 2],
                code[b_conv_done_b + 3],
            ]);
            let new_inst = (inst & 0xFC000000) | imm26;
            code[b_conv_done_b..b_conv_done_b + 4].copy_from_slice(&new_inst.to_le_bytes());
        }

        // Perform float operation: D0 = D0 op D1
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            float_op(&mut asm);
        }

        // Store result as float
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.fmov_x_d(regs::TMP0, 0); // TMP0 = D0 bits
            asm.mov_imm(regs::TMP1, value_tags::TAG_FLOAT as u16);
            asm.str(regs::TMP1, regs::VSTACK, 0);
            asm.str(regs::TMP0, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // End
        let end_pos = self.buf.len();

        // Patch the B (jump to end) from INT path
        {
            let offset = (end_pos as i32) - (b_end_pos as i32);
            let code = self.buf.code_mut();
            let imm26 = ((offset / 4) as u32) & 0x03FFFFFF;
            let inst = u32::from_le_bytes([
                code[b_end_pos],
                code[b_end_pos + 1],
                code[b_end_pos + 2],
                code[b_end_pos + 3],
            ]);
            let new_inst = (inst & 0xFC000000) | imm26;
            code[b_end_pos..b_end_pos + 4].copy_from_slice(&new_inst.to_le_bytes());
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Addition: pop two values, push their sum (handles both int and float).
    fn emit_add(&mut self) -> Result<(), String> {
        self.emit_arith_op(
            |asm| asm.add(regs::TMP0, regs::TMP0, regs::TMP1),
            |asm| asm.fadd_d(0, 0, 1),
        )
    }

    /// Subtraction: pop two values, push their difference (handles both int and float).
    fn emit_sub(&mut self) -> Result<(), String> {
        self.emit_arith_op(
            |asm| asm.sub(regs::TMP0, regs::TMP0, regs::TMP1),
            |asm| asm.fsub_d(0, 0, 1),
        )
    }

    /// Multiplication: pop two values, push their product (handles both int and float).
    fn emit_mul(&mut self) -> Result<(), String> {
        self.emit_arith_op(
            |asm| asm.mul(regs::TMP0, regs::TMP0, regs::TMP1),
            |asm| asm.fmul_d(0, 0, 1),
        )
    }

    /// Division: pop two values, push their quotient (handles both int and float).
    fn emit_div(&mut self) -> Result<(), String> {
        self.emit_arith_op(
            |asm| asm.sdiv(regs::TMP0, regs::TMP0, regs::TMP1),
            |asm| asm.fdiv_d(0, 0, 1),
        )
    }

    /// Integer comparison: pop two values, push bool result.
    fn emit_cmp_int(&mut self, cond: Cond) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop two operands
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP1, regs::VSTACK, 8); // b
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP0, regs::VSTACK, 8); // a

        // Compare
        asm.cmp(regs::TMP0, regs::TMP1);

        // Set result based on condition
        // CSET Xd, cond -> if cond then Xd = 1 else Xd = 0
        // CSET is alias for CSINC Xd, XZR, XZR, invert(cond)
        let inv_cond = match cond {
            Cond::Lt => Cond::Ge,
            Cond::Le => Cond::Gt,
            Cond::Gt => Cond::Le,
            Cond::Ge => Cond::Lt,
            Cond::Eq => Cond::Ne,
            Cond::Ne => Cond::Eq,
            _ => cond,
        };
        // CSINC Xd, XZR, XZR, inv_cond
        // 1001 1010 1001 1111 cccc 0111 1111 dddd
        let inst = 0x9A9F07E0 | ((inv_cond as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);

        // Push bool result
        asm.mov_imm(regs::TMP1, value_tags::TAG_BOOL as u16);
        asm.str(regs::TMP1, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Equality comparison.
    fn emit_eq(&mut self) -> Result<(), String> {
        self.emit_cmp_int(Cond::Eq)
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

        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Emit placeholder that will be patched
        asm.b(0);

        Ok(())
    }

    /// Jump if top of stack is false (pop value).
    fn emit_jmp_if_false(&mut self, target: usize) -> Result<(), String> {
        // Pop value and load it
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
            asm.ldr(regs::TMP0, regs::VSTACK, 8); // value
        }

        // Record forward reference and emit branch
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbz(regs::TMP0, 0);
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Jump if top of stack is true (pop value).
    fn emit_jmp_if_true(&mut self, target: usize) -> Result<(), String> {
        // Pop value and load it
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
            asm.ldr(regs::TMP0, regs::VSTACK, 8); // value
        }

        // Record forward reference and emit branch
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbnz(regs::TMP0, 0);
        }

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Return from function.
    /// Emit a function call via jit_call_helper.
    ///
    /// This calls the runtime helper to execute the function.
    /// Arguments are on the JIT value stack (VSTACK).
    ///
    /// AArch64 calling convention (AAPCS64):
    ///   x0 = ctx (*mut JitCallContext)
    ///   x1 = func_index (u64)
    ///   x2 = argc (u64)
    ///   x3 = args pointer (points to argc JitValues on our stack)
    ///
    /// The helper returns JitReturn in x0 (tag) and x1 (payload).
    fn emit_call(&mut self, func_index: usize, argc: usize) -> Result<(), String> {
        let args_offset = (argc as u16) * VALUE_SIZE;

        // Save callee-saved registers that we use
        // These might be clobbered when call_helper calls into other JIT functions
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // stp x19, x20, [sp, #-16]!  (VM_CTX, VSTACK)
            asm.stp_pre(regs::VM_CTX, regs::VSTACK, -16);
            // stp x21, x22, [sp, #-16]!  (LOCALS, CONSTS)
            asm.stp_pre(regs::LOCALS, regs::CONSTS, -16);
        }

        // Calculate args pointer: x3 = VSTACK - argc * VALUE_SIZE
        // VSTACK points to the next free slot, args start before that
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            if args_offset > 0 {
                asm.sub_imm(Reg::X3, regs::VSTACK, args_offset);
            } else {
                asm.mov(Reg::X3, regs::VSTACK);
            }
        }

        // Set up arguments for jit_call_helper:
        // x0 = ctx (VM_CTX register)
        // x1 = func_index
        // x2 = argc
        // x3 = args pointer (already set above)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
        }

        // Load func_index into x1
        self.emit_load_imm64_to_reg(func_index as i64, Reg::X1);

        // Load argc into x2
        self.emit_load_imm64_to_reg(argc as i64, Reg::X2);

        // Load the call_helper function pointer from JitCallContext.
        // JitCallContext layout:
        //   offset 0: vm (*mut u8)
        //   offset 8: chunk (*const u8)
        //   offset 16: call_helper (fn pointer)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, 16); // x9 = ctx->call_helper
        }

        // Call the helper function
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.blr(regs::TMP4);
        }

        // Restore saved registers (in reverse order)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // ldp x21, x22, [sp], #16
            asm.ldp_post(regs::LOCALS, regs::CONSTS, 16);
            // ldp x19, x20, [sp], #16
            asm.ldp_post(regs::VM_CTX, regs::VSTACK, 16);
        }

        // Pop the arguments from JIT stack (they've been consumed)
        // VSTACK is now restored to its original value (after the arguments)
        if args_offset > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, args_offset);
        }

        // Push the return value onto the JIT stack
        // Return value is in x0 (tag) and x1 (payload)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X0, regs::VSTACK, 0); // store tag
            asm.str(Reg::X1, regs::VSTACK, 8); // store payload
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // Update stack depth: -argc + 1 (pop args, push result)
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

        Ok(())
    }

    /// Load a 64-bit immediate into a specific register.
    fn emit_load_imm64_to_reg(&mut self, n: i64, rd: Reg) {
        let u = n as u64;

        // MOVZ for first 16 bits
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(rd, (u & 0xFFFF) as u16);
        }

        // MOVK for remaining bits if needed
        if u > 0xFFFF {
            // MOVK Xd, #imm16, LSL #16
            let inst = 0xF2A00000 | ((((u >> 16) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF {
            // MOVK Xd, #imm16, LSL #32
            let inst = 0xF2C00000 | ((((u >> 32) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF_FFFF {
            // MOVK Xd, #imm16, LSL #48
            let inst = 0xF2E00000 | ((((u >> 48) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
    }

    /// Emit optimized self-recursive call.
    ///
    /// This directly calls the function entry point instead of going through
    /// jit_call_helper, avoiding the overhead of runtime dispatch.
    ///
    /// Strategy:
    /// 1. Save callee-saved registers (VM_CTX, VSTACK, LOCALS)
    /// 2. Allocate new locals on native stack
    /// 3. Copy arguments from VSTACK to new locals
    /// 4. Set up call arguments: x0=ctx, x1=VSTACK, x2=new_locals
    /// 5. BL to entry point (offset 0)
    /// 6. Deallocate locals
    /// 7. Restore registers
    /// 8. Pop args and push return value
    fn emit_call_self(&mut self, argc: usize) -> Result<(), String> {
        // TODO: Implement direct BL optimization for aarch64
        // For now, fall back to jit_call_helper which has overhead
        // but works correctly
        self.emit_call(self.self_func_index, argc)
    }

    fn emit_ret(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop return value from JIT stack into x0 (tag) and x1 (payload)
        // Stack layout: [tag: 8 bytes][payload: 8 bytes]
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(Reg::X0, regs::VSTACK, 0); // tag
        asm.ldr(Reg::X1, regs::VSTACK, 8); // payload

        // Restore callee-saved registers and return
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);
        asm.ret();

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
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::VM_CTX, 56); // TMP0 = string_cache
        }

        // Calculate offset: string_index * 16
        self.emit_load_imm64_to_reg((string_index * 16) as i64, regs::TMP1);

        // TMP0 = string_cache + offset
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add(regs::TMP0, regs::TMP0, regs::TMP1);
        }

        // Load discriminant (tag) from cache entry
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP1, regs::TMP0, 0); // TMP1 = discriminant
        }

        // Check if discriminant is 0 (None) - need to call helper
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cmp_imm(regs::TMP1, 0);
        }

        // Branch to helper call if None (discriminant == 0)
        let branch_to_helper_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b_cond(Cond::Eq, 0); // B.EQ placeholder - will patch later
        }

        // === FAST PATH: Cache hit ===
        // Load cached ref value from cache entry (offset 8)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP1, regs::TMP0, 8); // TMP1 = cached GcRef.index
        }

        // Push Ref onto JIT stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Store tag (4 = PTR)
            asm.mov_imm(regs::TMP0, value_tags::TAG_PTR as u16);
            asm.str(regs::TMP0, regs::VSTACK, 0);
            // Store payload (ref index)
            asm.str(regs::TMP1, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // Branch to end (skip slow path)
        let branch_to_end_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // B placeholder - will patch later
        }

        // === SLOW PATH: Cache miss - call helper ===
        let helper_start_pos = self.buf.len();

        // Patch the conditional branch to jump here
        {
            let offset = (helper_start_pos as i32) - (branch_to_helper_pos as i32);
            let code = self.buf.code_mut();
            // B.cond encoding: 0101 0100 iiii iiii iiii iiii iii0 cccc
            let imm19 = ((offset / 4) as u32) & 0x7FFFF;
            let inst = 0x54000000 | (imm19 << 5); // cond=0 (EQ)
            code[branch_to_helper_pos..branch_to_helper_pos + 4]
                .copy_from_slice(&inst.to_le_bytes());
        }

        // Save callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::VSTACK, -16);
            asm.stp_pre(regs::LOCALS, regs::CONSTS, -16);
        }

        // Set up arguments for push_string_helper:
        // x0 = ctx (VM_CTX register)
        // x1 = string_index
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
        }

        self.emit_load_imm64_to_reg(string_index as i64, Reg::X1);

        // Load push_string_helper from JitCallContext (offset 24)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, 24);
        }

        // Call the helper
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.blr(regs::TMP4);
        }

        // Restore saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(regs::LOCALS, regs::CONSTS, 16);
            asm.ldp_post(regs::VM_CTX, regs::VSTACK, 16);
        }

        // Push the return value onto the JIT stack
        // Return value is in x0 (tag) and x1 (payload)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X0, regs::VSTACK, 0);
            asm.str(Reg::X1, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // === END ===
        let end_pos = self.buf.len();

        // Patch the unconditional branch to jump here
        {
            let offset = (end_pos as i32) - (branch_to_end_pos as i32);
            let code = self.buf.code_mut();
            // B encoding: 0001 01ii iiii iiii iiii iiii iiii iiii
            let imm26 = ((offset / 4) as u32) & 0x03FFFFFF;
            let inst = 0x14000000 | imm26;
            code[branch_to_end_pos..branch_to_end_pos + 4].copy_from_slice(&inst.to_le_bytes());
        }

        self.stack_depth += 1;

        Ok(())
    }

    /// Emit ArrayLen operation.
    /// Pops a Ref from stack, reads slot_count from heap header, pushes i64.
    ///
    /// Heap header layout (64 bits):
    /// - bits 30-61: slot_count (32 bits)
    ///
    /// JitCallContext layout:
    /// - offset 48: heap_base (*const u64)
    fn emit_array_len(&mut self) -> Result<(), String> {
        // Pop the Ref from JIT stack and load ref_index
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
            // Load payload (ref index) - tag at offset 0, payload at offset 8
            asm.ldr(regs::TMP0, regs::VSTACK, 8); // TMP0 = ref_index
        }

        // Load heap_base from JitCallContext (offset 48)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP1, regs::VM_CTX, 48); // TMP1 = heap_base
        }

        // Calculate header address: heap_base + ref_index * 8
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // TMP0 = ref_index << 3 (multiply by 8)
            asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
            // TMP1 = heap_base + offset
            asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        }

        // Load header from memory
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::TMP1, 0); // TMP0 = header
        }

        // Extract slot_count: (header >> 30) & 0xFFFFFFFF
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Shift right by 30 bits
            asm.lsr_imm(regs::TMP0, regs::TMP0, 30);
            // Mask to 32 bits (AND with 0xFFFFFFFF)
            // On AArch64, we can use UBFX or just use the lower 32 bits
            // Since we shifted by 30, we have at most 34 bits, but slot_count is 32 bits
            // We need to mask: AND with 0xFFFFFFFF
            self.emit_load_imm64_to_reg(0xFFFFFFFF, regs::TMP1);
        }
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.and(regs::TMP0, regs::TMP0, regs::TMP1);
        }

        // Push result as i64 onto the JIT stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Store tag (0 = int)
            asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
            asm.str(regs::TMP1, regs::VSTACK, 0);
            // Store slot_count as payload
            asm.str(regs::TMP0, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // Stack depth unchanged: -1 (pop ref) + 1 (push len) = 0

        Ok(())
    }

    /// Emit Syscall operation.
    /// Pops argc arguments, calls syscall_helper, pushes result.
    fn emit_syscall(&mut self, syscall_num: usize, argc: usize) -> Result<(), String> {
        let args_offset = (argc as u16) * VALUE_SIZE;

        // Save callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::VSTACK, -16);
            asm.stp_pre(regs::LOCALS, regs::CONSTS, -16);
        }

        // Calculate args pointer: x3 = VSTACK - argc * VALUE_SIZE
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            if args_offset > 0 {
                asm.sub_imm(Reg::X3, regs::VSTACK, args_offset);
            } else {
                asm.mov(Reg::X3, regs::VSTACK);
            }
        }

        // Set up arguments for syscall_helper:
        // x0 = ctx (VM_CTX register)
        // x1 = syscall_num
        // x2 = argc
        // x3 = args pointer (already set above)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
        }

        self.emit_load_imm64_to_reg(syscall_num as i64, Reg::X1);
        self.emit_load_imm64_to_reg(argc as i64, Reg::X2);

        // Load syscall_helper from JitCallContext (offset 40)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, 40);
        }

        // Call the helper
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.blr(regs::TMP4);
        }

        // Restore saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(regs::LOCALS, regs::CONSTS, 16);
            asm.ldp_post(regs::VM_CTX, regs::VSTACK, 16);
        }

        // Pop the arguments from JIT stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::VSTACK, regs::VSTACK, args_offset);
        }

        // Push the return value onto the JIT stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X0, regs::VSTACK, 0);
            asm.str(Reg::X1, regs::VSTACK, 8);
            asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        }

        // Update stack depth: -argc + 1 (pop args, push result)
        self.stack_depth = self.stack_depth.saturating_sub(argc) + 1;

        Ok(())
    }

    /// Emit Neg operation.
    /// Negates the top value on the stack (int or float).
    fn emit_neg(&mut self) -> Result<(), String> {
        // Load tag and payload from top of stack
        // Stack layout: [tag: 8 bytes][payload: 8 bytes]
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldur(regs::TMP0, regs::VSTACK, -(VALUE_SIZE as i16)); // tag
            asm.ldur(regs::TMP1, regs::VSTACK, -(VALUE_SIZE as i16) + 8); // payload
        }

        // Check if it's an int (tag == 0)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cmp_imm(regs::TMP0, value_tags::TAG_INT as u16);
        }

        // Record position for conditional branch to float path
        let bne_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // B.NE to float path (placeholder, will patch)
            asm.b_cond(Cond::Ne, 0);
        }

        // INT path: negate using SUB from zero (NEG is alias for SUB Xd, XZR, Xn)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub(regs::TMP1, Reg::XZR, regs::TMP1);
            // Store back the negated payload
            // STUR for unscaled offset
            // STUR Xt, [Xn, #simm9]: 1111 1000 000i iiii iiii 00nn nnnt tttt
            let simm9 = (-(VALUE_SIZE as i16) + 8) as u32;
            let inst = 0xF8000000
                | ((simm9 & 0x1FF) << 12)
                | ((regs::VSTACK.code() as u32) << 5)
                | (regs::TMP1.code() as u32);
            asm.emit_raw(inst);
        }

        // Jump over float path
        let b_end_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // placeholder, will patch
        }

        // Patch B.NE to jump here (float path start)
        let float_path_start = self.buf.len();
        {
            let offset = (float_path_start as i32) - (bne_pos as i32);
            let code = self.buf.code_mut();
            // B.cond encoding: 0101 0100 iiii iiii iiii iiii iii0 cccc
            let inst = 0x54000000 | (((offset / 4) as u32 & 0x7FFFF) << 5) | (Cond::Ne as u32);
            let bytes = inst.to_le_bytes();
            code[bne_pos] = bytes[0];
            code[bne_pos + 1] = bytes[1];
            code[bne_pos + 2] = bytes[2];
            code[bne_pos + 3] = bytes[3];
        }

        // FLOAT path: XOR sign bit (bit 63) to negate
        // Load sign bit mask: 0x8000000000000000
        self.emit_load_imm64_to_reg(0x8000000000000000u64 as i64, regs::TMP2);

        // XOR to flip sign bit
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.eor(regs::TMP1, regs::TMP1, regs::TMP2);
            // Store back
            let simm9 = (-(VALUE_SIZE as i16) + 8) as u32;
            let inst = 0xF8000000
                | ((simm9 & 0x1FF) << 12)
                | ((regs::VSTACK.code() as u32) << 5)
                | (regs::TMP1.code() as u32);
            asm.emit_raw(inst);
        }

        // Patch B to jump here (end)
        let end_pos = self.buf.len();
        {
            let offset = (end_pos as i32) - (b_end_pos as i32);
            let code = self.buf.code_mut();
            // B encoding: 0001 01ii iiii iiii iiii iiii iiii iiii
            let inst = 0x14000000 | ((offset / 4) as u32 & 0x03FFFFFF);
            let bytes = inst.to_le_bytes();
            code[b_end_pos] = bytes[0];
            code[b_end_pos + 1] = bytes[1];
            code[b_end_pos + 2] = bytes[2];
            code[b_end_pos + 3] = bytes[3];
        }

        // Stack depth unchanged (we modify in-place)
        Ok(())
    }

    /// Patch forward references for jumps.
    fn patch_forward_refs(&mut self) {
        for (native_offset, bytecode_target) in &self.forward_refs {
            if let Some(&target_offset) = self.labels.get(bytecode_target) {
                let offset = target_offset as i32 - *native_offset as i32;

                // Read the instruction at native_offset
                let code = self.buf.code_mut();
                let inst = u32::from_le_bytes([
                    code[*native_offset],
                    code[*native_offset + 1],
                    code[*native_offset + 2],
                    code[*native_offset + 3],
                ]);

                // Determine instruction type and patch
                let patched = if (inst & 0xFC000000) == 0x14000000 {
                    // B instruction
                    0x14000000 | ((offset as u32 / 4) & 0x03FFFFFF)
                } else if (inst & 0xFF000000) == 0xB4000000 {
                    // CBZ instruction
                    let reg = inst & 0x1F;
                    0xB4000000 | (((offset as u32 / 4) & 0x7FFFF) << 5) | reg
                } else if (inst & 0xFF000000) == 0xB5000000 {
                    // CBNZ instruction
                    let reg = inst & 0x1F;
                    0xB5000000 | (((offset as u32 / 4) & 0x7FFFF) << 5) | reg
                } else {
                    // Unknown instruction type, leave as-is
                    inst
                };

                // Write back
                let bytes = patched.to_le_bytes();
                code[*native_offset] = bytes[0];
                code[*native_offset + 1] = bytes[1];
                code[*native_offset + 2] = bytes[2];
                code[*native_offset + 3] = bytes[3];
            }
        }
    }
}

#[cfg(target_arch = "aarch64")]
impl Default for JitCompiler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(all(test, target_arch = "aarch64"))]
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
        let result = compiler.compile(&func, 0);

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
        let result = compiler.compile(&func, 0);
        assert!(result.is_ok());
    }
}
