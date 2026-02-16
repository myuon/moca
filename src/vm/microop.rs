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
    /// old_pc / old_target carry the original Op bytecode indices
    /// so the loop-level JIT can use Op coordinates for compilation.
    Jmp {
        target: usize,
        old_pc: usize,
        old_target: usize,
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
    /// Indirect call via callable reference (closure, function pointer, etc.).
    /// Callee vreg holds the callable reference (heap object with func_index in slot 0).
    CallIndirect {
        callee: VReg,
        args: Vec<VReg>,
        ret: Option<VReg>,
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
    /// dst = a + imm (i64 immediate add)
    AddI64Imm {
        dst: VReg,
        a: VReg,
        imm: i64,
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
    // Heap operations (register-based)
    // ========================================
    /// dst = heap[src][offset] (static offset field access)
    HeapLoad {
        dst: VReg,
        src: VReg,
        offset: usize,
    },
    /// dst = heap[obj][idx] (dynamic index access)
    HeapLoadDyn {
        dst: VReg,
        obj: VReg,
        idx: VReg,
    },
    /// heap[dst_obj][offset] = src (static offset field store)
    HeapStore {
        dst_obj: VReg,
        offset: usize,
        src: VReg,
    },
    /// heap[obj][idx] = src (dynamic index store)
    HeapStoreDyn {
        obj: VReg,
        idx: VReg,
        src: VReg,
    },
    /// dst = heap[heap[obj][0]][idx] (ptr-indirect dynamic access)
    HeapLoad2 {
        dst: VReg,
        obj: VReg,
        idx: VReg,
    },
    /// heap[heap[obj][0]][idx] = src (ptr-indirect dynamic store)
    HeapStore2 {
        obj: VReg,
        idx: VReg,
        src: VReg,
    },

    // ========================================
    // Heap allocation operations
    // ========================================
    /// Allocate a heap object with `size` null-initialized slots.
    /// dst = Ref to newly allocated object.
    HeapAllocDynSimple {
        dst: VReg,
        size: VReg,
    },
    /// Allocate a typed 2-slot object: [data_ref, len] with specified ObjectKind.
    /// kind: 0=Slots, 1=String, 2=Array.
    HeapAllocTyped {
        dst: VReg,
        data_ref: VReg,
        len: VReg,
        kind: u8,
    },

    // ========================================
    // String operations
    // ========================================
    /// Load string constant from cache (or allocate via helper).
    /// dst = string_cache[idx] (Ref to heap string)
    StringConst {
        dst: VReg,
        idx: usize,
    },
    /// Convert value to string representation.
    /// dst = to_string(src) (Ref to newly allocated heap string)
    FloatToString {
        dst: VReg,
        src: VReg,
    },
    /// Print value to output and return original value.
    /// dst = src (after printing src to output)
    PrintDebug {
        dst: VReg,
        src: VReg,
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
    /// old Op PC → new MicroOp PC mapping (length = original code.len() + 1).
    /// Used to translate JIT loop exit PCs back to MicroOp space.
    pub pc_map: Vec<usize>,
}
