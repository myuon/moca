//! FFI type definitions for the C API.

// Allow dead code for FFI types that will be exposed to C
#![allow(dead_code)]

use std::ffi::c_char;

/// Result codes for FFI operations.
///
/// These map to the `mica_result` enum in C.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicaResult {
    /// Operation succeeded
    Ok = 0,
    /// Runtime error during execution
    ErrorRuntime = 1,
    /// Type mismatch error
    ErrorType = 2,
    /// Bytecode verification failed
    ErrorVerify = 3,
    /// Out of memory
    ErrorMemory = 4,
    /// Invalid argument passed to function
    ErrorInvalidArg = 5,
    /// Function or global not found
    ErrorNotFound = 6,
}

impl MicaResult {
    pub fn is_ok(self) -> bool {
        self == MicaResult::Ok
    }

    pub fn is_err(self) -> bool {
        self != MicaResult::Ok
    }
}

impl From<Result<(), String>> for MicaResult {
    fn from(result: Result<(), String>) -> Self {
        match result {
            Ok(()) => MicaResult::Ok,
            Err(_) => MicaResult::ErrorRuntime,
        }
    }
}

/// Error callback function type.
///
/// Called when an error occurs, with the error message and user data.
pub type MicaErrorFn = Option<unsafe extern "C" fn(message: *const c_char, userdata: *mut std::ffi::c_void)>;

/// Host function type.
///
/// A C function that can be registered and called from mica code.
/// The function receives the VM instance and should:
/// 1. Read arguments from the stack using `mica_to_*` functions
/// 2. Perform the operation
/// 3. Push the result using `mica_push_*` functions
/// 4. Return `MICA_OK` or an error code
pub type MicaCFunc = unsafe extern "C" fn(vm: *mut MicaVm) -> MicaResult;

/// Opaque VM instance type.
///
/// This is the main entry point for the FFI. All operations require
/// a valid `MicaVm` pointer created by `mica_vm_new()`.
#[repr(C)]
pub struct MicaVm {
    _private: [u8; 0],
}

/// Internal VM wrapper that holds the actual Rust VM and FFI state.
pub(crate) struct VmWrapper {
    /// The actual mica VM
    pub vm: crate::vm::VM,
    /// Loaded chunk (if any)
    pub chunk: Option<crate::vm::Chunk>,
    /// Last error message (as CString for FFI compatibility)
    pub last_error: Option<std::ffi::CString>,
    /// Error callback
    pub error_callback: MicaErrorFn,
    /// Error callback userdata
    pub error_userdata: *mut std::ffi::c_void,
    /// Registered host functions
    pub host_functions: std::collections::HashMap<String, HostFunction>,
    /// FFI stack for passing values between host and VM
    pub ffi_stack: Vec<crate::vm::Value>,
    /// Global variables accessible via FFI
    pub globals: std::collections::HashMap<String, crate::vm::Value>,
}

/// A registered host function.
pub(crate) struct HostFunction {
    pub func: MicaCFunc,
    pub arity: usize,
}

impl VmWrapper {
    pub fn new() -> Self {
        Self {
            vm: crate::vm::VM::new(),
            chunk: None,
            last_error: None,
            error_callback: None,
            error_userdata: std::ptr::null_mut(),
            host_functions: std::collections::HashMap::new(),
            ffi_stack: Vec::with_capacity(64),
            globals: std::collections::HashMap::new(),
        }
    }

    /// Set an error message and optionally call the error callback.
    pub fn set_error(&mut self, message: impl Into<String>) {
        let msg = message.into();
        let c_msg = std::ffi::CString::new(msg).unwrap_or_default();

        // Call error callback if set
        if let Some(callback) = self.error_callback {
            unsafe {
                callback(c_msg.as_ptr(), self.error_userdata);
            }
        }

        self.last_error = Some(c_msg);
    }

    /// Clear the last error.
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
}

impl Default for VmWrapper {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_result_codes() {
        assert!(MicaResult::Ok.is_ok());
        assert!(!MicaResult::Ok.is_err());
        assert!(!MicaResult::ErrorRuntime.is_ok());
        assert!(MicaResult::ErrorRuntime.is_err());
    }

    #[test]
    fn test_vm_wrapper_creation() {
        let wrapper = VmWrapper::new();
        assert!(wrapper.chunk.is_none());
        assert!(wrapper.last_error.is_none());
        assert!(wrapper.host_functions.is_empty());
    }

    #[test]
    fn test_vm_wrapper_error() {
        let mut wrapper = VmWrapper::new();
        wrapper.set_error("test error");
        assert!(wrapper.last_error.is_some());
        assert_eq!(
            wrapper.last_error.as_ref().unwrap().to_str().unwrap(),
            "test error"
        );
        wrapper.clear_error();
        assert!(wrapper.last_error.is_none());
    }
}
