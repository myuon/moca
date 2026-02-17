//! MicroOp-based JIT compiler for AArch64.
//!
//! This compiler takes MicroOp IR (register-based) as input and generates
//! native AArch64 code using a frame-slot model where each VReg maps to
//! a fixed offset from the frame base pointer (FRAME_BASE register).
//!
//! Frame layout (unboxed with shadow tags):
//!   VReg(n) payload → [FRAME_BASE + n * 8]          (8 bytes per slot)
//!   VReg(n) shadow tag → [FRAME_BASE + (total_regs + n) * 8]  (8 bytes per slot)

#[cfg(target_arch = "aarch64")]
use super::aarch64::{AArch64Assembler, Cond, Reg};
#[cfg(target_arch = "aarch64")]
use super::codebuf::CodeBuffer;
#[cfg(target_arch = "aarch64")]
use super::compiler::{CompiledCode, CompiledLoop, value_tags};
#[cfg(target_arch = "aarch64")]
use super::memory::ExecutableMemory;
#[cfg(target_arch = "aarch64")]
use crate::vm::ValueType;
#[cfg(target_arch = "aarch64")]
use crate::vm::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
#[cfg(target_arch = "aarch64")]
use std::collections::{HashMap, HashSet};

/// Register conventions (same as compiler.rs).
#[cfg(target_arch = "aarch64")]
mod regs {
    use super::Reg;

    pub const VM_CTX: Reg = Reg::X19;
    /// Frame base pointer: VReg(n) is at [FRAME_BASE + n*8].
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
    /// Static type for each VReg (used to reconstruct tags at boundaries).
    vreg_types: Vec<ValueType>,
    /// VRegs that need unconditional shadow tag updates because they are written
    /// with multiple different tag types across different MicroOps.
    shadow_conflict_vregs: HashSet<usize>,
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
            vreg_types: Vec::new(),
            shadow_conflict_vregs: HashSet::new(),
        }
    }

    /// Convert a ValueType to the corresponding JIT tag constant.
    fn value_type_to_tag(ty: &ValueType) -> u64 {
        match ty {
            ValueType::I32 | ValueType::I64 => value_tags::TAG_INT,
            ValueType::F32 | ValueType::F64 => value_tags::TAG_FLOAT,
            ValueType::Ref => value_tags::TAG_PTR,
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
        self.vreg_types = converted.vreg_types.clone();
        self.shadow_conflict_vregs = Self::compute_shadow_conflicts(&converted.micro_ops);

        // Emit prologue and shadow tag initialization
        self.emit_prologue();
        self.emit_shadow_init();

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
        self.vreg_types = converted.vreg_types.clone();
        self.shadow_conflict_vregs = Self::compute_shadow_conflicts(&converted.micro_ops);

        // Emit prologue and shadow tag initialization
        self.emit_prologue();
        self.emit_shadow_init();

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

    /// Byte offset of a VReg's slot from FRAME_BASE (8 bytes per slot, payload only).
    fn vreg_offset(vreg: &VReg) -> u16 {
        (vreg.0 * 8) as u16
    }

    /// Byte offset of a VReg's shadow tag from FRAME_BASE.
    /// Shadow tags are stored after all payload slots: [total_regs * 8 + vreg.0 * 8].
    fn shadow_tag_offset(&self, vreg: &VReg) -> u16 {
        ((self.total_regs + vreg.0) * 8) as u16
    }

    /// Initialize the shadow tag area from vreg_types.
    /// Sets default tags so that HeapStore/Ret/etc. can always read from shadow.
    fn emit_shadow_init(&mut self) {
        for i in 0..self.vreg_types.len() {
            let tag = Self::value_type_to_tag(&self.vreg_types[i]);
            let shadow_off = ((self.total_regs + i) * 8) as u16;
            self.emit_load_imm64(tag as i64, regs::TMP0);
            {
                let mut asm = AArch64Assembler::new(&mut self.buf);
                asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
            }
        }
    }

    /// Pre-scan MicroOps to find VRegs written with different shadow tag types.
    /// These VRegs need unconditional shadow updates at every write.
    fn compute_shadow_conflicts(ops: &[MicroOp]) -> HashSet<usize> {
        let mut vreg_tags: HashMap<usize, HashSet<u64>> = HashMap::new();

        fn record(map: &mut HashMap<usize, HashSet<u64>>, vreg: usize, tag: u64) {
            map.entry(vreg).or_default().insert(tag);
        }

        for op in ops {
            match op {
                MicroOp::ConstI64 { dst, .. } | MicroOp::ConstI32 { dst, .. } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_INT);
                }
                MicroOp::ConstF64 { dst, .. } | MicroOp::ConstF32 { dst, .. } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_FLOAT);
                }
                MicroOp::AddI64 { dst, .. }
                | MicroOp::SubI64 { dst, .. }
                | MicroOp::MulI64 { dst, .. }
                | MicroOp::DivI64 { dst, .. }
                | MicroOp::RemI64 { dst, .. }
                | MicroOp::NegI64 { dst, .. }
                | MicroOp::AddI64Imm { dst, .. }
                | MicroOp::AddI32 { dst, .. }
                | MicroOp::SubI32 { dst, .. }
                | MicroOp::MulI32 { dst, .. }
                | MicroOp::DivI32 { dst, .. }
                | MicroOp::RemI32 { dst, .. }
                | MicroOp::CmpI64 { dst, .. }
                | MicroOp::CmpI64Imm { dst, .. }
                | MicroOp::CmpI32 { dst, .. }
                | MicroOp::EqzI32 { dst, .. }
                | MicroOp::I64ExtendI32S { dst, .. }
                | MicroOp::I64ExtendI32U { dst, .. }
                | MicroOp::I32WrapI64 { dst, .. }
                | MicroOp::I64TruncF64S { dst, .. }
                | MicroOp::I32TruncF32S { dst, .. }
                | MicroOp::I32TruncF64S { dst, .. }
                | MicroOp::I64TruncF32S { dst, .. }
                | MicroOp::RefEq { dst, .. }
                | MicroOp::RefIsNull { dst, .. } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_INT);
                }
                MicroOp::AddF64 { dst, .. }
                | MicroOp::SubF64 { dst, .. }
                | MicroOp::MulF64 { dst, .. }
                | MicroOp::DivF64 { dst, .. }
                | MicroOp::NegF64 { dst, .. }
                | MicroOp::CmpF64 { dst, .. }
                | MicroOp::AddF32 { dst, .. }
                | MicroOp::SubF32 { dst, .. }
                | MicroOp::MulF32 { dst, .. }
                | MicroOp::DivF32 { dst, .. }
                | MicroOp::NegF32 { dst, .. }
                | MicroOp::CmpF32 { dst, .. }
                | MicroOp::F64ConvertI64S { dst, .. }
                | MicroOp::F64ConvertI32S { dst, .. }
                | MicroOp::F32ConvertI32S { dst, .. }
                | MicroOp::F32ConvertI64S { dst, .. }
                | MicroOp::F32DemoteF64 { dst, .. }
                | MicroOp::F64PromoteF32 { dst, .. } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_FLOAT);
                }
                MicroOp::RefNull { dst } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_NIL);
                }
                // Dynamic tag sources: always write the correct shadow tag
                MicroOp::HeapLoad { dst, .. }
                | MicroOp::HeapLoadDyn { dst, .. }
                | MicroOp::HeapLoad2 { dst, .. }
                | MicroOp::StackPop { dst }
                | MicroOp::FloatToString { dst, .. }
                | MicroOp::PrintDebug { dst, .. }
                | MicroOp::HeapAllocDynSimple { dst, .. }
                | MicroOp::HeapAllocTyped { dst, .. }
                | MicroOp::StringConst { dst, .. } => {
                    record(&mut vreg_tags, dst.0, u64::MAX);
                }
                MicroOp::Call { ret: Some(ret), .. }
                | MicroOp::CallIndirect { ret: Some(ret), .. } => {
                    record(&mut vreg_tags, ret.0, u64::MAX);
                }
                _ => {}
            }
        }

        vreg_tags
            .into_iter()
            .filter(|(_, tags)| tags.len() > 1)
            .map(|(vreg, _)| vreg)
            .collect()
    }

    /// Check if a shadow tag update is needed for `dst` with `expected_tag`.
    /// Returns `Some(shadow_offset)` if update needed, `None` if already correct.
    fn needs_shadow_update(&self, dst: &VReg, expected_tag: u64) -> Option<u16> {
        if self.shadow_conflict_vregs.contains(&dst.0) {
            return Some(self.shadow_tag_offset(dst));
        }
        let static_tag = self
            .vreg_types
            .get(dst.0)
            .map(Self::value_type_to_tag)
            .unwrap_or(value_tags::TAG_INT);
        if static_tag != expected_tag {
            Some(self.shadow_tag_offset(dst))
        } else {
            None
        }
    }

    /// Emit a shadow tag update store.
    fn emit_shadow_update(&mut self, shadow_off: u16, tag: u64) {
        self.emit_load_imm64(tag as i64, regs::TMP0);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
        }
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

            MicroOp::HeapLoad { dst, src, offset } => self.emit_heap_load(dst, src, *offset),
            MicroOp::HeapLoadDyn { dst, obj, idx } => self.emit_heap_load_dyn(dst, obj, idx),
            MicroOp::HeapStore {
                dst_obj,
                offset,
                src,
            } => self.emit_heap_store(dst_obj, *offset, src),
            MicroOp::HeapStoreDyn { obj, idx, src } => self.emit_heap_store_dyn(obj, idx, src),
            MicroOp::HeapLoad2 { dst, obj, idx } => self.emit_heap_load2(dst, obj, idx),
            MicroOp::HeapStore2 { obj, idx, src } => self.emit_heap_store2(obj, idx, src),

            // f64 ALU
            MicroOp::ConstF64 { dst, imm } => self.emit_const_f64(dst, *imm),
            MicroOp::AddF64 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Add),
            MicroOp::SubF64 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Sub),
            MicroOp::MulF64 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Mul),
            MicroOp::DivF64 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Div),
            MicroOp::NegF64 { dst, src } => self.emit_neg_f64(dst, src),
            MicroOp::CmpF64 { dst, a, b, cond } => self.emit_cmp_f64(dst, a, b, cond),

            // f32 ALU (stored as f64 in frame slots)
            MicroOp::ConstF32 { dst, imm } => self.emit_const_f64(dst, *imm as f64),
            MicroOp::AddF32 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Add),
            MicroOp::SubF32 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Sub),
            MicroOp::MulF32 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Mul),
            MicroOp::DivF32 { dst, a, b } => self.emit_binop_f64(dst, a, b, FpBinOp::Div),
            MicroOp::NegF32 { dst, src } => self.emit_neg_f64(dst, src),
            MicroOp::CmpF32 { dst, a, b, cond } => self.emit_cmp_f64(dst, a, b, cond),

            // i32 ALU (widened to i64 in frame slots)
            MicroOp::AddI32 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Add),
            MicroOp::SubI32 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Sub),
            MicroOp::MulI32 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Mul),
            MicroOp::DivI32 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Div),
            MicroOp::RemI32 { dst, a, b } => self.emit_rem_i64(dst, a, b),
            MicroOp::EqzI32 { dst, src } => self.emit_eqz(dst, src),
            MicroOp::CmpI32 { dst, a, b, cond } => self.emit_cmp_i64(dst, a, b, cond),

            // Type conversions
            MicroOp::I32WrapI64 { dst, src } => self.emit_mov(dst, src),
            MicroOp::I64ExtendI32S { dst, src } => self.emit_i64_extend_i32s(dst, src),
            MicroOp::I64ExtendI32U { dst, src } => self.emit_i64_extend_i32u(dst, src),
            MicroOp::F64ConvertI64S { dst, src } => self.emit_f64_convert_i64s(dst, src),
            MicroOp::I64TruncF64S { dst, src } => self.emit_i64_trunc_f64s(dst, src),
            MicroOp::F64ConvertI32S { dst, src } => self.emit_f64_convert_i64s(dst, src),
            MicroOp::F32ConvertI32S { dst, src } => self.emit_f64_convert_i64s(dst, src),
            MicroOp::F32ConvertI64S { dst, src } => self.emit_f64_convert_i64s(dst, src),
            MicroOp::I32TruncF32S { dst, src } => self.emit_i64_trunc_f64s(dst, src),
            MicroOp::I32TruncF64S { dst, src } => self.emit_i64_trunc_f64s(dst, src),
            MicroOp::I64TruncF32S { dst, src } => self.emit_i64_trunc_f64s(dst, src),
            MicroOp::F32DemoteF64 { dst, src } => self.emit_mov(dst, src),
            MicroOp::F64PromoteF32 { dst, src } => self.emit_mov(dst, src),

            // Ref ops
            MicroOp::RefEq { dst, a, b } => self.emit_ref_eq(dst, a, b),
            MicroOp::RefIsNull { dst, src } => self.emit_ref_is_null(dst, src),
            MicroOp::RefNull { dst } => self.emit_ref_null(dst),

            // Indirect call
            MicroOp::CallIndirect { callee, args, ret } => {
                self.emit_call_indirect(callee, args, ret.as_ref())
            }

            // String operations
            MicroOp::StringConst { dst, idx } => self.emit_string_const(dst, *idx),
            MicroOp::FloatToString { dst, src } => self.emit_float_to_string(dst, src),
            MicroOp::PrintDebug { dst, src } => self.emit_print_debug(dst, src),
            // Heap allocation operations
            MicroOp::HeapAllocDynSimple { dst, size } => self.emit_heap_alloc_dyn_simple(dst, size),
            MicroOp::HeapAllocTyped {
                dst,
                data_ref,
                len,
                kind,
            } => self.emit_heap_alloc_typed(dst, data_ref, len, *kind),
            // Stack bridge (spill/restore across calls)
            MicroOp::StackPush { src } => self.emit_stack_push(src),
            MicroOp::StackPop { dst } => self.emit_stack_pop(dst),

            _ => Err(format!(
                "Unsupported MicroOp for JIT: {:?}",
                std::mem::discriminant(op)
            )),
        }
    }

    // ==================== Constants ====================

    fn emit_const_i64(&mut self, dst: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        self.emit_load_imm64(imm, regs::TMP0);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        }
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Mov ====================

    fn emit_mov(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        if dst == src {
            return Ok(());
        }
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Copy payload
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        // Copy shadow tag
        asm.ldr(regs::TMP0, regs::FRAME_BASE, self.shadow_tag_offset(src));
        asm.str(regs::TMP0, regs::FRAME_BASE, self.shadow_tag_offset(dst));
        Ok(())
    }

    // ==================== i64 ALU ====================

    fn emit_binop_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg, op: BinOp) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        match op {
            BinOp::Add => asm.add(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Sub => asm.sub(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Mul => asm.mul(regs::TMP0, regs::TMP0, regs::TMP1),
            BinOp::Div => asm.sdiv(regs::TMP0, regs::TMP0, regs::TMP1),
        }
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_rem_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.sdiv(regs::TMP2, regs::TMP0, regs::TMP1);
        asm.mul(regs::TMP2, regs::TMP2, regs::TMP1);
        asm.sub(regs::TMP0, regs::TMP0, regs::TMP2);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_neg_i64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        // NEG Xd, Xm  →  SUB Xd, XZR, Xm
        let inst = 0xCB000000
            | ((regs::TMP0.code() as u32) << 16)
            | (31 << 5)
            | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_add_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        }

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
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        }
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
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
        cond.invert()
    }

    fn emit_cmp_i64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let aarch64_cond = Self::cmp_cond_to_aarch64(cond);
        let inv = Self::invert_cond(aarch64_cond);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.cmp(regs::TMP0, regs::TMP1);
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_cmp_i64_imm(
        &mut self,
        dst: &VReg,
        a: &VReg,
        imm: i64,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let aarch64_cond = Self::cmp_cond_to_aarch64(cond);
        let inv = Self::invert_cond(aarch64_cond);

        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        }

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
            let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
            asm.emit_raw(inst);
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        }
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Branches ====================

    fn emit_br_if_false(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(cond));
        drop(asm);

        let current = self.buf.len();
        self.forward_refs.push((current, target));
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.cbz(regs::TMP0, 0);
        Ok(())
    }

    fn emit_br_if(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(cond));
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
        // Load a
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        }

        // Load b / immediate and compare
        match b {
            CmpOperand::Reg(b_vreg) => {
                let mut asm = AArch64Assembler::new(&mut self.buf);
                asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b_vreg));
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
        if func_id == self.self_func_index {
            return self.emit_call_self(args, ret);
        }

        self.emit_call_via_table(func_id, args, ret)
    }

    /// JitCallContext offset for jit_function_table pointer.
    const JIT_FUNC_TABLE_OFFSET: u16 = 104;

    /// Emit a function call that looks up the callee in the JIT function table at runtime.
    /// If the callee is compiled (entry != 0), calls it directly. Otherwise falls back to
    /// call_helper.
    fn emit_call_via_table(
        &mut self,
        func_id: usize,
        args: &[VReg],
        ret: Option<&VReg>,
    ) -> Result<(), String> {
        let argc = args.len();
        let table_entry_offset = (func_id * 16) as u16;

        // Load entry_addr from function table
        // TMP4 = table base, TMP5 = entry_addr
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, Self::JIT_FUNC_TABLE_OFFSET);
            asm.ldr(regs::TMP5, regs::TMP4, table_entry_offset);
        }

        // cbz TMP5, slow_path (will patch offset later)
        let cbz_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbz(regs::TMP5, 0); // placeholder
        }

        // === Fast path: direct call via table ===
        // Load total_regs from table, compute frame size
        // total_regs * 16 (payload + shadow tags, 8 bytes each)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::TMP4, table_entry_offset + 8); // total_regs
            asm.lsl_imm(regs::TMP4, regs::TMP4, 4); // * 16 (payload + shadow)
            // Ensure 16-byte alignment: AND TMP4, TMP4, #~0xF
            // AArch64 logical immediate: N=1, immr=4, imms=59 → 0xFFFFFFFFFFFFFFF0
            let and_inst =
                0x9244EC00 | ((regs::TMP4.code() as u32) << 5) | (regs::TMP4.code() as u32);
            asm.emit_raw(and_inst);
        }

        // Save callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.stp_pre(Reg::X21, Reg::X22, -16);
        }

        // Allocate frame + save frame_aligned (TMP4) on stack
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub(Reg::Sp, Reg::Sp, regs::TMP4); // allocate frame
            asm.stp_pre(regs::TMP4, regs::TMP4, -16); // save frame_aligned
        }

        // Copy args from caller frame to new frame on stack (at SP+16, past saved TMP4 pair)
        // Payload-only copy: 8 bytes per arg
        for i in 0..argc {
            let arg = &args[i];
            let new_offset = (i * 8) as u16 + 16;
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, new_offset);
        }

        // Set up arguments: x0=ctx, x1=new_frame(sp+16), x2=unused
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.add_imm(Reg::X1, Reg::Sp, 16); // skip saved frame_aligned pair
            asm.mov(Reg::X2, Reg::X1);
        }

        // Call via TMP5 (entry_addr loaded earlier)
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.blr(regs::TMP5);
        }

        // Deallocate frame
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(regs::TMP4, regs::TMP0, 16); // restore frame_aligned
            asm.add(Reg::Sp, Reg::Sp, regs::TMP4); // deallocate frame
        }

        // Restore callee-saved
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldp_post(Reg::X21, Reg::X22, 16);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
        }

        // Store return value (x0=tag, x1=payload) → payload to frame, tag to shadow
        if let Some(ret_vreg) = ret {
            let ret_shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(ret_vreg));
            asm.str(Reg::X0, regs::FRAME_BASE, ret_shadow_off);
        }

        // b done (skip slow path, will patch offset later)
        let b_done_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // placeholder
        }

        // === Slow path: call_helper ===
        // call_helper expects JitValue (tag+payload, 16B per arg)
        let slow_path_start = self.buf.len();

        let args_size = argc * 16; // JitValue is 16 bytes
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(Reg::Sp, Reg::Sp, args_aligned as u16);
        }

        // Copy args: read tag from shadow area, load payload from frame
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * 16) as u16;
            let sp_payload_offset = sp_tag_offset + 8;
            let shadow_off = self.shadow_tag_offset(arg);
            {
                let mut asm = AArch64Assembler::new(&mut self.buf);
                asm.ldr(regs::TMP0, regs::FRAME_BASE, shadow_off);
                asm.str(regs::TMP0, Reg::Sp, sp_tag_offset);
                asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
                asm.str(regs::TMP0, Reg::Sp, sp_payload_offset);
            }
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

        // Store return value (x0=tag, x1=payload) → payload to frame, tag to shadow
        if let Some(ret_vreg) = ret {
            let ret_shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(ret_vreg));
            asm.str(Reg::X0, regs::FRAME_BASE, ret_shadow_off);
        }

        let done_pos = self.buf.len();

        // Patch cbz → slow_path
        {
            let offset = (slow_path_start as i32 - cbz_pos as i32) / 4;
            let code = self.buf.code_mut();
            let inst = u32::from_le_bytes([
                code[cbz_pos],
                code[cbz_pos + 1],
                code[cbz_pos + 2],
                code[cbz_pos + 3],
            ]);
            let patched = (inst & 0xFF00001F) | (((offset as u32) & 0x7FFFF) << 5);
            code[cbz_pos..cbz_pos + 4].copy_from_slice(&patched.to_le_bytes());
        }

        // Patch b → done
        {
            let offset = (done_pos as i32 - b_done_pos as i32) / 4;
            let code = self.buf.code_mut();
            let patched = 0x14000000u32 | ((offset as u32) & 0x03FFFFFF);
            code[b_done_pos..b_done_pos + 4].copy_from_slice(&patched.to_le_bytes());
        }

        Ok(())
    }

    fn emit_call_self(&mut self, args: &[VReg], ret: Option<&VReg>) -> Result<(), String> {
        let argc = args.len();
        // Allocate new frame on native stack: payload + shadow tags, 16 bytes per VReg
        let frame_size = self.total_regs * 16;
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

        // Copy args: payload-only (8B per arg)
        for i in 0..argc {
            let arg = &args[i];
            let new_offset = (i * 8) as u16;
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
            asm.str(regs::TMP0, Reg::Sp, new_offset);
        }

        // Set up arguments: x0=ctx, x1=new_frame(sp), x2=unused
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.mov(Reg::X1, Reg::Sp);
            asm.mov(Reg::X2, Reg::Sp);
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

        // Store return value (payload to frame, tag to shadow)
        if let Some(ret_vreg) = ret {
            let ret_shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(ret_vreg));
            asm.str(Reg::X0, regs::FRAME_BASE, ret_shadow_off);
        }

        Ok(())
    }

    // ==================== CallIndirect ====================

    fn emit_call_indirect(
        &mut self,
        callee: &VReg,
        args: &[VReg],
        ret: Option<&VReg>,
    ) -> Result<(), String> {
        let argc = args.len();

        // Step 1: Resolve func_index from callee's heap object slot 0.
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(callee));
            asm.ldr(regs::TMP1, regs::VM_CTX, 48); // heap_base
            asm.add_imm(regs::TMP0, regs::TMP0, 1); // skip header
            asm.lsl_imm(regs::TMP0, regs::TMP0, 3); // byte offset
            asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
            // TMP1 now points to slot 0 tag; slot 0 payload is at +8
            asm.ldr(regs::TMP4, regs::TMP1, 8); // func_index in TMP4
        }

        // Step 2: Allocate space for args (JitValue = 16B for call_helper)
        let args_size = argc * 16;
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.sub_imm(Reg::Sp, Reg::Sp, args_aligned as u16);
        }

        // Step 3: Copy args with tag from shadow area, payload from frame
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * 16) as u16;
            let sp_payload_offset = sp_tag_offset + 8;
            let shadow_off = self.shadow_tag_offset(arg);
            {
                let mut asm = AArch64Assembler::new(&mut self.buf);
                asm.ldr(regs::TMP0, regs::FRAME_BASE, shadow_off);
                asm.str(regs::TMP0, Reg::Sp, sp_tag_offset);
                asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
                asm.str(regs::TMP0, Reg::Sp, sp_payload_offset);
            }
        }

        // Step 4: Save callee-saved registers
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.stp_pre(Reg::X21, Reg::X22, -16);
        }

        // Step 5: Set up call arguments: x0=ctx, x1=func_index, x2=argc, x3=args_ptr
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.mov(Reg::X1, regs::TMP4); // func_index
        }
        self.emit_load_imm64(argc as i64, Reg::X2);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add_imm(Reg::X3, Reg::Sp, 32);
        }

        // Step 6: Load call_helper and call
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP4, regs::VM_CTX, 16);
            asm.blr(regs::TMP4);
        }

        // Step 7: Restore callee-saved registers
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

        // Store return value → payload to frame, tag to shadow
        if let Some(ret_vreg) = ret {
            let ret_shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(ret_vreg));
            asm.str(Reg::X0, regs::FRAME_BASE, ret_shadow_off);
        }

        Ok(())
    }

    // ==================== Return ====================

    fn emit_ret(&mut self, src: Option<&VReg>) -> Result<(), String> {
        if let Some(vreg) = src {
            // Read tag from shadow area, payload from frame
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(Reg::X0, regs::FRAME_BASE, self.shadow_tag_offset(vreg));
            asm.ldr(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(vreg));
        } else {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.mov_imm(Reg::X0, value_tags::TAG_NIL as u16);
            asm.mov_imm(Reg::X1, 0);
        }

        // Inline epilogue
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldp_post(Reg::X21, Reg::X22, 16);
        asm.ldp_post(Reg::X19, Reg::X20, 16);
        asm.ldp_post(Reg::Fp, Reg::Lr, 16);
        asm.ret();

        Ok(())
    }

    // ==================== f64 / f32 ALU ====================

    fn emit_const_f64(&mut self, dst: &VReg, imm: f64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        self.emit_load_imm64(imm.to_bits() as i64, regs::TMP0);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        }
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    fn emit_binop_f64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        op: FpBinOp,
    ) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.fmov_d_x(0, regs::TMP0);
        asm.fmov_d_x(1, regs::TMP1);
        match op {
            FpBinOp::Add => asm.fadd_d(0, 0, 1),
            FpBinOp::Sub => asm.fsub_d(0, 0, 1),
            FpBinOp::Mul => asm.fmul_d(0, 0, 1),
            FpBinOp::Div => asm.fdiv_d(0, 0, 1),
        }
        asm.fmov_x_d(regs::TMP0, 0);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    fn emit_neg_f64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.fmov_d_x(0, regs::TMP0);
        asm.fneg_d(0, 0);
        asm.fmov_x_d(regs::TMP0, 0);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    /// Map CmpCond to AArch64 condition code for floating-point comparisons.
    /// After FCMP, the NZCV flags use different conditions than integer CMP:
    /// - Lt → Mi (negative flag)
    /// - Le → Ls (lower or same, unsigned)
    /// - Gt, Ge, Eq, Ne work the same
    fn fp_cmp_cond_to_aarch64(cond: &CmpCond) -> Cond {
        match cond {
            CmpCond::Eq => Cond::Eq,
            CmpCond::Ne => Cond::Ne,
            CmpCond::LtS => Cond::Mi,
            CmpCond::LeS => Cond::Ls,
            CmpCond::GtS => Cond::Gt,
            CmpCond::GeS => Cond::Ge,
        }
    }

    fn emit_cmp_f64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let aarch64_cond = Self::fp_cmp_cond_to_aarch64(cond);
        let inv = Self::invert_cond(aarch64_cond);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.fmov_d_x(0, regs::TMP0);
        asm.fmov_d_x(1, regs::TMP1);
        asm.fcmp_d(0, 1);
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== i32 extras ====================

    fn emit_eqz(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.cmp_imm(regs::TMP0, 0);
        let inv = Cond::Ne;
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Type Conversions ====================

    /// Sign-extend i32 to i64: SXTW Xd, Wn
    fn emit_i64_extend_i32s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        // SXTW X0, W0: SBFM X0, X0, #0, #31
        let inst = 0x93400000
            | (31 << 10)
            | (0 << 16)
            | ((regs::TMP0.code() as u32) << 5)
            | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// Zero-extend i32 to i64: AND Xd, Xn, #0xFFFFFFFF
    fn emit_i64_extend_i32u(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        // UBFM Xd, Xn, #0, #31 (UXTW)
        let inst = 0xD3400000
            | (31 << 10)
            | (0 << 16)
            | ((regs::TMP0.code() as u32) << 5)
            | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// Convert signed i64 to f64: SCVTF Dd, Xn
    fn emit_f64_convert_i64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.scvtf_d_x(0, regs::TMP0);
        asm.fmov_x_d(regs::TMP0, 0);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    /// Truncate f64 to signed i64: FCVTZS Xd, Dn
    fn emit_i64_trunc_f64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.fmov_d_x(0, regs::TMP0);
        // FCVTZS X0, D0
        let inst = 0x9E780000 | ((0u32) << 5) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Ref Operations ====================

    /// RefEq: dst = (a == b) as i64, comparing payloads (reference identity)
    fn emit_ref_eq(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.cmp(regs::TMP0, regs::TMP1);
        let inv = Cond::Ne;
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// RefIsNull: dst = (src payload == 0) as i64
    /// Heap offset 0 is reserved (next_alloc starts at 1), so payload==0 means null.
    fn emit_ref_is_null(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.cmp_imm(regs::TMP0, 0);
        let inv = Cond::Ne;
        let inst = 0x9A9F07E0 | ((inv as u32) << 12) | (regs::TMP0.code() as u32);
        asm.emit_raw(inst);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// RefNull: dst = null ref (payload=0)
    fn emit_ref_null(&mut self, dst: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_NIL);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.mov_imm(regs::TMP0, 0);
        asm.str(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst));
        drop(asm);
        if let Some(off) = shadow {
            self.emit_shadow_update(off, value_tags::TAG_NIL);
        }
        Ok(())
    }

    // ==================== Heap Operations ====================

    /// Emit HeapLoad: dst = heap[src][offset] (static offset field access).
    /// Loads tag+payload from heap; stores payload to frame, tag to shadow.
    fn emit_heap_load(&mut self, dst: &VReg, src: &VReg, offset: usize) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.ldr(regs::TMP1, regs::VM_CTX, 48); // heap_base
        let slot_offset = (1 + 2 * offset) as u16;
        asm.add_imm(regs::TMP0, regs::TMP0, slot_offset);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        // Load tag and payload from heap
        asm.ldr(regs::TMP0, regs::TMP1, 0); // tag
        asm.ldr(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.str(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
        Ok(())
    }

    /// Emit HeapLoadDyn: dst = heap[obj][idx] (dynamic index access).
    /// Loads tag+payload from heap; stores payload to frame, tag to shadow.
    fn emit_heap_load_dyn(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        asm.ldr(regs::TMP1, regs::VM_CTX, 48);
        asm.lsl_imm(regs::TMP2, regs::TMP2, 1);
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.add(regs::TMP0, regs::TMP0, regs::TMP2);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        // Load tag and payload from heap
        asm.ldr(regs::TMP0, regs::TMP1, 0); // tag
        asm.ldr(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.str(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
        Ok(())
    }

    /// Emit HeapStore: heap[dst_obj][offset] = src (static offset field store).
    /// Reads tag from shadow area; stores tag+payload to heap.
    fn emit_heap_store(&mut self, dst_obj: &VReg, offset: usize, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // TMP2 = tag (from shadow), TMP3 = payload
        asm.ldr(regs::TMP2, regs::FRAME_BASE, shadow_off);
        asm.ldr(regs::TMP3, regs::FRAME_BASE, Self::vreg_offset(src));
        // TMP0 = ref payload
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst_obj));
        // TMP1 = heap_base
        asm.ldr(regs::TMP1, regs::VM_CTX, 48);
        let slot_offset = (1 + 2 * offset) as u16;
        asm.add_imm(regs::TMP0, regs::TMP0, slot_offset);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        // Store tag and payload to heap
        asm.str(regs::TMP2, regs::TMP1, 0);
        asm.str(regs::TMP3, regs::TMP1, 8);
        Ok(())
    }

    /// Emit HeapStoreDyn: heap[obj][idx] = src (dynamic index store).
    /// Reads tag from shadow area; stores tag+payload to heap.
    fn emit_heap_store_dyn(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // TMP4 = tag (from shadow), TMP5 = payload
        asm.ldr(regs::TMP4, regs::FRAME_BASE, shadow_off);
        asm.ldr(regs::TMP5, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.ldr(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        asm.ldr(regs::TMP1, regs::VM_CTX, 48);
        asm.lsl_imm(regs::TMP2, regs::TMP2, 1);
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.add(regs::TMP0, regs::TMP0, regs::TMP2);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        asm.str(regs::TMP4, regs::TMP1, 0);
        asm.str(regs::TMP5, regs::TMP1, 8);
        Ok(())
    }

    /// Emit HeapLoad2: dst = heap[heap[obj][0]][idx] (ptr-indirect dynamic access).
    /// Loads tag+payload from heap; stores payload to frame, tag to shadow.
    fn emit_heap_load2(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        asm.ldr(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        asm.ldr(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: load slot 0 of outer object → inner ref payload
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP3, regs::TMP1, regs::TMP0);
        asm.ldr(regs::TMP0, regs::TMP3, 8); // inner ref payload

        // Step 2: load slot[idx] of inner object → tag + payload
        asm.lsl_imm(regs::TMP2, regs::TMP2, 1);
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.add(regs::TMP0, regs::TMP0, regs::TMP2);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);

        // Load tag and payload from heap
        asm.ldr(regs::TMP0, regs::TMP1, 0); // tag
        asm.ldr(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.str(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
        Ok(())
    }

    /// Emit HeapStore2: heap[heap[obj][0]][idx] = src (ptr-indirect dynamic store).
    /// Reads tag from shadow area; stores tag+payload to heap.
    fn emit_heap_store2(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // TMP4 = tag (from shadow), TMP5 = payload
        asm.ldr(regs::TMP4, regs::FRAME_BASE, shadow_off);
        asm.ldr(regs::TMP5, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.ldr(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        asm.ldr(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        asm.ldr(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: load slot 0 of outer object → inner ref payload
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP3, regs::TMP1, regs::TMP0);
        asm.ldr(regs::TMP0, regs::TMP3, 8);

        // Step 2: store at slot[idx] of inner object
        asm.lsl_imm(regs::TMP2, regs::TMP2, 1);
        asm.add_imm(regs::TMP0, regs::TMP0, 1);
        asm.add(regs::TMP0, regs::TMP0, regs::TMP2);
        asm.lsl_imm(regs::TMP0, regs::TMP0, 3);
        asm.add(regs::TMP1, regs::TMP1, regs::TMP0);
        asm.str(regs::TMP4, regs::TMP1, 0);
        asm.str(regs::TMP5, regs::TMP1, 8);
        Ok(())
    }

    // ==================== String Operations ====================

    /// Emit StringConst: load string from cache (fast path) or call helper (slow path).
    fn emit_string_const(&mut self, dst: &VReg, string_index: usize) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        // Fast path: check string_cache[string_index]
        // string_cache is at JitCallContext offset 56
        // Each cache entry is 16 bytes: Option<GcRef> = [discriminant: u64, index: u64]

        // TMP0 = string_cache pointer
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.ldr(regs::TMP0, regs::VM_CTX, 56); // TMP0 = string_cache
        }
        // TMP0 = &string_cache[string_index]
        self.emit_load_imm64((string_index * 16) as i64, regs::TMP3);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.add(regs::TMP0, regs::TMP0, regs::TMP3);
            // TMP1 = discriminant (0 = None, non-0 = Some)
            asm.ldr(regs::TMP1, regs::TMP0, 0);
        }

        // CBZ TMP1, slow_path (cache miss)
        let cbz_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.cbz(regs::TMP1, 0); // placeholder
        }

        // === FAST PATH: cache hit ===
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // TMP1 = cached GcRef.index (offset 8 from entry)
            asm.ldr(regs::TMP1, regs::TMP0, 8);
            // Store payload to frame
            asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(dst));
        }
        // Write TAG_PTR to shadow
        self.emit_shadow_update(shadow_off, value_tags::TAG_PTR);

        // B to end (skip slow path)
        let b_pos = self.buf.len();
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.b(0); // placeholder
        }

        // === SLOW PATH: cache miss — call push_string_helper ===
        let slow_start = self.buf.len();
        // Patch CBZ
        {
            let offset = (slow_start as i32 - cbz_pos as i32) / 4;
            let code = self.buf.code_mut();
            let inst = u32::from_le_bytes([
                code[cbz_pos],
                code[cbz_pos + 1],
                code[cbz_pos + 2],
                code[cbz_pos + 3],
            ]);
            let patched = (inst & 0xFF00001F) | (((offset as u32) & 0x7FFFF) << 5);
            code[cbz_pos..cbz_pos + 4].copy_from_slice(&patched.to_le_bytes());
        }

        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Save callee-saved
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            // Args: X0=ctx, X1=string_index
            asm.mov(Reg::X0, regs::VM_CTX);
        }
        self.emit_load_imm64(string_index as i64, Reg::X1);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            // Load push_string_helper from JitCallContext offset 24
            asm.ldr(regs::TMP4, regs::VM_CTX, 24);
            asm.blr(regs::TMP4);
            // Restore callee-saved
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
            // Result: X0=tag, X1=payload
            // Store payload to frame, tag to shadow
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(dst));
            asm.str(Reg::X0, regs::FRAME_BASE, shadow_off);
        }

        // === END ===
        let end_pos = self.buf.len();
        // Patch B (unconditional branch)
        {
            let offset = (end_pos as i32 - b_pos as i32) / 4;
            let code = self.buf.code_mut();
            let patched = 0x14000000 | ((offset as u32) & 0x03FFFFFF);
            code[b_pos..b_pos + 4].copy_from_slice(&patched.to_le_bytes());
        }

        Ok(())
    }

    /// Emit FloatToString: call float_to_string_helper(ctx, tag, payload) -> (tag, payload)
    /// Reads source tag from shadow area.
    fn emit_float_to_string(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let src_shadow_off = self.shadow_tag_offset(src);
        let dst_shadow_off = self.shadow_tag_offset(dst);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.mov(Reg::X0, regs::VM_CTX);
            // X1 = tag from shadow area
            asm.ldr(Reg::X1, regs::FRAME_BASE, src_shadow_off);
            asm.ldr(Reg::X2, regs::FRAME_BASE, Self::vreg_offset(src));
            asm.ldr(regs::TMP4, regs::VM_CTX, 72);
            asm.blr(regs::TMP4);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
            // Store payload to frame, tag to shadow
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(dst));
            asm.str(Reg::X0, regs::FRAME_BASE, dst_shadow_off);
        }
        Ok(())
    }

    /// Emit PrintDebug: call print_debug_helper(ctx, tag, payload) -> (tag, payload)
    /// Reads source tag from shadow area.
    fn emit_print_debug(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let src_shadow_off = self.shadow_tag_offset(src);
        let dst_shadow_off = self.shadow_tag_offset(dst);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.mov(Reg::X0, regs::VM_CTX);
            // X1 = tag from shadow area
            asm.ldr(Reg::X1, regs::FRAME_BASE, src_shadow_off);
            asm.ldr(Reg::X2, regs::FRAME_BASE, Self::vreg_offset(src));
            asm.ldr(regs::TMP4, regs::VM_CTX, 80);
            asm.blr(regs::TMP4);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
            // Store payload to frame, tag to shadow
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(dst));
            asm.str(Reg::X0, regs::FRAME_BASE, dst_shadow_off);
        }
        Ok(())
    }

    // ==================== Heap Allocation ====================

    /// Emit HeapAllocDynSimple: call helper(ctx, size_payload) -> (tag, payload)
    fn emit_heap_alloc_dyn_simple(&mut self, dst: &VReg, size: &VReg) -> Result<(), String> {
        let dst_shadow_off = self.shadow_tag_offset(dst);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.ldr(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(size));
            asm.ldr(regs::TMP4, regs::VM_CTX, 88);
            asm.blr(regs::TMP4);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
            // Store payload to frame, tag to shadow
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(dst));
            asm.str(Reg::X0, regs::FRAME_BASE, dst_shadow_off);
        }
        Ok(())
    }

    /// Emit HeapAllocTyped: call helper(ctx, data_ref_payload, len_payload, kind) -> (tag, payload)
    fn emit_heap_alloc_typed(
        &mut self,
        dst: &VReg,
        data_ref: &VReg,
        len: &VReg,
        kind: u8,
    ) -> Result<(), String> {
        let dst_shadow_off = self.shadow_tag_offset(dst);
        {
            let mut asm = AArch64Assembler::new(&mut self.buf);
            asm.stp_pre(regs::VM_CTX, regs::FRAME_BASE, -16);
            asm.mov(Reg::X0, regs::VM_CTX);
            asm.ldr(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(data_ref));
            asm.ldr(Reg::X2, regs::FRAME_BASE, Self::vreg_offset(len));
            asm.movz(Reg::X3, kind as u16, 0);
            asm.ldr(regs::TMP4, regs::VM_CTX, 96);
            asm.blr(regs::TMP4);
            asm.ldp_post(regs::VM_CTX, regs::FRAME_BASE, 16);
            // Store payload to frame, tag to shadow
            asm.str(Reg::X1, regs::FRAME_BASE, Self::vreg_offset(dst));
            asm.str(Reg::X0, regs::FRAME_BASE, dst_shadow_off);
        }
        Ok(())
    }

    // ==================== Stack Bridge ====================

    /// Emit StackPush: push a VReg's tag+payload onto the machine stack.
    /// Reads tag from shadow area.
    fn emit_stack_push(&mut self, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // TMP0 = tag from shadow, TMP1 = payload
        asm.ldr(regs::TMP0, regs::FRAME_BASE, shadow_off);
        asm.ldr(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(src));
        // Push tag+payload pair
        asm.stp_pre(regs::TMP0, regs::TMP1, -16);
        Ok(())
    }

    /// Emit StackPop: pop tag+payload from the machine stack into a VReg.
    /// Stores payload to frame, tag to shadow.
    fn emit_stack_pop(&mut self, dst: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = AArch64Assembler::new(&mut self.buf);
        // Pop pair (tag at lower, payload at higher)
        asm.ldp_post(regs::TMP0, regs::TMP1, 16);
        // Store payload to frame, tag to shadow
        asm.str(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(dst));
        asm.str(regs::TMP0, regs::FRAME_BASE, shadow_off);
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

#[cfg(target_arch = "aarch64")]
enum FpBinOp {
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
