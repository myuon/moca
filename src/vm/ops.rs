/// Bytecode operations for the BCVM.
///
/// Core Instructions:
/// - Constants & Locals: PushInt, PushFloat, PushTrue, PushFalse, PushNull, PushString, GetL, SetL
/// - Stack: Pop, Dup
/// - Arithmetic: Add, Sub, Mul, Div
/// - Comparison: Eq, Ne, Lt, Le, Gt, Ge
/// - Control: Jmp, JmpIfTrue, JmpIfFalse
/// - Calls: Call, Ret
/// - Heap: AllocHeap, HeapLoad, HeapStore
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
    Swap,              // SWAP: Swap top two stack elements
    Pick(usize),       // PICK(n): Copy n-th element (0=top) to top
    PickDyn,           // PICKDYN: [depth] -> [value], copy element at dynamic depth

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
    // Array operations (legacy, kept for len() on multiple types)
    // ========================================
    ArrayLen, // Works on Array, Slots, Vector, and String types

    // ========================================
    // Heap slot operations (low-level array support)
    // ========================================
    AllocHeap(usize), // Allocate heap object with n slots from stack: [v1..vn] -> [ref]
    HeapLoad(usize),  // Load slot at static offset: [ref] -> [value]
    HeapStore(usize), // Store to slot at static offset: [ref, value] -> []
    HeapLoadDyn,      // Load slot at dynamic index: [ref, index] -> [value]
    HeapStoreDyn,     // Store to slot at dynamic index: [ref, index, value] -> []

    // ========================================
    // Dynamic heap allocation
    // ========================================
    AllocHeapDyn,       // Allocate heap with dynamic size: [size, v1..vN] -> [ref]
    AllocHeapDynSimple, // Allocate heap with dynamic size, null-initialized: [size] -> [ref]

    // VectorPush and VectorPop removed - now expanded by compiler to low-level ops

    // ========================================
    // Type operations
    // ========================================
    TypeOf,   // Push type name as string
    ToString, // Convert any value to string
    ParseInt, // Parse string to int
    StrLen,   // Get string length: [string] -> [length]

    // ========================================
    // Exception handling
    // ========================================
    Throw,
    TryBegin(usize), // Jump target for catch handler
    TryEnd,

    // ========================================
    // Builtins
    // ========================================
    PrintDebug,

    // ========================================
    // CLI arguments
    // ========================================
    Argc, // Push argc (number of CLI arguments) onto stack
    Argv, // stack: [index] -> [arg_string]
    Args, // Push all CLI arguments as an array onto stack

    // ========================================
    // Syscall
    // ========================================
    /// System call instruction
    /// - syscall_num: syscall number (1 = write)
    /// - argc: number of arguments on stack
    ///
    /// Stack: `[..., arg1, arg2, ..., argN] -> [..., result]`
    Syscall(usize, usize),

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

impl Op {
    /// Returns the name of the opcode for profiling purposes.
    pub fn name(&self) -> &'static str {
        match self {
            Op::PushInt(_) => "PushInt",
            Op::PushFloat(_) => "PushFloat",
            Op::PushTrue => "PushTrue",
            Op::PushFalse => "PushFalse",
            Op::PushNull => "PushNull",
            Op::PushString(_) => "PushString",
            Op::Pop => "Pop",
            Op::Dup => "Dup",
            Op::Swap => "Swap",
            Op::Pick(_) => "Pick",
            Op::PickDyn => "PickDyn",
            Op::GetL(_) => "GetL",
            Op::SetL(_) => "SetL",
            Op::Add => "Add",
            Op::Sub => "Sub",
            Op::Mul => "Mul",
            Op::Div => "Div",
            Op::Mod => "Mod",
            Op::Neg => "Neg",
            Op::Eq => "Eq",
            Op::Ne => "Ne",
            Op::Lt => "Lt",
            Op::Le => "Le",
            Op::Gt => "Gt",
            Op::Ge => "Ge",
            Op::Not => "Not",
            Op::Jmp(_) => "Jmp",
            Op::JmpIfFalse(_) => "JmpIfFalse",
            Op::JmpIfTrue(_) => "JmpIfTrue",
            Op::Call(_, _) => "Call",
            Op::Ret => "Ret",
            Op::ArrayLen => "ArrayLen",
            Op::AllocHeap(_) => "AllocHeap",
            Op::HeapLoad(_) => "HeapLoad",
            Op::HeapStore(_) => "HeapStore",
            Op::HeapLoadDyn => "HeapLoadDyn",
            Op::HeapStoreDyn => "HeapStoreDyn",
            Op::AllocHeapDyn => "AllocHeapDyn",
            Op::AllocHeapDynSimple => "AllocHeapDynSimple",
            Op::TypeOf => "TypeOf",
            Op::ToString => "ToString",
            Op::ParseInt => "ParseInt",
            Op::StrLen => "StrLen",
            Op::Throw => "Throw",
            Op::TryBegin(_) => "TryBegin",
            Op::TryEnd => "TryEnd",
            Op::PrintDebug => "PrintDebug",
            Op::Argc => "Argc",
            Op::Argv => "Argv",
            Op::Args => "Args",
            Op::Syscall(_, _) => "Syscall",
            Op::GcHint(_) => "GcHint",
            Op::ThreadSpawn(_) => "ThreadSpawn",
            Op::ChannelCreate => "ChannelCreate",
            Op::ChannelSend => "ChannelSend",
            Op::ChannelRecv => "ChannelRecv",
            Op::ThreadJoin => "ThreadJoin",
        }
    }
}
