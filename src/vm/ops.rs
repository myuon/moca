/// Bytecode operations for the BCVM v0.
///
/// v0 Core Instructions:
/// - Constants & Locals: PushInt, PushFloat, PushTrue, PushFalse, PushNull, PushString, GetL, SetL
/// - Stack: Pop, Dup
/// - Arithmetic: Add, Sub, Mul, Div (generic), AddI64, SubI64, MulI64, DivI64, AddF64, SubF64, MulF64, DivF64
/// - Comparison: Eq, Lt (generic), LtI64, LtF64
/// - Control: Jmp, JmpIfTrue, JmpIfFalse
/// - Calls: Call, Ret
/// - Heap: New, GetF, SetF
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    // ========================================
    // v0 Core: Constants & Stack
    // ========================================
    PushInt(i64),      // CONST (i64)
    PushFloat(f64),    // CONST (f64)
    PushTrue,          // CONST (bool true)
    PushFalse,         // CONST (bool false)
    PushNull,          // CONST (null) - renamed from PushNil
    PushString(usize), // CONST (string index)
    Pop,               // POP: Discard top of stack
    Dup,               // DUP: Duplicate top of stack (v0 new)

    // ========================================
    // v0 Core: Local Variables
    // ========================================
    GetL(usize), // GETL: Push local (renamed from LoadLocal)
    SetL(usize), // SETL: Store local with write barrier (renamed from StoreLocal)

    // ========================================
    // v0 Core: Arithmetic (generic)
    // ========================================
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    // v0 Core: Typed arithmetic (I64)
    AddI64, // ADD_I64: i64 + i64 -> i64 (renamed from AddInt)
    SubI64, // SUB_I64: i64 - i64 -> i64 (renamed from SubInt)
    MulI64, // MUL_I64: i64 * i64 -> i64 (renamed from MulInt)
    DivI64, // DIV_I64: i64 / i64 -> i64 (renamed from DivInt)

    // v0 Extension: Typed arithmetic (F64)
    AddF64, // ADD_F64: f64 + f64 -> f64 (renamed from AddFloat)
    SubF64, // SUB_F64: f64 - f64 -> f64 (renamed from SubFloat)
    MulF64, // MUL_F64: f64 * f64 -> f64 (renamed from MulFloat)
    DivF64, // DIV_F64: f64 / f64 -> f64 (renamed from DivFloat)

    // ========================================
    // v0 Core: Comparison
    // ========================================
    Eq, // EQ: same-type equality
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // v0 Core: Typed comparison
    LtI64, // LT_I64: i64 < i64 (renamed from LtInt)
    LeI64, // (renamed from LeInt)
    GtI64, // (renamed from GtInt)
    GeI64, // (renamed from GeInt)

    // v0 Extension: F64 comparison
    LtF64, // LT_F64: f64 < f64 (v0 new)

    // ========================================
    // Logical
    // ========================================
    Not,

    // ========================================
    // v0 Core: Control Flow
    // ========================================
    Jmp(usize),        // JMP: Unconditional jump
    JmpIfFalse(usize), // JMP_IF_FALSE
    JmpIfTrue(usize),  // JMP_IF_TRUE

    // ========================================
    // v0 Core: Functions
    // ========================================
    Call(usize, usize), // CALL: (func_index, argc) -> -argc + 1
    Ret,                // RET: Return (stack height must be 1)

    // ========================================
    // v0 Core: Heap & Objects
    // ========================================
    New(usize),  // NEW: Allocate object (renamed from AllocObject)
    GetF(usize), // GETF: Get field by string index (renamed from GetField)
    SetF(usize), // SETF: Set field with write barrier (renamed from SetField)

    // Quickened field access (with cached offset) - extension
    GetFCached(usize, u16), // (field_name_idx, cached_offset)
    SetFCached(usize, u16), // (field_name_idx, cached_offset)

    // ========================================
    // Extension: Array operations (not in v0 core)
    // ========================================
    AllocArray(usize), // Allocate array with n elements from stack
    ArrayLen,
    ArrayGet,    // stack: [array, index] -> [value]
    ArraySet,    // stack: [array, index, value] -> []
    ArrayPush,   // stack: [array, value] -> []
    ArrayPop,    // stack: [array] -> [value]
    ArrayGetInt, // Quickened: Array access with int index

    // ========================================
    // Extension: Type operations
    // ========================================
    TypeOf,   // Push type name as string
    ToString, // Convert any value to string
    ParseInt, // Parse string to int

    // ========================================
    // Extension: Exception handling (not in v0 core)
    // ========================================
    Throw,
    TryBegin(usize), // Jump target for catch handler
    TryEnd,

    // ========================================
    // Extension: Builtins
    // ========================================
    Print,

    // ========================================
    // Extension: GC hint
    // ========================================
    GcHint(usize), // Hint about upcoming allocation size

    // ========================================
    // Extension: Thread operations (not in v0 core)
    // ========================================
    ThreadSpawn(usize), // Spawn thread with function at given index
    ChannelCreate,      // Create channel, push [sender, receiver] array
    ChannelSend,        // stack: [channel_id, value] -> []
    ChannelRecv,        // stack: [channel_id] -> [value]
    ThreadJoin,         // stack: [handle] -> [result]
}
