mod value;
mod ops;
mod heap;
mod vm;

pub use value::Value;
pub use ops::Op;
pub use heap::{GcRef, Heap, HeapObject, MicaArray, MicaObject, MicaString, ObjectType};
pub use vm::VM;

/// A compiled function.
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub arity: usize,
    pub locals_count: usize,
    pub code: Vec<Op>,
}

/// A compiled chunk of bytecode.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub functions: Vec<Function>,
    pub main: Function,
    /// String constants pool
    pub strings: Vec<String>,
}
