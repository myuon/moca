/// JIT compilation infrastructure for mica.
///
/// This module provides the foundation for Just-In-Time compilation:
/// - Executable memory allocation
/// - Code buffer for building machine code
/// - AArch64 instruction encoding (future)

mod memory;
mod codebuf;
pub mod aarch64;

pub use memory::ExecutableMemory;
pub use codebuf::CodeBuffer;
