//! Stack maps for precise GC in JIT-compiled code.
//!
//! Stack maps track which stack slots contain references at each safepoint
//! in the generated code, allowing the GC to accurately trace roots.

use std::collections::HashMap;

/// A single stack map entry for a safepoint.
#[derive(Debug, Clone)]
pub struct StackMapEntry {
    /// Native code offset (PC) where this safepoint occurs
    pub native_pc: u32,
    /// Corresponding bytecode PC
    pub bytecode_pc: u32,
    /// Bitmap of stack slots that contain references (bit N = 1 means slot N is a ref)
    pub stack_refs: u64,
    /// Bitmap of local slots that contain references
    pub locals_refs: u64,
    /// Number of valid stack slots
    pub stack_depth: u16,
    /// Number of local variables
    pub locals_count: u16,
}

impl StackMapEntry {
    /// Create a new stack map entry.
    pub fn new(native_pc: u32, bytecode_pc: u32, stack_depth: u16, locals_count: u16) -> Self {
        Self {
            native_pc,
            bytecode_pc,
            stack_refs: 0,
            locals_refs: 0,
            stack_depth,
            locals_count,
        }
    }

    /// Mark a stack slot as containing a reference.
    pub fn mark_stack_ref(&mut self, slot: usize) {
        if slot < 64 {
            self.stack_refs |= 1 << slot;
        }
    }

    /// Mark a local slot as containing a reference.
    pub fn mark_local_ref(&mut self, slot: usize) {
        if slot < 64 {
            self.locals_refs |= 1 << slot;
        }
    }

    /// Check if a stack slot contains a reference.
    pub fn is_stack_ref(&self, slot: usize) -> bool {
        if slot < 64 {
            (self.stack_refs & (1 << slot)) != 0
        } else {
            false
        }
    }

    /// Check if a local slot contains a reference.
    pub fn is_local_ref(&self, slot: usize) -> bool {
        if slot < 64 {
            (self.locals_refs & (1 << slot)) != 0
        } else {
            false
        }
    }

    /// Get all stack slots that contain references.
    pub fn stack_ref_slots(&self) -> Vec<usize> {
        (0..self.stack_depth as usize)
            .filter(|&i| self.is_stack_ref(i))
            .collect()
    }

    /// Get all local slots that contain references.
    pub fn local_ref_slots(&self) -> Vec<usize> {
        (0..self.locals_count as usize)
            .filter(|&i| self.is_local_ref(i))
            .collect()
    }
}

/// Stack map table for a compiled function.
#[derive(Debug, Clone, Default)]
pub struct StackMapTable {
    /// Entries indexed by native PC offset
    entries: HashMap<u32, StackMapEntry>,
}

impl StackMapTable {
    /// Create a new empty stack map table.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Add a stack map entry.
    pub fn add_entry(&mut self, entry: StackMapEntry) {
        self.entries.insert(entry.native_pc, entry);
    }

    /// Look up a stack map entry by native PC.
    /// Returns the entry at or before the given PC.
    pub fn lookup(&self, native_pc: u32) -> Option<&StackMapEntry> {
        // First try exact match
        if let Some(entry) = self.entries.get(&native_pc) {
            return Some(entry);
        }

        // Find the closest entry at or before the given PC
        self.entries
            .iter()
            .filter(|(pc, _)| **pc <= native_pc)
            .max_by_key(|(pc, _)| *pc)
            .map(|(_, entry)| entry)
    }

    /// Get all entries.
    pub fn entries(&self) -> impl Iterator<Item = &StackMapEntry> {
        self.entries.values()
    }

    /// Get the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if the table is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Builder for constructing stack maps during JIT compilation.
#[derive(Debug)]
pub struct StackMapBuilder {
    /// Current stack type information (true = reference, false = primitive)
    stack_types: Vec<bool>,
    /// Local type information
    locals_types: Vec<bool>,
    /// Entries being built
    entries: Vec<StackMapEntry>,
}

impl StackMapBuilder {
    /// Create a new stack map builder.
    pub fn new(locals_count: usize) -> Self {
        Self {
            stack_types: Vec::new(),
            locals_types: vec![false; locals_count],
            entries: Vec::new(),
        }
    }

    /// Push a value type onto the stack.
    pub fn push(&mut self, is_ref: bool) {
        self.stack_types.push(is_ref);
    }

    /// Pop a value from the stack.
    pub fn pop(&mut self) -> Option<bool> {
        self.stack_types.pop()
    }

    /// Pop N values from the stack.
    pub fn pop_n(&mut self, n: usize) {
        for _ in 0..n {
            self.stack_types.pop();
        }
    }

    /// Set the type of a local variable.
    pub fn set_local(&mut self, slot: usize, is_ref: bool) {
        if slot < self.locals_types.len() {
            self.locals_types[slot] = is_ref;
        }
    }

    /// Get the current stack depth.
    pub fn stack_depth(&self) -> usize {
        self.stack_types.len()
    }

    /// Record a safepoint at the current position.
    pub fn record_safepoint(&mut self, native_pc: u32, bytecode_pc: u32) {
        let mut entry = StackMapEntry::new(
            native_pc,
            bytecode_pc,
            self.stack_types.len() as u16,
            self.locals_types.len() as u16,
        );

        // Mark reference slots on stack
        for (i, &is_ref) in self.stack_types.iter().enumerate() {
            if is_ref {
                entry.mark_stack_ref(i);
            }
        }

        // Mark reference slots in locals
        for (i, &is_ref) in self.locals_types.iter().enumerate() {
            if is_ref {
                entry.mark_local_ref(i);
            }
        }

        self.entries.push(entry);
    }

    /// Build the final stack map table.
    pub fn build(self) -> StackMapTable {
        let mut table = StackMapTable::new();
        for entry in self.entries {
            table.add_entry(entry);
        }
        table
    }
}

impl Default for StackMapBuilder {
    fn default() -> Self {
        Self::new(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stack_map_entry() {
        let mut entry = StackMapEntry::new(100, 5, 3, 2);

        entry.mark_stack_ref(0);
        entry.mark_stack_ref(2);
        entry.mark_local_ref(1);

        assert!(entry.is_stack_ref(0));
        assert!(!entry.is_stack_ref(1));
        assert!(entry.is_stack_ref(2));

        assert!(!entry.is_local_ref(0));
        assert!(entry.is_local_ref(1));

        assert_eq!(entry.stack_ref_slots(), vec![0, 2]);
        assert_eq!(entry.local_ref_slots(), vec![1]);
    }

    #[test]
    fn test_stack_map_table_lookup() {
        let mut table = StackMapTable::new();

        table.add_entry(StackMapEntry::new(0, 0, 0, 0));
        table.add_entry(StackMapEntry::new(20, 5, 2, 0));
        table.add_entry(StackMapEntry::new(50, 10, 3, 0));

        // Exact match
        assert_eq!(table.lookup(20).unwrap().native_pc, 20);

        // Find closest before
        assert_eq!(table.lookup(25).unwrap().native_pc, 20);
        assert_eq!(table.lookup(49).unwrap().native_pc, 20);
        assert_eq!(table.lookup(50).unwrap().native_pc, 50);
        assert_eq!(table.lookup(100).unwrap().native_pc, 50);
    }

    #[test]
    fn test_stack_map_builder() {
        let mut builder = StackMapBuilder::new(2);

        // Simulate: push int, push ref, store local 0
        builder.push(false); // int
        builder.push(true); // ref
        builder.set_local(0, true);

        builder.record_safepoint(10, 3);

        // Pop and push
        builder.pop();
        builder.push(false);

        builder.record_safepoint(20, 5);

        let table = builder.build();
        assert_eq!(table.len(), 2);

        let entry1 = table.lookup(10).unwrap();
        assert_eq!(entry1.stack_depth, 2);
        assert!(!entry1.is_stack_ref(0));
        assert!(entry1.is_stack_ref(1));
        assert!(entry1.is_local_ref(0));

        let entry2 = table.lookup(20).unwrap();
        assert_eq!(entry2.stack_depth, 2);
        assert!(!entry2.is_stack_ref(1)); // Now a primitive
    }
}
