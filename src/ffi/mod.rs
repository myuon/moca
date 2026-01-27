//! C FFI for BCVM v0 Embed Mode
//!
//! This module provides a C-compatible API for embedding the moca VM
//! in host applications. All public functions use `extern "C"` ABI.
//!
//! # Safety
//!
//! All FFI functions that take raw pointers require:
//! - Non-null pointers (unless documented otherwise)
//! - Valid VM instances created by `moca_vm_new()`
//! - Proper lifetime management (VM must outlive all operations)

mod call;
mod error;
mod load;
mod stack;
mod types;
mod vm_ffi;

// Re-export all FFI types and functions for public use
#[allow(unused_imports)]
pub use call::*;
#[allow(unused_imports)]
pub use error::*;
#[allow(unused_imports)]
pub use load::*;
#[allow(unused_imports)]
pub use stack::*;
#[allow(unused_imports)]
pub use types::*;
#[allow(unused_imports)]
pub use vm_ffi::*;

/// Version information
pub const MOCA_VERSION_MAJOR: u32 = 0;
pub const MOCA_VERSION_MINOR: u32 = 1;
pub const MOCA_VERSION_PATCH: u32 = 0;

/// Get the version string
#[unsafe(no_mangle)]
pub extern "C" fn moca_version() -> *const std::ffi::c_char {
    static VERSION: &[u8] = b"0.1.0\0";
    VERSION.as_ptr() as *const std::ffi::c_char
}

/// Get the major version number
#[unsafe(no_mangle)]
pub extern "C" fn moca_version_major() -> u32 {
    MOCA_VERSION_MAJOR
}

/// Get the minor version number
#[unsafe(no_mangle)]
pub extern "C" fn moca_version_minor() -> u32 {
    MOCA_VERSION_MINOR
}

/// Get the patch version number
#[unsafe(no_mangle)]
pub extern "C" fn moca_version_patch() -> u32 {
    MOCA_VERSION_PATCH
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert_eq!(moca_version_major(), 0);
        assert_eq!(moca_version_minor(), 1);
        assert_eq!(moca_version_patch(), 0);

        let version = unsafe { std::ffi::CStr::from_ptr(moca_version()).to_str().unwrap() };
        assert_eq!(version, "0.1.0");
    }
}
