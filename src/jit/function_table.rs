//! JIT function table for indirect calls.
//!
//! Stores entry point addresses and frame sizes (total_regs) for JIT-compiled functions.
//! JIT code reads this table at runtime to dispatch direct calls to already-compiled functions,
//! with a fallback to call_helper for functions not yet compiled.
//!
//! Layout: [entry_0, total_regs_0, entry_1, total_regs_1, ...]
//! Each entry is 16 bytes (2 x u64). entry == 0 means the function is not yet compiled.

/// A table of JIT function entry points and frame sizes.
///
/// This table is writable and can be updated as new functions are compiled.
/// JIT-generated code reads from this table to resolve call targets at runtime.
pub struct JitFunctionTable {
    /// Flat array: [entry_0, total_regs_0, entry_1, total_regs_1, ...]
    data: Vec<u64>,
}

impl JitFunctionTable {
    /// Create a new function table with space for `max_funcs` functions.
    /// All entries are initialized to 0 (not compiled).
    pub fn new(max_funcs: usize) -> Self {
        JitFunctionTable {
            data: vec![0; max_funcs * 2],
        }
    }

    /// Update the table entry for a function after JIT compilation.
    ///
    /// # Arguments
    /// * `func_id` - Function index
    /// * `entry_addr` - Entry point address of the compiled function
    /// * `total_regs` - Number of virtual registers (locals + temps) for frame allocation
    pub fn update(&mut self, func_id: usize, entry_addr: u64, total_regs: usize) {
        let idx = func_id * 2;
        self.data[idx] = entry_addr;
        self.data[idx + 1] = total_regs as u64;
    }

    /// Get the base pointer to the table data.
    /// JIT code uses this pointer to index into the table.
    pub fn base_ptr(&self) -> *const u64 {
        self.data.as_ptr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_table_is_zeroed() {
        let table = JitFunctionTable::new(4);
        for &val in &table.data {
            assert_eq!(val, 0);
        }
    }

    #[test]
    fn test_update_and_read() {
        let mut table = JitFunctionTable::new(4);
        table.update(0, 0x1000, 10);
        table.update(2, 0x2000, 20);

        let ptr = table.base_ptr();
        unsafe {
            // func 0: entry=0x1000, total_regs=10
            assert_eq!(*ptr.add(0), 0x1000);
            assert_eq!(*ptr.add(1), 10);
            // func 1: not compiled
            assert_eq!(*ptr.add(2), 0);
            assert_eq!(*ptr.add(3), 0);
            // func 2: entry=0x2000, total_regs=20
            assert_eq!(*ptr.add(4), 0x2000);
            assert_eq!(*ptr.add(5), 20);
        }
    }
}
