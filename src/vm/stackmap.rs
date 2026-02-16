//! StackMap for BCVM v0 GC Integration
//!
//! StackMaps record which stack slots and locals contain references (Ref values)
//! at each safepoint. This enables precise garbage collection.
//!
//! Safepoints are defined at:
//! - CALL instructions
//! - NEW instructions
//! - Backward jumps (JMP* where target < pc)

use std::collections::HashMap;

/// A bitset for tracking reference slots.
/// Supports up to 64 slots (sufficient for most functions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RefBitset(u64);

impl RefBitset {
    /// Create an empty bitset
    pub const fn new() -> Self {
        Self(0)
    }

    /// Set bit at index
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < 64, "RefBitset supports max 64 slots");
        self.0 |= 1 << index;
    }

    /// Clear bit at index
    pub fn clear(&mut self, index: usize) {
        debug_assert!(index < 64, "RefBitset supports max 64 slots");
        self.0 &= !(1 << index);
    }

    /// Check if bit at index is set
    pub fn is_set(&self, index: usize) -> bool {
        if index >= 64 {
            return false;
        }
        (self.0 & (1 << index)) != 0
    }

    /// Get raw bits
    pub fn bits(&self) -> u64 {
        self.0
    }

    /// Create from raw bits
    pub const fn from_bits(bits: u64) -> Self {
        Self(bits)
    }

    /// Count number of set bits
    pub fn count_ones(&self) -> u32 {
        self.0.count_ones()
    }

    /// Iterate over indices of set bits
    pub fn iter_set_indices(&self) -> impl Iterator<Item = usize> + '_ {
        (0..64).filter(|&i| self.is_set(i))
    }
}

/// A StackMap entry for a single safepoint.
///
/// Records the state at a specific program counter where GC may occur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackMapEntry {
    /// Program counter (bytecode offset) of the safepoint
    pub pc: u32,
    /// Number of valid stack slots at this point
    pub stack_height: u16,
    /// Bitset indicating which stack slots contain references
    /// Bit i is set if stack[i] is a Ref
    pub stack_ref_bits: RefBitset,
    /// Bitset indicating which local slots contain references
    /// Bit i is set if locals[i] is a Ref
    pub locals_ref_bits: RefBitset,
}

impl StackMapEntry {
    /// Create a new StackMap entry
    pub fn new(pc: u32, stack_height: u16) -> Self {
        Self {
            pc,
            stack_height,
            stack_ref_bits: RefBitset::new(),
            locals_ref_bits: RefBitset::new(),
        }
    }

    /// Mark a stack slot as containing a reference
    pub fn mark_stack_ref(&mut self, index: usize) {
        self.stack_ref_bits.set(index);
    }

    /// Mark a local slot as containing a reference
    pub fn mark_local_ref(&mut self, index: usize) {
        self.locals_ref_bits.set(index);
    }

    /// Check if a stack slot contains a reference
    pub fn is_stack_ref(&self, index: usize) -> bool {
        self.stack_ref_bits.is_set(index)
    }

    /// Check if a local slot contains a reference
    pub fn is_local_ref(&self, index: usize) -> bool {
        self.locals_ref_bits.is_set(index)
    }
}

/// StackMap for a function.
///
/// Contains entries for all safepoints in the function.
#[derive(Debug, Clone, Default)]
pub struct FunctionStackMap {
    /// Map from PC to StackMapEntry
    entries: HashMap<u32, StackMapEntry>,
}

impl FunctionStackMap {
    /// Create an empty StackMap
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Add an entry for a safepoint
    pub fn add_entry(&mut self, entry: StackMapEntry) {
        self.entries.insert(entry.pc, entry);
    }

    /// Get entry for a specific PC
    pub fn get(&self, pc: u32) -> Option<&StackMapEntry> {
        self.entries.get(&pc)
    }

    /// Check if a safepoint exists at PC
    pub fn has_safepoint(&self, pc: u32) -> bool {
        self.entries.contains_key(&pc)
    }

    /// Get all entries
    pub fn entries(&self) -> impl Iterator<Item = &StackMapEntry> {
        self.entries.values()
    }

    /// Get number of entries
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Determine if an instruction is a safepoint.
///
/// Safepoints are where GC may run:
/// - CALL: may trigger GC in callee
/// - NEW: may trigger GC for allocation
/// - Backward jumps: to ensure bounded GC latency in loops
pub fn is_safepoint(op: &super::ops::Op, pc: usize) -> bool {
    use super::ops::Op;

    match op {
        // CALL is always a safepoint
        Op::Call(_, _) => true,

        // Heap allocation is also a safepoint
        Op::HeapAlloc(_) | Op::HeapAllocArray(_, _) => true,

        // Backward jumps are safepoints
        Op::Jmp(target) => *target < pc,
        Op::BrIf(target) => *target < pc,
        Op::BrIfFalse(target) => *target < pc,

        // Thread operations may allocate
        Op::ThreadSpawn(_) | Op::ChannelCreate => true,

        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ref_bitset_basic() {
        let mut bs = RefBitset::new();
        assert!(!bs.is_set(0));
        assert!(!bs.is_set(5));

        bs.set(0);
        bs.set(5);
        bs.set(63);

        assert!(bs.is_set(0));
        assert!(bs.is_set(5));
        assert!(bs.is_set(63));
        assert!(!bs.is_set(1));

        bs.clear(5);
        assert!(!bs.is_set(5));
    }

    #[test]
    fn test_ref_bitset_count() {
        let mut bs = RefBitset::new();
        assert_eq!(bs.count_ones(), 0);

        bs.set(0);
        bs.set(10);
        bs.set(20);
        assert_eq!(bs.count_ones(), 3);
    }

    #[test]
    fn test_ref_bitset_iter() {
        let mut bs = RefBitset::new();
        bs.set(1);
        bs.set(5);
        bs.set(10);

        let indices: Vec<usize> = bs.iter_set_indices().collect();
        assert_eq!(indices, vec![1, 5, 10]);
    }

    #[test]
    fn test_stackmap_entry() {
        let mut entry = StackMapEntry::new(42, 3);
        assert_eq!(entry.pc, 42);
        assert_eq!(entry.stack_height, 3);

        entry.mark_stack_ref(0);
        entry.mark_stack_ref(2);
        entry.mark_local_ref(1);

        assert!(entry.is_stack_ref(0));
        assert!(!entry.is_stack_ref(1));
        assert!(entry.is_stack_ref(2));
        assert!(entry.is_local_ref(1));
    }

    #[test]
    fn test_function_stackmap() {
        let mut fsm = FunctionStackMap::new();
        assert!(fsm.is_empty());

        let entry1 = StackMapEntry::new(10, 2);
        let entry2 = StackMapEntry::new(20, 3);

        fsm.add_entry(entry1);
        fsm.add_entry(entry2);

        assert_eq!(fsm.len(), 2);
        assert!(fsm.has_safepoint(10));
        assert!(fsm.has_safepoint(20));
        assert!(!fsm.has_safepoint(15));

        let e = fsm.get(10).unwrap();
        assert_eq!(e.stack_height, 2);
    }

    #[test]
    fn test_is_safepoint() {
        use super::super::ops::Op;

        // CALL is safepoint
        assert!(is_safepoint(&Op::Call(0, 2), 5));

        // Backward jump is safepoint
        assert!(is_safepoint(&Op::Jmp(0), 5)); // 0 < 5, backward
        assert!(!is_safepoint(&Op::Jmp(10), 5)); // 10 > 5, forward

        // Forward conditional is not safepoint
        assert!(!is_safepoint(&Op::BrIf(10), 5));

        // Backward conditional is safepoint
        assert!(is_safepoint(&Op::BrIf(0), 5));

        // Regular ops are not safepoints
        assert!(!is_safepoint(&Op::I64Add, 5));
        assert!(!is_safepoint(&Op::LocalGet(0), 5));
    }

    // ============================================================
    // BCVM v0 Specification Compliance Tests
    // ============================================================

    /// Test: Spec 7.4 - RefBitset supports up to 64 slots
    #[test]
    fn test_spec_refbitset_max_slots() {
        let mut bs = RefBitset::new();

        // Set all 64 bits
        for i in 0..64 {
            bs.set(i);
            assert!(bs.is_set(i), "Bit {} should be set", i);
        }

        // All 64 bits should be set
        assert_eq!(bs.count_ones(), 64);

        // Clear all
        for i in 0..64 {
            bs.clear(i);
        }
        assert_eq!(bs.count_ones(), 0);
    }

    /// Test: Spec 7.4 - RefBitset boundary conditions
    #[test]
    fn test_spec_refbitset_boundary() {
        let mut bs = RefBitset::new();

        // First and last slots
        bs.set(0);
        bs.set(63);
        assert!(bs.is_set(0));
        assert!(bs.is_set(63));
        assert!(!bs.is_set(32)); // Middle should be clear

        // Verify iteration
        let indices: Vec<usize> = bs.iter_set_indices().collect();
        assert_eq!(indices, vec![0, 63]);
    }

    /// Test: Spec 7.4 - StackMapEntry tracks references at safepoint
    #[test]
    fn test_spec_stackmap_entry_references() {
        let mut entry = StackMapEntry::new(10, 5);

        // Mark slots 1 and 3 as references on stack
        entry.stack_ref_bits.set(1);
        entry.stack_ref_bits.set(3);

        // Mark local 0 as reference
        entry.locals_ref_bits.set(0);

        // Verify
        assert_eq!(entry.pc, 10);
        assert_eq!(entry.stack_height, 5);
        assert!(entry.stack_ref_bits.is_set(1));
        assert!(entry.stack_ref_bits.is_set(3));
        assert!(!entry.stack_ref_bits.is_set(2));
        assert!(entry.locals_ref_bits.is_set(0));
    }

    /// Test: Spec 7.3 - AllocHeap is also a safepoint (GC may trigger)
    #[test]
    fn test_spec_alloc_heap_safepoint() {
        use super::super::ops::Op;

        // Heap allocation can trigger GC
        assert!(is_safepoint(&Op::HeapAlloc(10), 5));
    }

    /// Test: Spec 7.4 - FunctionStackMap lookup by PC
    #[test]
    fn test_spec_stackmap_lookup() {
        let mut fsm = FunctionStackMap::new();

        // Add entries for different safepoints
        fsm.add_entry(StackMapEntry::new(5, 2));
        fsm.add_entry(StackMapEntry::new(10, 3));
        fsm.add_entry(StackMapEntry::new(15, 1));

        // Verify lookups
        assert!(fsm.get(5).is_some());
        assert!(fsm.get(10).is_some());
        assert!(fsm.get(15).is_some());

        // Non-existent entries
        assert!(fsm.get(0).is_none());
        assert!(fsm.get(7).is_none());
        assert!(fsm.get(20).is_none());

        // Verify correct stack heights
        assert_eq!(fsm.get(5).unwrap().stack_height, 2);
        assert_eq!(fsm.get(10).unwrap().stack_height, 3);
        assert_eq!(fsm.get(15).unwrap().stack_height, 1);
    }

    /// Test: Spec 7.3 - All allocation operations are safepoints
    #[test]
    fn test_spec_all_allocation_safepoints() {
        use super::super::ops::Op;

        // Heap allocation
        assert!(is_safepoint(&Op::HeapAlloc(0), 0));
        assert!(is_safepoint(&Op::HeapAlloc(100), 0));

        // Function call (may allocate)
        assert!(is_safepoint(&Op::Call(0, 0), 0));
        assert!(is_safepoint(&Op::Call(5, 3), 0));
    }
}
