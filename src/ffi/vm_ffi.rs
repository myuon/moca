//! VM lifecycle FFI functions.

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::needless_return)]

use super::types::{MocaVm, VmWrapper};

/// Create a new VM instance.
///
/// Returns a pointer to a new VM instance, or NULL if allocation fails.
/// The returned VM must be freed with `moca_vm_free()`.
///
/// # Example (C)
/// ```c
/// moca_vm *vm = moca_vm_new();
/// if (vm == NULL) {
///     // Handle allocation failure
/// }
/// // ... use vm ...
/// moca_vm_free(vm);
/// ```
#[unsafe(no_mangle)]
pub extern "C" fn moca_vm_new() -> *mut MocaVm {
    let wrapper = Box::new(VmWrapper::new());
    Box::into_raw(wrapper) as *mut MocaVm
}

/// Free a VM instance.
///
/// After this call, the VM pointer is invalid and must not be used.
///
/// # Safety
///
/// - `vm` must be a valid pointer returned by `moca_vm_new()`
/// - `vm` must not have been freed already
/// - No other operations may be in progress on this VM
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_vm_free(vm: *mut MocaVm) {
    if vm.is_null() {
        return;
    }
    // Reconstruct the Box and let it drop
    let _ = Box::from_raw(vm as *mut VmWrapper);
}

/// Set the memory limit for the VM.
///
/// This must be called before loading bytecode.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `bytes`: Maximum memory in bytes (0 = no limit)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_set_memory_limit(vm: *mut MocaVm, _bytes: usize) {
    if vm.is_null() {
        return;
    }
    // TODO: Implement memory limiting in the VM
    // For now, this is a no-op placeholder
}

/// Set the error callback function.
///
/// The callback will be invoked whenever an error occurs.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `callback`: Error callback function (or NULL to disable)
/// - `userdata`: User data passed to callback
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_set_error_callback(
    vm: *mut MocaVm,
    callback: super::types::MocaErrorFn,
    userdata: *mut std::ffi::c_void,
) {
    if vm.is_null() {
        return;
    }
    let wrapper = &mut *(vm as *mut VmWrapper);
    wrapper.error_callback = callback;
    wrapper.error_userdata = userdata;
}

/// Check if the VM has a loaded chunk.
///
/// Returns true if bytecode has been loaded, false otherwise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_has_chunk(vm: *mut MocaVm) -> bool {
    if vm.is_null() {
        return false;
    }
    let wrapper = &*(vm as *const VmWrapper);
    wrapper.chunk.is_some()
}

/// Helper to get a mutable reference to the wrapper from a raw pointer.
///
/// Returns None if the pointer is null.
pub(crate) unsafe fn get_wrapper_mut(vm: *mut MocaVm) -> Option<&'static mut VmWrapper> {
    if vm.is_null() {
        None
    } else {
        Some(&mut *(vm as *mut VmWrapper))
    }
}

/// Helper to get an immutable reference to the wrapper from a raw pointer.
///
/// Returns None if the pointer is null.
pub(crate) unsafe fn get_wrapper(vm: *const MocaVm) -> Option<&'static VmWrapper> {
    if vm.is_null() {
        None
    } else {
        Some(&*(vm as *const VmWrapper))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vm_new_free() {
        let vm = moca_vm_new();
        assert!(!vm.is_null());
        unsafe {
            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_vm_free_null() {
        // Should not crash
        unsafe {
            moca_vm_free(std::ptr::null_mut());
        }
    }

    #[test]
    fn test_has_chunk() {
        let vm = moca_vm_new();
        unsafe {
            assert!(!moca_has_chunk(vm));
            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_error_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        static CALLBACK_CALLED: AtomicBool = AtomicBool::new(false);

        unsafe extern "C" fn test_callback(
            _message: *const std::ffi::c_char,
            _userdata: *mut std::ffi::c_void,
        ) {
            CALLBACK_CALLED.store(true, Ordering::SeqCst);
        }

        let vm = moca_vm_new();
        unsafe {
            moca_set_error_callback(vm, Some(test_callback), std::ptr::null_mut());

            // Trigger an error
            let wrapper = get_wrapper_mut(vm).unwrap();
            wrapper.set_error("test error");

            assert!(CALLBACK_CALLED.load(Ordering::SeqCst));
            moca_vm_free(vm);
        }
    }
}
