//! Moca - A minimal programming language
//!
//! This library provides the moca virtual machine and compiler.
//! For C/C++ embedding, use the FFI module functions.

pub mod compiler;
pub mod config;
pub mod debugger;
pub mod ffi;
#[cfg(feature = "jit")]
pub mod jit;
pub mod lsp;
pub mod package;
pub mod vm;

// Re-export commonly used types
pub use config::{GcMode, JitMode, RuntimeConfig};
pub use vm::{Chunk, VM, Value};

// Re-export FFI types for C bindings
pub use ffi::*;
