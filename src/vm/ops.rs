/// Bytecode operations for the mica VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    // Stack operations - constants
    PushInt(i64),
    PushFloat(f64),
    PushTrue,
    PushFalse,
    PushNil,
    PushString(usize), // Index into string constants pool
    Pop,

    // Local variables
    LoadLocal(usize),
    StoreLocal(usize),

    // Arithmetic (works on int and float)
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Neg,

    // Quickened arithmetic (specialized for known types)
    AddInt,   // int + int -> int
    AddFloat, // float + float -> float
    SubInt,   // int - int -> int
    SubFloat, // float - float -> float
    MulInt,   // int * int -> int
    MulFloat, // float * float -> float
    DivInt,   // int / int -> int
    DivFloat, // float / float -> float

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

    // Quickened comparison
    LtInt, // int < int
    LeInt, // int <= int
    GtInt, // int > int
    GeInt, // int >= int

    // Logical
    Not,

    // Control flow
    Jmp(usize),
    JmpIfFalse(usize),
    JmpIfTrue(usize),

    // Functions
    Call(usize, usize), // (func_index, argc)
    Ret,

    // Array operations
    AllocArray(usize), // Allocate array with n elements from stack
    ArrayLen,
    ArrayGet,  // stack: [array, index] -> [value]
    ArraySet,  // stack: [array, index, value] -> []
    ArrayPush, // stack: [array, value] -> []
    ArrayPop,  // stack: [array] -> [value]

    // Quickened array access (with int index)
    ArrayGetInt, // Array access with int index (no type check)

    // Object operations
    AllocObject(usize), // Allocate object with n field pairs from stack
    GetField(usize),    // Get field by string constant index
    SetField(usize),    // Set field by string constant index

    // Quickened field access (with cached offset)
    GetFieldCached(usize, u16), // (field_name_idx, cached_offset)
    SetFieldCached(usize, u16), // (field_name_idx, cached_offset)

    // String operations
    StringLen,
    StringConcat,

    // Type operations
    TypeOf,   // Push type name as string
    ToString, // Convert any value to string
    ParseInt, // Parse string to int

    // Exception handling
    Throw,
    TryBegin(usize), // Jump target for catch handler
    TryEnd,

    // Builtins
    Print,

    // GC hint
    GcHint(usize), // Hint about upcoming allocation size

    // Thread operations
    ThreadSpawn(usize), // Spawn thread with function at given index, push handle
    ChannelCreate,      // Create channel, push [sender, receiver] array
    ChannelSend,        // stack: [channel_id, value] -> []
    ChannelRecv,        // stack: [channel_id] -> [value]
    ThreadJoin,         // stack: [handle] -> [result]
}
