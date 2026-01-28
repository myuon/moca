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

            Op::Add => self.emit_add_int(),
            Op::Sub => self.emit_sub_int(),
            Op::Mul => self.emit_mul_int(),
            Op::Div => self.emit_div_int(),

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

    /// Integer addition: pop two values, push their sum.
    fn emit_add_int(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop second operand (b)
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP1, regs::VSTACK, 8); // b value

        // Pop first operand (a)
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP0, regs::VSTACK, 8); // a value

        // Add
        asm.add(regs::TMP0, regs::TMP0, regs::TMP1);

        // Push result with int tag
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer subtraction: pop two values, push their difference.
    fn emit_sub_int(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop second operand (b)
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP1, regs::VSTACK, 8);

        // Pop first operand (a)
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP0, regs::VSTACK, 8);

        // Subtract
        asm.sub(regs::TMP0, regs::TMP0, regs::TMP1);

        // Push result
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer multiplication.
    fn emit_mul_int(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop two operands
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP1, regs::VSTACK, 8);
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP0, regs::VSTACK, 8);

        // Multiply
        asm.mul(regs::TMP0, regs::TMP0, regs::TMP1);

        // Push result
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
    }

    /// Integer division.
    fn emit_div_int(&mut self) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);

        // Pop two operands
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP1, regs::VSTACK, 8);
        asm.sub_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);
        asm.ldr(regs::TMP0, regs::VSTACK, 8);

        // Divide
        asm.sdiv(regs::TMP0, regs::TMP0, regs::TMP1);

        // Push result
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::VSTACK, 0);
        asm.str(regs::TMP0, regs::VSTACK, 8);
        asm.add_imm(regs::VSTACK, regs::VSTACK, VALUE_SIZE);

        self.stack_depth = self.stack_depth.saturating_sub(1);
        Ok(())
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
            asm.str(Reg::X0, regs::VSTACK, 0);  // store tag
            asm.str(Reg::X1, regs::VSTACK, 8);  // store payload
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
            let inst =
                0xF2A00000 | ((((u >> 16) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF {
            // MOVK Xd, #imm16, LSL #32
            let inst =
                0xF2C00000 | ((((u >> 32) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF_FFFF {
            // MOVK Xd, #imm16, LSL #48
            let inst =
                0xF2E00000 | ((((u >> 48) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
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
        asm.ldr(Reg::X0, regs::VSTACK, 0);  // tag
        asm.ldr(Reg::X1, regs::VSTACK, 8);  // payload

        // Restore callee-saved registers and return
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);
        asm.ret();

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
