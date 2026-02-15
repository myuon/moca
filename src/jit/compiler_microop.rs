//! MicroOp-based JIT compiler for AArch64.
//!
//! This compiler takes MicroOp IR (register-based) as input and generates
//! native AArch64 code using a frame-slot model where each VReg maps to
//! a fixed offset from the frame base pointer (VSTACK register).
//!
//! Frame layout:
//!   VReg(n) → [VSTACK + n * 16]  (tag at +0, payload at +8)

#[cfg(target_arch = "aarch64")]
use super::aarch64::{AArch64Assembler, Cond, Reg};
#[cfg(target_arch = "aarch64")]
use super::codebuf::CodeBuffer;
#[cfg(target_arch = "aarch64")]
use super::compiler::{CompiledCode, CompiledLoop, VALUE_SIZE, value_tags};
#[cfg(target_arch = "aarch64")]
use super::memory::ExecutableMemory;
#[cfg(target_arch = "aarch64")]
use crate::vm::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
#[cfg(target_arch = "aarch64")]
use std::collections::{HashMap, HashSet};

/// Register conventions (same as compiler.rs).
#[cfg(target_arch = "aarch64")]
mod regs {
    use super::Reg;

    pub const VM_CTX: Reg = Reg::X19;
    /// Frame base pointer: VReg(n) is at [FRAME_BASE + n*16].
    pub const FRAME_BASE: Reg = Reg::X20;
    pub const _LOCALS: Reg = Reg::X21;
    pub const _CONSTS: Reg = Reg::X22;

    // Temporaries
    pub const TMP0: Reg = Reg::X0;
    pub const TMP1: Reg = Reg::X1;
    pub const TMP2: Reg = Reg::X2;
    pub const TMP3: Reg = Reg::X3;
    pub const TMP4: Reg = Reg::X9;
    pub const TMP5: Reg = Reg::X10;
}

/// MicroOp-based JIT compiler for AArch64.
#[cfg(target_arch = "aarch64")]
pub struct MicroOpJitCompiler {
    buf: CodeBuffer,
    /// Labels: MicroOp PC → native code offset.
    labels: HashMap<usize, usize>,
    /// Forward references: (native_offset, microop_target_pc).
    forward_refs: Vec<(usize, usize)>,
    /// Total number of VRegs (locals + temps).
    total_regs: usize,
    /// Function index being compiled (for self-recursion detection).
    self_func_index: usize,
    /// Number of locals in the function.
    self_locals_count: usize,
}

#[cfg(target_arch = "aarch64")]
impl MicroOpJitCompiler {
    pub fn new() -> Self {
        Self {
            buf: CodeBuffer::new(),
            labels: HashMap::new(),
            forward_refs: Vec::new(),
            total_regs: 0,
            self_func_index: 0,
            self_locals_count: 0,
        }
    }

    /// Compile a MicroOp function to native AArch64 code.
    pub fn compile(
        mut self,
        converted: &ConvertedFunction,
        locals_count: usize,
        func_index: usize,
    ) -> Result<CompiledCode, String> {
        self.total_regs = locals_count + converted.temps_count;
        self.self_func_index = func_index;
        self.self_locals_count = locals_count;

        // Emit prologue
        self.emit_prologue();

        // Pre-compute jump targets for peephole optimization safety
        let jump_targets: HashSet<usize> = converted
            .micro_ops
            .iter()
            .filter_map(|op| match op {
                MicroOp::Jmp { target, .. } => Some(*target),
                MicroOp::BrIf { target, .. } => Some(*target),
                MicroOp::BrIfFalse { target, .. } => Some(*target),
                _ => None,
            })
            .collect();

        // Compile each MicroOp
        let ops = &converted.micro_ops;
        let mut pc = 0;
        while pc < ops.len() {
            self.labels.insert(pc, self.buf.len());

            // Peephole: fuse CmpI64/CmpI64Imm + BrIfFalse/BrIf
            let next_pc = pc + 1;
            if next_pc < ops.len() && !jump_targets.contains(&next_pc) {
                if let Some(fused) = self.try_fuse_cmp_branch(&ops[pc], &ops[next_pc]) {
                    fused?;
                    self.labels.insert(next_pc, self.buf.len());
                    pc += 2;
                    continue;
                }
            }

            self.compile_microop(&ops[pc], pc)?;
            pc += 1;
        }

        // Patch forward references
        self.patch_forward_refs();

        // Emit epilogue label
        self.labels.insert(ops.len(), self.buf.len());
        self.emit_epilogue();

        // Allocate executable memory
        let code = self.buf.into_code();
        let mut memory = ExecutableMemory::new(code.len())
            .map_err(|e| format!("Failed to allocate executable memory: {}", e))?;
        memory
            .write(0, &code)
            .map_err(|e| format!("Failed to write code: {}", e))?;
        memory
            .make_executable()
            .map_err(|e| format!("Failed to make memory executable: {}", e))?;

        Ok(CompiledCode {
            memory,
            entry_offset: 0,
            stack_map: HashMap::new(),
            total_regs: self.total_regs,
        })
    }

    /// Compile a loop (MicroOp range) to native AArch64 code.
    ///
    /// # Arguments
    /// * `converted` - The MicroOp-converted function
    /// * `locals_count` - Number of locals in the function
    /// * `func_index` - Function index (for self-recursion detection)
    /// * `loop_start_microop_pc` - MicroOp PC of loop start (backward jump target)
    /// * `loop_end_microop_pc` - MicroOp PC of backward Jmp instruction
    /// * `loop_start_op_pc` - Op PC for CompiledLoop fields
    /// * `loop_end_op_pc` - Op PC for CompiledLoop fields
    pub fn compile_loop(
        mut self,
        converted: &ConvertedFunction,
        locals_count: usize,
        func_index: usize,
        loop_start_microop_pc: usize,
        loop_end_microop_pc: usize,
        loop_start_op_pc: usize,
        loop_end_op_pc: usize,
    ) -> Result<CompiledLoop, String> {
        self.total_regs = locals_count + converted.temps_count;
        self.self_func_index = func_index;
        self.self_locals_count = locals_count;

        // Emit prologue
        self.emit_prologue();

        // Epilogue label: one past the loop end
        let epilogue_label = loop_end_microop_pc + 1;

        // Pre-compute jump targets for peephole optimization safety
        let jump_targets: HashSet<usize> = converted.micro_ops
            [loop_start_microop_pc..=loop_end_microop_pc]
            .iter()
            .filter_map(|op| match op {
                MicroOp::Jmp { target, .. } => Some(*target),
                MicroOp::BrIf { target, .. } => Some(*target),
                MicroOp::BrIfFalse { target, .. } => Some(*target),
                _ => None,
            })
            .collect();

        // Compile each MicroOp in the loop range
        let ops = &converted.micro_ops;
        let mut pc = loop_start_microop_pc;
        while pc <= loop_end_microop_pc {
            self.labels.insert(pc, self.buf.len());

            // Peephole: fuse CmpI64/CmpI64Imm + BrIfFalse/BrIf
            let next_pc = pc + 1;
            if next_pc <= loop_end_microop_pc && !jump_targets.contains(&next_pc) {
                // Check if the branch in a fused pair is a loop exit
                if let Some(fused) =
                    self.try_fuse_cmp_branch_loop(&ops[pc], &ops[next_pc], loop_end_microop_pc)
                {
                    fused?;
                    self.labels.insert(next_pc, self.buf.len());
                    pc += 2;
                    continue;
                }
            }

            // Handle loop-specific patterns
            match &ops[pc] {
                MicroOp::BrIfFalse { target, .. } if *target > loop_end_microop_pc => {
                    // Loop exit: branch to epilogue
                    let cond = match &ops[pc] {
                        MicroOp::BrIfFalse { cond, .. } => cond,
                        _ => unreachable!(),
                    };
                    self.emit_br_if_false(cond, epilogue_label)?;
                }
                MicroOp::BrIf { target, .. } if *target > loop_end_microop_pc => {
                    // Loop exit: branch to epilogue
                    let cond = match &ops[pc] {
                        MicroOp::BrIf { cond, .. } => cond,
                        _ => unreachable!(),
                    };
                    self.emit_br_if(cond, epilogue_label)?;
                }
                MicroOp::Jmp { target, .. } if *target == loop_start_microop_pc => {
                    // Backward branch: jump to loop start
                    self.emit_jmp(loop_start_microop_pc)?;
                }
                MicroOp::Ret { .. } => {
                    return Err("Loop contains Ret instruction".to_string());
                }
                _ => {
                    self.compile_microop(&ops[pc], pc)?;
                }
            }

            pc += 1;
        }

        // Emit epilogue label and code, then patch forward refs
        self.labels.insert(epilogue_label, self.buf.len());
        self.emit_epilogue();
        self.patch_forward_refs();

        // Allocate executable memory
        let code = self.buf.into_code();
        let mut memory = ExecutableMemory::new(code.len())
            .map_err(|e| format!("Failed to allocate executable memory: {}", e))?;
        memory
            .write(0, &code)
            .map_err(|e| format!("Failed to write code: {}", e))?;
        memory
            .make_executable()
            .map_err(|e| format!("Failed to make memory executable: {}", e))?;

        Ok(CompiledLoop {
            memory,
            entry_offset: 0,
            loop_start_pc: loop_start_op_pc,
            loop_end_pc: loop_end_op_pc,
            stack_map: HashMap::new(),
            total_regs: self.total_regs,
        })
    }

    /// Byte offset of a VReg's tag field from FRAME_BASE.
    fn vreg_tag_offset(vreg: &VReg) -> u16 {
        (vreg.0 * VALUE_SIZE as usize) as u16
    }

    /// Byte offset of a VReg's payload field from FRAME_BASE.
    fn vreg_payload_offset(vreg: &VReg) -> u16 {
        (vreg.0 * VALUE_SIZE as usize + 8) as u16
    }

    // ==================== Prologue / Epilogue ====================

    fn emit_prologue(&mut self) {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Save callee-saved registers
        asm.stp_pre(Reg::Fp, Reg::Lr, -16);
        asm.stp_pre(Reg::X19, Reg::X20, -16);
        asm.stp_pre(Reg::X21, Reg::X22, -16);
        // Set up frame pointer
        asm.add_imm(Reg::Fp, Reg::Sp, 0);
        // x0 = VM_CTX, x1 = frame base (locals/regs array), x2 = unused
        asm.mov(regs::VM_CTX, Reg::X0);
        asm.mov(regs::FRAME_BASE, Reg::X1);
    }

    fn emit_epilogue(&mut self) {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);
        asm.ret();
    }

    // ==================== MicroOp compilation ====================

    fn compile_microop(&mut self, op: &MicroOp, _pc: usize) -> Result<(), String> {
        match op {
            MicroOp::ConstI64 { dst, imm } => self.emit_const_i64(dst, *imm),
            MicroOp::ConstI32 { dst, imm } => self.emit_const_i64(dst, *imm as i64),
            MicroOp::Mov { dst, src } => self.emit_mov(dst, src),

            MicroOp::AddI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Add),
            MicroOp::SubI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Sub),
            MicroOp::MulI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Mul),
            MicroOp::DivI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Div),
            MicroOp::RemI64 { dst, a, b } => self.emit_rem_i64(dst, a, b),
            MicroOp::NegI64 { dst, src } => self.emit_neg_i64(dst, src),
            MicroOp::AddI64Imm { dst, a, imm } => self.emit_add_i64_imm(dst, a, *imm),

            MicroOp::CmpI64 { dst, a, b, cond } => self.emit_cmp_i64(dst, a, b, cond),
            MicroOp::CmpI64Imm { dst, a, imm, cond } => self.emit_cmp_i64_imm(dst, a, *imm, cond),

            MicroOp::BrIfFalse { cond, target } => self.emit_br_if_false(cond, *target),
            MicroOp::BrIf { cond, target } => self.emit_br_if(cond, *target),
            MicroOp::Jmp { target, .. } => self.emit_jmp(*target),

            MicroOp::Call { func_id, args, ret } => self.emit_call(*func_id, args, ret.as_ref()),
            MicroOp::Ret { src } => self.emit_ret(src.as_ref()),

            _ => Err(format!(
                "Unsupported MicroOp for JIT: {:?}",
                std::mem::discriminant(op)
            )),
        }
    }

    // ==================== Constants ====================

    fn emit_const_i64(&mut self, dst: &VReg, imm: i64) -> Result<(), String> {
        // Store TAG_INT
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(regs::TMP0, value_tags::TAG_INT as u16);
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        }
        // Store immediate value
        self.emit_load_imm64(imm, regs::TMP0);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        }
        Ok(())
    }

    // ==================== Mov ====================

    fn emit_mov(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        if dst == src {
            return Ok(());
        }
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Copy tag
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        // Copy payload
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        Ok(())
    }

    // ==================== i64 ALU ====================

    fn emit_binop_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg, op: BinOp) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Load payloads
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        // Perform operation
        match op {
            BinOp::Add => asm.add(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Sub => asm.sub(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Mul => asm.mul(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Div => asm.sdiv(regs::TMP0, regs::TMP0, regs::TMP1),
        }
        // Store TAG_INT + result
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        Ok(())
    }

    fn emit_rem_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // TMP0 = a, TMP1 = b
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        // TMP2 = a / b
        asm.sdiv(regs::TMP2, regs::TMP0, regs::TMP1);
        // TMP2 = (a / b) * b
        asm.mul(regs::TMP2, regs::TMP2, regs::TMP1);
        // TMP0 = a - (a / b) * b = a % b
        asm.sub(regs::TMP0, regs::TMP0, regs::TMP2);
        // Store
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        Ok(())
    }

    fn emit_neg_i64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // NEG Xd, Xm  →  SUB Xd, XZR, Xm
        // Encoding: SUB X0, XZR, X0 = 0xCB000000 | (TMP0 << 16) | (31 << 5) | TMP0
        let inst = 0xCB000000
            | ((regs::TMP0.code() as u32) << 16)
            | (31 << 5)
            | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        Ok(())
    }

    fn emit_add_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        }

        // Use add_imm or sub_imm for small values, otherwise load immediate
        if imm >= 0 && imm <= 4095 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add_imm(regs::TMP0, regs::TMP0, imm as u16);
        } else if imm < 0 && (-imm) <= 4095 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(regs::TMP0, regs::TMP0, (-imm) as u16);
        } else {
            self.emit_load_imm64(imm, regs::TMP1);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add(regs::TMP0, regs::TMP0, regs::TMP1);
        }

        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
            asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        }
        Ok(())
    }

    // ==================== Comparisons ====================

    fn cmp_cond_to_aarch64(cond: &CmpCond) -> Cond {
        match cond {
            CmpCond::Eq => Cond::Eq,
            CmpCond::Ne => Cond::Ne,
            CmpCond::LtS => Cond::Lt,
            CmpCond::LeS => Cond::Le,
            CmpCond::GtS => Cond::Gt,
            CmpCond::GeS => Cond::Ge,
        }
    }

    fn invert_cond(cond: Cond) -> Cond {
        match cond {
            Cond::Eq => Cond::Ne,
            Cond::Ne => Cond::Eq,
            Cond::Lt => Cond::Ge,
            Cond::Ge => Cond::Lt,
            Cond::Gt => Cond::Le,
            Cond::Le => Cond::Gt,
            other => other,
        }
    }

    fn emit_cmp_i64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let aarch64_cond = Self::cmp_cond_to_aarch64(cond);
        let inv = Self::invert_cond(aarch64_cond);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Load a and b payloads
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        asm.cmp(regs::TMP0, regs::TMP1);
        // CSINC TMP0, XZR, XZR, inv_cond  → TMP0 = 1 if cond, 0 otherwise
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        // Store as TAG_INT (comparisons in MicroOp produce i64 0/1)
        asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
        asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        Ok(())
    }

    fn emit_cmp_i64_imm(
        &mut self,
        dst: &VReg,
        a: &VReg,
        imm: i64,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let aarch64_cond = Self::cmp_cond_to_aarch64(cond);
        let inv = Self::invert_cond(aarch64_cond);

        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        }

        // Use cmp_imm if possible, otherwise load immediate
        if imm >= 0 && imm <= 4095 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cmp_imm(regs::TMP0, imm as u16);
        } else {
            self.emit_load_imm64(imm, regs::TMP1);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cmp(regs::TMP0, regs::TMP1);
        }

        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // CSINC
            let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
            asm.emit_raw(inst);
            asm.mov_imm(regs::TMP1, value_tags::TAG_INT as u16);
            asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_tag_offset(dst));
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(dst));
        }
        Ok(())
    }

    // ==================== Branches ====================

    fn emit_br_if_false(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(
            regs::TMP0,
            regs::FRAME_BASE,
            Self::vreg_payload_offset(cond),
        );
        drop(asm);

        let current = self.buf.len();
        self.forward_refs.push((current, target));
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.cbz(regs::TMP0, 0);
        Ok(())
    }

    fn emit_br_if(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(
            regs::TMP0,
            regs::FRAME_BASE,
            Self::vreg_payload_offset(cond),
        );
        drop(asm);

        let current = self.buf.len();
        self.forward_refs.push((current, target));
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.cbnz(regs::TMP0, 0);
        Ok(())
    }

    fn emit_jmp(&mut self, target: usize) -> Result<(), String> {
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.b(0);
        Ok(())
    }

    // ==================== Fused Cmp+Branch ====================

    /// Try to fuse CmpI64/CmpI64Imm + BrIfFalse/BrIf into a single compare-and-branch.
    /// Returns None if not fusable, Some(Result) if fused.
    fn try_fuse_cmp_branch(
        &mut self,
        cmp_op: &MicroOp,
        branch_op: &MicroOp,
    ) -> Option<Result<(), String>> {
        let (cmp_dst, cmp_cond, load_a, load_b_or_imm) = match cmp_op {
            MicroOp::CmpI64 { dst, a, b, cond } => (dst, cond, a, CmpOperand::Reg(b)),
            MicroOp::CmpI64Imm { dst, a, imm, cond } => (dst, cond, a, CmpOperand::Imm(*imm)),
            _ => return None,
        };

        let (branch_cond_vreg, target, invert) = match branch_op {
            MicroOp::BrIfFalse { cond, target } => (cond, *target, true),
            MicroOp::BrIf { cond, target } => (cond, *target, false),
            _ => return None,
        };

        // Only fuse if the branch reads the same vreg the cmp wrote to
        if branch_cond_vreg != cmp_dst {
            return None;
        }

        Some(self.emit_fused_cmp_branch(load_a, load_b_or_imm, cmp_cond, target, invert))
    }

    /// Loop-aware version of try_fuse_cmp_branch.
    /// Redirects loop exit branches (target > loop_end) to the epilogue label.
    fn try_fuse_cmp_branch_loop(
        &mut self,
        cmp_op: &MicroOp,
        branch_op: &MicroOp,
        loop_end_microop_pc: usize,
    ) -> Option<Result<(), String>> {
        let (cmp_dst, cmp_cond, load_a, load_b_or_imm) = match cmp_op {
            MicroOp::CmpI64 { dst, a, b, cond } => (dst, cond, a, CmpOperand::Reg(b)),
            MicroOp::CmpI64Imm { dst, a, imm, cond } => (dst, cond, a, CmpOperand::Imm(*imm)),
            _ => return None,
        };

        let (branch_cond_vreg, target, invert) = match branch_op {
            MicroOp::BrIfFalse { cond, target } => (cond, *target, true),
            MicroOp::BrIf { cond, target } => (cond, *target, false),
            _ => return None,
        };

        if branch_cond_vreg != cmp_dst {
            return None;
        }

        // Redirect loop exit branches to epilogue label
        let resolved_target = if target > loop_end_microop_pc {
            loop_end_microop_pc + 1 // epilogue label
        } else {
            target
        };

        Some(self.emit_fused_cmp_branch(load_a, load_b_or_imm, cmp_cond, resolved_target, invert))
    }

    fn emit_fused_cmp_branch(
        &mut self,
        a: &VReg,
        b: CmpOperand,
        cond: &CmpCond,
        target: usize,
        invert: bool,
    ) -> Result<(), String> {
        // Load a payload
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        }

        // Load b / immediate and compare
        match b {
            CmpOperand::Reg(b_vreg) => {
                let mut asm = AArch64Assembler::new(&mut self.buf);
                asm.ldr(
                    regs::TMP1,
                    regs::FRAME_BASE,
                    Self::vreg_payload_offset(b_vreg),
                );
                asm.cmp(regs::TMP0, regs::TMP1);
            }
            CmpOperand::Imm(imm) => {
                if imm >= 0 && imm <= 4095 {
                    let mut asm = AArch64Assembler::new(&mut self.buf);
                    asm.cmp_imm(regs::TMP0, imm as u16);
                } else {
                    self.emit_load_imm64(imm, regs::TMP1);
                    let mut asm = AArch64Assembler::new(&mut self.buf);
                    asm.cmp(regs::TMP0, regs::TMP1);
                }
            }
        }

        // Determine branch condition
        let mut aarch64_cond = Self::cmp_cond_to_aarch64(cond);
        if invert {
            // BrIfFalse: branch when condition is FALSE
            aarch64_cond = Self::invert_cond(aarch64_cond);
        }

        // Emit B.cond with forward reference
        let current = self.buf.len();
        self.forward_refs.push((current, target));
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b_cond(aarch64_cond, 0);
        }

        Ok(())
    }

    // ==================== Call ====================

    fn emit_call(
        &mut self,
        func_id: usize,
        args: &[VReg],
        ret: Option<&VReg>,
    ) -> Result<(), String> {
        let argc = args.len();

        if func_id == self.self_func_index {
            return self.emit_call_self(args, ret);
        }

        // Allocate space on native stack for args array
        let args_size = argc * VALUE_SIZE as usize;
        let args_aligned = (args_size + 15) & !15; // 16-byte align

        if args_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(Reg::Sp, Reg::Sp, args_aligned as u16);
        }

        // Copy args from frame slots to native stack
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * VALUE_SIZE as usize) as u16;
            let sp_payload_offset = sp_tag_offset + 8;
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Load from frame
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, sp_tag_offset);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, sp_payload_offset);
        }

        // Save callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.stp_pre(Reg::X21, Reg::X22, -16);
        }

        // Set up call arguments: x0=ctx, x1=func_id, x2=argc, x3=args_ptr
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
        }
        self.emit_load_imm64(func_id as i64, Reg::X1);
        self.emit_load_imm64(argc as i64, Reg::X2);
        // x3 = sp + 32 (args are below the saved registers: 2 * stp_pre = 32 bytes)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add_imm(Reg::X3, Reg::Sp, 32);
        }

        // Load call_helper from JitCallContext offset 16
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, 16);
            asm.blr(regs::TMP4);
        }

        // Restore callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(Reg::X21, Reg::X22, 16);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
        }

        // Deallocate args space
        if args_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add_imm(Reg::Sp, Reg::Sp, args_aligned as u16);
        }

        // Store return value (x0=tag, x1=payload) into ret vreg
        if let Some(ret_vreg) = ret {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X0, regs::FRAME_BASE, Self::vreg_tag_offset(ret_vreg));
            asm.str(
                Reg::X1,
                regs::FRAME_BASE,
                Self::vreg_payload_offset(ret_vreg),
            );
        }

        Ok(())
    }

    fn emit_call_self(&mut self, args: &[VReg], ret: Option<&VReg>) -> Result<(), String> {
        let argc = args.len();
        // Allocate new frame on native stack for callee locals
        let frame_size = self.total_regs * VALUE_SIZE as usize;
        let frame_aligned = (frame_size + 15) & !15;

        // Save callee-saved registers first
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.stp_pre(Reg::X21, Reg::X22, -16);
        }

        // Allocate frame on native stack
        if frame_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(Reg::Sp, Reg::Sp, frame_aligned as u16);
        }

        // Copy args from current frame to new frame on stack
        for i in 0..argc {
            let arg = &args[i];
            let new_tag_offset = (i * VALUE_SIZE as usize) as u16;
            let new_payload_offset = new_tag_offset + 8;
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, new_tag_offset);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, new_payload_offset);
        }

        // Set up arguments: x0=ctx, x1=new_frame(sp), x2=unused
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.mov(Reg::X1, Reg::Sp); // new frame base
            asm.mov(Reg::X2, Reg::Sp); // unused but match signature
        }

        // BL to function entry (offset 0)
        let bl_site = self.buf.len();
        let rel_offset = -(bl_site as i32);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.bl(rel_offset);
        }

        // Deallocate frame
        if frame_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add_imm(Reg::Sp, Reg::Sp, frame_aligned as u16);
        }

        // Restore callee-saved
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(Reg::X21, Reg::X22, 16);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
        }

        // Store return value
        if let Some(ret_vreg) = ret {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X0, regs::FRAME_BASE, Self::vreg_tag_offset(ret_vreg));
            asm.str(
                Reg::X1,
                regs::FRAME_BASE,
                Self::vreg_payload_offset(ret_vreg),
            );
        }

        Ok(())
    }

    // ==================== Return ====================

    fn emit_ret(&mut self, src: Option<&VReg>) -> Result<(), String> {
        if let Some(vreg) = src {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(Reg::X0, regs::FRAME_BASE, Self::vreg_tag_offset(vreg));
            asm.ldr(Reg::X1, regs::FRAME_BASE, Self::vreg_payload_offset(vreg));
        } else {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(Reg::X0, value_tags::TAG_NIL as u16);
            asm.mov_imm(Reg::X1, 0);
        }

        // Inline epilogue (each Ret needs its own since we can't fall through)
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);
        asm.ret();

        Ok(())
    }

    // ==================== Utilities ====================

    /// Load a 64-bit immediate into a register.
    fn emit_load_imm64(&mut self, n: i64, rd: Reg) {
        let u = n as u64;
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(rd, (u & 0xFFFF) as u16);
        }
        if u > 0xFFFF {
            let inst = 0xF2A00000 | ((((u >> 16) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF {
            let inst = 0xF2C00000 | ((((u >> 32) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
        if u > 0xFFFF_FFFF_FFFF {
            let inst = 0xF2E00000 | ((((u >> 48) & 0xFFFF) as u32) << 5) | (rd.code() as u32);
            self.buf.emit_u32(inst);
        }
    }

    /// Patch all forward jump references with resolved offsets.
    fn patch_forward_refs(&mut self) {
        for (native_offset, target_pc) in &self.forward_refs {
            if let Some(&target_offset) = self.labels.get(target_pc) {
                let offset = target_offset as i32 - *native_offset as i32;
                let code = self.buf.code_mut();
                let inst = u32::from_le_bytes([
                    code[*native_offset],
                    code[*native_offset + 1],
                    code[*native_offset + 2],
                    code[*native_offset + 3],
                ]);

                let patched = if (inst & 0xFC000000) == 0x14000000 {
                    // B instruction
                    0x14000000 | ((offset as u32 / 4) & 0x03FFFFFF)
                } else if (inst & 0xFF000000) == 0xB4000000 {
                    // CBZ
                    let reg = inst & 0x1F;
                    0xB4000000 | (((offset as u32 / 4) & 0x7FFFF) << 5) | reg
                } else if (inst & 0xFF000000) == 0xB5000000 {
                    // CBNZ
                    let reg = inst & 0x1F;
                    0xB5000000 | (((offset as u32 / 4) & 0x7FFFF) << 5) | reg
                } else if (inst & 0xFF000010) == 0x54000000 {
                    // B.cond
                    let cond_bits = inst & 0x0F;
                    0x54000000 | (((offset as u32 / 4) & 0x7FFFF) << 5) | cond_bits
                } else {
                    inst
                };

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
impl Default for MicroOpJitCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Binary operation type for integer ALU.
#[cfg(target_arch = "aarch64")]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Comparison operand (register or immediate).
#[cfg(target_arch = "aarch64")]
enum CmpOperand<'a> {
    Reg(&'a VReg),
    Imm(i64),
}
