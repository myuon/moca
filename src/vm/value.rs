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
    pub fn eq(&self, other: &Value) -> bool {
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
        Value::eq(self, other)
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
        assert!(Value::I64(42).eq(&Value::I64(42)));
        assert!(Value::F64(3.14).eq(&Value::F64(3.14)));
        assert!(Value::I64(42).eq(&Value::F64(42.0)));
        assert!(Value::Null.eq(&Value::Null));
        assert!(!Value::I64(1).eq(&Value::Null));
    }
}
