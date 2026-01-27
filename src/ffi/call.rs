//! Function call and registration FFI functions.

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::missing_safety_doc)]

use super::types::{HostFunction, MocaCFunc, MocaResult, MocaVm};
use super::vm_ffi::get_wrapper_mut;
use std::ffi::{CStr, c_char};

/// Call a moca function by name.
///
/// Arguments must be pushed onto the stack before calling.
/// The result will be on the stack after a successful call.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `func_name`: Name of the function to call (null-terminated)
/// - `nargs`: Number of arguments on the stack
///
/// # Returns
/// - `MOCA_OK` on success
/// - `MOCA_ERROR_NOT_FOUND` if function not found
/// - `MOCA_ERROR_RUNTIME` on execution error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_call(
    vm: *mut MocaVm,
    func_name: *const c_char,
    nargs: i32,
) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if func_name.is_null() {
        wrapper.set_error("Function name is null");
        return MocaResult::ErrorInvalidArg;
    }

    let name = match CStr::from_ptr(func_name).to_str() {
        Ok(s) => s,
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in function name");
            return MocaResult::ErrorInvalidArg;
        }
    };

    // Check for host function first
    if let Some(host_fn) = wrapper.host_functions.get(name) {
        if nargs as usize != host_fn.arity {
            wrapper.set_error(format!(
                "Function '{}' expects {} arguments, got {}",
                name, host_fn.arity, nargs
            ));
            return MocaResult::ErrorInvalidArg;
        }

        // Call host function
        let func = host_fn.func;
        return func(vm);
    }

    // Look for moca function in chunk
    let Some(chunk) = &wrapper.chunk else {
        wrapper.set_error("No bytecode loaded");
        return MocaResult::ErrorNotFound;
    };

    // Find function by name
    let func_idx = chunk
        .functions
        .iter()
        .position(|f| f.name == name)
        .or_else(|| {
            if chunk.main.name == name {
                Some(usize::MAX) // Special marker for main
            } else {
                None
            }
        });

    let Some(_func_idx) = func_idx else {
        wrapper.set_error(format!("Function '{}' not found", name));
        return MocaResult::ErrorNotFound;
    };

    // TODO: Implement actual function call
    // This requires transferring values from ffi_stack to VM stack,
    // executing the function, and transferring result back
    wrapper.set_error("Function calls not yet implemented");
    MocaResult::ErrorRuntime
}

/// Protected call - catches errors instead of aborting.
///
/// Same as `moca_call`, but errors are caught and returned as a result code
/// instead of propagating.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_pcall(
    vm: *mut MocaVm,
    func_name: *const c_char,
    nargs: i32,
) -> MocaResult {
    // For now, pcall is the same as call since we already return error codes
    moca_call(vm, func_name, nargs)
}

/// Register a host function.
///
/// The function can then be called from moca code.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Function name (null-terminated)
/// - `func`: Function pointer
/// - `arity`: Number of arguments the function expects
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_register_function(
    vm: *mut MocaVm,
    name: *const c_char,
    func: MocaCFunc,
    arity: i32,
) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Function name is null");
        return MocaResult::ErrorInvalidArg;
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in function name");
            return MocaResult::ErrorInvalidArg;
        }
    };

    wrapper.host_functions.insert(
        name_str,
        HostFunction {
            func,
            arity: arity.max(0) as usize,
        },
    );

    MocaResult::Ok
}

/// Set a global variable.
///
/// Pops the top value from the stack and sets it as a global.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Global variable name (null-terminated)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_set_global(vm: *mut MocaVm, name: *const c_char) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Global name is null");
        return MocaResult::ErrorInvalidArg;
    }

    // Pop value from FFI stack
    let Some(value) = wrapper.ffi_stack.pop() else {
        wrapper.set_error("Stack is empty");
        return MocaResult::ErrorInvalidArg;
    };

    // Convert name to Rust string
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in global name");
            return MocaResult::ErrorInvalidArg;
        }
    };

    // Store the global
    wrapper.globals.insert(name_str, value);
    wrapper.clear_error();
    MocaResult::Ok
}

/// Get a global variable.
///
/// Pushes the global's value onto the stack.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Global variable name (null-terminated)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_get_global(vm: *mut MocaVm, name: *const c_char) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Global name is null");
        return MocaResult::ErrorInvalidArg;
    }

    // Convert name to Rust string
    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s,
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in global name");
            return MocaResult::ErrorInvalidArg;
        }
    };

    // Look up the global
    let Some(value) = wrapper.globals.get(name_str).cloned() else {
        wrapper.set_error(format!("Global '{}' not found", name_str));
        return MocaResult::ErrorNotFound;
    };

    // Push onto FFI stack
    wrapper.ffi_stack.push(value);
    wrapper.clear_error();
    MocaResult::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::stack::*;
    use crate::ffi::vm_ffi::{moca_vm_free, moca_vm_new};
    use std::ffi::CString;

    #[test]
    fn test_register_host_function() {
        unsafe extern "C" fn add_fn(vm: *mut MocaVm) -> MocaResult {
            unsafe {
                let a = moca_to_i64(vm, 0);
                let b = moca_to_i64(vm, 1);
                moca_pop(vm, 2);
                moca_push_i64(vm, a + b);
                MocaResult::Ok
            }
        }

        unsafe {
            let vm = moca_vm_new();
            let name = CString::new("add").unwrap();

            let result = moca_register_function(vm, name.as_ptr(), add_fn, 2);
            assert_eq!(result, MocaResult::Ok);

            // Push arguments and call
            moca_push_i64(vm, 10);
            moca_push_i64(vm, 20);

            let result = moca_call(vm, name.as_ptr(), 2);
            assert_eq!(result, MocaResult::Ok);

            // Check result
            assert_eq!(moca_to_i64(vm, -1), 30);

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_call_not_found() {
        unsafe {
            let vm = moca_vm_new();
            let name = CString::new("nonexistent").unwrap();

            let result = moca_call(vm, name.as_ptr(), 0);
            assert_eq!(result, MocaResult::ErrorNotFound);

            moca_vm_free(vm);
        }
    }
}
