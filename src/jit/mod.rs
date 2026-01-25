//! JIT compilation infrastructure for mica.
//!
//! This module provides the foundation for Just-In-Time compilation:
//! - Executable memory allocation
//! - Code buffer for building machine code
//! - AArch64 instruction encoding
//! - Template-based bytecode compiler
//! - Stack maps for GC integration
//!
//! This module is only compiled when the `jit` feature is enabled.
//! Use `cargo build --features jit` to include JIT support.

// JIT is not yet fully integrated into the main VM, allow dead code
#![allow(dead_code)]

pub mod aarch64;
mod codebuf;
pub mod compiler;
mod memory;
pub mod stackmap;
