/// Bytecode operations for the BCVM.
///
/// Core Instructions:
/// - Constants & Locals: PushInt, PushFloat, PushTrue, PushFalse, PushNull, PushString, GetL, SetL
/// - Stack: Pop, Dup
/// - Arithmetic: Add, Sub, Mul, Div
/// - Comparison: Eq, Ne, Lt, Le, Gt, Ge
/// - Control: Jmp, JmpIfTrue, JmpIfFalse
/// - Calls: Call, Ret
/// - Heap: New, GetF, SetF
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    // ========================================
    // Constants & Stack
    // ========================================
    PushInt(i64),      // CONST (i64)
    PushFloat(f64),    // CONST (f64)
    PushTrue,          // CONST (bool true)
    PushFalse,         // CONST (bool false)
    PushNull,          // CONST (null)
    PushString(usize), // CONST (string index)
    Pop,               // POP: Discard top of stack
    Dup,               // DUP: Duplicate top of stack

    // ========================================
    // Local Variables
    // ========================================
    GetL(usize), // GETL: Push local
    SetL(usize), // SETL: Store local with write barrier

    // ========================================
    // Arithmetic
    // ========================================
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    // ========================================
    // Comparison
    // ========================================
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // ========================================
    // Logical
    // ========================================
    Not,

    // ========================================
    // Control Flow
    // ========================================
    Jmp(usize),        // JMP: Unconditional jump
    JmpIfFalse(usize), // JMP_IF_FALSE
    JmpIfTrue(usize),  // JMP_IF_TRUE

    // ========================================
    // Functions
    // ========================================
    Call(usize, usize), // CALL: (func_index, argc) -> -argc + 1
    Ret,                // RET: Return (stack height must be 1)

    // ========================================
    // Heap & Objects
    // ========================================
    New(usize),  // NEW: Allocate object
    GetF(usize), // GETF: Get field by string index
    SetF(usize), // SETF: Set field with write barrier

    // ========================================
    // Array operations
    // ========================================
    AllocArray(usize), // Allocate array with n elements from stack
    ArrayLen,
    ArrayGet,  // stack: [array, index] -> [value]
    ArraySet,  // stack: [array, index, value] -> []
    ArrayPush, // stack: [array, value] -> []
    ArrayPop,  // stack: [array] -> [value]

    // ========================================
    // Heap slot operations (low-level array support)
    // ========================================
    AllocHeap(usize),    // Allocate heap object with n slots from stack: [v1..vn] -> [ref]
    HeapLoad(usize),     // Load slot at static offset: [ref] -> [value]
    HeapStore(usize),    // Store to slot at static offset: [ref, value] -> []
    HeapLoadDyn,         // Load slot at dynamic index: [ref, index] -> [value]
    HeapStoreDyn,        // Store to slot at dynamic index: [ref, index, value] -> []

    // ========================================
    // Type operations
    // ========================================
    TypeOf,   // Push type name as string
    ToString, // Convert any value to string
    ParseInt, // Parse string to int

    // ========================================
    // Exception handling
    // ========================================
    Throw,
    TryBegin(usize), // Jump target for catch handler
    TryEnd,

    // ========================================
    // Builtins
    // ========================================
    Print,

    // ========================================
    // GC hint
    // ========================================
    GcHint(usize), // Hint about upcoming allocation size

    // ========================================
    // Thread operations
    // ========================================
    ThreadSpawn(usize), // Spawn thread with function at given index
    ChannelCreate,      // Create channel, push [sender, receiver] array
    ChannelSend,        // stack: [channel_id, value] -> []
    ChannelRecv,        // stack: [channel_id] -> [value]
    ThreadJoin,         // stack: [handle] -> [result]
}
