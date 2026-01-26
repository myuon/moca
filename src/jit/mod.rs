//! JIT compilation infrastructure for moca.
//!
//! This module provides the foundation for Just-In-Time compilation:
//! - Executable memory allocation
//! - Code buffer for building machine code
//! - AArch64 instruction encoding
//! - x86-64 instruction encoding
//! - Template-based bytecode compiler
//! - Stack maps for GC integration
//!
//! This module is only compiled when the `jit` feature is enabled.
//! Use `cargo build --features jit` to include JIT support.

// JIT is not yet fully integrated into the main VM, allow dead code
#![allow(dead_code)]

#[cfg(target_arch = "aarch64")]
pub mod aarch64;
#[cfg(target_arch = "x86_64")]
pub mod x86_64;
mod codebuf;
#[cfg(target_arch = "aarch64")]
pub mod compiler;
#[cfg(target_arch = "x86_64")]
pub mod compiler_x86_64;
pub mod marshal;
mod memory;
pub mod stackmap;
