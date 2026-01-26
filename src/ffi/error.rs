//! Error handling FFI functions.

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::collapsible_if)]

use super::types::{MocaVm, VmWrapper};
use super::vm_ffi::get_wrapper;
use std::ffi::c_char;

/// Get the last error message.
///
/// Returns a pointer to the error message string, or NULL if no error.
/// The returned pointer is valid until the next API call that may set an error.
///
/// # Example (C)
/// ```c
/// moca_result res = moca_call(vm, "func", 0);
/// if (res != MOCA_OK) {
///     printf("Error: %s\n", moca_get_error(vm));
/// }
/// ```
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_get_error(vm: *const MocaVm) -> *const c_char {
    if let Some(wrapper) = get_wrapper(vm) {
        if let Some(ref error) = wrapper.last_error {
            // Return pointer to the error string
            // This is safe as long as the caller doesn't modify the VM
            // before using the pointer
            return error.as_ptr() as *const c_char;
        }
    }
    std::ptr::null()
}

/// Clear the last error.
///
/// After calling this, `moca_get_error` will return NULL until
/// another error occurs.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_clear_error(vm: *mut MocaVm) {
    if vm.is_null() {
        return;
    }
    let wrapper = &mut *(vm as *mut VmWrapper);
    wrapper.clear_error();
}

/// Check if there is a pending error.
///
/// Returns true if an error is set, false otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_has_error(vm: *const MocaVm) -> bool {
    if let Some(wrapper) = get_wrapper(vm) {
        return wrapper.last_error.is_some();
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::vm_ffi::{get_wrapper_mut, moca_vm_free, moca_vm_new};
    use std::ffi::CStr;

    #[test]
    fn test_get_error() {
        unsafe {
            let vm = moca_vm_new();

            // No error initially
            assert!(moca_get_error(vm).is_null());
            assert!(!moca_has_error(vm));

            // Set an error
            let wrapper = get_wrapper_mut(vm).unwrap();
            wrapper.set_error("test error message");

            // Check error
            assert!(moca_has_error(vm));
            let error = moca_get_error(vm);
            assert!(!error.is_null());

            let error_str = CStr::from_ptr(error).to_str().unwrap();
            assert_eq!(error_str, "test error message");

            // Clear error
            moca_clear_error(vm);
            assert!(!moca_has_error(vm));
            assert!(moca_get_error(vm).is_null());

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_error_null_vm() {
        unsafe {
            assert!(moca_get_error(std::ptr::null()).is_null());
            assert!(!moca_has_error(std::ptr::null()));
            // Should not crash
            moca_clear_error(std::ptr::null_mut());
        }
    }
}
