use std::fmt;

use super::Value;

// =============================================================================
// Header Layout (64 bits)
// =============================================================================
//
// +--------+------+------------------+------+----------------+
// | marked | free | slot_count (32)  | kind | reserved (28)  |
// | 1 bit  | 1 bit| 32 bits          | 2 bit| 28 bits        |
// +--------+------+------------------+------+----------------+
//
// - Bit 63: marked flag for GC
// - Bit 62: free flag (1 = free block in free list, 0 = allocated)
// - Bits 30-61: slot count (max 2^32 - 1 slots)
// - Bits 28-29: object kind (0=slots, 1=string, 2=array, 3=reserved)
// - Bits 0-27: reserved for future use
//
// Free block layout:
// +----------------+----------------+
// | Header         | Next Free Ptr  |
// | (free=1, size) | (offset or 0)  |
// +----------------+----------------+

const HEADER_MARKED_BIT: u64 = 1 << 63;
const HEADER_FREE_BIT: u64 = 1 << 62;
const HEADER_SLOT_COUNT_SHIFT: u32 = 30;
const HEADER_SLOT_COUNT_MASK: u64 = 0xFFFF_FFFF << HEADER_SLOT_COUNT_SHIFT;
const HEADER_KIND_SHIFT: u32 = 28;
const HEADER_KIND_MASK: u64 = 0x3 << HEADER_KIND_SHIFT;

/// Object kinds for distinguishing heap object types at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ObjectKind {
    Slots = 0,  // Default: structs, vectors, etc.
    String = 1, // String objects
    Array = 2,  // Arrays (for future use)
}

impl ObjectKind {
    fn from_u64(value: u64) -> Self {
        match value {
            1 => ObjectKind::String,
            2 => ObjectKind::Array,
            _ => ObjectKind::Slots,
        }
    }
}

/// Encode a header word from marked flag, slot count, and object kind.
fn encode_header(marked: bool, slot_count: u32, kind: ObjectKind) -> u64 {
    let mut header = (slot_count as u64) << HEADER_SLOT_COUNT_SHIFT;
    header |= (kind as u64) << HEADER_KIND_SHIFT;
    if marked {
        header |= HEADER_MARKED_BIT;
    }
    header
}

/// Encode a free block header.
fn encode_free_header(size_words: usize) -> u64 {
    // For free blocks, we store the size in words (not slot count)
    // in the slot_count field for simplicity
    let size = size_words as u64;
    (size << HEADER_SLOT_COUNT_SHIFT) | HEADER_FREE_BIT
}

/// Decode marked flag from header word.
fn decode_marked(header: u64) -> bool {
    (header & HEADER_MARKED_BIT) != 0
}

/// Decode free flag from header word.
fn decode_free(header: u64) -> bool {
    (header & HEADER_FREE_BIT) != 0
}

/// Decode slot count from header word (for allocated objects).
fn decode_slot_count(header: u64) -> u32 {
    ((header & HEADER_SLOT_COUNT_MASK) >> HEADER_SLOT_COUNT_SHIFT) as u32
}

/// Decode object kind from header word.
fn decode_kind(header: u64) -> ObjectKind {
    ObjectKind::from_u64((header & HEADER_KIND_MASK) >> HEADER_KIND_SHIFT)
}

/// Decode size in words from free block header.
fn decode_free_size(header: u64) -> usize {
    ((header & HEADER_SLOT_COUNT_MASK) >> HEADER_SLOT_COUNT_SHIFT) as usize
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
    /// The kind of object (slots, string, array)
    pub kind: ObjectKind,
    /// The slots containing values
    pub slots: Vec<Value>,
}

impl HeapObject {
    /// Create a new heap object with the given slots.
    pub fn new(slots: Vec<Value>) -> Self {
        Self {
            marked: false,
            kind: ObjectKind::Slots,
            slots,
        }
    }

    /// Create a new heap object with the given slots and kind.
    pub fn new_with_kind(slots: Vec<Value>, kind: ObjectKind) -> Self {
        Self {
            marked: false,
            kind,
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
        let kind = decode_kind(header);
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

        Some(HeapObject {
            marked,
            kind,
            slots,
        })
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
    /// Encodes both base offset and slot offset via bit-packing.
    /// Lower 40 bits: base offset in words (heap header position).
    /// Upper 24 bits: slot offset (added to slot_index in read_slot/write_slot).
    /// Offset 0 is reserved as an invalid/null reference.
    pub index: usize,
}

impl GcRef {
    const BASE_MASK: usize = (1 << 40) - 1;
    const OFFSET_SHIFT: usize = 40;

    /// Create a GcRef from a base offset. Slot offset is 0.
    pub fn from_offset(offset: usize) -> Self {
        Self { index: offset }
    }

    /// Create a GcRef with both base and slot offset.
    pub fn new_with_slot_offset(base: usize, slot_offset: usize) -> Self {
        Self {
            index: (slot_offset << Self::OFFSET_SHIFT) | (base & Self::BASE_MASK),
        }
    }

    /// Get the base offset into linear memory (heap header position).
    pub fn base(&self) -> usize {
        self.index & Self::BASE_MASK
    }

    /// Get the base offset into linear memory (alias for base(), backward compat).
    pub fn offset(&self) -> usize {
        self.base()
    }

    /// Get the slot offset (added to slot_index in heap operations).
    pub fn slot_offset(&self) -> usize {
        self.index >> Self::OFFSET_SHIFT
    }

    /// Create a new GcRef with an additional slot offset.
    pub fn with_added_slot_offset(&self, n: usize) -> Self {
        Self::new_with_slot_offset(self.base(), self.slot_offset() + n)
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

    /// Get a raw pointer to the heap memory base.
    /// This is used by JIT code to directly access heap objects.
    ///
    /// # Safety
    /// The returned pointer is valid as long as no heap reallocation occurs.
    /// JIT code should only use this during a single execution without GC.
    pub fn memory_base_ptr(&self) -> *const u64 {
        self.memory.as_ptr()
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
    /// String is stored as a struct [ptr, len] where ptr points to a data array
    /// containing each character as Value::I64 (Unicode code point).
    pub fn alloc_string(&mut self, value: String) -> Result<GcRef, String> {
        let slots: Vec<Value> = value.chars().map(|c| Value::I64(c as i64)).collect();
        let len = slots.len();
        let data_ref = self.alloc_slots(slots)?; // ObjectKind::Slots
        let struct_slots = vec![Value::Ref(data_ref), Value::I64(len as i64)];
        self.alloc_slots_with_kind(struct_slots, ObjectKind::String)
    }

    /// Allocate a new slot-based heap object.
    pub fn alloc_slots(&mut self, slots: Vec<Value>) -> Result<GcRef, String> {
        self.alloc_slots_with_kind(slots, ObjectKind::Slots)
    }

    /// Allocate a new heap object with the given slots and kind.
    pub fn alloc_slots_with_kind(
        &mut self,
        slots: Vec<Value>,
        kind: ObjectKind,
    ) -> Result<GcRef, String> {
        let slot_count = slots.len() as u32;
        let obj_size_words = object_size_words(slot_count);
        let obj_size_bytes = obj_size_words * 8;

        self.check_heap_limit(obj_size_bytes)?;

        // Try to find a suitable free block (first-fit)
        let offset = if let Some(offset) = self.find_free_block(obj_size_words) {
            offset
        } else {
            // No suitable free block, allocate from bump pointer
            let required_len = self.next_alloc + obj_size_words;
            if required_len > self.memory.len() {
                self.memory
                    .resize(required_len.max(self.memory.len() * 2), 0);
            }

            let offset = self.next_alloc;
            self.next_alloc += obj_size_words;
            offset
        };

        self.bytes_allocated += obj_size_bytes;

        // Write header (not marked, not free, with kind)
        self.memory[offset] = encode_header(false, slot_count, kind);

        // Write slots
        for (i, value) in slots.iter().enumerate() {
            let (tag, payload) = value.encode();
            self.memory[offset + 1 + 2 * i] = tag;
            self.memory[offset + 1 + 2 * i + 1] = payload;
        }

        Ok(GcRef::from_offset(offset))
    }

    /// Find a free block of at least the given size (first-fit).
    /// If found, removes it from the free list and returns its offset.
    /// May split the block if it's larger than needed.
    fn find_free_block(&mut self, needed_words: usize) -> Option<usize> {
        // Minimum free block size: header + next pointer = 2 words
        const MIN_FREE_BLOCK_SIZE: usize = 2;

        let mut prev_offset: Option<usize> = None;
        let mut current = self.free_list_head;

        while current != 0 {
            let header = self.memory[current];
            let block_size = decode_free_size(header);
            let next = self.memory[current + 1] as usize;

            if block_size >= needed_words {
                // Found a suitable block
                // Remove from free list
                if let Some(prev) = prev_offset {
                    self.memory[prev + 1] = next as u64;
                } else {
                    self.free_list_head = next;
                }

                // Check if we should split the block
                let remaining = block_size - needed_words;
                if remaining >= MIN_FREE_BLOCK_SIZE {
                    // Split: create a new free block for the remainder
                    let new_free_offset = current + needed_words;
                    self.memory[new_free_offset] = encode_free_header(remaining);
                    self.memory[new_free_offset + 1] = self.free_list_head as u64;
                    self.free_list_head = new_free_offset;
                }

                return Some(current);
            }

            prev_offset = Some(current);
            current = next;
        }

        None
    }

    /// Add a block to the free list.
    fn add_to_free_list(&mut self, offset: usize, size_words: usize) {
        // Write free block header
        self.memory[offset] = encode_free_header(size_words);
        // Link to current head
        self.memory[offset + 1] = self.free_list_head as u64;
        // Update head
        self.free_list_head = offset;
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

        let actual_slot = slot_index + r.slot_offset();
        let offset = r.base();
        let header = *self.memory.get(offset)?;
        let slot_count = decode_slot_count(header) as usize;

        if actual_slot >= slot_count {
            return None;
        }

        let tag_offset = offset + 1 + 2 * actual_slot;
        let tag = *self.memory.get(tag_offset)?;
        let payload = *self.memory.get(tag_offset + 1)?;
        Value::decode(tag, payload)
    }

    /// Write a single slot to an object.
    pub fn write_slot(&mut self, r: GcRef, slot_index: usize, value: Value) -> Result<(), String> {
        if !r.is_valid() {
            return Err("invalid reference".to_string());
        }

        let actual_slot = slot_index + r.slot_offset();
        let offset = r.base();
        let header = *self
            .memory
            .get(offset)
            .ok_or("invalid reference: out of bounds")?;
        let slot_count = decode_slot_count(header) as usize;

        if actual_slot >= slot_count {
            return Err(format!(
                "slot index {} out of bounds (count: {})",
                actual_slot, slot_count
            ));
        }

        let tag_offset = offset + 1 + 2 * actual_slot;
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

    /// Check if an offset could be a valid allocated object start.
    /// Used for conservative GC stack scanning in the typed opcode architecture
    /// where stack values are raw u64 and type information is in opcodes.
    pub fn is_possible_object_ref(&self, offset: usize) -> bool {
        if offset == 0 || offset >= self.next_alloc {
            return false;
        }
        if let Some(&header) = self.memory.get(offset) {
            if decode_free(header) {
                return false;
            }
            let slot_count = decode_slot_count(header);
            let obj_size = object_size_words(slot_count);
            offset + obj_size <= self.next_alloc
        } else {
            false
        }
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

    /// Sweep phase: free all unmarked objects by adding them to the free list.
    pub fn sweep(&mut self) {
        // Walk through all allocated objects
        let mut offset = 1; // Start after reserved word
        let mut live_bytes = 0;

        while offset < self.next_alloc {
            let header = self.memory[offset];

            // Skip free blocks (already in free list)
            if decode_free(header) {
                let block_size = decode_free_size(header);
                offset += block_size;
                continue;
            }

            let slot_count = decode_slot_count(header);
            let obj_size = object_size_words(slot_count);

            if decode_marked(header) {
                // Live object - reset mark for next GC cycle
                self.set_marked(offset, false);
                live_bytes += obj_size * 8;
            } else {
                // Dead object - add to free list if large enough.
                // Free blocks need at least 2 words (header + next pointer).
                // 1-word objects (slot_count=0) cannot hold a next pointer,
                // so adding them would corrupt adjacent memory.
                if obj_size >= 2 {
                    self.add_to_free_list(offset, obj_size);
                }
            }

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

    /// Get count of allocated (non-free) objects.
    /// Note: This counts all allocated objects (some may be garbage before GC).
    pub fn object_count(&self) -> usize {
        let mut count = 0;
        let mut offset = 1;

        while offset < self.next_alloc {
            let header = self.memory[offset];

            // Skip free blocks
            if decode_free(header) {
                let block_size = decode_free_size(header);
                offset += block_size;
                continue;
            }

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
        let h1 = encode_header(false, 0, ObjectKind::Slots);
        assert!(!decode_marked(h1));
        assert_eq!(decode_slot_count(h1), 0);
        assert_eq!(decode_kind(h1), ObjectKind::Slots);

        let h2 = encode_header(true, 42, ObjectKind::String);
        assert!(decode_marked(h2));
        assert_eq!(decode_slot_count(h2), 42);
        assert_eq!(decode_kind(h2), ObjectKind::String);

        let h3 = encode_header(false, u32::MAX, ObjectKind::Array);
        assert!(!decode_marked(h3));
        assert_eq!(decode_slot_count(h3), u32::MAX);
        assert_eq!(decode_kind(h3), ObjectKind::Array);
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
        // String struct: [ptr, len]
        assert_eq!(obj.kind, ObjectKind::String);
        assert_eq!(obj.slots.len(), 2);
        assert_eq!(obj.slots[1], Value::I64(5)); // len = 5
        // Follow ptr to data array
        let data_ref = obj.slots[0].as_ref().unwrap();
        let data = heap.get(data_ref).unwrap();
        let str_value = data.slots_to_string();
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

    // =========================================================================
    // Free List Tests
    // =========================================================================

    #[test]
    fn test_free_header_encoding() {
        // Test free block header encoding/decoding
        let h = encode_free_header(10);
        assert!(decode_free(h));
        assert!(!decode_marked(h));
        assert_eq!(decode_free_size(h), 10);

        let h2 = encode_free_header(1000);
        assert!(decode_free(h2));
        assert_eq!(decode_free_size(h2), 1000);
    }

    #[test]
    fn test_gc_adds_to_free_list() {
        let mut heap = Heap::new();

        // Allocate objects
        let r1 = heap
            .alloc_slots(vec![Value::I64(1), Value::I64(2)])
            .unwrap();
        let _r2 = heap
            .alloc_slots(vec![Value::I64(3), Value::I64(4)])
            .unwrap();
        let r3 = heap
            .alloc_slots(vec![Value::I64(5), Value::I64(6)])
            .unwrap();

        assert_eq!(heap.object_count(), 3);

        // GC with only r1 and r3 as roots (r2 becomes garbage)
        heap.collect(&[Value::Ref(r1), Value::Ref(r3)]);

        // r2 should have been freed and added to free list
        assert_eq!(heap.object_count(), 2);
        assert!(heap.free_list_head != 0); // Free list should not be empty

        // r1 and r3 should still be accessible
        assert_eq!(heap.get(r1).unwrap().slots[0], Value::I64(1));
        assert_eq!(heap.get(r3).unwrap().slots[0], Value::I64(5));
    }

    #[test]
    fn test_free_list_reuse() {
        let mut heap = Heap::new();

        // Allocate 3 objects of the same size (2 slots each)
        let r1 = heap
            .alloc_slots(vec![Value::I64(1), Value::I64(2)])
            .unwrap();
        let r2 = heap
            .alloc_slots(vec![Value::I64(3), Value::I64(4)])
            .unwrap();
        let r3 = heap
            .alloc_slots(vec![Value::I64(5), Value::I64(6)])
            .unwrap();

        let r2_offset = r2.offset();

        // GC with only r1 and r3 (r2 becomes garbage)
        heap.collect(&[Value::Ref(r1), Value::Ref(r3)]);

        // Allocate a new object of the same size
        let r4 = heap
            .alloc_slots(vec![Value::I64(7), Value::I64(8)])
            .unwrap();

        // r4 should reuse r2's memory
        assert_eq!(r4.offset(), r2_offset);

        // r4 should have correct values
        assert_eq!(heap.get(r4).unwrap().slots[0], Value::I64(7));
        assert_eq!(heap.get(r4).unwrap().slots[1], Value::I64(8));
    }

    #[test]
    fn test_free_list_block_splitting() {
        let mut heap = Heap::new();

        // Allocate a large object (5 slots)
        let r1 = heap
            .alloc_slots(vec![
                Value::I64(1),
                Value::I64(2),
                Value::I64(3),
                Value::I64(4),
                Value::I64(5),
            ])
            .unwrap();
        let r1_offset = r1.offset();

        // GC with no roots (r1 becomes garbage)
        heap.collect(&[]);

        // The 5-slot object takes 1 + 2*5 = 11 words
        // Allocate a smaller object (1 slot = 3 words)
        let r2 = heap.alloc_slots(vec![Value::I64(100)]).unwrap();

        // r2 should reuse r1's memory
        assert_eq!(r2.offset(), r1_offset);

        // There should be remaining free space (11 - 3 = 8 words)
        // which should be in the free list
        assert!(heap.free_list_head != 0);
    }

    #[test]
    fn test_multiple_gc_cycles() {
        let mut heap = Heap::new();

        // First cycle: allocate and collect
        let r1 = heap.alloc_slots(vec![Value::I64(1)]).unwrap();
        let _garbage1 = heap.alloc_slots(vec![Value::I64(2)]).unwrap();
        heap.collect(&[Value::Ref(r1)]);
        assert_eq!(heap.object_count(), 1);

        // Second cycle: allocate more and collect
        let r2 = heap.alloc_slots(vec![Value::I64(3)]).unwrap();
        let _garbage2 = heap.alloc_slots(vec![Value::I64(4)]).unwrap();
        heap.collect(&[Value::Ref(r1), Value::Ref(r2)]);
        assert_eq!(heap.object_count(), 2);

        // Both objects should still be valid
        assert_eq!(heap.get(r1).unwrap().slots[0], Value::I64(1));
        assert_eq!(heap.get(r2).unwrap().slots[0], Value::I64(3));
    }

    #[test]
    fn test_gc_all_garbage() {
        let mut heap = Heap::new();

        // Allocate some objects
        let _r1 = heap.alloc_slots(vec![Value::I64(1)]).unwrap();
        let _r2 = heap.alloc_slots(vec![Value::I64(2)]).unwrap();
        let _r3 = heap.alloc_slots(vec![Value::I64(3)]).unwrap();

        assert_eq!(heap.object_count(), 3);

        // GC with no roots - everything is garbage
        heap.collect(&[]);

        assert_eq!(heap.object_count(), 0);
        assert!(heap.free_list_head != 0); // Free list should have all the blocks
    }
}
