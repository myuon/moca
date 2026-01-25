//! JIT compilation infrastructure for mica.
//!
//! This module provides the foundation for Just-In-Time compilation:
//! - Executable memory allocation
//! - Code buffer for building machine code
//! - AArch64 instruction encoding
//! - Template-based bytecode compiler
//! - Stack maps for GC integration

// JIT is not yet integrated into the main VM, allow dead code
#![allow(dead_code)]

mod memory;
mod codebuf;
pub mod aarch64;
pub mod compiler;
pub mod stackmap;

