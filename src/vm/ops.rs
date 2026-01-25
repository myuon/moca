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

    // Comparison
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,

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

    // Object operations
    AllocObject(usize), // Allocate object with n field pairs from stack
    GetField(usize),    // Get field by string constant index
    SetField(usize),    // Set field by string constant index

    // String operations
    StringLen,
    StringConcat,

    // Type operations
    TypeOf,    // Push type name as string
    ToString,  // Convert any value to string
    ParseInt,  // Parse string to int

    // Exception handling
    Throw,
    TryBegin(usize), // Jump target for catch handler
    TryEnd,

    // Builtins
    Print,

    // GC hint
    GcHint(usize), // Hint about upcoming allocation size
}
