use std::fmt;

use super::heap::GcRef;

/// A runtime value in BCVM v0.
///
/// Value kinds:
/// - I64: 64-bit signed integer
/// - F64: 64-bit IEEE 754 double (v0 extension)
/// - Bool: true/false
/// - Null: null value
/// - Ref: reference to heap object (String, Array, Object)
#[derive(Clone, Copy)]
pub enum Value {
    I64(i64),
    F64(f64),
    Bool(bool),
    Null,
    Ref(GcRef),
}

impl Value {
    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_i64(&self) -> bool {
        matches!(self, Value::I64(_))
    }

    pub fn is_f64(&self) -> bool {
        matches!(self, Value::F64(_))
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    pub fn is_ref(&self) -> bool {
        matches!(self, Value::Ref(_))
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::I64(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::F64(f) => Some(*f),
            Value::I64(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::I64(n) => *n != 0,
            Value::F64(f) => *f != 0.0,
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Ref(_) => true, // Objects are truthy
        }
    }

    pub fn is_truthy(&self) -> bool {
        self.as_bool()
    }

    pub fn as_ref(&self) -> Option<GcRef> {
        match self {
            Value::Ref(r) => Some(*r),
            _ => None,
        }
    }

    /// Get the type name of this value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::I64(_) => "i64",
            Value::F64(_) => "f64",
            Value::Bool(_) => "bool",
            Value::Null => "null",
            Value::Ref(_) => "ref", // Will be refined based on heap object type
        }
    }

    /// Check if two values are equal.
    /// Note: This allows cross-type comparison (e.g., I64 == F64).
    pub fn value_eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::I64(a), Value::I64(b)) => a == b,
            (Value::F64(a), Value::F64(b)) => a == b,
            (Value::I64(a), Value::F64(b)) => (*a as f64) == *b,
            (Value::F64(a), Value::I64(b)) => *a == (*b as f64),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Ref(a), Value::Ref(b)) => a.index == b.index,
            _ => false,
        }
    }

    // Backward compatibility aliases (deprecated)
    #[deprecated(note = "Use is_null() instead")]
    pub fn is_nil(&self) -> bool {
        self.is_null()
    }

    #[deprecated(note = "Use is_i64() instead")]
    pub fn is_int(&self) -> bool {
        self.is_i64()
    }

    #[deprecated(note = "Use is_f64() instead")]
    pub fn is_float(&self) -> bool {
        self.is_f64()
    }

    #[deprecated(note = "Use is_ref() instead")]
    pub fn is_ptr(&self) -> bool {
        self.is_ref()
    }

    #[deprecated(note = "Use as_i64() instead")]
    pub fn as_int(&self) -> Option<i64> {
        self.as_i64()
    }

    #[deprecated(note = "Use as_f64() instead")]
    pub fn as_float(&self) -> Option<f64> {
        self.as_f64()
    }

    #[deprecated(note = "Use as_ref() instead")]
    pub fn as_ptr(&self) -> Option<GcRef> {
        self.as_ref()
    }

    // =========================================================================
    // Linear Memory Encoding (for heap storage)
    // =========================================================================

    /// Tag values for linear memory encoding.
    /// Each Value is stored as 2 u64 words: [tag, payload]
    const TAG_I64: u64 = 0;
    const TAG_F64: u64 = 1;
    const TAG_BOOL: u64 = 2;
    const TAG_NULL: u64 = 3;
    const TAG_REF: u64 = 4;

    /// Encode this Value into two u64 words for linear memory storage.
    /// Returns (tag, payload).
    pub fn encode(&self) -> (u64, u64) {
        match self {
            Value::I64(n) => (Self::TAG_I64, *n as u64),
            Value::F64(f) => (Self::TAG_F64, f.to_bits()),
            Value::Bool(b) => (Self::TAG_BOOL, if *b { 1 } else { 0 }),
            Value::Null => (Self::TAG_NULL, 0),
            Value::Ref(r) => (Self::TAG_REF, r.index as u64),
        }
    }

    /// Decode a Value from two u64 words read from linear memory.
    /// Returns None if the tag is invalid.
    pub fn decode(tag: u64, payload: u64) -> Option<Self> {
        match tag {
            Self::TAG_I64 => Some(Value::I64(payload as i64)),
            Self::TAG_F64 => Some(Value::F64(f64::from_bits(payload))),
            Self::TAG_BOOL => Some(Value::Bool(payload != 0)),
            Self::TAG_NULL => Some(Value::Null),
            Self::TAG_REF => Some(Value::Ref(GcRef {
                index: payload as usize,
            })),
            _ => None,
        }
    }
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        self.value_eq(other)
    }
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I64(n) => write!(f, "I64({})", n),
            Value::F64(n) => write!(f, "F64({})", n),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::Null => write!(f, "Null"),
            Value::Ref(r) => write!(f, "Ref({})", r.index),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::I64(n) => write!(f, "{}", n),
            Value::F64(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{}.0", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Null => write!(f, "null"),
            Value::Ref(_) => write!(f, "<ref>"), // Will be refined when heap is accessible
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i64_as_i64() {
        assert_eq!(Value::I64(42).as_i64(), Some(42));
    }

    #[test]
    fn test_f64_as_f64() {
        assert_eq!(Value::F64(3.14).as_f64(), Some(3.14));
    }

    #[test]
    fn test_i64_as_f64() {
        assert_eq!(Value::I64(42).as_f64(), Some(42.0));
    }

    #[test]
    fn test_bool_as_bool() {
        assert!(Value::Bool(true).as_bool());
        assert!(!Value::Bool(false).as_bool());
    }

    #[test]
    fn test_null_is_falsy() {
        assert!(!Value::Null.is_truthy());
    }

    #[test]
    fn test_i64_truthiness() {
        assert!(Value::I64(1).is_truthy());
        assert!(Value::I64(-1).is_truthy());
        assert!(!Value::I64(0).is_truthy());
    }

    #[test]
    fn test_f64_truthiness() {
        assert!(Value::F64(1.0).is_truthy());
        assert!(!Value::F64(0.0).is_truthy());
    }

    #[test]
    fn test_equality() {
        assert!(Value::I64(42).value_eq(&Value::I64(42)));
        assert!(Value::F64(3.14).value_eq(&Value::F64(3.14)));
        assert!(Value::I64(42).value_eq(&Value::F64(42.0)));
        assert!(Value::Null.value_eq(&Value::Null));
        assert!(!Value::I64(1).value_eq(&Value::Null));
    }

    // ============================================================
    // BCVM v0 Specification Compliance Tests
    // ============================================================

    /// Test: Spec 7.1 - Value enum has exactly 5 variants per spec
    #[test]
    fn test_spec_value_variants() {
        // Test that all 5 spec-defined value types can be created
        let _i64_val = Value::I64(0);
        let _f64_val = Value::F64(0.0);
        let _bool_val = Value::Bool(false);
        let _null_val = Value::Null;
        let _ref_val = Value::Ref(GcRef { index: 0 });

        // Verify type predicates
        assert!(Value::I64(0).is_i64());
        assert!(Value::F64(0.0).is_f64());
        assert!(Value::Bool(false).is_bool());
        assert!(Value::Null.is_null());
        assert!(Value::Ref(GcRef { index: 0 }).is_ref());
    }

    /// Test: Spec 7.1 - Type names match spec
    #[test]
    fn test_spec_value_type_names() {
        assert_eq!(Value::I64(0).type_name(), "i64");
        assert_eq!(Value::F64(0.0).type_name(), "f64");
        assert_eq!(Value::Bool(true).type_name(), "bool");
        assert_eq!(Value::Null.type_name(), "null");
        assert_eq!(Value::Ref(GcRef { index: 0 }).type_name(), "ref");
    }

    /// Test: Spec - Value is Copy (fits in 64-bit tagged pointer)
    #[test]
    fn test_spec_value_is_copy() {
        let v1 = Value::I64(42);
        let v2 = v1; // Copy
        assert_eq!(v1, v2); // v1 is still valid (wasn't moved)
    }

    /// Test: Spec - I64 is full 64-bit signed integer
    #[test]
    fn test_spec_i64_range() {
        // Min and max values
        let min = Value::I64(i64::MIN);
        let max = Value::I64(i64::MAX);

        assert_eq!(min.as_i64(), Some(i64::MIN));
        assert_eq!(max.as_i64(), Some(i64::MAX));
    }

    /// Test: Spec - F64 is IEEE 754 double precision
    #[test]
    fn test_spec_f64_precision() {
        // Test floating point precision
        let pi = Value::F64(std::f64::consts::PI);
        assert_eq!(pi.as_f64(), Some(std::f64::consts::PI));

        // Special values
        let inf = Value::F64(f64::INFINITY);
        let neg_inf = Value::F64(f64::NEG_INFINITY);

        assert_eq!(inf.as_f64(), Some(f64::INFINITY));
        assert_eq!(neg_inf.as_f64(), Some(f64::NEG_INFINITY));
    }

    /// Test: Spec - Bool to I64 coercion
    #[test]
    fn test_spec_bool_coercion() {
        // Bool can be coerced to I64
        assert_eq!(Value::Bool(true).as_i64(), Some(1));
        assert_eq!(Value::Bool(false).as_i64(), Some(0));
    }

    // =========================================================================
    // Linear Memory Encoding Tests
    // =========================================================================

    /// Test: encode/decode roundtrip for I64
    #[test]
    fn test_encode_decode_i64() {
        let values = [
            Value::I64(0),
            Value::I64(42),
            Value::I64(-42),
            Value::I64(i64::MIN),
            Value::I64(i64::MAX),
        ];
        for v in values {
            let (tag, payload) = v.encode();
            let decoded = Value::decode(tag, payload).unwrap();
            assert_eq!(v, decoded, "Roundtrip failed for {:?}", v);
        }
    }

    /// Test: encode/decode roundtrip for F64
    #[test]
    fn test_encode_decode_f64() {
        let values = [
            Value::F64(0.0),
            Value::F64(-0.0),
            Value::F64(3.14159),
            Value::F64(-273.15),
            Value::F64(f64::INFINITY),
            Value::F64(f64::NEG_INFINITY),
            Value::F64(f64::MIN),
            Value::F64(f64::MAX),
        ];
        for v in values {
            let (tag, payload) = v.encode();
            let decoded = Value::decode(tag, payload).unwrap();
            // Use to_bits comparison for F64 to handle -0.0 correctly
            match (&v, &decoded) {
                (Value::F64(a), Value::F64(b)) => {
                    assert_eq!(a.to_bits(), b.to_bits(), "Roundtrip failed for {:?}", v)
                }
                _ => panic!("Expected F64"),
            }
        }
    }

    /// Test: encode/decode roundtrip for F64 NaN
    #[test]
    fn test_encode_decode_f64_nan() {
        let v = Value::F64(f64::NAN);
        let (tag, payload) = v.encode();
        let decoded = Value::decode(tag, payload).unwrap();
        match decoded {
            Value::F64(f) => assert!(f.is_nan(), "Decoded NaN should be NaN"),
            _ => panic!("Expected F64"),
        }
    }

    /// Test: encode/decode roundtrip for Bool
    #[test]
    fn test_encode_decode_bool() {
        for v in [Value::Bool(true), Value::Bool(false)] {
            let (tag, payload) = v.encode();
            let decoded = Value::decode(tag, payload).unwrap();
            assert_eq!(v, decoded, "Roundtrip failed for {:?}", v);
        }
    }

    /// Test: encode/decode roundtrip for Null
    #[test]
    fn test_encode_decode_null() {
        let v = Value::Null;
        let (tag, payload) = v.encode();
        let decoded = Value::decode(tag, payload).unwrap();
        assert_eq!(v, decoded);
    }

    /// Test: encode/decode roundtrip for Ref
    #[test]
    fn test_encode_decode_ref() {
        let values = [
            Value::Ref(GcRef { index: 0 }),
            Value::Ref(GcRef { index: 42 }),
            Value::Ref(GcRef { index: usize::MAX }),
        ];
        for v in values {
            let (tag, payload) = v.encode();
            let decoded = Value::decode(tag, payload).unwrap();
            assert_eq!(v, decoded, "Roundtrip failed for {:?}", v);
        }
    }

    /// Test: decode with invalid tag returns None
    #[test]
    fn test_decode_invalid_tag() {
        assert!(Value::decode(99, 0).is_none());
        assert!(Value::decode(u64::MAX, 0).is_none());
    }
}
