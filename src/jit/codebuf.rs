//! Code buffer for building JIT code.
//!
//! This module provides a buffer for incrementally building machine code
//! before copying it to executable memory.

use super::memory::{ExecutableMemory, MemoryError};

/// A buffer for building machine code.
pub struct CodeBuffer {
    /// The code bytes
    code: Vec<u8>,
    /// Labels for forward references (name -> offset)
    labels: std::collections::HashMap<String, usize>,
    /// Pending forward references (offset, label_name, reference_size)
    forward_refs: Vec<(usize, String, ReferenceSize)>,
}

/// Size of a reference to patch.
#[derive(Clone, Copy)]
pub enum ReferenceSize {
    /// 32-bit relative offset
    Rel32,
    /// 26-bit relative offset for AArch64 branch
    AArch64Branch,
}

impl CodeBuffer {
    /// Create a new empty code buffer.
    pub fn new() -> Self {
        Self {
            code: Vec::new(),
            labels: std::collections::HashMap::new(),
            forward_refs: Vec::new(),
        }
    }

    /// Create a new code buffer with pre-allocated capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            code: Vec::with_capacity(capacity),
            labels: std::collections::HashMap::new(),
            forward_refs: Vec::new(),
        }
    }

    /// Get the current size of the code.
    pub fn len(&self) -> usize {
        self.code.len()
    }

    /// Check if the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.code.is_empty()
    }

    /// Get the current offset (for labels).
    pub fn offset(&self) -> usize {
        self.code.len()
    }

    /// Emit a single byte.
    pub fn emit_u8(&mut self, byte: u8) {
        self.code.push(byte);
    }

    /// Emit a 16-bit value (little-endian).
    pub fn emit_u16(&mut self, value: u16) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 32-bit value (little-endian).
    pub fn emit_u32(&mut self, value: u32) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit a 64-bit value (little-endian).
    pub fn emit_u64(&mut self, value: u64) {
        self.code.extend_from_slice(&value.to_le_bytes());
    }

    /// Emit multiple bytes.
    pub fn emit_bytes(&mut self, bytes: &[u8]) {
        self.code.extend_from_slice(bytes);
    }

    /// Define a label at the current position.
    pub fn define_label(&mut self, name: &str) {
        self.labels.insert(name.to_string(), self.code.len());
    }

    /// Get the offset of a label (if defined).
    pub fn get_label(&self, name: &str) -> Option<usize> {
        self.labels.get(name).copied()
    }

    /// Emit a forward reference to a label.
    /// The reference will be patched when `patch_forward_refs` is called.
    pub fn emit_forward_ref(&mut self, label: &str, size: ReferenceSize) {
        let offset = self.code.len();
        self.forward_refs.push((offset, label.to_string(), size));

        // Emit placeholder bytes
        match size {
            ReferenceSize::Rel32 => self.emit_u32(0),
            ReferenceSize::AArch64Branch => self.emit_u32(0),
        }
    }

    /// Patch all forward references.
    /// Returns an error if any label is undefined.
    pub fn patch_forward_refs(&mut self) -> Result<(), String> {
        for (offset, label, size) in self.forward_refs.drain(..) {
            let target = self.labels.get(&label)
                .ok_or_else(|| format!("undefined label: {}", label))?;

            match size {
                ReferenceSize::Rel32 => {
                    // Calculate relative offset from end of reference
                    let rel_offset = (*target as i64) - (offset as i64 + 4);
                    if rel_offset < i32::MIN as i64 || rel_offset > i32::MAX as i64 {
                        return Err(format!("relative offset out of range for label: {}", label));
                    }
                    let bytes = (rel_offset as i32).to_le_bytes();
                    self.code[offset..offset + 4].copy_from_slice(&bytes);
                }
                ReferenceSize::AArch64Branch => {
                    // AArch64 branch encoding: offset is in instructions (4-byte units)
                    let rel_offset = ((*target as i64) - (offset as i64)) / 4;
                    if !(-(1 << 25)..(1 << 25)).contains(&rel_offset) {
                        return Err(format!("branch offset out of range for label: {}", label));
                    }
                    // Branch instruction: bits 25:0 are the offset
                    let current = u32::from_le_bytes([
                        self.code[offset],
                        self.code[offset + 1],
                        self.code[offset + 2],
                        self.code[offset + 3],
                    ]);
                    let new_inst = (current & 0xFC000000) | ((rel_offset as u32) & 0x03FFFFFF);
                    self.code[offset..offset + 4].copy_from_slice(&new_inst.to_le_bytes());
                }
            }
        }
        Ok(())
    }

    /// Finalize the code buffer and copy to executable memory.
    pub fn finalize(mut self) -> Result<ExecutableMemory, MemoryError> {
        // Patch forward references
        self.patch_forward_refs().map_err(|_| MemoryError::InvalidSize)?;

        // Allocate executable memory
        let mut mem = ExecutableMemory::new(self.code.len())?;
        mem.write(0, &self.code)?;
        mem.make_executable()?;

        Ok(mem)
    }

    /// Get the code bytes (for inspection).
    pub fn code(&self) -> &[u8] {
        &self.code
    }

    /// Get mutable access to the code bytes (for patching).
    pub fn code_mut(&mut self) -> &mut [u8] {
        &mut self.code
    }

    /// Consume the buffer and return the raw code bytes.
    /// Note: This does not patch forward references - use patch_forward_refs first.
    pub fn into_code(self) -> Vec<u8> {
        self.code
    }

    /// Align the code to the given boundary.
    pub fn align(&mut self, alignment: usize) {
        let current = self.code.len();
        let aligned = (current + alignment - 1) & !(alignment - 1);
        let padding = aligned - current;
        for _ in 0..padding {
            self.emit_u8(0x00); // NOP or padding
        }
    }
}

impl Default for CodeBuffer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_bytes() {
        let mut buf = CodeBuffer::new();
        buf.emit_u8(0x90);
        buf.emit_u16(0x1234);
        buf.emit_u32(0xDEADBEEF);

        assert_eq!(buf.len(), 7);
        assert_eq!(buf.code(), &[0x90, 0x34, 0x12, 0xEF, 0xBE, 0xAD, 0xDE]);
    }

    #[test]
    fn test_labels() {
        let mut buf = CodeBuffer::new();
        buf.emit_u8(0x90);
        buf.define_label("test");
        buf.emit_u8(0x90);

        assert_eq!(buf.get_label("test"), Some(1));
    }

    #[test]
    fn test_alignment() {
        let mut buf = CodeBuffer::new();
        buf.emit_u8(0x90);
        buf.align(4);

        assert_eq!(buf.len(), 4);
    }
}
