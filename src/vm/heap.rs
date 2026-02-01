use std::fmt;

use super::Value;

// =============================================================================
// Header Layout (64 bits)
// =============================================================================
//
// +--------+------------------+------------------------+
// | marked | slot_count (32)  | reserved (31)          |
// | 1 bit  | 32 bits          | 31 bits                |
// +--------+------------------+------------------------+
//
// - Bit 63: marked flag for GC
// - Bits 31-62: slot count (max 2^32 - 1 slots)
// - Bits 0-30: reserved for future use

const HEADER_MARKED_BIT: u64 = 1 << 63;
const HEADER_SLOT_COUNT_SHIFT: u32 = 31;
const HEADER_SLOT_COUNT_MASK: u64 = 0xFFFF_FFFF << HEADER_SLOT_COUNT_SHIFT;

/// Encode a header word from marked flag and slot count.
fn encode_header(marked: bool, slot_count: u32) -> u64 {
    let mut header = (slot_count as u64) << HEADER_SLOT_COUNT_SHIFT;
    if marked {
        header |= HEADER_MARKED_BIT;
    }
    header
}

/// Decode marked flag from header word.
fn decode_marked(header: u64) -> bool {
    (header & HEADER_MARKED_BIT) != 0
}

/// Decode slot count from header word.
fn decode_slot_count(header: u64) -> u32 {
    ((header & HEADER_SLOT_COUNT_MASK) >> HEADER_SLOT_COUNT_SHIFT) as u32
}

// =============================================================================
// Object Layout (in u64 words)
// =============================================================================
//
// +----------------+------+------+------+------+-----+
// | Header (1 word)| Tag0 | Val0 | Tag1 | Val1 | ... |
// +----------------+------+------+------+------+-----+
//
// Each slot is 2 words: tag + payload (see Value::encode/decode)
// Total object size = 1 + 2 * slot_count words

/// Calculate the total size in words for an object with n slots.
const fn object_size_words(slot_count: u32) -> usize {
    1 + 2 * (slot_count as usize)
}

// =============================================================================
// HeapObject - View into linear memory
// =============================================================================

/// A heap object containing a vector of slots.
/// Used for arrays, vectors, and strings.
///
/// This is a view that can be constructed from linear memory.
#[derive(Debug)]
pub struct HeapObject {
    /// Whether this object is marked during GC
    pub marked: bool,
    /// The slots containing values
    pub slots: Vec<Value>,
}

impl HeapObject {
    /// Create a new heap object with the given slots.
    pub fn new(slots: Vec<Value>) -> Self {
        Self {
            marked: false,
            slots,
        }
    }

    /// Parse a HeapObject from linear memory at the given offset.
    ///
    /// # Arguments
    /// * `memory` - The linear memory buffer
    /// * `offset` - Offset in words (not bytes) where the object starts
    ///
    /// # Returns
    /// * `Some(HeapObject)` if the offset is valid and the object can be parsed
    /// * `None` if the offset is out of bounds or invalid
    pub fn from_memory(memory: &[u64], offset: usize) -> Option<Self> {
        // Read header
        let header = *memory.get(offset)?;
        let marked = decode_marked(header);
        let slot_count = decode_slot_count(header) as usize;

        // Check if we have enough memory for all slots
        let total_size = object_size_words(slot_count as u32);
        if offset + total_size > memory.len() {
            return None;
        }

        // Read slots
        let mut slots = Vec::with_capacity(slot_count);
        for i in 0..slot_count {
            let tag_offset = offset + 1 + 2 * i;
            let tag = memory[tag_offset];
            let payload = memory[tag_offset + 1];
            let value = Value::decode(tag, payload)?;
            slots.push(value);
        }

        Some(HeapObject { marked, slots })
    }

    /// Convert slots to a Rust String (interpreting slots as Unicode code points)
    pub fn slots_to_string(&self) -> String {
        self.slots
            .iter()
            .filter_map(|v| v.as_i64())
            .filter_map(|c| char::from_u32(c as u32))
            .collect()
    }

    /// Get all Value references in this object for GC tracing.
    pub fn trace(&self) -> Vec<GcRef> {
        self.slots.iter().filter_map(|v| v.as_ref()).collect()
    }
}

impl fmt::Display for HeapObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HeapObject({:?})", self.slots)
    }
}

// =============================================================================
// GcRef - Reference to heap object
// =============================================================================

/// A reference to a heap object.
/// The index field represents the offset in words into linear memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcRef {
    /// Offset in words (u64 units) into linear memory.
    /// Offset 0 is reserved as an invalid/null reference.
    pub index: usize,
}

impl GcRef {
    /// Create a GcRef from an offset. Offset 0 is reserved.
    pub fn from_offset(offset: usize) -> Self {
        Self { index: offset }
    }

    /// Get the offset into linear memory.
    pub fn offset(&self) -> usize {
        self.index
    }

    /// Check if this is a valid (non-null) reference.
    pub fn is_valid(&self) -> bool {
        self.index != 0
    }
}

// =============================================================================
// Heap - Linear memory based heap
// =============================================================================

/// The garbage-collected heap using linear memory (Vec<u64>).
pub struct Heap {
    /// Linear memory buffer
    memory: Vec<u64>,
    /// Next allocation offset (in words)
    next_alloc: usize,
    /// Head of free list (offset, or 0 if empty)
    free_list_head: usize,
    /// Bytes allocated (for GC threshold)
    bytes_allocated: usize,
    /// GC threshold in bytes
    gc_threshold: usize,
    /// Hard limit on heap size (None = unlimited)
    heap_limit: Option<usize>,
    /// Whether GC is enabled
    gc_enabled: bool,
}

impl Heap {
    /// Initial capacity in words (1 MB / 8 bytes = 128K words)
    const INITIAL_CAPACITY: usize = 128 * 1024;

    pub fn new() -> Self {
        Self::new_with_config(None, true)
    }

    /// Create a new heap with custom configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    pub fn new_with_config(heap_limit: Option<usize>, gc_enabled: bool) -> Self {
        let mut memory = Vec::with_capacity(Self::INITIAL_CAPACITY);
        // Reserve offset 0 as invalid/null (write a dummy word)
        memory.push(0);

        Self {
            memory,
            next_alloc: 1, // Start after reserved word
            free_list_head: 0,
            bytes_allocated: 0,
            gc_threshold: 1024 * 1024, // 1MB initial threshold
            heap_limit,
            gc_enabled,
        }
    }

    /// Check if allocation would exceed heap limit.
    fn check_heap_limit(&self, additional_bytes: usize) -> Result<(), String> {
        if let Some(limit) = self.heap_limit {
            let new_total = self.bytes_allocated + additional_bytes;
            if new_total > limit {
                return Err(format!(
                    "runtime error: heap limit exceeded (allocated: {} bytes, limit: {} bytes)",
                    new_total, limit
                ));
            }
        }
        Ok(())
    }

    /// Allocate a new string on the heap.
    /// String is stored with each character as Value::I64 (Unicode code point).
    pub fn alloc_string(&mut self, value: String) -> Result<GcRef, String> {
        let slots: Vec<Value> = value.chars().map(|c| Value::I64(c as i64)).collect();
        self.alloc_slots(slots)
    }

    /// Allocate a new slot-based heap object.
    pub fn alloc_slots(&mut self, slots: Vec<Value>) -> Result<GcRef, String> {
        let slot_count = slots.len() as u32;
        let obj_size_words = object_size_words(slot_count);
        let obj_size_bytes = obj_size_words * 8;

        self.check_heap_limit(obj_size_bytes)?;

        // Ensure we have enough capacity
        let required_len = self.next_alloc + obj_size_words;
        if required_len > self.memory.len() {
            self.memory
                .resize(required_len.max(self.memory.len() * 2), 0);
        }

        // Allocate at next_alloc
        let offset = self.next_alloc;
        self.next_alloc += obj_size_words;
        self.bytes_allocated += obj_size_bytes;

        // Write header
        self.memory[offset] = encode_header(false, slot_count);

        // Write slots
        for (i, value) in slots.iter().enumerate() {
            let (tag, payload) = value.encode();
            self.memory[offset + 1 + 2 * i] = tag;
            self.memory[offset + 1 + 2 * i + 1] = payload;
        }

        Ok(GcRef::from_offset(offset))
    }

    /// Get an object by reference, constructing a HeapObject view.
    pub fn get(&self, r: GcRef) -> Option<HeapObject> {
        if !r.is_valid() {
            return None;
        }
        HeapObject::from_memory(&self.memory, r.offset())
    }

    /// Read a single slot from an object.
    pub fn read_slot(&self, r: GcRef, slot_index: usize) -> Option<Value> {
        if !r.is_valid() {
            return None;
        }

        let offset = r.offset();
        let header = *self.memory.get(offset)?;
        let slot_count = decode_slot_count(header) as usize;

        if slot_index >= slot_count {
            return None;
        }

        let tag_offset = offset + 1 + 2 * slot_index;
        let tag = *self.memory.get(tag_offset)?;
        let payload = *self.memory.get(tag_offset + 1)?;
        Value::decode(tag, payload)
    }

    /// Write a single slot to an object.
    pub fn write_slot(&mut self, r: GcRef, slot_index: usize, value: Value) -> Result<(), String> {
        if !r.is_valid() {
            return Err("invalid reference".to_string());
        }

        let offset = r.offset();
        let header = *self
            .memory
            .get(offset)
            .ok_or("invalid reference: out of bounds")?;
        let slot_count = decode_slot_count(header) as usize;

        if slot_index >= slot_count {
            return Err(format!(
                "slot index {} out of bounds (count: {})",
                slot_index, slot_count
            ));
        }

        let tag_offset = offset + 1 + 2 * slot_index;
        let (tag, payload) = value.encode();
        self.memory[tag_offset] = tag;
        self.memory[tag_offset + 1] = payload;
        Ok(())
    }

    /// Get the slot count for an object.
    pub fn slot_count(&self, r: GcRef) -> Option<usize> {
        if !r.is_valid() {
            return None;
        }
        let header = *self.memory.get(r.offset())?;
        Some(decode_slot_count(header) as usize)
    }

    /// Check if GC should be triggered.
    pub fn should_gc(&self) -> bool {
        self.gc_enabled && self.bytes_allocated >= self.gc_threshold
    }

    /// Get the number of bytes currently allocated.
    pub fn bytes_allocated(&self) -> usize {
        self.bytes_allocated
    }

    /// Set the marked flag for an object.
    fn set_marked(&mut self, offset: usize, marked: bool) {
        if let Some(header) = self.memory.get_mut(offset) {
            if marked {
                *header |= HEADER_MARKED_BIT;
            } else {
                *header &= !HEADER_MARKED_BIT;
            }
        }
    }

    /// Get the marked flag for an object.
    fn is_marked(&self, offset: usize) -> bool {
        self.memory
            .get(offset)
            .map(|h| decode_marked(*h))
            .unwrap_or(false)
    }

    /// Mark phase: mark all reachable objects.
    pub fn mark(&mut self, roots: &[Value]) {
        // Collect all root references
        let mut worklist: Vec<GcRef> = roots.iter().filter_map(|v| v.as_ref()).collect();

        // Mark and trace
        while let Some(r) = worklist.pop() {
            if !r.is_valid() {
                continue;
            }

            let offset = r.offset();
            if self.is_marked(offset) {
                continue;
            }

            // Mark this object
            self.set_marked(offset, true);

            // Trace children - need to read the object to find references
            if let Some(obj) = HeapObject::from_memory(&self.memory, offset) {
                worklist.extend(obj.trace());
            }
        }
    }

    /// Sweep phase: free all unmarked objects.
    /// Note: With bump allocation, we don't actually free memory yet.
    /// Full free list support will be added in a later task.
    pub fn sweep(&mut self) {
        // Walk through all allocated objects
        let mut offset = 1; // Start after reserved word
        let mut live_bytes = 0;

        while offset < self.next_alloc {
            let header = self.memory[offset];
            let slot_count = decode_slot_count(header);
            let obj_size = object_size_words(slot_count);

            if decode_marked(header) {
                // Reset mark for next GC cycle
                self.set_marked(offset, false);
                live_bytes += obj_size * 8;
            }
            // TODO: Add to free list when unmarked (future task)

            offset += obj_size;
        }

        self.bytes_allocated = live_bytes;
        self.gc_threshold = (self.bytes_allocated * 2).max(1024 * 1024);
    }

    /// Perform a full garbage collection cycle.
    pub fn collect(&mut self, roots: &[Value]) {
        self.mark(roots);
        self.sweep();
    }

    /// Get count of live (marked after GC) objects.
    /// Note: This counts all allocated objects (some may be garbage before GC).
    pub fn object_count(&self) -> usize {
        let mut count = 0;
        let mut offset = 1;

        while offset < self.next_alloc {
            let header = self.memory[offset];
            let slot_count = decode_slot_count(header);
            count += 1;
            offset += object_size_words(slot_count);
        }

        count
    }

    /// Get raw memory for testing/debugging.
    #[cfg(test)]
    pub fn memory(&self) -> &[u64] {
        &self.memory
    }
}

impl Default for Heap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encoding() {
        // Test various combinations
        let h1 = encode_header(false, 0);
        assert!(!decode_marked(h1));
        assert_eq!(decode_slot_count(h1), 0);

        let h2 = encode_header(true, 42);
        assert!(decode_marked(h2));
        assert_eq!(decode_slot_count(h2), 42);

        let h3 = encode_header(false, u32::MAX);
        assert!(!decode_marked(h3));
        assert_eq!(decode_slot_count(h3), u32::MAX);
    }

    #[test]
    fn test_alloc_and_get() {
        let mut heap = Heap::new();

        // Allocate an object with 3 slots
        let values = vec![Value::I64(1), Value::I64(2), Value::I64(3)];
        let r = heap.alloc_slots(values.clone()).unwrap();

        // Get it back
        let obj = heap.get(r).unwrap();
        assert!(!obj.marked);
        assert_eq!(obj.slots.len(), 3);
        assert_eq!(obj.slots[0], Value::I64(1));
        assert_eq!(obj.slots[1], Value::I64(2));
        assert_eq!(obj.slots[2], Value::I64(3));
    }

    #[test]
    fn test_alloc_string() {
        let mut heap = Heap::new();
        let r = heap.alloc_string("hello".to_string()).unwrap();
        let obj = heap.get(r).unwrap();
        let str_value = obj.slots_to_string();
        assert_eq!(str_value, "hello");
    }

    #[test]
    fn test_read_write_slot() {
        let mut heap = Heap::new();
        let r = heap
            .alloc_slots(vec![Value::I64(10), Value::I64(20)])
            .unwrap();

        // Read slots
        assert_eq!(heap.read_slot(r, 0), Some(Value::I64(10)));
        assert_eq!(heap.read_slot(r, 1), Some(Value::I64(20)));
        assert_eq!(heap.read_slot(r, 2), None); // Out of bounds

        // Write slot
        heap.write_slot(r, 0, Value::I64(100)).unwrap();
        assert_eq!(heap.read_slot(r, 0), Some(Value::I64(100)));

        // Write out of bounds should fail
        assert!(heap.write_slot(r, 5, Value::I64(0)).is_err());
    }

    #[test]
    fn test_multiple_objects() {
        let mut heap = Heap::new();

        let r1 = heap.alloc_slots(vec![Value::I64(1)]).unwrap();
        let r2 = heap
            .alloc_slots(vec![Value::I64(2), Value::I64(3)])
            .unwrap();
        let r3 = heap.alloc_slots(vec![Value::Bool(true)]).unwrap();

        // All objects should be retrievable
        assert_eq!(heap.get(r1).unwrap().slots[0], Value::I64(1));
        assert_eq!(heap.get(r2).unwrap().slots.len(), 2);
        assert_eq!(heap.get(r3).unwrap().slots[0], Value::Bool(true));

        assert_eq!(heap.object_count(), 3);
    }

    #[test]
    fn test_gc_marks_reachable() {
        let mut heap = Heap::new();

        let r1 = heap.alloc_string("keep".to_string()).unwrap();
        let _r2 = heap.alloc_string("garbage".to_string()).unwrap();

        // Only r1 in roots
        heap.mark(&[Value::Ref(r1)]);

        // r1 should be marked
        assert!(heap.is_marked(r1.offset()));
    }

    #[test]
    fn test_gc_traces_references() {
        let mut heap = Heap::new();

        // Create a string
        let str_ref = heap.alloc_string("nested".to_string()).unwrap();

        // Create an object containing the string reference
        let container = heap.alloc_slots(vec![Value::Ref(str_ref)]).unwrap();

        // Mark only the container as root
        heap.mark(&[Value::Ref(container)]);

        // Both should be marked (container and nested string)
        assert!(heap.is_marked(container.offset()));
        assert!(heap.is_marked(str_ref.offset()));
    }

    #[test]
    fn test_invalid_ref() {
        let heap = Heap::new();

        // Offset 0 is invalid
        let invalid = GcRef::from_offset(0);
        assert!(!invalid.is_valid());
        assert!(heap.get(invalid).is_none());
    }

    #[test]
    fn test_slot_count() {
        let mut heap = Heap::new();

        let r1 = heap.alloc_slots(vec![]).unwrap();
        let r2 = heap
            .alloc_slots(vec![Value::I64(1), Value::I64(2)])
            .unwrap();

        assert_eq!(heap.slot_count(r1), Some(0));
        assert_eq!(heap.slot_count(r2), Some(2));
    }
}
