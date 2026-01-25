// Some fields are stored for future use
#![allow(dead_code)]

mod value;
mod ops;
mod heap;
#[allow(clippy::module_inception)]
mod vm;
pub mod debug;
pub mod ic;
pub mod concurrent_gc;
pub mod threads;

pub use value::Value;
pub use ops::Op;
pub use heap::{Heap, HeapObject, ObjectType};
pub use vm::VM;
pub use debug::{DebugInfo, FunctionDebugInfo};

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
    /// Debug information (optional)
    pub debug: Option<DebugInfo>,
}
