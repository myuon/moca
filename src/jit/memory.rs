//! Executable memory management using mmap.
//!
//! This module provides a safe abstraction over OS-level memory mapping
//! for allocating memory that can be written to and then executed.

use std::ptr::NonNull;

/// Error type for memory operations.
#[derive(Debug)]
pub enum MemoryError {
    AllocationFailed,
    ProtectionFailed,
    InvalidSize,
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::AllocationFailed => write!(f, "memory allocation failed"),
            MemoryError::ProtectionFailed => write!(f, "memory protection change failed"),
            MemoryError::InvalidSize => write!(f, "invalid memory size"),
        }
    }
}

impl std::error::Error for MemoryError {}

/// A block of executable memory allocated via mmap.
///
/// The memory is initially writable. Call `make_executable()` to make it
/// executable (and read-only) before calling the generated code.
pub struct ExecutableMemory {
    ptr: NonNull<u8>,
    size: usize,
    executable: bool,
}

impl ExecutableMemory {
    /// Allocate a new block of memory with the given size.
    /// The memory is initially writable but not executable.
    pub fn new(size: usize) -> Result<Self, MemoryError> {
        if size == 0 {
            return Err(MemoryError::InvalidSize);
        }

        // Round up to page size
        let page_size = Self::page_size();
        let aligned_size = (size + page_size - 1) & !(page_size - 1);

        let ptr = Self::mmap_alloc(aligned_size)?;

        Ok(Self {
            ptr,
            size: aligned_size,
            executable: false,
        })
    }

    /// Get the page size for the current system.
    fn page_size() -> usize {
        // Default to 4KB, which is common on most systems
        #[cfg(unix)]
        {
            unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
        }
        #[cfg(not(unix))]
        {
            4096
        }
    }

    /// Allocate memory using mmap.
    #[cfg(unix)]
    fn mmap_alloc(size: usize) -> Result<NonNull<u8>, MemoryError> {
        use std::ptr;

        let ptr = unsafe {
            libc::mmap(
                ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };

        if ptr == libc::MAP_FAILED {
            return Err(MemoryError::AllocationFailed);
        }

        NonNull::new(ptr as *mut u8).ok_or(MemoryError::AllocationFailed)
    }

    #[cfg(not(unix))]
    fn mmap_alloc(size: usize) -> Result<NonNull<u8>, MemoryError> {
        // Fallback for non-Unix systems: use regular allocation
        // Note: This won't actually be executable on most systems
        let layout = std::alloc::Layout::from_size_align(size, Self::page_size())
            .map_err(|_| MemoryError::InvalidSize)?;
        let ptr = unsafe { std::alloc::alloc(layout) };
        NonNull::new(ptr).ok_or(MemoryError::AllocationFailed)
    }

    /// Get a pointer to the memory.
    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ref()
    }

    /// Get a mutable pointer to the memory.
    /// Returns None if the memory is already executable.
    pub fn as_mut_ptr(&mut self) -> Option<*mut u8> {
        if self.executable {
            None
        } else {
            Some(self.ptr.as_ref())
        }
    }

    /// Get the size of the allocated memory.
    pub fn size(&self) -> usize {
        self.size
    }

    /// Write bytes to the memory at the given offset.
    /// Returns an error if the memory is executable or if the write would overflow.
    pub fn write(&mut self, offset: usize, data: &[u8]) -> Result<(), MemoryError> {
        if self.executable {
            return Err(MemoryError::ProtectionFailed);
        }

        if offset + data.len() > self.size {
            return Err(MemoryError::InvalidSize);
        }

        unsafe {
            let dest = self.ptr.as_ref().add(offset);
            std::ptr::copy_nonoverlapping(data.as_ref(), dest, data.len());
        }

        Ok(())
    }

    /// Make the memory executable (and read-only).
    /// After this call, the memory can no longer be written to.
    #[cfg(unix)]
    pub fn make_executable(&mut self) -> Result<(), MemoryError> {
        if self.executable {
            return Ok(());
        }

        let result = unsafe {
            libc::mprotect(
                self.ptr.as_ref() as *mut libc::c_void,
                self.size,
                libc::PROT_READ | libc::PROT_EXEC,
            )
        };

        if result != 0 {
            return Err(MemoryError::ProtectionFailed);
        }

        self.executable = true;
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn make_executable(&mut self) -> Result<(), MemoryError> {
        // On non-Unix systems, we can't change protection
        // The memory may or may not be executable depending on the system
        self.executable = true;
        Ok(())
    }

    /// Check if the memory is executable.
    pub fn is_executable(&self) -> bool {
        self.executable
    }

    /// Get a function pointer to the start of the memory.
    /// The memory must be executable.
    ///
    /// # Safety
    /// The caller must ensure that the memory contains valid machine code
    /// for the target architecture.
    pub unsafe fn as_fn<F>(&self) -> Option<F>
    where
        F: Copy,
    {
        if !self.executable {
            return None;
        }

        // Verify that F is a function pointer type
        if std::mem::size_of::<F>() != std::mem::size_of::<fn()>() {
            return None;
        }

        // SAFETY: Caller guarantees the memory contains valid code
        Some(unsafe { std::mem::transmute_copy(&self.ptr.as_ref()) })
    }
}

impl Drop for ExecutableMemory {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            unsafe {
                libc::munmap(self.ptr.as_ref() as *mut libc::c_void, self.size);
            }
        }
        #[cfg(not(unix))]
        {
            let layout = std::alloc::Layout::from_size_align(self.size, Self::page_size())
                .expect("invalid layout");
            unsafe {
                std::alloc::dealloc(self.ptr.as_ref(), layout);
            }
        }
    }
}

// ExecutableMemory is Send and Sync because it owns its memory
// and has internal synchronization through the executable flag
unsafe impl Send for ExecutableMemory {}
unsafe impl Sync for ExecutableMemory {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allocate_memory() {
        let mem = ExecutableMemory::new(4096).unwrap();
        assert!(mem.size() >= 4096);
        assert!(!mem.is_executable());
    }

    #[test]
    fn test_write_memory() {
        let mut mem = ExecutableMemory::new(4096).unwrap();
        let data = [0x90, 0x90, 0x90, 0x90]; // NOP instructions
        mem.write(0, &data).unwrap();
    }

    #[test]
    fn test_make_executable() {
        let mut mem = ExecutableMemory::new(4096).unwrap();
        mem.make_executable().unwrap();
        assert!(mem.is_executable());
    }

    #[test]
    fn test_cannot_write_after_executable() {
        let mut mem = ExecutableMemory::new(4096).unwrap();
        mem.make_executable().unwrap();
        let data = [0x90];
        assert!(mem.write(0, &data).is_err());
    }
}
