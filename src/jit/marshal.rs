//! Value marshaling between VM and JIT representations.
//!
//! VM uses Rust enum `Value`, JIT uses 128-bit (tag: u64, payload: u64) format.

use crate::vm::Value;

/// Value tags for JIT representation.
pub mod tags {
    pub const TAG_INT: u64 = 0;
    pub const TAG_FLOAT: u64 = 1;
    pub const TAG_BOOL: u64 = 2;
    pub const TAG_NIL: u64 = 3;
    pub const TAG_PTR: u64 = 4;
}

/// JIT value representation (128-bit: tag + payload).
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JitValue {
    pub tag: u64,
    pub payload: u64,
}

impl JitValue {
    /// Convert VM Value to JIT representation.
    pub fn from_value(value: &Value) -> Self {
        match value {
            Value::I64(n) => JitValue {
                tag: tags::TAG_INT,
                payload: *n as u64,
            },
            Value::F64(f) => JitValue {
                tag: tags::TAG_FLOAT,
                payload: f.to_bits(),
            },
            Value::Bool(b) => JitValue {
                tag: tags::TAG_BOOL,
                payload: if *b { 1 } else { 0 },
            },
            Value::Null => JitValue {
                tag: tags::TAG_NIL,
                payload: 0,
            },
            Value::Ref(gc_ref) => JitValue {
                tag: tags::TAG_PTR,
                payload: gc_ref.index as u64,
            },
        }
    }

    /// Convert JIT representation back to VM Value.
    pub fn to_value(self) -> Value {
        match self.tag {
            tags::TAG_INT => Value::I64(self.payload as i64),
            tags::TAG_FLOAT => Value::F64(f64::from_bits(self.payload)),
            tags::TAG_BOOL => Value::Bool(self.payload != 0),
            tags::TAG_NIL => Value::Null,
            tags::TAG_PTR => {
                // Reconstruct GcRef from the payload (which contains the index)
                Value::Ref(crate::vm::GcRef {
                    index: self.payload as usize,
                })
            }
            _ => Value::Null, // Unknown tag
        }
    }
}

/// JIT execution context passed to compiled functions.
#[repr(C)]
pub struct JitContext {
    /// Value stack (array of JitValue)
    pub stack: *mut JitValue,
    /// Stack pointer (index into stack)
    pub sp: usize,
    /// Locals array
    pub locals: *mut JitValue,
    /// Number of locals
    pub locals_count: usize,
}

impl JitContext {
    /// Create a new JIT context with allocated stack and locals.
    pub fn new(locals_count: usize) -> Self {
        let stack = vec![JitValue { tag: 0, payload: 0 }; 256].into_boxed_slice();
        let locals = vec![
            JitValue {
                tag: tags::TAG_NIL,
                payload: 0
            };
            locals_count
        ]
        .into_boxed_slice();

        JitContext {
            stack: Box::into_raw(stack) as *mut JitValue,
            sp: 0,
            locals: Box::into_raw(locals) as *mut JitValue,
            locals_count,
        }
    }

    /// Push a value onto the JIT stack.
    pub fn push(&mut self, value: JitValue) {
        unsafe {
            *self.stack.add(self.sp) = value;
        }
        self.sp += 1;
    }

    /// Pop a value from the JIT stack.
    pub fn pop(&mut self) -> JitValue {
        self.sp -= 1;
        unsafe { *self.stack.add(self.sp) }
    }

    /// Set a local variable.
    pub fn set_local(&mut self, idx: usize, value: JitValue) {
        unsafe {
            *self.locals.add(idx) = value;
        }
    }

    /// Get a local variable.
    pub fn get_local(&self, idx: usize) -> JitValue {
        unsafe { *self.locals.add(idx) }
    }
}

impl Drop for JitContext {
    fn drop(&mut self) {
        unsafe {
            // Reconstruct boxes and drop them
            let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(self.stack, 256));
            let _ = Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.locals,
                self.locals_count,
            ));
        }
    }
}

/// Return value from JIT compiled functions.
/// Returned in RAX (tag) and RDX (payload) per System V AMD64 ABI.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct JitReturn {
    pub tag: u64,
    pub payload: u64,
}

impl JitReturn {
    /// Convert to VM Value.
    pub fn to_value(self) -> Value {
        JitValue {
            tag: self.tag,
            payload: self.payload,
        }
        .to_value()
    }
}

/// Type signature for JIT compiled functions.
/// Arguments: (vm_ctx: *mut u8, stack_ptr: *mut JitValue, locals_ptr: *mut JitValue)
/// Returns: JitReturn (tag in RAX, payload in RDX)
pub type JitFn = unsafe extern "C" fn(*mut u8, *mut JitValue, *mut JitValue) -> JitReturn;

/// Runtime call context passed to JIT code for function calls.
/// This allows JIT code to call back into VM for function execution.
#[repr(C)]
pub struct JitCallContext {
    /// Pointer to the VM instance
    pub vm: *mut u8,
    /// Pointer to the chunk (bytecode)
    pub chunk: *const u8,
    /// Function call helper: (ctx, func_index, argc, args_ptr) -> JitReturn
    pub call_helper:
        unsafe extern "C" fn(*mut JitCallContext, u64, u64, *const JitValue) -> JitReturn,
    /// Push string helper: (ctx, string_index) -> JitReturn (returns Ref)
    pub push_string_helper: unsafe extern "C" fn(*mut JitCallContext, u64) -> JitReturn,
    /// Array/string length helper: (ctx, ref_index) -> JitReturn (returns i64)
    /// Note: This is kept for compatibility but ArrayLen is now inlined in JIT code.
    pub array_len_helper: unsafe extern "C" fn(*mut JitCallContext, u64) -> JitReturn,
    /// Hostcall helper: (ctx, hostcall_num, argc, args_ptr) -> JitReturn
    pub hostcall_helper:
        unsafe extern "C" fn(*mut JitCallContext, u64, u64, *const JitValue) -> JitReturn,
    /// Pointer to heap memory base (for direct heap access from JIT code)
    /// This points to the first element of the heap's memory Vec<u64>.
    pub heap_base: *const u64,
    /// Pointer to string constant cache (for direct access from JIT code)
    /// This points to the first element of VM's string_cache Vec<Option<GcRef>>.
    /// Each entry is 16 bytes: Option<GcRef> where GcRef is 8 bytes (usize).
    pub string_cache: *const u64,
    /// Number of entries in the string cache
    pub string_cache_len: u64,
    /// HeapAllocDynSimple helper: (ctx, size, elem_kind) -> JitReturn (returns Ref)
    pub heap_alloc_dyn_simple_helper:
        unsafe extern "C" fn(*mut JitCallContext, u64, u64) -> JitReturn,
    /// Pointer to JIT function table for direct call dispatch.
    /// Layout: [entry_0, total_regs_0, entry_1, total_regs_1, ...] (u64 pairs).
    /// entry == 0 means the function is not yet JIT-compiled.
    pub jit_function_table: *const u64,
}

/// Type signature for call helper function.
/// Called from JIT code to execute a function call via VM.
///
/// Arguments:
///   - ctx: *mut JitCallContext - context with VM and chunk pointers
///   - func_index: u64 - index of function to call
///   - argc: u64 - number of arguments
///   - args: *const JitValue - pointer to arguments array (argc values)
///
/// Returns: JitReturn with the function's return value
pub type CallHelperFn =
    unsafe extern "C" fn(*mut JitCallContext, u64, u64, *const JitValue) -> JitReturn;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_roundtrip() {
        let value = Value::I64(42);
        let jit_val = JitValue::from_value(&value);
        assert_eq!(jit_val.tag, tags::TAG_INT);
        assert_eq!(jit_val.payload, 42);

        let back = jit_val.to_value();
        assert!(matches!(back, Value::I64(42)));
    }

    #[test]
    fn test_float_roundtrip() {
        let value = Value::F64(3.14);
        let jit_val = JitValue::from_value(&value);
        assert_eq!(jit_val.tag, tags::TAG_FLOAT);

        let back = jit_val.to_value();
        if let Value::F64(f) = back {
            assert!((f - 3.14).abs() < 0.0001);
        } else {
            panic!("Expected Float");
        }
    }

    #[test]
    fn test_bool_roundtrip() {
        let value = Value::Bool(true);
        let jit_val = JitValue::from_value(&value);
        assert_eq!(jit_val.tag, tags::TAG_BOOL);
        assert_eq!(jit_val.payload, 1);

        let back = jit_val.to_value();
        assert!(matches!(back, Value::Bool(true)));
    }

    #[test]
    fn test_nil_roundtrip() {
        let value = Value::Null;
        let jit_val = JitValue::from_value(&value);
        assert_eq!(jit_val.tag, tags::TAG_NIL);

        let back = jit_val.to_value();
        assert!(matches!(back, Value::Null));
    }
}
