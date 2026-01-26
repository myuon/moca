//! Function call and registration FFI functions.

#![allow(unsafe_op_in_unsafe_fn)]

use super::types::{HostFunction, MicaCFunc, MicaResult, MicaVm};
use super::vm_ffi::get_wrapper_mut;
use std::ffi::{c_char, CStr};

/// Call a mica function by name.
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
/// - `MICA_OK` on success
/// - `MICA_ERROR_NOT_FOUND` if function not found
/// - `MICA_ERROR_RUNTIME` on execution error
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_call(
    vm: *mut MicaVm,
    func_name: *const c_char,
    nargs: i32,
) -> MicaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MicaResult::ErrorInvalidArg;
    };

    if func_name.is_null() {
        wrapper.set_error("Function name is null");
        return MicaResult::ErrorInvalidArg;
    }

    let name = match CStr::from_ptr(func_name).to_str() {
        Ok(s) => s,
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in function name");
            return MicaResult::ErrorInvalidArg;
        }
    };

    // Check for host function first
    if let Some(host_fn) = wrapper.host_functions.get(name) {
        if nargs as usize != host_fn.arity {
            wrapper.set_error(format!(
                "Function '{}' expects {} arguments, got {}",
                name, host_fn.arity, nargs
            ));
            return MicaResult::ErrorInvalidArg;
        }

        // Call host function
        let func = host_fn.func;
        return func(vm);
    }

    // Look for mica function in chunk
    let Some(chunk) = &wrapper.chunk else {
        wrapper.set_error("No bytecode loaded");
        return MicaResult::ErrorNotFound;
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
        return MicaResult::ErrorNotFound;
    };

    // TODO: Implement actual function call
    // This requires transferring values from ffi_stack to VM stack,
    // executing the function, and transferring result back
    wrapper.set_error("Function calls not yet implemented");
    MicaResult::ErrorRuntime
}

/// Protected call - catches errors instead of aborting.
///
/// Same as `mica_call`, but errors are caught and returned as a result code
/// instead of propagating.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_pcall(
    vm: *mut MicaVm,
    func_name: *const c_char,
    nargs: i32,
) -> MicaResult {
    // For now, pcall is the same as call since we already return error codes
    mica_call(vm, func_name, nargs)
}

/// Register a host function.
///
/// The function can then be called from mica code.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Function name (null-terminated)
/// - `func`: Function pointer
/// - `arity`: Number of arguments the function expects
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_register_function(
    vm: *mut MicaVm,
    name: *const c_char,
    func: MicaCFunc,
    arity: i32,
) -> MicaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MicaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Function name is null");
        return MicaResult::ErrorInvalidArg;
    }

    let name_str = match CStr::from_ptr(name).to_str() {
        Ok(s) => s.to_string(),
        Err(_) => {
            wrapper.set_error("Invalid UTF-8 in function name");
            return MicaResult::ErrorInvalidArg;
        }
    };

    wrapper.host_functions.insert(
        name_str,
        HostFunction {
            func,
            arity: arity.max(0) as usize,
        },
    );

    MicaResult::Ok
}

/// Set a global variable.
///
/// Pops the top value from the stack and sets it as a global.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Global variable name (null-terminated)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_set_global(
    vm: *mut MicaVm,
    name: *const c_char,
) -> MicaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MicaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Global name is null");
        return MicaResult::ErrorInvalidArg;
    }

    // TODO: Implement globals storage
    wrapper.set_error("Globals not yet implemented");
    MicaResult::ErrorRuntime
}

/// Get a global variable.
///
/// Pushes the global's value onto the stack.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `name`: Global variable name (null-terminated)
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mica_get_global(
    vm: *mut MicaVm,
    name: *const c_char,
) -> MicaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MicaResult::ErrorInvalidArg;
    };

    if name.is_null() {
        wrapper.set_error("Global name is null");
        return MicaResult::ErrorInvalidArg;
    }

    // TODO: Implement globals storage
    wrapper.set_error("Globals not yet implemented");
    MicaResult::ErrorNotFound
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::stack::*;
    use crate::ffi::vm_ffi::{mica_vm_free, mica_vm_new};
    use std::ffi::CString;

    #[test]
    fn test_register_host_function() {
        unsafe extern "C" fn add_fn(vm: *mut MicaVm) -> MicaResult {
            unsafe {
                let a = mica_to_i64(vm, 0);
                let b = mica_to_i64(vm, 1);
                mica_pop(vm, 2);
                mica_push_i64(vm, a + b);
                MicaResult::Ok
            }
        }

        unsafe {
            let vm = mica_vm_new();
            let name = CString::new("add").unwrap();

            let result = mica_register_function(vm, name.as_ptr(), add_fn, 2);
            assert_eq!(result, MicaResult::Ok);

            // Push arguments and call
            mica_push_i64(vm, 10);
            mica_push_i64(vm, 20);

            let result = mica_call(vm, name.as_ptr(), 2);
            assert_eq!(result, MicaResult::Ok);

            // Check result
            assert_eq!(mica_to_i64(vm, -1), 30);

            mica_vm_free(vm);
        }
    }

    #[test]
    fn test_call_not_found() {
        unsafe {
            let vm = mica_vm_new();
            let name = CString::new("nonexistent").unwrap();

            let result = mica_call(vm, name.as_ptr(), 0);
            assert_eq!(result, MicaResult::ErrorNotFound);

            mica_vm_free(vm);
        }
    }
}
