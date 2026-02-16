//! MicroOp-based JIT compiler for x86-64.
//!
//! This compiler takes MicroOp IR (register-based) as input and generates
//! native x86-64 code using a frame-slot model where each VReg maps to
//! a fixed offset from the frame base pointer (FRAME_BASE register).
//!
//! Frame layout:
//!   VReg(n) → [FRAME_BASE + n * 16]  (tag at +0, payload at +8)

#[cfg(target_arch = "x86_64")]
use super::codebuf::CodeBuffer;
#[cfg(target_arch = "x86_64")]
use super::compiler_x86_64::{CompiledCode, CompiledLoop, VALUE_SIZE, value_tags};
#[cfg(target_arch = "x86_64")]
use super::memory::ExecutableMemory;
#[cfg(target_arch = "x86_64")]
use super::x86_64::{Cond, Reg, X86_64Assembler};
#[cfg(target_arch = "x86_64")]
use crate::vm::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
#[cfg(target_arch = "x86_64")]
use std::collections::{HashMap, HashSet};

/// Register conventions for MicroOp JIT on x86-64.
#[cfg(target_arch = "x86_64")]
mod regs {
    use super::Reg;

    /// JitCallContext pointer (callee-saved).
    pub const VM_CTX: Reg = Reg::R12;
    /// Frame base pointer: VReg(n) is at [FRAME_BASE + n*16] (callee-saved).
    pub const FRAME_BASE: Reg = Reg::R13;

    // Temporaries (caller-saved)
    pub const TMP0: Reg = Reg::Rax; // Return value (tag)
    pub const TMP1: Reg = Reg::Rcx;
    pub const TMP2: Reg = Reg::Rdx; // Return value (payload), IDIV uses RDX:RAX
    pub const TMP3: Reg = Reg::Rsi;
    pub const TMP4: Reg = Reg::R8;
    pub const TMP5: Reg = Reg::R9;
}

/// MicroOp-based JIT compiler for x86-64.
#[cfg(target_arch = "x86_64")]
pub struct MicroOpJitCompiler {
    buf: CodeBuffer,
    /// Labels: MicroOp PC → native code offset.
    labels: HashMap<usize, usize>,
    /// Forward references: (native_offset, microop_target_pc, ref_kind).
    forward_refs: Vec<(usize, usize, RefKind)>,
    /// Total number of VRegs (locals + temps).
    total_regs: usize,
    /// Function index being compiled (for self-recursion detection).
    self_func_index: usize,
    /// Number of locals in the function.
    self_locals_count: usize,
}

/// Kind of forward reference for patching.
#[cfg(target_arch = "x86_64")]
#[derive(Debug, Clone, Copy)]
enum RefKind {
    /// JMP rel32 (5 bytes: E9 xx xx xx xx)
    Jmp,
    /// JE/JNE rel32 (6 bytes: 0F 84/85 xx xx xx xx)
    Je,
    /// Jcc rel32 (6 bytes: 0F 8x xx xx xx xx)
    Jcc,
}

#[cfg(target_arch = "x86_64")]
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

    /// Compile a MicroOp function to native x86-64 code.
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
            if next_pc < ops.len()
                && !jump_targets.contains(&next_pc)
                && let Some(fused) = self.try_fuse_cmp_branch(&ops[pc], &ops[next_pc])
            {
                fused?;
                self.labels.insert(next_pc, self.buf.len());
                pc += 2;
                continue;
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

    /// Compile a loop (MicroOp range) to native x86-64 code.
    #[allow(clippy::too_many_arguments)]
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
            if next_pc <= loop_end_microop_pc
                && !jump_targets.contains(&next_pc)
                && let Some(fused) =
                    self.try_fuse_cmp_branch_loop(&ops[pc], &ops[next_pc], loop_end_microop_pc)
            {
                fused?;
                self.labels.insert(next_pc, self.buf.len());
                pc += 2;
                continue;
            }

            // Handle loop-specific patterns
            match &ops[pc] {
                MicroOp::BrIfFalse { target, .. } if *target > loop_end_microop_pc => {
                    let cond = match &ops[pc] {
                        MicroOp::BrIfFalse { cond, .. } => cond,
                        _ => unreachable!(),
                    };
                    self.emit_br_if_false(cond, epilogue_label)?;
                }
                MicroOp::BrIf { target, .. } if *target > loop_end_microop_pc => {
                    let cond = match &ops[pc] {
                        MicroOp::BrIf { cond, .. } => cond,
                        _ => unreachable!(),
                    };
                    self.emit_br_if(cond, epilogue_label)?;
                }
                MicroOp::Jmp { target, .. } if *target == loop_start_microop_pc => {
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
    fn vreg_tag_offset(vreg: &VReg) -> i32 {
        (vreg.0 * VALUE_SIZE as usize) as i32
    }

    /// Byte offset of a VReg's payload field from FRAME_BASE.
    fn vreg_payload_offset(vreg: &VReg) -> i32 {
        (vreg.0 * VALUE_SIZE as usize + 8) as i32
    }

    // ==================== Prologue / Epilogue ====================

    fn emit_prologue(&mut self) {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved registers
        asm.push(Reg::Rbp);
        asm.mov_rr(Reg::Rbp, Reg::Rsp);
        asm.push(Reg::Rbx);
        asm.push(Reg::R12);
        asm.push(Reg::R13);
        asm.push(Reg::R14);
        asm.push(Reg::R15);
        // We pushed 6 registers (rbp + 5) = 6 pushes. With the return address that's 7 * 8 = 56.
        // 56 mod 16 = 8, so RSP is 8-byte aligned but not 16-byte aligned.
        // Add sub rsp, 8 to align to 16 bytes before any CALL.
        asm.sub_ri32(Reg::Rsp, 8);
        // Set up context registers: RDI=ctx, RSI=frame_base
        asm.mov_rr(regs::VM_CTX, Reg::Rdi);
        asm.mov_rr(regs::FRAME_BASE, Reg::Rsi);
    }

    fn emit_epilogue(&mut self) {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.add_ri32(Reg::Rsp, 8);
        asm.pop(Reg::R15);
        asm.pop(Reg::R14);
        asm.pop(Reg::R13);
        asm.pop(Reg::R12);
        asm.pop(Reg::Rbx);
        asm.pop(Reg::Rbp);
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
            MicroOp::ToString { dst, src } => self.emit_to_string(dst, src),
            MicroOp::PrintDebug { dst, src } => self.emit_print_debug(dst, src),
            // Heap allocation operations
            MicroOp::HeapAllocDynSimple { dst, size } => self.emit_heap_alloc_dyn_simple(dst, size),
            MicroOp::HeapAllocString { dst, data_ref, len } => {
                self.emit_heap_alloc_string(dst, data_ref, len)
            }
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
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Store TAG_INT
        asm.mov_ri64(regs::TMP0, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
        // Store immediate value
        asm.mov_ri64(regs::TMP0, imm);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Mov ====================

    fn emit_mov(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        if dst == src {
            return Ok(());
        }
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Copy tag
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
        // Copy payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== i64 ALU ====================

    fn emit_binop_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg, op: BinOp) -> Result<(), String> {
        // Polymorphic: check tag of operand `a` to dispatch int vs float.
        // Load tag of `a`
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(a));
            asm.cmp_ri32(regs::TMP0, value_tags::TAG_FLOAT as i32);
        }
        // JE to float path (offset patched below)
        let je_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0); // placeholder offset
        }

        // === Integer path ===
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
            asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
            match op {
                BinOp::Add => asm.add_rr(regs::TMP0, regs::TMP1),
                BinOp::Sub => asm.sub_rr(regs::TMP0, regs::TMP1),
                BinOp::Mul => asm.imul_rr(regs::TMP0, regs::TMP1),
                BinOp::Div => {
                    asm.cqo();
                    asm.idiv(regs::TMP1);
                }
            }
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        }
        // JMP over float path
        let jmp_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder
        }

        // === Float path ===
        let float_path_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
            asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_xmm_r64(1, regs::TMP1);
            match op {
                BinOp::Add => asm.addsd(0, 1),
                BinOp::Sub => asm.subsd(0, 1),
                BinOp::Mul => asm.mulsd(0, 1),
                BinOp::Div => asm.divsd(0, 1),
            }
            asm.movq_r64_xmm(regs::TMP0, 0);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        }
        let end_pos = self.buf.len();

        // Patch JE offset: target = float_path_pos, from = je_patch_pos + 6 (JE rel32 = 6 bytes)
        let je_offset = (float_path_pos as i32) - (je_patch_pos as i32 + 6);
        self.patch_i32(je_patch_pos + 2, je_offset);

        // Patch JMP offset: target = end_pos, from = jmp_patch_pos + 5 (JMP rel32 = 5 bytes)
        let jmp_offset = (end_pos as i32) - (jmp_patch_pos as i32 + 5);
        self.patch_i32(jmp_patch_pos + 1, jmp_offset);

        Ok(())
    }

    fn emit_rem_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP0 (RAX) = a, TMP1 (RCX) = b
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        // CQO + IDIV: remainder is in RDX (TMP2)
        asm.cqo();
        asm.idiv(regs::TMP1);
        // Store TAG_INT + remainder (RDX)
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP2);
        Ok(())
    }

    fn emit_neg_i64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        // Polymorphic: check tag for int vs float negation.
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(src));
            asm.cmp_ri32(regs::TMP0, value_tags::TAG_FLOAT as i32);
        }
        let je_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0);
        }

        // === Integer negation ===
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
            asm.neg(regs::TMP0);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        }
        let jmp_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }

        // === Float negation (XOR sign bit) ===
        let float_path_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
            asm.mov_ri64(regs::TMP1, i64::MIN); // 0x8000000000000000 sign bit mask
            asm.xor_rr(regs::TMP0, regs::TMP1);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        }
        let end_pos = self.buf.len();

        let je_offset = (float_path_pos as i32) - (je_patch_pos as i32 + 6);
        self.patch_i32(je_patch_pos + 2, je_offset);
        let jmp_offset = (end_pos as i32) - (jmp_patch_pos as i32 + 5);
        self.patch_i32(jmp_patch_pos + 1, jmp_offset);

        Ok(())
    }

    fn emit_add_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
            asm.add_ri32(regs::TMP0, imm as i32);
        } else {
            asm.mov_ri64(regs::TMP1, imm);
            asm.add_rr(regs::TMP0, regs::TMP1);
        }
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Comparisons ====================

    fn cmp_cond_to_x86(cond: &CmpCond) -> Cond {
        match cond {
            CmpCond::Eq => Cond::E,
            CmpCond::Ne => Cond::Ne,
            CmpCond::LtS => Cond::L,
            CmpCond::LeS => Cond::Le,
            CmpCond::GtS => Cond::G,
            CmpCond::GeS => Cond::Ge,
        }
    }

    /// Map CmpCond to x86-64 condition code for floating-point comparisons.
    /// After UCOMISD, use unsigned condition codes:
    /// - Lt → B (below), Le → Be (below or equal)
    /// - Gt → A (above), Ge → Ae (above or equal)
    fn fp_cmp_cond_to_x86(cond: &CmpCond) -> Cond {
        match cond {
            CmpCond::Eq => Cond::E,
            CmpCond::Ne => Cond::Ne,
            CmpCond::LtS => Cond::B,
            CmpCond::LeS => Cond::Be,
            CmpCond::GtS => Cond::A,
            CmpCond::GeS => Cond::Ae,
        }
    }

    fn emit_cmp_i64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        cond: &CmpCond,
    ) -> Result<(), String> {
        // Polymorphic: check tag of operand `a` to dispatch int vs float comparison.
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(a));
            asm.cmp_ri32(regs::TMP0, value_tags::TAG_FLOAT as i32);
        }
        // JE to float path
        let je_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0);
        }

        // === Integer comparison path ===
        {
            let x86_cond = Self::cmp_cond_to_x86(cond);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
            asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
            asm.cmp_rr(regs::TMP0, regs::TMP1);
            asm.setcc(x86_cond, regs::TMP0);
            asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        }
        // JMP over float path
        let jmp_patch_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0);
        }

        // === Float comparison path ===
        let float_path_pos = self.buf.len();
        {
            let fp_cond = Self::fp_cmp_cond_to_x86(cond);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
            asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
            asm.movq_xmm_r64(0, regs::TMP0);
            asm.movq_xmm_r64(1, regs::TMP1);
            asm.ucomisd(0, 1);
            asm.setcc(fp_cond, regs::TMP0);
            asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        }

        // === Merge: store result (same for both paths) ===
        let end_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // CmpI64 produces a bool (TAG_INT with 0 or 1)
            asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        }

        // Patch JE: je_patch_pos + 6 → float_path_pos
        let je_offset = (float_path_pos as i32) - (je_patch_pos as i32 + 6);
        self.patch_i32(je_patch_pos + 2, je_offset);

        // Patch JMP: jmp_patch_pos + 5 → end_pos
        let jmp_offset = (end_pos as i32) - (jmp_patch_pos as i32 + 5);
        self.patch_i32(jmp_patch_pos + 1, jmp_offset);

        Ok(())
    }

    fn emit_cmp_i64_imm(
        &mut self,
        dst: &VReg,
        a: &VReg,
        imm: i64,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let x86_cond = Self::cmp_cond_to_x86(cond);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
            asm.cmp_ri32(regs::TMP0, imm as i32);
        } else {
            asm.mov_ri64(regs::TMP1, imm);
            asm.cmp_rr(regs::TMP0, regs::TMP1);
        }
        asm.setcc(x86_cond, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Branches ====================

    fn emit_br_if_false(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(
                regs::TMP0,
                regs::FRAME_BASE,
                Self::vreg_payload_offset(cond),
            );
            asm.test_rr(regs::TMP0, regs::TMP0);
        }

        let current = self.buf.len();
        self.forward_refs.push((current, target, RefKind::Je));
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.je_rel32(0); // placeholder, will be patched
        Ok(())
    }

    fn emit_br_if(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(
                regs::TMP0,
                regs::FRAME_BASE,
                Self::vreg_payload_offset(cond),
            );
            asm.test_rr(regs::TMP0, regs::TMP0);
        }

        let current = self.buf.len();
        self.forward_refs.push((current, target, RefKind::Je));
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.jne_rel32(0); // placeholder
        Ok(())
    }

    fn emit_jmp(&mut self, target: usize) -> Result<(), String> {
        let current = self.buf.len();
        self.forward_refs.push((current, target, RefKind::Jmp));
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.jmp_rel32(0); // placeholder
        Ok(())
    }

    // ==================== Fused Cmp+Branch ====================

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

        if branch_cond_vreg != cmp_dst {
            return None;
        }

        Some(self.emit_fused_cmp_branch(load_a, load_b_or_imm, cmp_cond, target, invert))
    }

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

        let resolved_target = if target > loop_end_microop_pc {
            loop_end_microop_pc + 1
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
        // For register-register comparison (CmpI64), need polymorphic dispatch
        // since the operands may be floats. CmpI64Imm is always integer.
        if let CmpOperand::Reg(b_vreg) = b {
            // Check tag of operand `a`
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(a));
                asm.cmp_ri32(regs::TMP0, value_tags::TAG_FLOAT as i32);
            }
            // JE to float comparison path
            let je_patch_pos = self.buf.len();
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.je_rel32(0);
            }

            // === Integer comparison + branch ===
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
                asm.mov_rm(
                    regs::TMP1,
                    regs::FRAME_BASE,
                    Self::vreg_payload_offset(b_vreg),
                );
                asm.cmp_rr(regs::TMP0, regs::TMP1);
            }
            let mut int_cond = Self::cmp_cond_to_x86(cond);
            if invert {
                int_cond = int_cond.invert();
            }
            let current = self.buf.len();
            self.forward_refs.push((current, target, RefKind::Jcc));
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.jcc_rel32(int_cond, 0);
            }
            // JMP over float path
            let jmp_patch_pos = self.buf.len();
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.jmp_rel32(0);
            }

            // === Float comparison + branch ===
            let float_path_pos = self.buf.len();
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
                asm.mov_rm(
                    regs::TMP1,
                    regs::FRAME_BASE,
                    Self::vreg_payload_offset(b_vreg),
                );
                asm.movq_xmm_r64(0, regs::TMP0);
                asm.movq_xmm_r64(1, regs::TMP1);
                asm.ucomisd(0, 1);
            }
            let mut fp_cond = Self::fp_cmp_cond_to_x86(cond);
            if invert {
                fp_cond = fp_cond.invert();
            }
            let current_fp = self.buf.len();
            self.forward_refs.push((current_fp, target, RefKind::Jcc));
            {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.jcc_rel32(fp_cond, 0);
            }

            let end_pos = self.buf.len();

            // Patch JE: target float_path_pos
            let je_offset = (float_path_pos as i32) - (je_patch_pos as i32 + 6);
            self.patch_i32(je_patch_pos + 2, je_offset);

            // Patch JMP: target end_pos
            let jmp_offset = (end_pos as i32) - (jmp_patch_pos as i32 + 5);
            self.patch_i32(jmp_patch_pos + 1, jmp_offset);

            return Ok(());
        }

        // CmpI64Imm path: always integer
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        }
        if let CmpOperand::Imm(imm) = b {
            if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.cmp_ri32(regs::TMP0, imm as i32);
            } else {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.mov_ri64(regs::TMP1, imm);
                asm.cmp_rr(regs::TMP0, regs::TMP1);
            }
        }

        // Determine branch condition
        let mut x86_cond = Self::cmp_cond_to_x86(cond);
        if invert {
            // BrIfFalse: branch when condition is FALSE
            x86_cond = x86_cond.invert();
        }

        // Emit Jcc with forward reference
        let current = self.buf.len();
        self.forward_refs.push((current, target, RefKind::Jcc));
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jcc_rel32(x86_cond, 0);
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
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Copy args from frame slots to native stack
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * VALUE_SIZE as usize) as i32;
            let sp_payload_offset = sp_tag_offset + 8;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(arg));
            asm.mov_mr(Reg::Rsp, sp_tag_offset, regs::TMP0);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(arg));
            asm.mov_mr(Reg::Rsp, sp_payload_offset, regs::TMP0);
        }

        // Save callee-saved registers (VM_CTX, FRAME_BASE)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
        }

        // Set up call arguments: RDI=ctx, RSI=func_id, RDX=argc, RCX=args_ptr
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_ri64(Reg::Rsi, func_id as i64);
            asm.mov_ri64(Reg::Rdx, argc as i64);
            // RCX = rsp + 16 (args are below the 2 pushed registers)
            asm.mov_rr(Reg::Rcx, Reg::Rsp);
            asm.add_ri32(Reg::Rcx, 16);
        }

        // Load call_helper from JitCallContext offset 16
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP4, regs::VM_CTX, 16);
            asm.call_r(regs::TMP4);
        }

        // Restore callee-saved registers
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
        }

        // Deallocate args space
        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.add_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Store return value (RAX=tag, RDX=payload) into ret vreg
        if let Some(ret_vreg) = ret {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(ret_vreg), Reg::Rax);
            asm.mov_mr(
                regs::FRAME_BASE,
                Self::vreg_payload_offset(ret_vreg),
                Reg::Rdx,
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
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
        }

        // Allocate frame on native stack
        if frame_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(Reg::Rsp, frame_aligned as i32);
        }

        // Copy args from current frame to new frame on stack
        for (i, arg) in args.iter().enumerate().take(argc) {
            let new_tag_offset = (i * VALUE_SIZE as usize) as i32;
            let new_payload_offset = new_tag_offset + 8;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(arg));
            asm.mov_mr(Reg::Rsp, new_tag_offset, regs::TMP0);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(arg));
            asm.mov_mr(Reg::Rsp, new_payload_offset, regs::TMP0);
        }

        // Set up arguments: RDI=ctx, RSI=new_frame(rsp), RDX=unused
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_rr(Reg::Rsi, Reg::Rsp);
            asm.mov_rr(Reg::Rdx, Reg::Rsp); // unused but match signature
        }

        // CALL to function entry (offset 0)
        // Calculate relative offset: target is 0, current position is buf.len()
        // CALL rel32 is 5 bytes, offset = target - (current + 5)
        let call_site = self.buf.len();
        let rel_offset = -(call_site as i32 + 5);
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.call_rel32(rel_offset);
        }

        // Deallocate frame
        if frame_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.add_ri32(Reg::Rsp, frame_aligned as i32);
        }

        // Restore callee-saved
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
        }

        // Store return value (RAX=tag, RDX=payload)
        if let Some(ret_vreg) = ret {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(ret_vreg), Reg::Rax);
            asm.mov_mr(
                regs::FRAME_BASE,
                Self::vreg_payload_offset(ret_vreg),
                Reg::Rdx,
            );
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
        // func_index = heap[callee][0].payload
        // Address: heap_base + (ref_payload + 1) * 8 + 8
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(
                regs::TMP0,
                regs::FRAME_BASE,
                Self::vreg_payload_offset(callee),
            );
            asm.mov_rm(regs::TMP1, regs::VM_CTX, 48); // heap_base
            asm.add_ri32(regs::TMP0, 1); // skip header
            asm.shl_ri(regs::TMP0, 3); // byte offset
            asm.add_rr(regs::TMP1, regs::TMP0);
            // TMP1 now points to slot 0 tag; slot 0 payload is at +8
            asm.mov_rm(regs::TMP4, regs::TMP1, 8); // func_index in TMP4 (R8)
        }

        // Step 2: Allocate space on native stack for args array
        let args_size = argc * VALUE_SIZE as usize;
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Step 3: Copy args from frame slots to native stack
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * VALUE_SIZE as usize) as i32;
            let sp_payload_offset = sp_tag_offset + 8;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(arg));
            asm.mov_mr(Reg::Rsp, sp_tag_offset, regs::TMP0);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(arg));
            asm.mov_mr(Reg::Rsp, sp_payload_offset, regs::TMP0);
        }

        // Step 4: Save callee-saved registers
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
        }

        // Step 5: Set up call arguments: RDI=ctx, RSI=func_index, RDX=argc, RCX=args_ptr
        // TMP4 (R8) still holds func_index
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_rr(Reg::Rsi, regs::TMP4); // func_index
            asm.mov_ri64(Reg::Rdx, argc as i64);
            asm.mov_rr(Reg::Rcx, Reg::Rsp);
            asm.add_ri32(Reg::Rcx, 16); // skip 2 pushed registers
        }

        // Step 6: Load call_helper from JitCallContext offset 16 and call
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP4, regs::VM_CTX, 16);
            asm.call_r(regs::TMP4);
        }

        // Step 7: Restore callee-saved registers
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
        }

        // Deallocate args space
        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.add_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Store return value (RAX=tag, RDX=payload) into ret vreg
        if let Some(ret_vreg) = ret {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(ret_vreg), Reg::Rax);
            asm.mov_mr(
                regs::FRAME_BASE,
                Self::vreg_payload_offset(ret_vreg),
                Reg::Rdx,
            );
        }

        Ok(())
    }

    // ==================== Return ====================

    fn emit_ret(&mut self, src: Option<&VReg>) -> Result<(), String> {
        if let Some(vreg) = src {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // RAX = tag, RDX = payload
            asm.mov_rm(Reg::Rax, regs::FRAME_BASE, Self::vreg_tag_offset(vreg));
            asm.mov_rm(Reg::Rdx, regs::FRAME_BASE, Self::vreg_payload_offset(vreg));
        } else {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_ri64(Reg::Rax, value_tags::TAG_NIL as i64);
            asm.xor_rr(Reg::Rdx, Reg::Rdx);
        }

        // Inline epilogue
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.add_ri32(Reg::Rsp, 8);
        asm.pop(Reg::R15);
        asm.pop(Reg::R14);
        asm.pop(Reg::R13);
        asm.pop(Reg::R12);
        asm.pop(Reg::Rbx);
        asm.pop(Reg::Rbp);
        asm.ret();

        Ok(())
    }

    // ==================== f64 / f32 ALU ====================

    fn emit_const_f64(&mut self, dst: &VReg, imm: f64) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Store TAG_FLOAT
        asm.mov_ri64(regs::TMP0, value_tags::TAG_FLOAT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
        // Store f64 bits as payload
        asm.mov_ri64(regs::TMP0, imm.to_bits() as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    fn emit_binop_f64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        op: FpBinOp,
    ) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Load payloads (f64 bits) into GP regs, then move to XMM regs
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        asm.movq_xmm_r64(0, regs::TMP0); // XMM0 = a
        asm.movq_xmm_r64(1, regs::TMP1); // XMM1 = b
        // Perform FP operation
        match op {
            FpBinOp::Add => asm.addsd(0, 1),
            FpBinOp::Sub => asm.subsd(0, 1),
            FpBinOp::Mul => asm.mulsd(0, 1),
            FpBinOp::Div => asm.divsd(0, 1),
        }
        // Move result back to GP
        asm.movq_r64_xmm(regs::TMP0, 0);
        // Store TAG_FLOAT + result
        asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    fn emit_neg_f64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // XOR sign bit: flip bit 63
        asm.mov_ri64(regs::TMP1, i64::MIN); // 0x8000000000000000
        asm.xor_rr(regs::TMP0, regs::TMP1);
        // Store TAG_FLOAT + result
        asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    fn emit_cmp_f64(
        &mut self,
        dst: &VReg,
        a: &VReg,
        b: &VReg,
        cond: &CmpCond,
    ) -> Result<(), String> {
        let x86_cond = Self::fp_cmp_cond_to_x86(cond);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Load payloads into XMM regs
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        asm.movq_xmm_r64(0, regs::TMP0);
        asm.movq_xmm_r64(1, regs::TMP1);
        asm.ucomisd(0, 1);
        asm.setcc(x86_cond, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        // Store as TAG_INT
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== i32 extras ====================

    fn emit_eqz(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.test_rr(regs::TMP0, regs::TMP0);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Type Conversions ====================

    /// Sign-extend i32 to i64: MOVSXD r64, r32
    fn emit_i64_extend_i32s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.movsxd(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    /// Zero-extend i32 to i64: MOV r32, r32 (clears upper 32 bits)
    fn emit_i64_extend_i32u(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // MOV r32, r32 zero-extends to 64-bit
        asm.mov_r32_r32(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    /// Convert signed i64 to f64: CVTSI2SD xmm, r64
    fn emit_f64_convert_i64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
        asm.movq_r64_xmm(regs::TMP0, 0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_FLOAT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    /// Truncate f64 to signed i64: CVTTSD2SI r64, xmm
    fn emit_i64_trunc_f64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.movq_xmm_r64(0, regs::TMP0);
        asm.cvttsd2si_r64_xmm(regs::TMP0, 0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Ref Operations ====================

    fn emit_ref_eq(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_payload_offset(b));
        asm.cmp_rr(regs::TMP0, regs::TMP1);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    fn emit_ref_is_null(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.cmp_ri32(regs::TMP0, value_tags::TAG_NIL as i32);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_ri64(regs::TMP1, value_tags::TAG_INT as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    fn emit_ref_null(&mut self, dst: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_ri64(regs::TMP0, value_tags::TAG_NIL as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
        asm.xor_rr(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== String Operations ====================

    /// Emit StringConst: load string from cache (fast path) or call helper (slow path).
    fn emit_string_const(&mut self, dst: &VReg, string_index: usize) -> Result<(), String> {
        // Fast path: check string_cache[string_index]
        // string_cache is at JitCallContext offset 56
        // Each cache entry is 16 bytes: Option<GcRef> = [discriminant: u64, index: u64]

        // TMP0 = string_cache pointer
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::VM_CTX, 56);
            // TMP0 = &string_cache[string_index]
            asm.add_ri32(regs::TMP0, (string_index * 16) as i32);
            // TMP1 = discriminant (0 = None, non-0 = Some)
            asm.mov_rm(regs::TMP1, regs::TMP0, 0);
            // Check if None
            asm.test_rr(regs::TMP1, regs::TMP1);
        }

        // JE to slow path (cache miss)
        let je_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0); // placeholder
        }

        // === FAST PATH: cache hit ===
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // TMP1 = cached GcRef.index (offset 8 from entry)
            asm.mov_rm(regs::TMP1, regs::TMP0, 8);
            // Store TAG_PTR + index to dst
            asm.mov_ri64(regs::TMP0, value_tags::TAG_PTR as i64);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP1);
        }

        // JMP to end (skip slow path)
        let jmp_pos = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder
        }

        // === SLOW PATH: cache miss — call push_string_helper ===
        let slow_start = self.buf.len();
        // Patch JE
        {
            let offset = (slow_start as i32) - (je_pos as i32) - 6;
            let code = self.buf.code_mut();
            code[je_pos + 2..je_pos + 6].copy_from_slice(&offset.to_le_bytes());
        }

        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // Save callee-saved
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
            // Args: RDI=ctx, RSI=string_index
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_ri64(Reg::Rsi, string_index as i64);
            // Load push_string_helper from JitCallContext offset 24
            asm.mov_rm(regs::TMP4, regs::VM_CTX, 24);
            asm.call_r(regs::TMP4);
            // Restore callee-saved
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
            // Update heap_base from context (helper may have triggered GC)
            // Result: RAX=tag, RDX=payload
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), Reg::Rax);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), Reg::Rdx);
        }

        // === END ===
        let end_pos = self.buf.len();
        // Patch JMP
        {
            let offset = (end_pos as i32) - (jmp_pos as i32) - 5;
            let code = self.buf.code_mut();
            code[jmp_pos + 1..jmp_pos + 5].copy_from_slice(&offset.to_le_bytes());
        }

        Ok(())
    }

    /// Emit ToString: call to_string_helper(ctx, tag, payload) -> (tag, payload)
    fn emit_to_string(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved
        asm.push(regs::VM_CTX);
        asm.push(regs::FRAME_BASE);
        // Args: RDI=ctx, RSI=tag, RDX=payload
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_rm(Reg::Rsi, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_rm(Reg::Rdx, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // Load to_string_helper from JitCallContext offset 72
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 72);
        asm.call_r(regs::TMP4);
        // Restore callee-saved
        asm.pop(regs::FRAME_BASE);
        asm.pop(regs::VM_CTX);
        // Store result (RAX=tag, RDX=payload)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), Reg::Rax);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), Reg::Rdx);
        // Update heap_base (GC may have moved heap)
        asm.mov_rm(regs::TMP0, regs::VM_CTX, 48);
        // (heap_base is read fresh each time from VM_CTX, so no update needed here)
        Ok(())
    }

    /// Emit PrintDebug: call print_debug_helper(ctx, tag, payload) -> (tag, payload)
    fn emit_print_debug(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved
        asm.push(regs::VM_CTX);
        asm.push(regs::FRAME_BASE);
        // Args: RDI=ctx, RSI=tag, RDX=payload
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_rm(Reg::Rsi, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_rm(Reg::Rdx, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // Load print_debug_helper from JitCallContext offset 80
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 80);
        asm.call_r(regs::TMP4);
        // Restore callee-saved
        asm.pop(regs::FRAME_BASE);
        asm.pop(regs::VM_CTX);
        // Store result (returns original value: RAX=tag, RDX=payload)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), Reg::Rax);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), Reg::Rdx);
        Ok(())
    }

    // ==================== Heap Allocation ====================

    /// Emit HeapAllocDynSimple: call helper(ctx, size_payload) -> (tag, payload)
    fn emit_heap_alloc_dyn_simple(&mut self, dst: &VReg, size: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved
        asm.push(regs::VM_CTX);
        asm.push(regs::FRAME_BASE);
        // Args: RDI=ctx, RSI=size (payload only, since size is always i64)
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_rm(Reg::Rsi, regs::FRAME_BASE, Self::vreg_payload_offset(size));
        // Load heap_alloc_dyn_simple_helper from JitCallContext offset 88
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 88);
        asm.call_r(regs::TMP4);
        // Restore callee-saved
        asm.pop(regs::FRAME_BASE);
        asm.pop(regs::VM_CTX);
        // Store result (RAX=tag, RDX=payload)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), Reg::Rax);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), Reg::Rdx);
        Ok(())
    }

    /// Emit HeapAllocString: call helper(ctx, data_ref_payload, len_payload) -> (tag, payload)
    fn emit_heap_alloc_string(
        &mut self,
        dst: &VReg,
        data_ref: &VReg,
        len: &VReg,
    ) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved
        asm.push(regs::VM_CTX);
        asm.push(regs::FRAME_BASE);
        // Args: RDI=ctx, RSI=data_ref_payload, RDX=len_payload
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_rm(
            Reg::Rsi,
            regs::FRAME_BASE,
            Self::vreg_payload_offset(data_ref),
        );
        asm.mov_rm(Reg::Rdx, regs::FRAME_BASE, Self::vreg_payload_offset(len));
        // Load heap_alloc_string_helper from JitCallContext offset 96
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 96);
        asm.call_r(regs::TMP4);
        // Restore callee-saved
        asm.pop(regs::FRAME_BASE);
        asm.pop(regs::VM_CTX);
        // Store result (RAX=tag, RDX=payload)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), Reg::Rax);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), Reg::Rdx);
        Ok(())
    }

    // ==================== Stack Bridge ====================

    /// Emit StackPush: push a VReg's tag+payload onto the machine stack.
    /// Used to spill values across function calls.
    fn emit_stack_push(&mut self, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Push payload first (stack grows down, so payload will be at higher address after pop)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        asm.push(regs::TMP0);
        // Push tag second (will be popped first)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.push(regs::TMP0);
        Ok(())
    }

    /// Emit StackPop: pop tag+payload from the machine stack into a VReg.
    fn emit_stack_pop(&mut self, dst: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Pop tag first (was pushed second)
        asm.pop(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP0);
        // Pop payload second (was pushed first)
        asm.pop(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Heap Operations ====================

    /// Emit HeapLoad: dst = heap[src][offset] (static offset field access).
    fn emit_heap_load(&mut self, dst: &VReg, src: &VReg, offset: usize) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP0 = ref payload (heap word offset)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // TMP1 = heap_base (JitCallContext offset 48)
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);
        // TMP0 = ref_payload + 1 + 2*offset (skip header + slot offset)
        let slot_offset = (1 + 2 * offset) as i32;
        asm.add_ri32(regs::TMP0, slot_offset);
        // TMP0 = TMP0 * 8 (word to byte offset)
        asm.shl_ri(regs::TMP0, 3);
        // TMP1 = heap_base + byte_offset
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Load tag and payload from heap
        asm.mov_rm(regs::TMP2, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP3, regs::TMP1, 8); // payload
        // Store to dst VReg
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP3);
        Ok(())
    }

    /// Emit HeapLoadDyn: dst = heap[obj][idx] (dynamic index access).
    fn emit_heap_load_dyn(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_payload_offset(idx));
        // TMP0 = ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);
        // TMP2 = index * 2
        asm.shl_ri(regs::TMP2, 1);
        // TMP0 = ref + 1 (skip header)
        asm.add_ri32(regs::TMP0, 1);
        // TMP0 = ref + 1 + 2*index
        asm.add_rr(regs::TMP0, regs::TMP2);
        // TMP0 = byte offset
        asm.shl_ri(regs::TMP0, 3);
        // TMP1 = heap_base + byte_offset
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Load tag and payload
        asm.mov_rm(regs::TMP2, regs::TMP1, 0);
        asm.mov_rm(regs::TMP3, regs::TMP1, 8);
        // Store to dst
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP3);
        Ok(())
    }

    /// Emit HeapStore: heap[dst_obj][offset] = src (static offset field store).
    fn emit_heap_store(&mut self, dst_obj: &VReg, offset: usize, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Load value to store
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_rm(regs::TMP3, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // TMP0 = ref payload
        asm.mov_rm(
            regs::TMP0,
            regs::FRAME_BASE,
            Self::vreg_payload_offset(dst_obj),
        );
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);
        // Calculate address
        let slot_offset = (1 + 2 * offset) as i32;
        asm.add_ri32(regs::TMP0, slot_offset);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Store tag and payload to heap
        asm.mov_mr(regs::TMP1, 0, regs::TMP2);
        asm.mov_mr(regs::TMP1, 8, regs::TMP3);
        Ok(())
    }

    /// Emit HeapStoreDyn: heap[obj][idx] = src (dynamic index store).
    fn emit_heap_store_dyn(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Load value to store
        asm.mov_rm(regs::TMP4, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_rm(regs::TMP5, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_payload_offset(idx));
        // TMP0 = ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);
        // Calculate address
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Store tag and payload
        asm.mov_mr(regs::TMP1, 0, regs::TMP4);
        asm.mov_mr(regs::TMP1, 8, regs::TMP5);
        Ok(())
    }

    /// Emit HeapLoad2: dst = heap[heap[obj][0]][idx] (ptr-indirect dynamic access).
    fn emit_heap_load2(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_payload_offset(idx));
        // TMP0 = outer ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: load slot 0 of outer object → inner ref payload
        // addr = heap_base + (ref + 1) * 8
        asm.add_ri32(regs::TMP0, 1);
        asm.shl_ri(regs::TMP0, 3);
        asm.mov_rr(regs::TMP3, regs::TMP1);
        asm.add_rr(regs::TMP3, regs::TMP0);
        // TMP0 = inner ref payload (slot 0 payload at offset +8)
        asm.mov_rm(regs::TMP0, regs::TMP3, 8);

        // Step 2: load slot[idx] of inner object
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);

        // Load tag and payload
        asm.mov_rm(regs::TMP2, regs::TMP1, 0);
        asm.mov_rm(regs::TMP3, regs::TMP1, 8);
        // Store to dst
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_tag_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_payload_offset(dst), regs::TMP3);
        Ok(())
    }

    /// Emit HeapStore2: heap[heap[obj][0]][idx] = src (ptr-indirect dynamic store).
    fn emit_heap_store2(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Load value to store
        asm.mov_rm(regs::TMP4, regs::FRAME_BASE, Self::vreg_tag_offset(src));
        asm.mov_rm(regs::TMP5, regs::FRAME_BASE, Self::vreg_payload_offset(src));
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_payload_offset(idx));
        // TMP0 = outer ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_payload_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: load slot 0 of outer object → inner ref payload
        asm.add_ri32(regs::TMP0, 1);
        asm.shl_ri(regs::TMP0, 3);
        asm.mov_rr(regs::TMP3, regs::TMP1);
        asm.add_rr(regs::TMP3, regs::TMP0);
        asm.mov_rm(regs::TMP0, regs::TMP3, 8);

        // Step 2: store at slot[idx] of inner object
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Store tag and payload
        asm.mov_mr(regs::TMP1, 0, regs::TMP4);
        asm.mov_mr(regs::TMP1, 8, regs::TMP5);
        Ok(())
    }

    // ==================== Utilities ====================

    /// Patch a 32-bit immediate at the given offset in the code buffer.
    fn patch_i32(&mut self, offset: usize, value: i32) {
        let code = self.buf.code_mut();
        let bytes = value.to_le_bytes();
        code[offset] = bytes[0];
        code[offset + 1] = bytes[1];
        code[offset + 2] = bytes[2];
        code[offset + 3] = bytes[3];
    }

    /// Patch all forward jump references with resolved offsets.
    fn patch_forward_refs(&mut self) {
        for &(patch_offset, target_pc, kind) in &self.forward_refs {
            if let Some(&target_offset) = self.labels.get(&target_pc) {
                let code = self.buf.code_mut();
                // In x86-64, jump offsets are relative to the end of the instruction.
                let (imm_offset, inst_size) = match kind {
                    RefKind::Jmp => (patch_offset + 1, 5), // E9 xx xx xx xx
                    RefKind::Je => (patch_offset + 2, 6),  // 0F 84 xx xx xx xx
                    RefKind::Jcc => (patch_offset + 2, 6), // 0F 8x xx xx xx xx
                };
                let rel = target_offset as i32 - (patch_offset as i32 + inst_size);
                let bytes = rel.to_le_bytes();
                code[imm_offset] = bytes[0];
                code[imm_offset + 1] = bytes[1];
                code[imm_offset + 2] = bytes[2];
                code[imm_offset + 3] = bytes[3];
            }
        }
    }
}

#[cfg(target_arch = "x86_64")]
impl Default for MicroOpJitCompiler {
    fn default() -> Self {
        Self::new()
    }
}

/// Binary operation type for integer ALU.
#[cfg(target_arch = "x86_64")]
enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
}

#[cfg(target_arch = "x86_64")]
enum FpBinOp {
    Add,
    Sub,
    Mul,
    Div,
}

/// Comparison operand (register or immediate).
#[cfg(target_arch = "x86_64")]
enum CmpOperand<'a> {
    Reg(&'a VReg),
    Imm(i64),
}
