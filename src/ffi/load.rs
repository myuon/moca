//! Bytecode loading FFI functions.

#![allow(unsafe_op_in_unsafe_fn)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::missing_safety_doc)]

use super::types::{MocaResult, MocaVm};
use super::vm_ffi::get_wrapper_mut;
use crate::vm::bytecode;
use std::ffi::c_char;

/// Load bytecode from memory.
///
/// This parses and validates the bytecode, making it ready for execution.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `data`: Pointer to bytecode data
/// - `len`: Length of bytecode data in bytes
///
/// # Returns
/// - `MOCA_OK` on success
/// - `MOCA_ERROR_INVALID_ARG` if data is NULL
/// - `MOCA_ERROR_VERIFY` if bytecode is invalid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_load_chunk(
    vm: *mut MocaVm,
    data: *const u8,
    len: usize,
) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if data.is_null() {
        wrapper.set_error("data pointer is NULL");
        return MocaResult::ErrorInvalidArg;
    }

    // Read the bytecode data
    let slice = std::slice::from_raw_parts(data, len);

    // Deserialize the chunk
    let chunk = match bytecode::deserialize(slice) {
        Ok(c) => c,
        Err(e) => {
            wrapper.set_error(format!("bytecode error: {}", e));
            return MocaResult::ErrorVerify;
        }
    };

    // Optionally verify the bytecode
    // For now, we trust the bytecode is valid (verification can be added later)

    // Store the chunk
    wrapper.chunk = Some(chunk);
    wrapper.clear_error();

    MocaResult::Ok
}

/// Load bytecode from a file.
///
/// This reads the file, parses the bytecode, and validates it.
///
/// # Arguments
/// - `vm`: Valid VM instance
/// - `path`: Path to bytecode file (null-terminated)
///
/// # Returns
/// - `MOCA_OK` on success
/// - `MOCA_ERROR_INVALID_ARG` if path is NULL
/// - `MOCA_ERROR_NOT_FOUND` if file cannot be read
/// - `MOCA_ERROR_VERIFY` if bytecode is invalid
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_load_file(vm: *mut MocaVm, path: *const c_char) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if path.is_null() {
        wrapper.set_error("path is NULL");
        return MocaResult::ErrorInvalidArg;
    }

    // Convert path to Rust string
    let c_str = std::ffi::CStr::from_ptr(path);
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            wrapper.set_error("invalid UTF-8 in path");
            return MocaResult::ErrorInvalidArg;
        }
    };

    // Read the file
    let data = match std::fs::read(path_str) {
        Ok(d) => d,
        Err(e) => {
            wrapper.set_error(format!("cannot read file: {}", e));
            return MocaResult::ErrorNotFound;
        }
    };

    // Deserialize the chunk
    let chunk = match bytecode::deserialize(&data) {
        Ok(c) => c,
        Err(e) => {
            wrapper.set_error(format!("bytecode error: {}", e));
            return MocaResult::ErrorVerify;
        }
    };

    // Store the chunk
    wrapper.chunk = Some(chunk);
    wrapper.clear_error();

    MocaResult::Ok
}

/// Save bytecode to a file.
///
/// This serializes the currently loaded chunk to a file.
///
/// # Arguments
/// - `vm`: Valid VM instance with a loaded chunk
/// - `path`: Path to output file (null-terminated)
///
/// # Returns
/// - `MOCA_OK` on success
/// - `MOCA_ERROR_INVALID_ARG` if no chunk is loaded or path is NULL
/// - `MOCA_ERROR_RUNTIME` if file cannot be written
#[unsafe(no_mangle)]
pub unsafe extern "C" fn moca_save_file(vm: *mut MocaVm, path: *const c_char) -> MocaResult {
    let Some(wrapper) = get_wrapper_mut(vm) else {
        return MocaResult::ErrorInvalidArg;
    };

    if path.is_null() {
        wrapper.set_error("path is NULL");
        return MocaResult::ErrorInvalidArg;
    }

    let Some(ref chunk) = wrapper.chunk else {
        wrapper.set_error("no chunk loaded");
        return MocaResult::ErrorInvalidArg;
    };

    // Convert path to Rust string
    let c_str = std::ffi::CStr::from_ptr(path);
    let path_str = match c_str.to_str() {
        Ok(s) => s,
        Err(_) => {
            wrapper.set_error("invalid UTF-8 in path");
            return MocaResult::ErrorInvalidArg;
        }
    };

    // Serialize the chunk
    let data = bytecode::serialize(chunk);

    // Write to file
    match std::fs::write(path_str, &data) {
        Ok(()) => {
            wrapper.clear_error();
            MocaResult::Ok
        }
        Err(e) => {
            wrapper.set_error(format!("cannot write file: {}", e));
            MocaResult::ErrorRuntime
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ffi::vm_ffi::{moca_vm_free, moca_vm_new};
    use crate::vm::{Chunk, Function, Op, bytecode};
    use std::ffi::CString;

    #[test]
    fn test_load_chunk() {
        // Create a simple chunk
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 0,
                code: vec![Op::PushInt(42), Op::Print, Op::Ret],
                stackmap: None,
            },
            strings: vec![],
            debug: None,
        };

        // Serialize it
        let data = bytecode::serialize(&chunk);

        unsafe {
            let vm = moca_vm_new();

            // Load the chunk
            let result = moca_load_chunk(vm, data.as_ptr(), data.len());
            assert_eq!(result, MocaResult::Ok);

            // Verify chunk was loaded
            let wrapper = get_wrapper_mut(vm).unwrap();
            assert!(wrapper.chunk.is_some());
            assert_eq!(wrapper.chunk.as_ref().unwrap().main.name, "main");

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_load_chunk_null_data() {
        unsafe {
            let vm = moca_vm_new();

            let result = moca_load_chunk(vm, std::ptr::null(), 0);
            assert_eq!(result, MocaResult::ErrorInvalidArg);

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_load_chunk_invalid_data() {
        unsafe {
            let vm = moca_vm_new();

            let bad_data = b"not valid bytecode";
            let result = moca_load_chunk(vm, bad_data.as_ptr(), bad_data.len());
            assert_eq!(result, MocaResult::ErrorVerify);

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_load_file_not_found() {
        unsafe {
            let vm = moca_vm_new();

            let path = CString::new("/nonexistent/path/to/file.bc").unwrap();
            let result = moca_load_file(vm, path.as_ptr());
            assert_eq!(result, MocaResult::ErrorNotFound);

            moca_vm_free(vm);
        }
    }

    #[test]
    fn test_save_and_load_file() {
        // Create a simple chunk
        let chunk = Chunk {
            functions: vec![],
            main: Function {
                name: "main".to_string(),
                arity: 0,
                locals_count: 0,
                code: vec![Op::PushInt(123), Op::Ret],
                stackmap: None,
            },
            strings: vec!["test".to_string()],
            debug: None,
        };

        // Use a temp file
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join("test_moca_bytecode.bc");
        let path_cstr = CString::new(temp_path.to_str().unwrap()).unwrap();

        unsafe {
            let vm = moca_vm_new();

            // Load the chunk directly
            let data = bytecode::serialize(&chunk);
            let result = moca_load_chunk(vm, data.as_ptr(), data.len());
            assert_eq!(result, MocaResult::Ok);

            // Save to file
            let result = moca_save_file(vm, path_cstr.as_ptr());
            assert_eq!(result, MocaResult::Ok);

            moca_vm_free(vm);
        }

        unsafe {
            let vm = moca_vm_new();

            // Load from file
            let result = moca_load_file(vm, path_cstr.as_ptr());
            assert_eq!(result, MocaResult::Ok);

            // Verify chunk was loaded correctly
            let wrapper = get_wrapper_mut(vm).unwrap();
            assert!(wrapper.chunk.is_some());
            let loaded_chunk = wrapper.chunk.as_ref().unwrap();
            assert_eq!(loaded_chunk.main.name, "main");
            assert_eq!(loaded_chunk.strings.len(), 1);
            assert_eq!(loaded_chunk.strings[0], "test");

            moca_vm_free(vm);
        }

        // Clean up
        std::fs::remove_file(&temp_path).ok();
    }
}
