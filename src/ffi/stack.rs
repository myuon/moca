//! Stack manipulation FFI functions.
//!
//! These functions allow pushing and popping values to/from the FFI stack,
//! as well as type checking and conversion.

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::collapsible_if)]

use super::types::MicaVm;
use super::vm_ffi::get_wrapper_mut;
use crate::vm::Value;
use std::ffi::c_char;

// =============================================================================
// Push Functions
// =============================================================================

/// Push a null value onto the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_push_null(vm: *mut MicaVm) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        wrapper.ffi_stack.push(Value::Null);
    }
}

/// Push a boolean value onto the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_push_bool(vm: *mut MicaVm, value: bool) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        wrapper.ffi_stack.push(Value::Bool(value));
    }
}

/// Push an i64 value onto the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_push_i64(vm: *mut MicaVm, value: i64) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        wrapper.ffi_stack.push(Value::I64(value));
    }
}

/// Push an f64 value onto the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_push_f64(vm: *mut MicaVm, value: f64) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        wrapper.ffi_stack.push(Value::F64(value));
    }
}

/// Push a string value onto the stack.
///
/// The string is copied into the VM's heap. The caller retains ownership
/// of the original string.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `str`: Pointer to string data (does not need to be null-terminated)
/// - `len`: Length of string in bytes
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_push_string(vm: *mut MicaVm, str: *const c_char, len: usize) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if str.is_null() {
            wrapper.ffi_stack.push(Value::Null);
            return;
        }

        // Convert to Rust string
        let slice = std::slice::from_raw_parts(str as *const u8, len);
        let string = String::from_utf8_lossy(slice).into_owned();

        // Allocate on heap and push reference
        let gc_ref = wrapper.vm.heap_mut().alloc_string(string);
        wrapper.ffi_stack.push(Value::Ref(gc_ref));
    }
}

// =============================================================================
// Type Checking Functions
// =============================================================================

/// Resolve a stack index to an absolute index.
///
/// Positive indices are from the bottom (0 = first element).
/// Negative indices are from the top (-1 = last element).
fn resolve_index(stack_len: usize, index: i32) -> Option<usize> {
    if index >= 0 {
        let idx = index as usize;
        if idx < stack_len {
            Some(idx)
        } else {
            None
        }
    } else {
        let offset = (-index) as usize;
        if offset <= stack_len {
            Some(stack_len - offset)
        } else {
            None
        }
    }
}

/// Check if the value at the given index is null.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_null(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].is_null();
        }
    }
    false
}

/// Check if the value at the given index is a boolean.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_bool(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].is_bool();
        }
    }
    false
}

/// Check if the value at the given index is an i64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_i64(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].is_i64();
        }
    }
    false
}

/// Check if the value at the given index is an f64.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_f64(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].is_f64();
        }
    }
    false
}

/// Check if the value at the given index is a string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_string(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            if let Value::Ref(r) = wrapper.ffi_stack[idx] {
                if let Some(obj) = wrapper.vm.heap().get(r) {
                    return obj.as_string().is_some();
                }
            }
        }
    }
    false
}

/// Check if the value at the given index is a reference (object/array/string).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_is_ref(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].is_ref();
        }
    }
    false
}

// =============================================================================
// Conversion Functions
// =============================================================================

/// Get the boolean value at the given index.
///
/// Returns false if the value is not a boolean or index is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_to_bool(vm: *mut MicaVm, index: i32) -> bool {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].as_bool();
        }
    }
    false
}

/// Get the i64 value at the given index.
///
/// Returns 0 if the value is not an i64 or index is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_to_i64(vm: *mut MicaVm, index: i32) -> i64 {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].as_i64().unwrap_or(0);
        }
    }
    0
}

/// Get the f64 value at the given index.
///
/// Returns 0.0 if the value is not an f64 or index is invalid.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_to_f64(vm: *mut MicaVm, index: i32) -> f64 {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            return wrapper.ffi_stack[idx].as_f64().unwrap_or(0.0);
        }
    }
    0.0
}

/// Get the string value at the given index.
///
/// Returns NULL if the value is not a string or index is invalid.
/// The returned pointer is valid until the next GC or stack modification.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `index`: Stack index
/// - `len`: Output parameter for string length (can be NULL)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_to_string(
    vm: *mut MicaVm,
    index: i32,
    len: *mut usize,
) -> *const c_char {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        if let Some(idx) = resolve_index(wrapper.ffi_stack.len(), index) {
            if let Value::Ref(r) = wrapper.ffi_stack[idx] {
                if let Some(obj) = wrapper.vm.heap().get(r) {
                    if let Some(s) = obj.as_string() {
                        if !len.is_null() {
                            *len = s.value.len();
                        }
                        return s.value.as_ptr() as *const c_char;
                    }
                }
            }
        }
    }
    if !len.is_null() {
        *len = 0;
    }
    std::ptr::null()
}

// =============================================================================
// Stack Manipulation
// =============================================================================

/// Pop values from the stack.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `count`: Number of values to pop
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_pop(vm: *mut MicaVm, count: i32) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        let count = count.max(0) as usize;
        let new_len = wrapper.ffi_stack.len().saturating_sub(count);
        wrapper.ffi_stack.truncate(new_len);
    }
}

/// Get the current stack height.
///
/// Returns the number of values on the stack.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_get_top(vm: *mut MicaVm) -> i32 {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        wrapper.ffi_stack.len() as i32
    } else {
        0
    }
}

/// Set the stack height.
///
/// If `index` is less than current height, pops values.
/// If `index` is greater than current height, pushes nulls.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_set_top(vm: *mut MicaVm, index: i32) {
    if let Some(wrapper) = get_wrapper_mut(vm) {
        let target = index.max(0) as usize;
        let current = wrapper.ffi_stack.len();

        if target < current {
            wrapper.ffi_stack.truncate(target);
        } else {
            wrapper.ffi_stack.resize(target, Value::Null);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::vm_ffi::{mica_vm_free, mica_vm_new};

    #[test]
    fn test_resolve_index() {
        assert_eq!(resolve_index(5, 0), Some(0));
        assert_eq!(resolve_index(5, 4), Some(4));
        assert_eq!(resolve_index(5, 5), None);
        assert_eq!(resolve_index(5, -1), Some(4));
        assert_eq!(resolve_index(5, -5), Some(0));
        assert_eq!(resolve_index(5, -6), None);
    }

    #[test]
    fn test_push_pop() {
        unsafe {
            let vm = mica_vm_new();

            mica_push_i64(vm, 42);
            mica_push_f64(vm, 3.14);
            mica_push_bool(vm, true);
            mica_push_null(vm);

            assert_eq!(mica_get_top(vm), 4);

            assert!(mica_is_null(vm, -1));
            assert!(mica_is_bool(vm, -2));
            assert!(mica_is_f64(vm, -3));
            assert!(mica_is_i64(vm, -4));

            mica_pop(vm, 2);
            assert_eq!(mica_get_top(vm), 2);

            assert_eq!(mica_to_f64(vm, -1), 3.14);
            assert_eq!(mica_to_i64(vm, -2), 42);

            mica_vm_free(vm);
        }
    }

    #[test]
    fn test_set_top() {
        unsafe {
            let vm = mica_vm_new();

            mica_push_i64(vm, 1);
            mica_push_i64(vm, 2);
            mica_push_i64(vm, 3);

            assert_eq!(mica_get_top(vm), 3);

            mica_set_top(vm, 5);
            assert_eq!(mica_get_top(vm), 5);
            assert!(mica_is_null(vm, -1));
            assert!(mica_is_null(vm, -2));

            mica_set_top(vm, 1);
            assert_eq!(mica_get_top(vm), 1);
            assert_eq!(mica_to_i64(vm, -1), 1);

            mica_vm_free(vm);
        }
    }
}
