/// Bytecode operations for the moca VM (v2 — typed opcode architecture).
///
/// All arithmetic/comparison instructions are typed (e.g., I64Add, F64Lt).
/// The operand stack and locals use u64 slots; type information is in the opcodes.
///
/// Naming conventions follow WASM:
/// - `I32`/`I64`/`F32`/`F64` prefix for typed operations
/// - `S` suffix for signed variants
/// - `Ref` prefix for reference operations
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    // ========================================
    // Constants
    // ========================================
    I32Const(i32),
    I64Const(i64),
    F32Const(f32),
    F64Const(f64),
    RefNull,
    StringConst(usize), // string pool index → ref

    // ========================================
    // Local Variables
    // ========================================
    LocalGet(usize),
    LocalSet(usize),

    // ========================================
    // Stack Manipulation
    // ========================================
    Drop,
    Dup,
    Pick(usize),
    PickDyn,

    // ========================================
    // i32 Arithmetic
    // ========================================
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32RemS,
    I32Eqz, // [i32] → [i32] (x == 0 ? 1 : 0)

    // ========================================
    // i64 Arithmetic
    // ========================================
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64RemS,
    I64Neg, // [i64] → [i64] (0 - x)

    // ========================================
    // f32 Arithmetic
    // ========================================
    F32Add,
    F32Sub,
    F32Mul,
    F32Div,
    F32Neg,

    // ========================================
    // f64 Arithmetic
    // ========================================
    F64Add,
    F64Sub,
    F64Mul,
    F64Div,
    F64Neg,

    // ========================================
    // i32 Comparison → i32
    // ========================================
    I32Eq,
    I32Ne,
    I32LtS,
    I32LeS,
    I32GtS,
    I32GeS,

    // ========================================
    // i64 Comparison → i32
    // ========================================
    I64Eq,
    I64Ne,
    I64LtS,
    I64LeS,
    I64GtS,
    I64GeS,

    // ========================================
    // f32 Comparison → i32
    // ========================================
    F32Eq,
    F32Ne,
    F32Lt,
    F32Le,
    F32Gt,
    F32Ge,

    // ========================================
    // f64 Comparison → i32
    // ========================================
    F64Eq,
    F64Ne,
    F64Lt,
    F64Le,
    F64Gt,
    F64Ge,

    // ========================================
    // Ref Comparison → i32
    // ========================================
    RefEq,
    RefIsNull,

    // ========================================
    // Type Conversion
    // ========================================
    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,
    F64ConvertI64S,
    I64TruncF64S,
    F64ConvertI32S,
    F32ConvertI32S,
    F32ConvertI64S,
    I32TruncF32S,
    I32TruncF64S,
    I64TruncF32S,
    F32DemoteF64,
    F64PromoteF32,

    // ========================================
    // Control Flow
    // ========================================
    Jmp(usize),
    BrIf(usize),        // [i32] → [] (branch if != 0)
    BrIfFalse(usize),   // [i32] → [] (branch if == 0)
    Call(usize, usize), // (func_index, argc)
    Ret,

    // ========================================
    // Heap Operations
    // ========================================
    HeapAlloc(usize),
    /// Like HeapAlloc but marks the object with ObjectKind::Array for display.
    HeapAllocArray(usize),
    HeapAllocDyn,
    HeapAllocDynSimple,
    HeapLoad(usize),
    HeapStore(usize),
    HeapLoadDyn,
    HeapStoreDyn,

    // ========================================
    // System / Builtins
    // ========================================
    Syscall(usize, usize),
    GcHint(usize),
    PrintDebug,
    TypeOf,
    ToString,
    ParseInt,

    // ========================================
    // Exception Handling
    // ========================================
    Throw,
    TryBegin(usize),
    TryEnd,

    // ========================================
    // CLI Arguments
    // ========================================
    Argc,
    Argv,
    Args,

    // ========================================
    // Threading
    // ========================================
    ThreadSpawn(usize),
    ChannelCreate,
    ChannelSend,
    ChannelRecv,
    ThreadJoin,

    // ========================================
    // Closures
    // ========================================
    /// Create a closure object on the heap.
    /// Pops `n_captures` values from stack, creates heap object [func_index, cap0, cap1, ...].
    /// Pushes the closure reference onto the stack.
    MakeClosure(usize, usize), // (func_index, n_captures)
    /// Call a closure. Pops `argc` arguments, then the closure reference.
    /// Reads func_index from closure slot 0, pushes captured values as extra args,
    /// then calls the function with (n_captures + argc) arguments.
    CallClosure(usize), // (argc) — number of user-visible arguments
}

impl Op {
    /// Returns the name of the opcode for profiling purposes.
    pub fn name(&self) -> &'static str {
        match self {
            Op::I32Const(_) => "I32Const",
            Op::I64Const(_) => "I64Const",
            Op::F32Const(_) => "F32Const",
            Op::F64Const(_) => "F64Const",
            Op::RefNull => "RefNull",
            Op::StringConst(_) => "StringConst",
            Op::LocalGet(_) => "LocalGet",
            Op::LocalSet(_) => "LocalSet",
            Op::Drop => "Drop",
            Op::Dup => "Dup",
            Op::Pick(_) => "Pick",
            Op::PickDyn => "PickDyn",
            Op::I32Add => "I32Add",
            Op::I32Sub => "I32Sub",
            Op::I32Mul => "I32Mul",
            Op::I32DivS => "I32DivS",
            Op::I32RemS => "I32RemS",
            Op::I32Eqz => "I32Eqz",
            Op::I64Add => "I64Add",
            Op::I64Sub => "I64Sub",
            Op::I64Mul => "I64Mul",
            Op::I64DivS => "I64DivS",
            Op::I64RemS => "I64RemS",
            Op::I64Neg => "I64Neg",
            Op::F32Add => "F32Add",
            Op::F32Sub => "F32Sub",
            Op::F32Mul => "F32Mul",
            Op::F32Div => "F32Div",
            Op::F32Neg => "F32Neg",
            Op::F64Add => "F64Add",
            Op::F64Sub => "F64Sub",
            Op::F64Mul => "F64Mul",
            Op::F64Div => "F64Div",
            Op::F64Neg => "F64Neg",
            Op::I32Eq => "I32Eq",
            Op::I32Ne => "I32Ne",
            Op::I32LtS => "I32LtS",
            Op::I32LeS => "I32LeS",
            Op::I32GtS => "I32GtS",
            Op::I32GeS => "I32GeS",
            Op::I64Eq => "I64Eq",
            Op::I64Ne => "I64Ne",
            Op::I64LtS => "I64LtS",
            Op::I64LeS => "I64LeS",
            Op::I64GtS => "I64GtS",
            Op::I64GeS => "I64GeS",
            Op::F32Eq => "F32Eq",
            Op::F32Ne => "F32Ne",
            Op::F32Lt => "F32Lt",
            Op::F32Le => "F32Le",
            Op::F32Gt => "F32Gt",
            Op::F32Ge => "F32Ge",
            Op::F64Eq => "F64Eq",
            Op::F64Ne => "F64Ne",
            Op::F64Lt => "F64Lt",
            Op::F64Le => "F64Le",
            Op::F64Gt => "F64Gt",
            Op::F64Ge => "F64Ge",
            Op::RefEq => "RefEq",
            Op::RefIsNull => "RefIsNull",
            Op::I32WrapI64 => "I32WrapI64",
            Op::I64ExtendI32S => "I64ExtendI32S",
            Op::I64ExtendI32U => "I64ExtendI32U",
            Op::F64ConvertI64S => "F64ConvertI64S",
            Op::I64TruncF64S => "I64TruncF64S",
            Op::F64ConvertI32S => "F64ConvertI32S",
            Op::F32ConvertI32S => "F32ConvertI32S",
            Op::F32ConvertI64S => "F32ConvertI64S",
            Op::I32TruncF32S => "I32TruncF32S",
            Op::I32TruncF64S => "I32TruncF64S",
            Op::I64TruncF32S => "I64TruncF32S",
            Op::F32DemoteF64 => "F32DemoteF64",
            Op::F64PromoteF32 => "F64PromoteF32",
            Op::Jmp(_) => "Jmp",
            Op::BrIf(_) => "BrIf",
            Op::BrIfFalse(_) => "BrIfFalse",
            Op::Call(_, _) => "Call",
            Op::Ret => "Ret",
            Op::HeapAlloc(_) => "HeapAlloc",
            Op::HeapAllocArray(_) => "HeapAllocArray",
            Op::HeapAllocDyn => "HeapAllocDyn",
            Op::HeapAllocDynSimple => "HeapAllocDynSimple",
            Op::HeapLoad(_) => "HeapLoad",
            Op::HeapStore(_) => "HeapStore",
            Op::HeapLoadDyn => "HeapLoadDyn",
            Op::HeapStoreDyn => "HeapStoreDyn",
            Op::Syscall(_, _) => "Syscall",
            Op::GcHint(_) => "GcHint",
            Op::PrintDebug => "PrintDebug",
            Op::TypeOf => "TypeOf",
            Op::ToString => "ToString",
            Op::ParseInt => "ParseInt",
            Op::Throw => "Throw",
            Op::TryBegin(_) => "TryBegin",
            Op::TryEnd => "TryEnd",
            Op::Argc => "Argc",
            Op::Argv => "Argv",
            Op::Args => "Args",
            Op::ThreadSpawn(_) => "ThreadSpawn",
            Op::ChannelCreate => "ChannelCreate",
            Op::ChannelSend => "ChannelSend",
            Op::ChannelRecv => "ChannelRecv",
            Op::ThreadJoin => "ThreadJoin",
            Op::MakeClosure(_, _) => "MakeClosure",
            Op::CallClosure(_) => "CallClosure",
        }
    }
}
