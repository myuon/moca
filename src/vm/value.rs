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
}
