use std::fmt;

use super::Value;

// =============================================================================
// ElemKind - Element storage kind for heap objects
// =============================================================================

/// Describes how elements are stored within a heap object.
///
/// - `Tagged` (0): legacy 16B/slot format (tag u64 + payload u64). Used for structs, closures.
/// - `I64` (3): untagged 8B/element, no GC trace. For arrays of int, bool.
/// - `Ref` (4): untagged 8B/element, GC trace. For arrays of references.
/// - `F64` (5): untagged 8B/element, no GC trace. For arrays of float.
///
/// Values 1 (U8) and 2 (I32) are reserved for future 1B and 4B element support.
/// I64 and F64 have identical heap behavior (8B, no trace) but differ in
/// how the interpreter/JIT reconstructs the Value tag on load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ElemKind {
    Tagged = 0,
    // U8 = 1,  // reserved for future 1-byte elements
    // I32 = 2, // reserved for future 4-byte elements
    I64 = 3,
    Ref = 4,
    F64 = 5,
}

impl ElemKind {
    /// Decode from the 3-bit field stored in the header.
    fn from_bits(bits: u8) -> Self {
        match bits {
            3 => ElemKind::I64,
            4 => ElemKind::Ref,
            5 => ElemKind::F64,
            _ => ElemKind::Tagged,
        }
    }

    /// Decode from a raw u8 value (public version of from_bits).
    pub fn from_raw(raw: u8) -> Self {
        Self::from_bits(raw)
    }

    /// Whether this element kind requires GC tracing.
    pub fn needs_trace(self) -> bool {
        matches!(self, ElemKind::Ref)
    }

    /// Whether this is a typed (untagged) element kind.
    pub fn is_typed(self) -> bool {
        !matches!(self, ElemKind::Tagged)
    }
}

// =============================================================================
// Byte-level access helpers for Vec<u8> memory
// =============================================================================

/// Read a u64 from the byte buffer at the given byte offset (must be 8-byte aligned).
#[inline(always)]
fn read_u64(memory: &[u8], byte_offset: usize) -> u64 {
    let bytes: [u8; 8] = memory[byte_offset..byte_offset + 8]
        .try_into()
        .expect("aligned u64 read");
    u64::from_le_bytes(bytes)
}

/// Write a u64 to the byte buffer at the given byte offset (must be 8-byte aligned).
#[inline(always)]
fn write_u64(memory: &mut [u8], byte_offset: usize, value: u64) {
    memory[byte_offset..byte_offset + 8].copy_from_slice(&value.to_le_bytes());
}

/// Try to read a u64, returning None if out of bounds.
#[inline(always)]
fn try_read_u64(memory: &[u8], byte_offset: usize) -> Option<u64> {
    let slice = memory.get(byte_offset..byte_offset + 8)?;
    let bytes: [u8; 8] = slice.try_into().ok()?;
    Some(u64::from_le_bytes(bytes))
}

// =============================================================================
// Header Layout (64 bits)
// =============================================================================
//
// +--------+------+------------------+-----------+--------------------+
// | marked | free | count (32)       | elem_kind | reserved (27)      |
// | bit 63 | bit 62| bits 30-61      | bits 27-29| bits 0-26          |
// +--------+------+------------------+-----------+--------------------+
//
// - Bit 63: marked flag for GC
// - Bit 62: free flag (1 = free block in free list, 0 = allocated)
// - Bits 30-61: element/slot count (max 2^32 - 1)
// - Bits 27-29: ElemKind (0=Tagged, 3=I64, 4=Ref)
// - Bits 0-26: reserved for future use
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
const HEADER_ELEM_KIND_SHIFT: u32 = 27;
const HEADER_ELEM_KIND_MASK: u64 = 0b111 << HEADER_ELEM_KIND_SHIFT;

/// Encode a header word from marked flag, slot count, and element kind.
fn encode_header(marked: bool, slot_count: u32) -> u64 {
    encode_header_with_kind(marked, slot_count, ElemKind::Tagged)
}

/// Encode a header word with explicit element kind.
fn encode_header_with_kind(marked: bool, count: u32, kind: ElemKind) -> u64 {
    let mut header = (count as u64) << HEADER_SLOT_COUNT_SHIFT;
    header |= (kind as u64) << HEADER_ELEM_KIND_SHIFT;
    if marked {
        header |= HEADER_MARKED_BIT;
    }
    header
}

/// Encode a free block header.
fn encode_free_header(size_bytes: usize) -> u64 {
    // For free blocks, we store the size in bytes in the slot_count field.
    let size = size_bytes as u64;
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

/// Decode size in bytes from free block header.
fn decode_free_size_bytes(header: u64) -> usize {
    ((header & HEADER_SLOT_COUNT_MASK) >> HEADER_SLOT_COUNT_SHIFT) as usize
}

/// Decode element kind from header word.
fn decode_elem_kind(header: u64) -> ElemKind {
    let bits = ((header & HEADER_ELEM_KIND_MASK) >> HEADER_ELEM_KIND_SHIFT) as u8;
    ElemKind::from_bits(bits)
}

// =============================================================================
// Object Layout (in u64 words)
// =============================================================================
//
// Tagged (ElemKind::Tagged):
// +----------------+------+------+------+------+-----+
// | Header (1 word)| Tag0 | Val0 | Tag1 | Val1 | ... |
// +----------------+------+------+------+------+-----+
// Each slot is 2 words: tag + payload. Total = 1 + 2 * count
//
// Typed (ElemKind::I64 / ElemKind::Ref):
// +----------------+------+------+------+-----+
// | Header (1 word)| Val0 | Val1 | Val2 | ... |
// +----------------+------+------+------+-----+
// Each element is 1 word (raw payload, no tag). Total = 1 + count

/// Calculate the total size in words for a Tagged object with n slots.
const fn object_size_words(slot_count: u32) -> usize {
    1 + 2 * (slot_count as usize)
}

/// Calculate the total size in words for an object with the given elem kind.
const fn object_size_words_for_kind(count: u32, kind: ElemKind) -> usize {
    match kind {
        ElemKind::Tagged => 1 + 2 * (count as usize),
        ElemKind::I64 | ElemKind::Ref | ElemKind::F64 => 1 + (count as usize),
    }
}

/// Calculate the object size from a decoded header (in words).
fn object_size_from_header(header: u64) -> usize {
    let count = decode_slot_count(header);
    let kind = decode_elem_kind(header);
    object_size_words_for_kind(count, kind)
}

/// Calculate the total size in bytes for a Tagged object with n slots.
fn object_size_bytes(slot_count: u32) -> usize {
    object_size_words(slot_count) * 8
}

/// Calculate the total size in bytes for an object with the given elem kind.
fn object_size_bytes_for_kind(count: u32, kind: ElemKind) -> usize {
    object_size_words_for_kind(count, kind) * 8
}

/// Calculate the object size in bytes from a decoded header.
fn object_size_bytes_from_header(header: u64) -> usize {
    object_size_from_header(header) * 8
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

    /// Parse a HeapObject from linear memory at the given byte offset.
    ///
    /// # Arguments
    /// * `memory` - The linear memory buffer (byte-addressed)
    /// * `offset` - Byte offset where the object starts
    ///
    /// # Returns
    /// * `Some(HeapObject)` if the offset is valid and the object can be parsed
    /// * `None` if the offset is out of bounds or invalid
    pub fn from_memory(memory: &[u8], offset: usize) -> Option<Self> {
        // Read header
        let header = try_read_u64(memory, offset)?;
        let marked = decode_marked(header);
        let elem_kind = decode_elem_kind(header);
        let slot_count = decode_slot_count(header) as usize;

        // Check if we have enough memory for all slots
        let total_size_bytes = object_size_bytes_for_kind(slot_count as u32, elem_kind);
        if offset + total_size_bytes > memory.len() {
            return None;
        }

        // Read slots
        let mut slots = Vec::with_capacity(slot_count);
        match elem_kind {
            ElemKind::Tagged => {
                for i in 0..slot_count {
                    let tag_byte_offset = offset + 8 + 16 * i;
                    let tag = read_u64(memory, tag_byte_offset);
                    let payload = read_u64(memory, tag_byte_offset + 8);
                    let value = Value::decode(tag, payload)?;
                    slots.push(value);
                }
            }
            ElemKind::I64 => {
                for i in 0..slot_count {
                    slots.push(Value::I64(read_u64(memory, offset + 8 + i * 8) as i64));
                }
            }
            ElemKind::F64 => {
                for i in 0..slot_count {
                    slots.push(Value::F64(f64::from_bits(read_u64(
                        memory,
                        offset + 8 + i * 8,
                    ))));
                }
            }
            ElemKind::Ref => {
                for i in 0..slot_count {
                    slots.push(Value::Ref(GcRef {
                        index: read_u64(memory, offset + 8 + i * 8) as usize,
                    }));
                }
            }
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
/// The index field represents the byte offset into linear memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GcRef {
    /// Encodes both base offset and slot offset via bit-packing.
    /// Lower 40 bits: base offset in bytes (heap header position).
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

/// The garbage-collected heap using linear memory (Vec<u8>).
pub struct Heap {
    /// Linear memory buffer (byte-addressed)
    memory: Vec<u8>,
    /// Next allocation byte offset
    next_alloc: usize,
    /// Head of free list (byte offset, or 0 if empty)
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
    /// Initial capacity in bytes (1 MB)
    const INITIAL_CAPACITY: usize = 128 * 1024 * 8;

    pub fn new() -> Self {
        Self::new_with_config(None, true)
    }

    /// Create a new heap with custom configuration.
    ///
    /// # Arguments
    /// * `heap_limit` - Hard limit on heap size in bytes (None = unlimited)
    /// * `gc_enabled` - Whether GC is enabled
    pub fn new_with_config(heap_limit: Option<usize>, gc_enabled: bool) -> Self {
        let mut memory = vec![0u8; 8]; // Reserve first 8 bytes as invalid/null
        memory.reserve(Self::INITIAL_CAPACITY - 8);

        Self {
            memory,
            next_alloc: 8, // Start after reserved 8-byte null word
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
    pub fn memory_base_ptr(&self) -> *const u8 {
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
        let chars: Vec<i64> = value.chars().map(|c| c as i64).collect();
        let len = chars.len();
        // Allocate data as a typed I64 array (1 word per element, matching byte array layout)
        let data_ref = self.alloc_typed_array(len as u32, ElemKind::I64)?;
        for (i, &ch) in chars.iter().enumerate() {
            self.write_typed(data_ref, i, ch as u64)?;
        }
        let struct_slots = vec![Value::Ref(data_ref), Value::I64(len as i64)];
        self.alloc_slots(struct_slots)
    }

    /// Allocate a new slot-based heap object.
    pub fn alloc_slots(&mut self, slots: Vec<Value>) -> Result<GcRef, String> {
        let slot_count = slots.len() as u32;
        let obj_size_bytes = object_size_bytes(slot_count);

        self.check_heap_limit(obj_size_bytes)?;

        // Try to find a suitable free block (first-fit)
        let offset = if let Some(offset) = self.find_free_block(obj_size_bytes) {
            offset
        } else {
            // No suitable free block, allocate from bump pointer
            let required_len = self.next_alloc + obj_size_bytes;
            if required_len > self.memory.len() {
                self.memory
                    .resize(required_len.max(self.memory.len() * 2), 0);
            }

            let offset = self.next_alloc;
            self.next_alloc += obj_size_bytes;
            offset
        };

        self.bytes_allocated += obj_size_bytes;

        // Write header (not marked, not free)
        write_u64(&mut self.memory, offset, encode_header(false, slot_count));

        // Write slots
        for (i, value) in slots.iter().enumerate() {
            let (tag, payload) = value.encode();
            write_u64(&mut self.memory, offset + 8 + 16 * i, tag);
            write_u64(&mut self.memory, offset + 8 + 16 * i + 8, payload);
        }

        Ok(GcRef::from_offset(offset))
    }

    /// Allocate a typed array with `count` zero-initialized elements.
    ///
    /// For `ElemKind::I64` and `ElemKind::Ref`, each element occupies 8 bytes
    /// (no tag). This is 50% smaller than the tagged representation.
    pub fn alloc_typed_array(&mut self, count: u32, kind: ElemKind) -> Result<GcRef, String> {
        debug_assert!(
            kind.is_typed(),
            "alloc_typed_array only supports typed kinds (I64, F64, Ref)"
        );

        let obj_size_bytes = object_size_bytes_for_kind(count, kind);

        self.check_heap_limit(obj_size_bytes)?;

        let offset = if let Some(offset) = self.find_free_block(obj_size_bytes) {
            offset
        } else {
            let required_len = self.next_alloc + obj_size_bytes;
            if required_len > self.memory.len() {
                self.memory
                    .resize(required_len.max(self.memory.len() * 2), 0);
            }
            let offset = self.next_alloc;
            self.next_alloc += obj_size_bytes;
            offset
        };

        self.bytes_allocated += obj_size_bytes;

        // Write header with elem_kind
        write_u64(
            &mut self.memory,
            offset,
            encode_header_with_kind(false, count, kind),
        );

        // Zero-initialize elements (already 0 from resize, but be explicit for reused blocks)
        for i in 0..count as usize {
            write_u64(&mut self.memory, offset + 8 + i * 8, 0);
        }

        Ok(GcRef::from_offset(offset))
    }

    /// Get the ElemKind of the object at the given reference.
    pub fn get_elem_kind(&self, r: GcRef) -> ElemKind {
        if !r.is_valid() {
            return ElemKind::Tagged;
        }
        let offset = r.base();
        match try_read_u64(&self.memory, offset) {
            Some(header) => decode_elem_kind(header),
            None => ElemKind::Tagged,
        }
    }

    /// Read a single element from a typed array (ElemKind::I64 or ElemKind::Ref).
    /// Returns the raw u64 payload without tag.
    pub fn read_typed(&self, r: GcRef, index: usize) -> Option<u64> {
        if !r.is_valid() {
            return None;
        }
        let actual_index = index + r.slot_offset();
        let offset = r.base();
        let header = try_read_u64(&self.memory, offset)?;
        let count = decode_slot_count(header) as usize;

        if actual_index >= count {
            return None;
        }

        try_read_u64(&self.memory, offset + 8 + actual_index * 8)
    }

    /// Write a single element to a typed array (ElemKind::I64 or ElemKind::Ref).
    /// Stores the raw u64 payload without tag.
    pub fn write_typed(&mut self, r: GcRef, index: usize, value: u64) -> Result<(), String> {
        if !r.is_valid() {
            return Err("invalid reference".to_string());
        }
        let actual_index = index + r.slot_offset();
        let offset = r.base();
        let header =
            try_read_u64(&self.memory, offset).ok_or("invalid reference: out of bounds")?;
        let count = decode_slot_count(header) as usize;

        if actual_index >= count {
            return Err(format!(
                "typed array index {} out of bounds (count: {})",
                actual_index, count
            ));
        }

        write_u64(&mut self.memory, offset + 8 + actual_index * 8, value);
        Ok(())
    }

    /// Find a free block of at least the given size in bytes (first-fit).
    /// If found, removes it from the free list and returns its byte offset.
    /// May split the block if it's larger than needed.
    fn find_free_block(&mut self, needed_bytes: usize) -> Option<usize> {
        // Minimum free block size: header + next pointer = 16 bytes
        const MIN_FREE_BLOCK_SIZE: usize = 16;

        let mut prev_offset: Option<usize> = None;
        let mut current = self.free_list_head;

        while current != 0 {
            let header = read_u64(&self.memory, current);
            let block_size = decode_free_size_bytes(header);
            let next = read_u64(&self.memory, current + 8) as usize;

            if block_size >= needed_bytes {
                // Found a suitable block
                // Remove from free list
                if let Some(prev) = prev_offset {
                    write_u64(&mut self.memory, prev + 8, next as u64);
                } else {
                    self.free_list_head = next;
                }

                // Check if we should split the block
                let remaining = block_size - needed_bytes;
                if remaining >= MIN_FREE_BLOCK_SIZE {
                    // Split: create a new free block for the remainder
                    let new_free_offset = current + needed_bytes;
                    write_u64(
                        &mut self.memory,
                        new_free_offset,
                        encode_free_header(remaining),
                    );
                    write_u64(
                        &mut self.memory,
                        new_free_offset + 8,
                        self.free_list_head as u64,
                    );
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
    fn add_to_free_list(&mut self, offset: usize, size_bytes: usize) {
        // Write free block header
        write_u64(&mut self.memory, offset, encode_free_header(size_bytes));
        // Link to current head
        write_u64(&mut self.memory, offset + 8, self.free_list_head as u64);
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
    /// Automatically detects typed arrays from the header and uses the appropriate layout.
    pub fn read_slot(&self, r: GcRef, slot_index: usize) -> Option<Value> {
        if !r.is_valid() {
            return None;
        }

        let actual_slot = slot_index + r.slot_offset();
        let offset = r.base();
        let header = try_read_u64(&self.memory, offset)?;
        let elem_kind = decode_elem_kind(header);
        let slot_count = decode_slot_count(header) as usize;

        if actual_slot >= slot_count {
            return None;
        }

        match elem_kind {
            ElemKind::Tagged => {
                let tag_byte_offset = offset + 8 + 16 * actual_slot;
                let tag = try_read_u64(&self.memory, tag_byte_offset)?;
                let payload = try_read_u64(&self.memory, tag_byte_offset + 8)?;
                Value::decode(tag, payload)
            }
            ElemKind::I64 => {
                let raw = try_read_u64(&self.memory, offset + 8 + actual_slot * 8)?;
                Some(Value::I64(raw as i64))
            }
            ElemKind::F64 => {
                let raw = try_read_u64(&self.memory, offset + 8 + actual_slot * 8)?;
                Some(Value::F64(f64::from_bits(raw)))
            }
            ElemKind::Ref => {
                let raw = try_read_u64(&self.memory, offset + 8 + actual_slot * 8)?;
                Some(Value::Ref(GcRef {
                    index: raw as usize,
                }))
            }
        }
    }

    /// Write a single slot to an object.
    /// Automatically detects typed arrays from the header and uses the appropriate layout.
    pub fn write_slot(&mut self, r: GcRef, slot_index: usize, value: Value) -> Result<(), String> {
        if !r.is_valid() {
            return Err("invalid reference".to_string());
        }

        let actual_slot = slot_index + r.slot_offset();
        let offset = r.base();
        let header =
            try_read_u64(&self.memory, offset).ok_or("invalid reference: out of bounds")?;
        let elem_kind = decode_elem_kind(header);
        let slot_count = decode_slot_count(header) as usize;

        if actual_slot >= slot_count {
            return Err(format!(
                "slot index {} out of bounds (count: {})",
                actual_slot, slot_count
            ));
        }

        match elem_kind {
            ElemKind::Tagged => {
                let tag_byte_offset = offset + 8 + 16 * actual_slot;
                let (tag, payload) = value.encode();
                write_u64(&mut self.memory, tag_byte_offset, tag);
                write_u64(&mut self.memory, tag_byte_offset + 8, payload);
            }
            ElemKind::I64 | ElemKind::F64 => {
                let raw = match value {
                    Value::I64(v) => v as u64,
                    Value::F64(v) => v.to_bits(),
                    _ => value.encode().1,
                };
                write_u64(&mut self.memory, offset + 8 + actual_slot * 8, raw);
            }
            ElemKind::Ref => {
                let raw = match value {
                    Value::Ref(r) => r.index as u64,
                    Value::Null => 0,
                    _ => value.encode().1,
                };
                write_u64(&mut self.memory, offset + 8 + actual_slot * 8, raw);
            }
        }
        Ok(())
    }

    /// Get the slot count for an object.
    pub fn slot_count(&self, r: GcRef) -> Option<usize> {
        if !r.is_valid() {
            return None;
        }
        let header = try_read_u64(&self.memory, r.offset())?;
        Some(decode_slot_count(header) as usize)
    }

    /// Check if an offset could be a valid allocated object start.
    /// Used for conservative GC stack scanning in the typed opcode architecture
    /// where stack values are raw u64 and type information is in opcodes.
    pub fn is_possible_object_ref(&self, offset: usize) -> bool {
        if offset == 0 || offset >= self.next_alloc {
            return false;
        }
        if let Some(header) = try_read_u64(&self.memory, offset) {
            if decode_free(header) {
                return false;
            }
            let obj_size = object_size_bytes_from_header(header);
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
        if let Some(header) = try_read_u64(&self.memory, offset) {
            let new_header = if marked {
                header | HEADER_MARKED_BIT
            } else {
                header & !HEADER_MARKED_BIT
            };
            write_u64(&mut self.memory, offset, new_header);
        }
    }

    /// Get the marked flag for an object.
    fn is_marked(&self, offset: usize) -> bool {
        try_read_u64(&self.memory, offset)
            .map(decode_marked)
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

            // Trace children based on elem_kind
            let header = match try_read_u64(&self.memory, offset) {
                Some(h) => h,
                None => continue,
            };
            let kind = decode_elem_kind(header);
            let count = decode_slot_count(header) as usize;

            match kind {
                ElemKind::Tagged => {
                    // Legacy: scan tagged slots for Ref values
                    if let Some(obj) = HeapObject::from_memory(&self.memory, offset) {
                        worklist.extend(obj.trace());
                    }
                }
                ElemKind::I64 | ElemKind::F64 => {
                    // Primitive-only array: no references to trace
                }
                ElemKind::Ref => {
                    // All elements are references: trace each one
                    for i in 0..count {
                        let byte_off = offset + 8 + i * 8;
                        if let Some(payload) = try_read_u64(&self.memory, byte_off) {
                            let child = GcRef {
                                index: payload as usize,
                            };
                            if child.is_valid() {
                                worklist.push(child);
                            }
                        }
                    }
                }
            }
        }
    }

    /// Sweep phase: free all unmarked objects by adding them to the free list.
    pub fn sweep(&mut self) {
        // Walk through all allocated objects
        let mut offset = 8; // Start after reserved 8-byte null word
        let mut live_bytes = 0;

        while offset < self.next_alloc {
            let header = read_u64(&self.memory, offset);

            // Skip free blocks (already in free list)
            if decode_free(header) {
                let block_size = decode_free_size_bytes(header);
                offset += block_size;
                continue;
            }

            let obj_size = object_size_bytes_from_header(header);

            if decode_marked(header) {
                // Live object - reset mark for next GC cycle
                self.set_marked(offset, false);
                live_bytes += obj_size;
            } else {
                // Dead object - add to free list if large enough.
                // Free blocks need at least 16 bytes (header + next pointer).
                // 8-byte objects (slot_count=0) cannot hold a next pointer,
                // so adding them would corrupt adjacent memory.
                if obj_size >= 16 {
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
        let mut offset = 8;

        while offset < self.next_alloc {
            let header = read_u64(&self.memory, offset);

            // Skip free blocks
            if decode_free(header) {
                let block_size = decode_free_size_bytes(header);
                offset += block_size;
                continue;
            }

            count += 1;
            offset += object_size_bytes_from_header(header);
        }

        count
    }

    /// Get raw memory for testing/debugging.
    #[cfg(test)]
    pub fn memory(&self) -> &[u8] {
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
        // String struct: [ptr, len]
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
        // Test free block header encoding/decoding (now in bytes)
        let h = encode_free_header(80);
        assert!(decode_free(h));
        assert!(!decode_marked(h));
        assert_eq!(decode_free_size_bytes(h), 80);

        let h2 = encode_free_header(8000);
        assert!(decode_free(h2));
        assert_eq!(decode_free_size_bytes(h2), 8000);
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

    // =========================================================================
    // Typed Array Tests (ElemKind::I64 / ElemKind::Ref)
    // =========================================================================

    #[test]
    fn test_typed_header_encoding() {
        let h = encode_header_with_kind(false, 10, ElemKind::I64);
        assert!(!decode_marked(h));
        assert!(!decode_free(h));
        assert_eq!(decode_slot_count(h), 10);
        assert_eq!(decode_elem_kind(h), ElemKind::I64);

        let h2 = encode_header_with_kind(true, 5, ElemKind::Ref);
        assert!(decode_marked(h2));
        assert_eq!(decode_slot_count(h2), 5);
        assert_eq!(decode_elem_kind(h2), ElemKind::Ref);

        // Tagged (legacy) should decode as Tagged
        let h3 = encode_header(false, 3);
        assert_eq!(decode_elem_kind(h3), ElemKind::Tagged);
    }

    #[test]
    fn test_typed_object_size() {
        // Tagged: 1 + 2*count
        assert_eq!(object_size_words_for_kind(3, ElemKind::Tagged), 7);
        // I64: 1 + count
        assert_eq!(object_size_words_for_kind(3, ElemKind::I64), 4);
        // Ref: 1 + count
        assert_eq!(object_size_words_for_kind(3, ElemKind::Ref), 4);
    }

    #[test]
    fn test_alloc_typed_i64() {
        let mut heap = Heap::new();
        let r = heap.alloc_typed_array(3, ElemKind::I64).unwrap();

        // All elements should be zero-initialized
        assert_eq!(heap.read_typed(r, 0), Some(0));
        assert_eq!(heap.read_typed(r, 1), Some(0));
        assert_eq!(heap.read_typed(r, 2), Some(0));
        assert_eq!(heap.read_typed(r, 3), None); // out of bounds

        // Write and read back
        heap.write_typed(r, 0, 42).unwrap();
        heap.write_typed(r, 1, u64::MAX).unwrap();
        heap.write_typed(r, 2, 123).unwrap();

        assert_eq!(heap.read_typed(r, 0), Some(42));
        assert_eq!(heap.read_typed(r, 1), Some(u64::MAX));
        assert_eq!(heap.read_typed(r, 2), Some(123));

        // Write out of bounds should fail
        assert!(heap.write_typed(r, 3, 0).is_err());
    }

    #[test]
    fn test_typed_i64_memory_layout() {
        let mut heap = Heap::new();
        let r = heap.alloc_typed_array(3, ElemKind::I64).unwrap();
        heap.write_typed(r, 0, 10).unwrap();
        heap.write_typed(r, 1, 20).unwrap();
        heap.write_typed(r, 2, 30).unwrap();

        // Typed array should use 1 + 3 = 4 words = 32 bytes (vs Tagged: 1 + 2*3 = 7 words = 56 bytes)
        let offset = r.offset();
        let header = read_u64(heap.memory(), offset);
        assert_eq!(decode_slot_count(header), 3);
        assert_eq!(decode_elem_kind(header), ElemKind::I64);

        // Elements stored directly (no tags), each at 8-byte stride
        assert_eq!(read_u64(heap.memory(), offset + 8), 10);
        assert_eq!(read_u64(heap.memory(), offset + 16), 20);
        assert_eq!(read_u64(heap.memory(), offset + 24), 30);
    }

    #[test]
    fn test_gc_typed_i64_no_trace() {
        let mut heap = Heap::new();

        // Allocate a typed I64 array
        let r = heap.alloc_typed_array(3, ElemKind::I64).unwrap();
        heap.write_typed(r, 0, 100).unwrap();

        // Allocate a tagged object that should be garbage
        let _garbage = heap.alloc_slots(vec![Value::I64(999)]).unwrap();

        // GC: only typed array is root
        heap.collect(&[Value::Ref(r)]);

        // Typed array survives
        assert_eq!(heap.read_typed(r, 0), Some(100));
        assert_eq!(heap.object_count(), 1);
    }

    #[test]
    fn test_gc_typed_ref_traces() {
        let mut heap = Heap::new();

        // Allocate a child object
        let child = heap.alloc_slots(vec![Value::I64(42)]).unwrap();

        // Allocate a typed Ref array containing the child
        let r = heap.alloc_typed_array(2, ElemKind::Ref).unwrap();
        heap.write_typed(r, 0, child.index as u64).unwrap();

        // GC: only the Ref array is root, but child should be traced
        heap.collect(&[Value::Ref(r)]);

        // Both should survive
        assert_eq!(heap.object_count(), 2);
        assert_eq!(heap.get(child).unwrap().slots[0], Value::I64(42));
    }

    #[test]
    fn test_gc_typed_ref_collects_unreachable() {
        let mut heap = Heap::new();

        // Allocate an unreachable object
        let _garbage = heap.alloc_slots(vec![Value::I64(1)]).unwrap();

        // Allocate a typed Ref array with a null reference
        let r = heap.alloc_typed_array(1, ElemKind::Ref).unwrap();
        heap.write_typed(r, 0, 0).unwrap(); // null ref

        heap.collect(&[Value::Ref(r)]);

        // Only the Ref array survives
        assert_eq!(heap.object_count(), 1);
    }

    #[test]
    fn test_typed_array_mixed_with_tagged() {
        let mut heap = Heap::new();

        // Mix tagged and typed objects
        let tagged = heap
            .alloc_slots(vec![Value::I64(1), Value::I64(2)])
            .unwrap();
        let typed = heap.alloc_typed_array(3, ElemKind::I64).unwrap();
        heap.write_typed(typed, 0, 10).unwrap();
        heap.write_typed(typed, 1, 20).unwrap();
        heap.write_typed(typed, 2, 30).unwrap();

        assert_eq!(heap.object_count(), 2);

        // Both should survive GC
        heap.collect(&[Value::Ref(tagged), Value::Ref(typed)]);
        assert_eq!(heap.object_count(), 2);

        // Both should be readable
        assert_eq!(heap.read_slot(tagged, 0), Some(Value::I64(1)));
        assert_eq!(heap.read_typed(typed, 0), Some(10));
        assert_eq!(heap.read_typed(typed, 2), Some(30));
    }

    #[test]
    fn test_typed_array_free_list_reuse() {
        let mut heap = Heap::new();

        // Allocate and free a typed array
        let r1 = heap.alloc_typed_array(3, ElemKind::I64).unwrap();
        let r1_offset = r1.offset();
        heap.collect(&[]); // free it

        // Allocate another of the same size - should reuse
        let r2 = heap.alloc_typed_array(3, ElemKind::I64).unwrap();
        assert_eq!(r2.offset(), r1_offset);
    }

    #[test]
    fn test_slot_count_typed() {
        let mut heap = Heap::new();
        let r = heap.alloc_typed_array(5, ElemKind::I64).unwrap();
        assert_eq!(heap.slot_count(r), Some(5));
    }
}
