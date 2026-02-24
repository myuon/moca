//! MicroOp-based JIT compiler for x86-64.
//!
//! This compiler takes MicroOp IR (register-based) as input and generates
//! native x86-64 code using a frame-slot model where each VReg maps to
//! a fixed offset from the frame base pointer (FRAME_BASE register).
//!
//! Frame layout (unboxed):
//!   VReg(n) → [FRAME_BASE + n * 8]  (payload only, 8 bytes per slot)

#[cfg(target_arch = "x86_64")]
use super::codebuf::CodeBuffer;
#[cfg(target_arch = "x86_64")]
use super::compiler_x86_64::{CompiledCode, CompiledLoop, value_tags};
#[cfg(target_arch = "x86_64")]
use super::memory::ExecutableMemory;
#[cfg(target_arch = "x86_64")]
use super::x86_64::{Cond, Reg, X86_64Assembler};
#[cfg(target_arch = "x86_64")]
use crate::vm::ValueType;
#[cfg(target_arch = "x86_64")]
use crate::vm::microop::{CmpCond, ConvertedFunction, MicroOp, VReg};
#[cfg(target_arch = "x86_64")]
use crate::vm::{Function, microop_converter};
#[cfg(target_arch = "x86_64")]
use std::collections::{HashMap, HashSet};

/// Register conventions for MicroOp JIT on x86-64.
#[cfg(target_arch = "x86_64")]
mod regs {
    use super::Reg;

    /// JitCallContext pointer (callee-saved).
    pub const VM_CTX: Reg = Reg::R12;
    /// Frame base pointer: VReg(n) is at [FRAME_BASE + n*8] (callee-saved).
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
    /// Static type for each VReg (used to reconstruct tags at boundaries).
    vreg_types: Vec<ValueType>,
    /// VRegs that need unconditional shadow tag updates because they are written
    /// with multiple different tag types across different MicroOps.
    shadow_conflict_vregs: HashSet<usize>,
    /// Inline candidates: func_id → (converted IR, arity).
    inline_candidates: HashMap<usize, (ConvertedFunction, usize)>,
    /// Starting VReg index for inline temp pool.
    inline_vreg_base: usize,
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
            vreg_types: Vec::new(),
            shadow_conflict_vregs: HashSet::new(),
            inline_candidates: HashMap::new(),
            inline_vreg_base: 0,
        }
    }

    /// Pre-scan MicroOps to find VRegs that are written with different shadow tag types.
    /// These VRegs need unconditional shadow updates at every write, because
    /// `emit_shadow_init` + `needs_shadow_update` can't handle the case where
    /// a previous operation in a different basic block changed the shadow tag.
    fn compute_shadow_conflicts(ops: &[MicroOp]) -> HashSet<usize> {
        // Map: VReg index → set of tags written to it
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
                | MicroOp::AndI64 { dst, .. }
                | MicroOp::OrI64 { dst, .. }
                | MicroOp::XorI64 { dst, .. }
                | MicroOp::ShlI64 { dst, .. }
                | MicroOp::ShlI64Imm { dst, .. }
                | MicroOp::ShrI64 { dst, .. }
                | MicroOp::ShrI64Imm { dst, .. }
                | MicroOp::ShrU64 { dst, .. }
                | MicroOp::ShrU64Imm { dst, .. }
                | MicroOp::UMul128Hi { dst, .. }
                | MicroOp::CmpI64 { dst, .. }
                | MicroOp::CmpI64Imm { dst, .. }
                | MicroOp::AddI32 { dst, .. }
                | MicroOp::SubI32 { dst, .. }
                | MicroOp::MulI32 { dst, .. }
                | MicroOp::DivI32 { dst, .. }
                | MicroOp::RemI32 { dst, .. }
                | MicroOp::EqzI32 { dst, .. }
                | MicroOp::CmpI32 { dst, .. }
                | MicroOp::RefEq { dst, .. }
                | MicroOp::RefIsNull { dst, .. }
                | MicroOp::F64ReinterpretAsI64 { dst, .. }
                | MicroOp::I32WrapI64 { dst, .. }
                | MicroOp::I64ExtendI32S { dst, .. }
                | MicroOp::I64ExtendI32U { dst, .. }
                | MicroOp::I64TruncF64S { dst, .. }
                | MicroOp::I32TruncF32S { dst, .. }
                | MicroOp::I32TruncF64S { dst, .. }
                | MicroOp::I64TruncF32S { dst, .. }
                | MicroOp::CmpF64 { dst, .. }
                | MicroOp::CmpF32 { dst, .. } => {
                    record(&mut vreg_tags, dst.0, value_tags::TAG_INT);
                }
                MicroOp::AddF64 { dst, .. }
                | MicroOp::SubF64 { dst, .. }
                | MicroOp::MulF64 { dst, .. }
                | MicroOp::DivF64 { dst, .. }
                | MicroOp::NegF64 { dst, .. }
                | MicroOp::AddF32 { dst, .. }
                | MicroOp::SubF32 { dst, .. }
                | MicroOp::MulF32 { dst, .. }
                | MicroOp::DivF32 { dst, .. }
                | MicroOp::NegF32 { dst, .. }
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
                // These read tags from heap/call/stack — they're dynamic, always correct
                // Don't count them as a specific tag (they always write the correct shadow)
                MicroOp::HeapLoad { dst, .. }
                | MicroOp::HeapLoadDyn { dst, .. }
                | MicroOp::HeapLoad2 { dst, .. }
                | MicroOp::StackPop { dst }
                | MicroOp::HeapAlloc { dst, .. }
                | MicroOp::HeapAllocDynSimple { dst, .. }
                | MicroOp::StringConst { dst, .. } => {
                    // These always write the correct shadow tag directly
                    // Mark with a sentinel tag (u64::MAX) to indicate "dynamic"
                    record(&mut vreg_tags, dst.0, u64::MAX);
                }
                MicroOp::Call { ret: Some(ret), .. }
                | MicroOp::CallIndirect { ret: Some(ret), .. } => {
                    record(&mut vreg_tags, ret.0, u64::MAX);
                }
                // Mov copies shadow from src → doesn't set a specific tag
                // Other non-value-producing ops
                _ => {}
            }
        }

        // VRegs with more than one distinct tag type need unconditional updates
        vreg_tags
            .into_iter()
            .filter(|(_, tags)| tags.len() > 1)
            .map(|(vreg, _)| vreg)
            .collect()
    }

    /// Convert a ValueType to the corresponding JIT tag constant.
    fn value_type_to_tag(ty: &ValueType) -> u64 {
        match ty {
            ValueType::I32 | ValueType::I64 => value_tags::TAG_INT,
            ValueType::F32 | ValueType::F64 => value_tags::TAG_FLOAT,
            ValueType::Ref => value_tags::TAG_PTR,
        }
    }

    /// Check if a shadow tag update is needed for `dst` with `expected_tag`.
    /// Returns `Some(shadow_offset)` if:
    /// - The VReg's `vreg_types` entry doesn't match the expected tag, OR
    /// - The VReg is in `shadow_conflict_vregs` (written with multiple tag types across MicroOps)
    ///
    /// Returns `None` if the shadow is guaranteed to already have the correct tag.
    fn needs_shadow_update(&self, dst: &VReg, expected_tag: u64) -> Option<i32> {
        // If this VReg has conflicting shadow writes, always update
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

    /// Emit a shadow tag update for `dst` if its `vreg_types` entry doesn't match `expected_tag`.
    /// This handles the case where temp VRegs are reused across different types —
    /// `emit_shadow_init` sets the tag from `vreg_types` (which records the LAST allocation type),
    /// but at runtime the VReg may hold a value of a different type.
    /// Only emits code when there's a type mismatch, so the common case has zero overhead.
    ///
    /// Can be called while an assembler is borrowed by using the asm parameter,
    /// or after dropping any existing assembler by passing a freshly created one.
    fn emit_shadow_update(asm: &mut X86_64Assembler, shadow_off: i32, tag: u64) {
        asm.mov_ri64(regs::TMP0, tag as i64);
        asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
    }

    /// Compile a MicroOp function to native x86-64 code.
    pub fn compile(
        mut self,
        converted: &ConvertedFunction,
        locals_count: usize,
        func_index: usize,
        all_functions: &[Function],
    ) -> Result<CompiledCode, String> {
        self.total_regs = locals_count + converted.temps_count;
        self.self_func_index = func_index;
        self.self_locals_count = locals_count;
        self.vreg_types = converted.vreg_types.clone();
        self.shadow_conflict_vregs = Self::compute_shadow_conflicts(&converted.micro_ops);

        // Pre-scan for inlinable call targets
        self.scan_inline_candidates(&converted.micro_ops, all_functions);

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
        all_functions: &[Function],
    ) -> Result<CompiledLoop, String> {
        self.total_regs = locals_count + converted.temps_count;
        self.self_func_index = func_index;
        self.self_locals_count = locals_count;
        self.vreg_types = converted.vreg_types.clone();
        self.shadow_conflict_vregs = Self::compute_shadow_conflicts(&converted.micro_ops);

        // Pre-scan for inlinable call targets
        self.scan_inline_candidates(&converted.micro_ops, all_functions);

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

    /// Byte offset of a VReg's slot from FRAME_BASE (8 bytes per slot, payload only).
    fn vreg_offset(vreg: &VReg) -> i32 {
        (vreg.0 * 8) as i32
    }

    /// Byte offset of a VReg's shadow tag from FRAME_BASE.
    /// Shadow tags are stored after all payload slots: [total_regs * 8 + vreg.0 * 8].
    /// Used by HeapLoad to save the runtime tag, and HeapStore to restore it.
    fn shadow_tag_offset(&self, vreg: &VReg) -> i32 {
        ((self.total_regs + vreg.0) * 8) as i32
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

    /// Initialize the shadow tag area from vreg_types.
    /// This sets up default tags so that HeapStore can always read from shadow,
    /// even if the VReg was not produced by a HeapLoad.
    fn emit_shadow_init(&mut self) {
        for i in 0..self.vreg_types.len() {
            let tag = Self::value_type_to_tag(&self.vreg_types[i]);
            let shadow_off = ((self.total_regs + i) * 8) as i32;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_ri64(regs::TMP0, tag as i64);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
        }
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

    // ==================== Call inlining ====================

    /// Check if a callee function is small enough to inline.
    /// Only allows ops that `remap_op` can fully remap.
    fn is_inlinable(converted: &ConvertedFunction) -> bool {
        if converted.micro_ops.len() > 20 {
            return false;
        }
        for op in &converted.micro_ops {
            match op {
                // Supported ops (all VRegs are remapped by remap_op)
                MicroOp::Mov { .. }
                | MicroOp::ConstI64 { .. }
                | MicroOp::ConstI32 { .. }
                | MicroOp::ConstF64 { .. }
                | MicroOp::ConstF32 { .. }
                | MicroOp::AddI64 { .. }
                | MicroOp::SubI64 { .. }
                | MicroOp::MulI64 { .. }
                | MicroOp::DivI64 { .. }
                | MicroOp::RemI64 { .. }
                | MicroOp::NegI64 { .. }
                | MicroOp::AddI64Imm { .. }
                | MicroOp::CmpI64 { .. }
                | MicroOp::CmpI64Imm { .. }
                | MicroOp::HeapLoad2 { .. }
                | MicroOp::HeapStore2 { .. }
                | MicroOp::HeapLoad { .. }
                | MicroOp::HeapStore { .. }
                | MicroOp::HeapLoadDyn { .. }
                | MicroOp::HeapStoreDyn { .. }
                | MicroOp::RefNull { .. }
                | MicroOp::Ret { .. } => {}
                // Any unsupported op → not inlinable
                _ => return false,
            }
        }
        true
    }

    /// Pre-scan MicroOps for Call targets and identify inlinable functions.
    /// Extends total_regs and vreg_types for the inline VReg pool.
    fn scan_inline_candidates(&mut self, ops: &[MicroOp], all_functions: &[Function]) {
        let mut max_non_arg_vregs = 0usize;

        for op in ops {
            if let MicroOp::Call { func_id, .. } = op {
                let func_id = *func_id;
                if func_id == self.self_func_index {
                    continue; // Skip self-recursion
                }
                if self.inline_candidates.contains_key(&func_id) {
                    continue; // Already checked
                }
                if func_id >= all_functions.len() {
                    continue;
                }
                let callee = &all_functions[func_id];
                let callee_converted = microop_converter::convert(callee);
                if Self::is_inlinable(&callee_converted) {
                    let non_arg_vregs =
                        callee.locals_count - callee.arity + callee_converted.temps_count;
                    max_non_arg_vregs = max_non_arg_vregs.max(non_arg_vregs);
                    self.inline_candidates
                        .insert(func_id, (callee_converted, callee.arity));
                }
            }
        }

        if max_non_arg_vregs > 0 {
            self.inline_vreg_base = self.total_regs;
            self.total_regs += max_non_arg_vregs;

            // Extend vreg_types for inline pool (default to Int)
            self.vreg_types.resize(self.total_regs, ValueType::I64);
        }
    }

    /// Remap a VReg from callee space to caller space.
    fn remap_vreg(vreg: &VReg, vreg_map: &[VReg]) -> VReg {
        vreg_map[vreg.0]
    }

    /// Build the VReg mapping table for an inline expansion.
    fn build_inline_vreg_map(
        callee_arity: usize,
        callee_total_regs: usize,
        caller_args: &[VReg],
        inline_vreg_base: usize,
    ) -> Vec<VReg> {
        let mut map = Vec::with_capacity(callee_total_regs);
        // Args map to caller's arg VRegs
        for arg in caller_args.iter().take(callee_arity) {
            map.push(*arg);
        }
        // Non-arg locals and temps map to inline pool
        for i in callee_arity..callee_total_regs {
            map.push(VReg(inline_vreg_base + (i - callee_arity)));
        }
        map
    }

    /// Remap all VRegs in a MicroOp for inline expansion.
    fn remap_op(op: &MicroOp, vreg_map: &[VReg]) -> MicroOp {
        match op {
            MicroOp::Mov { dst, src } => MicroOp::Mov {
                dst: Self::remap_vreg(dst, vreg_map),
                src: Self::remap_vreg(src, vreg_map),
            },
            MicroOp::ConstI64 { dst, imm } => MicroOp::ConstI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                imm: *imm,
            },
            MicroOp::ConstI32 { dst, imm } => MicroOp::ConstI32 {
                dst: Self::remap_vreg(dst, vreg_map),
                imm: *imm,
            },
            MicroOp::ConstF64 { dst, imm } => MicroOp::ConstF64 {
                dst: Self::remap_vreg(dst, vreg_map),
                imm: *imm,
            },
            MicroOp::ConstF32 { dst, imm } => MicroOp::ConstF32 {
                dst: Self::remap_vreg(dst, vreg_map),
                imm: *imm,
            },
            MicroOp::AddI64 { dst, a, b } => MicroOp::AddI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
            },
            MicroOp::SubI64 { dst, a, b } => MicroOp::SubI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
            },
            MicroOp::MulI64 { dst, a, b } => MicroOp::MulI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
            },
            MicroOp::DivI64 { dst, a, b } => MicroOp::DivI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
            },
            MicroOp::RemI64 { dst, a, b } => MicroOp::RemI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
            },
            MicroOp::NegI64 { dst, src } => MicroOp::NegI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                src: Self::remap_vreg(src, vreg_map),
            },
            MicroOp::AddI64Imm { dst, a, imm } => MicroOp::AddI64Imm {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                imm: *imm,
            },
            MicroOp::CmpI64 { dst, a, b, cond } => MicroOp::CmpI64 {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                b: Self::remap_vreg(b, vreg_map),
                cond: *cond,
            },
            MicroOp::CmpI64Imm { dst, a, imm, cond } => MicroOp::CmpI64Imm {
                dst: Self::remap_vreg(dst, vreg_map),
                a: Self::remap_vreg(a, vreg_map),
                imm: *imm,
                cond: *cond,
            },
            MicroOp::HeapLoad2 { dst, obj, idx } => MicroOp::HeapLoad2 {
                dst: Self::remap_vreg(dst, vreg_map),
                obj: Self::remap_vreg(obj, vreg_map),
                idx: Self::remap_vreg(idx, vreg_map),
            },
            MicroOp::HeapStore2 { obj, idx, src } => MicroOp::HeapStore2 {
                obj: Self::remap_vreg(obj, vreg_map),
                idx: Self::remap_vreg(idx, vreg_map),
                src: Self::remap_vreg(src, vreg_map),
            },
            MicroOp::HeapLoad { dst, src, offset } => MicroOp::HeapLoad {
                dst: Self::remap_vreg(dst, vreg_map),
                src: Self::remap_vreg(src, vreg_map),
                offset: *offset,
            },
            MicroOp::HeapStore {
                dst_obj,
                offset,
                src,
            } => MicroOp::HeapStore {
                dst_obj: Self::remap_vreg(dst_obj, vreg_map),
                offset: *offset,
                src: Self::remap_vreg(src, vreg_map),
            },
            MicroOp::HeapLoadDyn { dst, obj, idx } => MicroOp::HeapLoadDyn {
                dst: Self::remap_vreg(dst, vreg_map),
                obj: Self::remap_vreg(obj, vreg_map),
                idx: Self::remap_vreg(idx, vreg_map),
            },
            MicroOp::HeapStoreDyn { obj, idx, src } => MicroOp::HeapStoreDyn {
                obj: Self::remap_vreg(obj, vreg_map),
                idx: Self::remap_vreg(idx, vreg_map),
                src: Self::remap_vreg(src, vreg_map),
            },
            MicroOp::RefNull { dst } => MicroOp::RefNull {
                dst: Self::remap_vreg(dst, vreg_map),
            },
            // Ret is handled by emit_inline_call, not remapped here.
            // All other ops should have been filtered by is_inlinable.
            other => unreachable!("remap_op: unsupported op {:?}", other),
        }
    }

    /// Emit an inlined function call.
    fn emit_inline_call(
        &mut self,
        func_id: usize,
        args: &[VReg],
        ret: Option<&VReg>,
    ) -> Result<(), String> {
        // Clone the candidate data to avoid borrow issues
        let (callee, arity) = self.inline_candidates.get(&func_id).unwrap();
        let callee_ops = callee.micro_ops.clone();
        let callee_total_regs = callee.vreg_types.len();
        let arity = *arity;

        let vreg_map =
            Self::build_inline_vreg_map(arity, callee_total_regs, args, self.inline_vreg_base);

        // Emit each callee op with remapped VRegs, skipping Ret
        for op in &callee_ops {
            match op {
                MicroOp::Ret { src } => {
                    // If caller expects a return value, emit a Mov
                    if let (Some(ret_vreg), Some(src_vreg)) = (ret, src) {
                        let remapped_src = Self::remap_vreg(src_vreg, &vreg_map);
                        self.emit_mov(ret_vreg, &remapped_src)?;
                    }
                    break;
                }
                _ => {
                    let remapped = Self::remap_op(op, &vreg_map);
                    self.compile_microop(&remapped, 0)?;
                }
            }
        }

        Ok(())
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
            MicroOp::AndI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::And),
            MicroOp::OrI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Or),
            MicroOp::XorI64 { dst, a, b } => self.emit_binop_i64(dst, a, b, BinOp::Xor),
            MicroOp::ShlI64 { dst, a, b } => self.emit_shl_i64(dst, a, b),
            MicroOp::ShlI64Imm { dst, a, imm } => self.emit_shl_i64_imm(dst, a, *imm),
            MicroOp::ShrI64 { dst, a, b } => self.emit_shr_i64(dst, a, b),
            MicroOp::ShrI64Imm { dst, a, imm } => self.emit_shr_i64_imm(dst, a, *imm),
            MicroOp::ShrU64 { dst, a, b } => self.emit_shr_u64(dst, a, b),
            MicroOp::ShrU64Imm { dst, a, imm } => self.emit_shr_u64_imm(dst, a, *imm),
            MicroOp::UMul128Hi { dst, a, b } => self.emit_umul128_hi(dst, a, b),

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
            MicroOp::F64ReinterpretAsI64 { dst, src } => self.emit_mov(dst, src),

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
            // Heap allocation operations
            MicroOp::HeapAlloc { dst, args } => self.emit_heap_alloc(dst, args),
            MicroOp::HeapAllocDynSimple { dst, size } => self.emit_heap_alloc_dyn_simple(dst, size),
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
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_ri64(regs::TMP0, imm);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Mov ====================

    fn emit_mov(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        if dst == src {
            return Ok(());
        }
        let src_shadow = self.shadow_tag_offset(src);
        let dst_shadow = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Copy payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        // Copy shadow tag
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, src_shadow);
        asm.mov_mr(regs::FRAME_BASE, dst_shadow, regs::TMP0);
        Ok(())
    }

    // ==================== i64 ALU ====================

    fn emit_binop_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg, op: BinOp) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        match op {
            BinOp::Add => asm.add_rr(regs::TMP0, regs::TMP1),
            BinOp::Sub => asm.sub_rr(regs::TMP0, regs::TMP1),
            BinOp::Mul => asm.imul_rr(regs::TMP0, regs::TMP1),
            BinOp::Div => {
                asm.cqo();
                asm.idiv(regs::TMP1);
            }
            BinOp::And => asm.and_rr(regs::TMP0, regs::TMP1),
            BinOp::Or => asm.or_rr(regs::TMP0, regs::TMP1),
            BinOp::Xor => asm.xor_rr(regs::TMP0, regs::TMP1),
        }
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_rem_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.cqo();
        asm.idiv(regs::TMP1);
        // Remainder is in RDX (TMP2)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP2);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shl_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        // TMP1 = RCX, so loading b into TMP1 puts shift count in CL
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.shl_cl(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shl_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.shl_ri(regs::TMP0, (imm as u8) & 63);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shr_i64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        // TMP1 = RCX, so loading b into TMP1 puts shift count in CL
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.sar_cl(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shr_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.sar_ri(regs::TMP0, (imm as u8) & 63);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shr_u64(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        // TMP1 = RCX, so loading b into TMP1 puts shift count in CL
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.shr_cl(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_shr_u64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.shr_ri(regs::TMP0, (imm as u8) & 63);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_umul128_hi(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // MUL r/m64: RDX:RAX = RAX * r/m64
        // TMP0 = RAX, TMP2 = RDX
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.mul_r(regs::TMP1);
        // High 64 bits are in RDX (TMP2)
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP2);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_neg_i64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.neg(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_add_i64_imm(&mut self, dst: &VReg, a: &VReg, imm: i64) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
            asm.add_ri32(regs::TMP0, imm as i32);
        } else {
            asm.mov_ri64(regs::TMP1, imm);
            asm.add_rr(regs::TMP0, regs::TMP1);
        }
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
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
        let x86_cond = Self::cmp_cond_to_x86(cond);
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.cmp_rr(regs::TMP0, regs::TMP1);
        asm.setcc(x86_cond, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
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
        let x86_cond = Self::cmp_cond_to_x86(cond);
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
            asm.cmp_ri32(regs::TMP0, imm as i32);
        } else {
            asm.mov_ri64(regs::TMP1, imm);
            asm.cmp_rr(regs::TMP0, regs::TMP1);
        }
        asm.setcc(x86_cond, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Branches ====================

    fn emit_br_if_false(&mut self, cond: &VReg, target: usize) -> Result<(), String> {
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(cond));
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
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(cond));
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
        // Load operand a
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        }

        // Compare with operand b
        match b {
            CmpOperand::Reg(b_vreg) => {
                let mut asm = X86_64Assembler::new(&mut self.buf);
                asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b_vreg));
                asm.cmp_rr(regs::TMP0, regs::TMP1);
            }
            CmpOperand::Imm(imm) => {
                if imm >= i32::MIN as i64 && imm <= i32::MAX as i64 {
                    let mut asm = X86_64Assembler::new(&mut self.buf);
                    asm.cmp_ri32(regs::TMP0, imm as i32);
                } else {
                    let mut asm = X86_64Assembler::new(&mut self.buf);
                    asm.mov_ri64(regs::TMP1, imm);
                    asm.cmp_rr(regs::TMP0, regs::TMP1);
                }
            }
        }

        // Determine branch condition
        let mut x86_cond = Self::cmp_cond_to_x86(cond);
        if invert {
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

    /// JitCallContext offset for jit_function_table pointer.
    const JIT_FUNC_TABLE_OFFSET: i32 = 80;

    fn emit_call(
        &mut self,
        func_id: usize,
        args: &[VReg],
        ret: Option<&VReg>,
    ) -> Result<(), String> {
        if self.inline_candidates.contains_key(&func_id) {
            return self.emit_inline_call(func_id, args, ret);
        }

        if func_id == self.self_func_index {
            return self.emit_call_self(args, ret);
        }

        self.emit_call_via_table(func_id, args, ret)
    }

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
        let table_entry_offset = (func_id * 16) as i32;

        // Load entry_addr from function table
        // TMP4 = table base, TMP5 = entry_addr
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP4, regs::VM_CTX, Self::JIT_FUNC_TABLE_OFFSET);
            asm.mov_rm(regs::TMP5, regs::TMP4, table_entry_offset);
            asm.test_rr(regs::TMP5, regs::TMP5);
        }

        // jz slow_path (will patch offset later)
        let jz_site = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.je_rel32(0); // placeholder
        }

        // === Fast path: direct call via table ===
        // Load total_regs from table, compute frame size (16 bytes per VReg: payload + shadow tag)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // TMP4 still has table base
            asm.mov_rm(regs::TMP4, regs::TMP4, table_entry_offset + 8); // total_regs
            asm.shl_ri(regs::TMP4, 4); // * 16 (payload + shadow tag)
            asm.add_ri32(regs::TMP4, 15);
            asm.and_ri32(regs::TMP4, -16); // 16-byte align → TMP4 = frame_aligned
        }

        // Save callee-saved registers
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
        }

        // Allocate frame + save frame_aligned (push twice to maintain 16-byte alignment)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_rr(Reg::Rsp, regs::TMP4); // allocate frame
            asm.push(regs::TMP4); // save frame_aligned for dealloc
            asm.push(regs::TMP4); // padding to keep 16-byte alignment
        }

        // Copy args (payload only, 8B/slot) from caller frame to new frame on stack (at RSP+16)
        for (i, arg) in args.iter().enumerate().take(argc) {
            let new_offset = (i * 8) as i32 + 16;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
            asm.mov_mr(Reg::Rsp, new_offset, regs::TMP0);
        }

        // Set up arguments: RDI=ctx, RSI=new_frame(rsp+16), RDX=unused
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_rr(Reg::Rsi, Reg::Rsp);
            asm.add_ri32(Reg::Rsi, 16); // skip 2 saved values
            asm.mov_rr(Reg::Rdx, Reg::Rsi);
        }

        // Call via TMP5 (entry_addr loaded earlier)
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.call_r(regs::TMP5);
        }

        // Deallocate frame
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::TMP4); // discard padding
            asm.pop(regs::TMP4); // restore frame_aligned
            asm.add_rr(Reg::Rsp, regs::TMP4); // deallocate frame
        }

        // Restore callee-saved
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
        }

        // Store return value: payload (RDX) to frame, tag (RAX) to shadow
        if let Some(ret_vreg) = ret {
            let shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(ret_vreg), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, Reg::Rax);
        }

        // jmp done (skip slow path)
        let jmp_done_site = self.buf.len();
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.jmp_rel32(0); // placeholder
        }

        // === Slow path: call_helper (needs 16B/arg with tag reconstruction) ===
        let slow_path_offset = self.buf.len();

        // Allocate space on native stack for args array (16B per arg for JitValue)
        let args_size = argc * 16;
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Copy args with tag from shadow area (set by HeapLoad, or initialized from vreg_types)
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * 16) as i32;
            let sp_payload_offset = sp_tag_offset + 8;
            let shadow_off = self.shadow_tag_offset(arg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, shadow_off);
            asm.mov_mr(Reg::Rsp, sp_tag_offset, regs::TMP0);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
            asm.mov_mr(Reg::Rsp, sp_payload_offset, regs::TMP0);
        }

        // Save callee-saved registers
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
        }

        // Set up call arguments: RDI=ctx, RSI=func_index, RDX=argc, RCX=args_ptr
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_ri64(Reg::Rsi, func_id as i64);
            asm.mov_ri64(Reg::Rdx, argc as i64);
            asm.mov_rr(Reg::Rcx, Reg::Rsp);
            asm.add_ri32(Reg::Rcx, 16); // skip 2 pushed registers
        }

        // Load call_helper from JitCallContext offset 16 and call
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

        // Store return value: payload (RDX) to frame, tag (RAX) to shadow
        if let Some(ret_vreg) = ret {
            let shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(ret_vreg), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, Reg::Rax);
        }

        let done_offset = self.buf.len();

        // Patch jz → slow_path
        {
            // je_rel32 is 6 bytes: 0F 84 xx xx xx xx; rel32 is at offset+2
            let rel = (slow_path_offset as i32) - (jz_site as i32 + 6);
            let bytes = rel.to_le_bytes();
            self.buf.code_mut()[jz_site + 2..jz_site + 6].copy_from_slice(&bytes);
        }

        // Patch jmp → done
        {
            // jmp_rel32 is 5 bytes: E9 xx xx xx xx; rel32 is at offset+1
            let rel = (done_offset as i32) - (jmp_done_site as i32 + 5);
            let bytes = rel.to_le_bytes();
            self.buf.code_mut()[jmp_done_site + 1..jmp_done_site + 5].copy_from_slice(&bytes);
        }

        Ok(())
    }

    fn emit_call_self(&mut self, args: &[VReg], ret: Option<&VReg>) -> Result<(), String> {
        let argc = args.len();
        // Allocate new frame on native stack for callee (payload + shadow tags, 16B per VReg)
        let frame_size = self.total_regs * 16;
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

        // Copy args (payload only, 8B/slot) from current frame to new frame
        for (i, arg) in args.iter().enumerate().take(argc) {
            let new_offset = (i * 8) as i32;
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
            asm.mov_mr(Reg::Rsp, new_offset, regs::TMP0);
        }

        // Set up arguments: RDI=ctx, RSI=new_frame(rsp), RDX=unused
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_rr(Reg::Rsi, Reg::Rsp);
            asm.mov_rr(Reg::Rdx, Reg::Rsp);
        }

        // CALL to function entry (offset 0)
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

        // Store return value: payload (RDX) to frame, tag (RAX) to shadow
        if let Some(ret_vreg) = ret {
            let shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(ret_vreg), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, Reg::Rax);
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
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(callee));
            asm.mov_rm(regs::TMP1, regs::VM_CTX, 48); // heap_base
            asm.add_ri32(regs::TMP0, 1); // skip header
            asm.shl_ri(regs::TMP0, 3); // byte offset
            asm.add_rr(regs::TMP1, regs::TMP0);
            // TMP1 now points to slot 0 tag; slot 0 payload is at +8
            asm.mov_rm(regs::TMP4, regs::TMP1, 8); // func_index in TMP4 (R8)
        }

        // Step 2: Allocate space on native stack for args array (16B per arg for JitValue)
        let args_size = argc * 16;
        let args_aligned = (args_size + 15) & !15;

        if args_aligned > 0 {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.sub_ri32(Reg::Rsp, args_aligned as i32);
        }

        // Step 3: Copy args with tag from shadow area
        for (i, arg) in args.iter().enumerate() {
            let sp_tag_offset = (i * 16) as i32;
            let sp_payload_offset = sp_tag_offset + 8;
            let shadow_off = self.shadow_tag_offset(arg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, shadow_off);
            asm.mov_mr(Reg::Rsp, sp_tag_offset, regs::TMP0);
            asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(arg));
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

        // Store return value: payload (RDX) to frame, tag (RAX) to shadow
        if let Some(ret_vreg) = ret {
            let shadow_off = self.shadow_tag_offset(ret_vreg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(ret_vreg), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, Reg::Rax);
        }

        Ok(())
    }

    // ==================== Return ====================

    fn emit_ret(&mut self, src: Option<&VReg>) -> Result<(), String> {
        if let Some(vreg) = src {
            // Read tag from shadow area, payload from frame
            let shadow_off = self.shadow_tag_offset(vreg);
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // RAX = tag (from shadow), RDX = payload (from frame)
            asm.mov_rm(Reg::Rax, regs::FRAME_BASE, shadow_off);
            asm.mov_rm(Reg::Rdx, regs::FRAME_BASE, Self::vreg_offset(vreg));
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
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_ri64(regs::TMP0, imm.to_bits() as i64);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_FLOAT);
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
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.movq_xmm_r64(0, regs::TMP0);
        asm.movq_xmm_r64(1, regs::TMP1);
        match op {
            FpBinOp::Add => asm.addsd(0, 1),
            FpBinOp::Sub => asm.subsd(0, 1),
            FpBinOp::Mul => asm.mulsd(0, 1),
            FpBinOp::Div => asm.divsd(0, 1),
        }
        asm.movq_r64_xmm(regs::TMP0, 0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    fn emit_neg_f64(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.mov_ri64(regs::TMP1, i64::MIN); // 0x8000000000000000 sign bit mask
        asm.xor_rr(regs::TMP0, regs::TMP1);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_FLOAT);
        }
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
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.movq_xmm_r64(0, regs::TMP0);
        asm.movq_xmm_r64(1, regs::TMP1);
        asm.ucomisd(0, 1);
        asm.setcc(x86_cond, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== i32 extras ====================

    fn emit_eqz(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.test_rr(regs::TMP0, regs::TMP0);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Type Conversions ====================

    /// Sign-extend i32 to i64: MOVSXD r64, r32
    fn emit_i64_extend_i32s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.movsxd(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// Zero-extend i32 to i64: MOV r32, r32 (clears upper 32 bits)
    fn emit_i64_extend_i32u(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.mov_r32_r32(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    /// Convert signed i64 to f64: CVTSI2SD xmm, r64
    fn emit_f64_convert_i64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_FLOAT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.cvtsi2sd_xmm_r64(0, regs::TMP0);
        asm.movq_r64_xmm(regs::TMP0, 0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_FLOAT);
        }
        Ok(())
    }

    /// Truncate f64 to signed i64: CVTTSD2SI r64, xmm
    fn emit_i64_trunc_f64s(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.movq_xmm_r64(0, regs::TMP0);
        asm.cvttsd2si_r64_xmm(regs::TMP0, 0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    // ==================== Ref Operations ====================

    fn emit_ref_eq(&mut self, dst: &VReg, a: &VReg, b: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(a));
        asm.mov_rm(regs::TMP1, regs::FRAME_BASE, Self::vreg_offset(b));
        asm.cmp_rr(regs::TMP0, regs::TMP1);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_ref_is_null(&mut self, dst: &VReg, src: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_INT);
        // In unboxed frames, null ref has payload == 0 (heap offset 0 is reserved)
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.test_rr(regs::TMP0, regs::TMP0);
        asm.setcc(Cond::E, regs::TMP0);
        asm.movzx_r64_r8(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_INT);
        }
        Ok(())
    }

    fn emit_ref_null(&mut self, dst: &VReg) -> Result<(), String> {
        let shadow = self.needs_shadow_update(dst, value_tags::TAG_NIL);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        asm.xor_rr(regs::TMP0, regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        if let Some(off) = shadow {
            Self::emit_shadow_update(&mut asm, off, value_tags::TAG_NIL);
        }
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
        let shadow_off = self.shadow_tag_offset(dst);
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            // TMP1 = cached GcRef.index (offset 8 from entry)
            asm.mov_rm(regs::TMP1, regs::TMP0, 8);
            // Store payload to frame, TAG_PTR to shadow
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP1);
            asm.mov_ri64(regs::TMP1, value_tags::TAG_PTR as i64);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP1);
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
            // Result: RAX=tag, RDX=payload — store both
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, shadow_off, Reg::Rax);
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

    // ==================== Heap Allocation ====================

    /// Emit HeapAllocDynSimple: call helper(ctx, size_payload) -> (tag, payload)
    fn emit_heap_alloc_dyn_simple(&mut self, dst: &VReg, size: &VReg) -> Result<(), String> {
        let dst_shadow_off = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Save callee-saved
        asm.push(regs::VM_CTX);
        asm.push(regs::FRAME_BASE);
        // Args: RDI=ctx, RSI=size (payload only)
        asm.mov_rr(Reg::Rdi, regs::VM_CTX);
        asm.mov_rm(Reg::Rsi, regs::FRAME_BASE, Self::vreg_offset(size));
        // Load heap_alloc_dyn_simple_helper from JitCallContext offset 72
        asm.mov_rm(regs::TMP4, regs::VM_CTX, 72);
        asm.call_r(regs::TMP4);
        // Restore callee-saved
        asm.pop(regs::FRAME_BASE);
        asm.pop(regs::VM_CTX);
        // Store result: payload (RDX) to frame, tag (RAX) to shadow
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), Reg::Rdx);
        asm.mov_mr(regs::FRAME_BASE, dst_shadow_off, Reg::Rax);
        Ok(())
    }

    /// Emit HeapAlloc: allocate object with args.len() slots and initialize from args.
    fn emit_heap_alloc(&mut self, dst: &VReg, args: &[VReg]) -> Result<(), String> {
        let size = args.len();
        let dst_shadow_off = self.shadow_tag_offset(dst);
        // 1. Call alloc helper to allocate size null-initialized slots
        {
            let mut asm = X86_64Assembler::new(&mut self.buf);
            asm.push(regs::VM_CTX);
            asm.push(regs::FRAME_BASE);
            asm.mov_rr(Reg::Rdi, regs::VM_CTX);
            asm.mov_ri64(Reg::Rsi, size as i64);
            // Load heap_alloc_dyn_simple_helper from JitCallContext offset 72
            asm.mov_rm(regs::TMP4, regs::VM_CTX, 72);
            asm.call_r(regs::TMP4);
            asm.pop(regs::FRAME_BASE);
            asm.pop(regs::VM_CTX);
            // Store result: payload (RDX) to frame, tag (RAX) to shadow
            asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), Reg::Rdx);
            asm.mov_mr(regs::FRAME_BASE, dst_shadow_off, Reg::Rax);
        }
        // 2. Store each arg into the allocated object's slots
        for (i, arg) in args.iter().enumerate() {
            self.emit_heap_store(dst, i, arg)?;
        }
        Ok(())
    }

    // ==================== Stack Bridge ====================

    /// Emit StackPush: push tag+payload onto the machine stack for cross-call spill.
    /// Tag is read from shadow area.
    fn emit_stack_push(&mut self, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Push payload first (stack grows down, so payload will be at higher address after pop)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
        asm.push(regs::TMP0);
        // Push tag second (from shadow area)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, shadow_off);
        asm.push(regs::TMP0);
        Ok(())
    }

    /// Emit StackPop: pop tag+payload from the machine stack into a VReg.
    /// Tag is saved to shadow area; payload is stored to frame.
    fn emit_stack_pop(&mut self, dst: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // Pop tag first (save to shadow)
        asm.pop(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
        // Pop payload second (store to frame)
        asm.pop(regs::TMP0);
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP0);
        Ok(())
    }

    // ==================== Heap Operations ====================

    /// Emit HeapLoad: dst = heap[src][offset] (static offset field access).
    /// Stores payload to frame and tag to shadow area (for HeapStore to recover).
    fn emit_heap_load(&mut self, dst: &VReg, src: &VReg, offset: usize) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP0 = ref payload (heap word offset)
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(src));
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
        asm.mov_rm(regs::TMP0, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
        Ok(())
    }

    /// Emit HeapLoadDyn: dst = heap[obj][idx] (dynamic index access).
    /// Stores payload to frame and tag to shadow area.
    fn emit_heap_load_dyn(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        // TMP0 = ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
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
        // Load tag and payload from heap
        asm.mov_rm(regs::TMP0, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
        Ok(())
    }

    /// Emit HeapStore: heap[dst_obj][offset] = src (static offset field store).
    /// Reads tag from shadow area (set by HeapLoad); stores tag+payload to heap.
    fn emit_heap_store(&mut self, dst_obj: &VReg, offset: usize, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP2 = tag (from shadow), TMP3 = payload
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, shadow_off);
        asm.mov_rm(regs::TMP3, regs::FRAME_BASE, Self::vreg_offset(src));
        // TMP0 = ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(dst_obj));
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
    /// Reads tag from shadow area (set by HeapLoad); stores tag+payload to heap.
    fn emit_heap_store_dyn(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP4 = tag (from shadow), TMP5 = payload
        asm.mov_rm(regs::TMP4, regs::FRAME_BASE, shadow_off);
        asm.mov_rm(regs::TMP5, regs::FRAME_BASE, Self::vreg_offset(src));
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        // TMP0 = ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);
        // Calculate address
        asm.shl_ri(regs::TMP2, 1);
        asm.add_ri32(regs::TMP0, 1);
        asm.add_rr(regs::TMP0, regs::TMP2);
        asm.shl_ri(regs::TMP0, 3);
        asm.add_rr(regs::TMP1, regs::TMP0);
        // Store tag and payload to heap
        asm.mov_mr(regs::TMP1, 0, regs::TMP4);
        asm.mov_mr(regs::TMP1, 8, regs::TMP5);
        Ok(())
    }

    /// Emit HeapLoad2: dst = heap[heap[obj][0]][idx] (ptr-indirect dynamic access).
    /// Stores payload to frame and tag to shadow area.
    fn emit_heap_load2(&mut self, dst: &VReg, obj: &VReg, idx: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(dst);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        // TMP0 = outer ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
        // TMP1 = heap_base
        asm.mov_rm(regs::TMP1, regs::VM_CTX, 48);

        // Step 1: load slot 0 of outer object → inner ref payload
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

        // Load tag and payload from heap
        asm.mov_rm(regs::TMP0, regs::TMP1, 0); // tag
        asm.mov_rm(regs::TMP2, regs::TMP1, 8); // payload
        // Store payload to frame, tag to shadow
        asm.mov_mr(regs::FRAME_BASE, Self::vreg_offset(dst), regs::TMP2);
        asm.mov_mr(regs::FRAME_BASE, shadow_off, regs::TMP0);
        Ok(())
    }

    /// Emit HeapStore2: heap[heap[obj][0]][idx] = src (ptr-indirect dynamic store).
    /// Reads tag from shadow area (set by HeapLoad); stores tag+payload to heap.
    fn emit_heap_store2(&mut self, obj: &VReg, idx: &VReg, src: &VReg) -> Result<(), String> {
        let shadow_off = self.shadow_tag_offset(src);
        let mut asm = X86_64Assembler::new(&mut self.buf);
        // TMP4 = tag (from shadow), TMP5 = payload
        asm.mov_rm(regs::TMP4, regs::FRAME_BASE, shadow_off);
        asm.mov_rm(regs::TMP5, regs::FRAME_BASE, Self::vreg_offset(src));
        // TMP2 = dynamic index
        asm.mov_rm(regs::TMP2, regs::FRAME_BASE, Self::vreg_offset(idx));
        // TMP0 = outer ref payload
        asm.mov_rm(regs::TMP0, regs::FRAME_BASE, Self::vreg_offset(obj));
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
        // Store tag and payload to heap
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
    And,
    Or,
    Xor,
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
