//! JIT shared types for x86-64.
//!
//! Contains compiled code containers, value tag constants, and size definitions
//! used by the MicroOp-based JIT compiler.

use super::memory::ExecutableMemory;
use std::collections::HashMap;

/// Value tag constants for JIT code.
/// Values are represented as 128-bit (tag: u64, payload: u64).
pub mod value_tags {
    pub const TAG_INT: u64 = 0;
    pub const TAG_FLOAT: u64 = 1;
    pub const TAG_BOOL: u64 = 2;
    pub const TAG_NIL: u64 = 3;
    pub const TAG_PTR: u64 = 4;
}

/// Size of a Value on the stack (128 bits = 16 bytes).
pub const VALUE_SIZE: i32 = 16;

/// Compiled JIT code for a function.
pub struct CompiledCode {
    /// The executable memory containing the compiled code
    pub memory: ExecutableMemory,
    /// Entry point offset within the memory
    pub entry_offset: usize,
    /// Stack map for GC (pc_offset -> bitmap of stack slots with refs)
    pub stack_map: HashMap<usize, Vec<bool>>,
    /// Total number of VRegs (locals + temps) for frame allocation.
    pub total_regs: usize,
}

impl CompiledCode {
    /// Get the entry point as a function pointer.
    ///
    /// # Safety
    /// The caller must ensure the function signature matches the expected ABI.
    pub unsafe fn entry_point<F>(&self) -> F
    where
        F: Copy,
    {
        unsafe {
            let ptr = self.memory.as_ptr().add(self.entry_offset);
            std::mem::transmute_copy(&ptr)
        }
    }
}

/// Compiled JIT code for a hot loop.
pub struct CompiledLoop {
    /// The executable memory containing the compiled code
    pub memory: ExecutableMemory,
    /// Entry point offset within the memory
    pub entry_offset: usize,
    /// Bytecode PC where the loop starts (backward jump target)
    pub loop_start_pc: usize,
    /// Bytecode PC where the loop ends (backward jump instruction)
    pub loop_end_pc: usize,
    /// Stack map for GC (pc_offset -> bitmap of stack slots with refs)
    pub stack_map: HashMap<usize, Vec<bool>>,
    /// Total number of VRegs (locals + temps) for MicroOp JIT.
    pub total_regs: usize,
}

impl CompiledLoop {
    /// Get the entry point as a function pointer.
    ///
    /// # Safety
    /// The caller must ensure the function signature matches the expected ABI.
    pub unsafe fn entry_point<F>(&self) -> F
    where
        F: Copy,
    {
        unsafe {
            let ptr = self.memory.as_ptr().add(self.entry_offset);
            std::mem::transmute_copy(&ptr)
        }
    }
}
