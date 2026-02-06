use super::ops::Op;

/// Virtual register index into a frame's register file.
///
/// Layout per frame:
///   [0 .. locals_count)                          → declared locals (including args)
///   [locals_count .. locals_count + temps_count)  → temporary registers
///
/// At runtime: VReg(n) = stack[frame.stack_base + n]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VReg(pub usize);

/// Comparison condition for CmpI64 / CmpI64Imm / CmpF64.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpCond {
    Eq,
    Ne,
    LtS,
    LeS,
    GtS,
    GeS,
}

/// Register-based micro-operations.
///
/// MicroOp PC is the single source of truth for control flow.
/// All branch targets are resolved to MicroOp indices during conversion.
#[derive(Debug, Clone, PartialEq)]
pub enum MicroOp {
    // ========================================
    // Control Flow (always native — never Raw)
    // ========================================
    /// Unconditional jump. Target is MicroOp PC.
    Jmp {
        target: usize,
    },
    /// Branch if cond vreg is truthy. Target is MicroOp PC.
    BrIf {
        cond: VReg,
        target: usize,
    },
    /// Branch if cond vreg is falsy. Target is MicroOp PC.
    BrIfFalse {
        cond: VReg,
        target: usize,
    },
    /// Function call. Args are copied from caller vregs to callee locals.
    Call {
        func_id: usize,
        args: Vec<VReg>,
        ret: Option<VReg>,
    },
    /// Return a value (or unit if src is None).
    Ret {
        src: Option<VReg>,
    },

    // ========================================
    // Move / Constants
    // ========================================
    /// Copy vreg to vreg.
    Mov {
        dst: VReg,
        src: VReg,
    },
    /// Load immediate i64.
    ConstI64 {
        dst: VReg,
        imm: i64,
    },
    /// Load immediate i32 (stored as i64 in the register).
    ConstI32 {
        dst: VReg,
        imm: i32,
    },
    /// Load immediate f64.
    ConstF64 {
        dst: VReg,
        imm: f64,
    },
    /// Load immediate f32 (stored as f64 in the register).
    ConstF32 {
        dst: VReg,
        imm: f32,
    },

    // ========================================
    // i64 ALU
    // ========================================
    AddI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    SubI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    MulI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    DivI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    RemI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    NegI64 {
        dst: VReg,
        src: VReg,
    },

    // ========================================
    // i32 ALU
    // ========================================
    AddI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    SubI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    MulI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    DivI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    RemI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    /// i32 equal-to-zero: dst = (src == 0) ? 1 : 0
    EqzI32 {
        dst: VReg,
        src: VReg,
    },

    // ========================================
    // f64 ALU
    // ========================================
    AddF64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    SubF64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    MulF64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    DivF64 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    NegF64 {
        dst: VReg,
        src: VReg,
    },

    // ========================================
    // f32 ALU
    // ========================================
    AddF32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    SubF32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    MulF32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    DivF32 {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    NegF32 {
        dst: VReg,
        src: VReg,
    },

    // ========================================
    // Comparisons (separated from branch)
    // ========================================
    /// dst = (a <cond> b) as i64 (1 or 0). Operands are i64.
    CmpI64 {
        dst: VReg,
        a: VReg,
        b: VReg,
        cond: CmpCond,
    },
    /// dst = (a <cond> imm) as i64 (1 or 0). Operands are i64.
    CmpI64Imm {
        dst: VReg,
        a: VReg,
        imm: i64,
        cond: CmpCond,
    },
    /// dst = (a <cond> b) as i64 (1 or 0). Operands are i32.
    CmpI32 {
        dst: VReg,
        a: VReg,
        b: VReg,
        cond: CmpCond,
    },
    /// dst = (a <cond> b) as i64 (1 or 0). Operands are f64.
    CmpF64 {
        dst: VReg,
        a: VReg,
        b: VReg,
        cond: CmpCond,
    },
    /// dst = (a <cond> b) as i64 (1 or 0). Operands are f32.
    CmpF32 {
        dst: VReg,
        a: VReg,
        b: VReg,
        cond: CmpCond,
    },

    // ========================================
    // Type Conversions
    // ========================================
    /// dst = src as i32 (wrap i64 to i32)
    I32WrapI64 {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i64 (sign-extend i32)
    I64ExtendI32S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i64 (zero-extend i32)
    I64ExtendI32U {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f64 (convert signed i64)
    F64ConvertI64S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i64 (truncate f64)
    I64TruncF64S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f64 (convert signed i32)
    F64ConvertI32S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f32 (convert signed i32)
    F32ConvertI32S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f32 (convert signed i64)
    F32ConvertI64S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i32 (truncate f32)
    I32TruncF32S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i32 (truncate f64)
    I32TruncF64S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as i64 (truncate f32)
    I64TruncF32S {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f32 (demote f64)
    F32DemoteF64 {
        dst: VReg,
        src: VReg,
    },
    /// dst = src as f64 (promote f32)
    F64PromoteF32 {
        dst: VReg,
        src: VReg,
    },

    // ========================================
    // Ref operations
    // ========================================
    /// dst = (a == b) as i64, reference equality
    RefEq {
        dst: VReg,
        a: VReg,
        b: VReg,
    },
    /// dst = (src is null) as i64
    RefIsNull {
        dst: VReg,
        src: VReg,
    },
    /// dst = null ref
    RefNull {
        dst: VReg,
    },

    // ========================================
    // Stack Bridge (for Raw op interop)
    // ========================================
    /// Push vreg value onto the operand stack (for Raw ops to consume).
    StackPush {
        src: VReg,
    },
    /// Pop from operand stack into vreg (capture Raw op results).
    StackPop {
        dst: VReg,
    },

    // ========================================
    // Fallback
    // ========================================
    /// Execute an original Op using the stack-based semantics.
    /// Control-flow ops must NOT appear here.
    Raw {
        op: Op,
    },
}

/// Result of converting a function's Op[] bytecode to MicroOp[].
#[derive(Debug, Clone)]
pub struct ConvertedFunction {
    /// The micro-op instruction sequence.
    pub micro_ops: Vec<MicroOp>,
    /// Number of temporary registers beyond locals_count.
    pub temps_count: usize,
}
