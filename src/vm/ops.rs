/// Bytecode operations for the mica VM.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    // Stack operations
    PushInt(i64),
    PushTrue,
    PushFalse,
    Pop,

    // Local variables
    LoadLocal(usize),
    StoreLocal(usize),

    // Arithmetic
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

    // Builtins
    Print,
}
