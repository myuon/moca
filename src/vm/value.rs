use std::fmt;

use super::heap::GcRef;

/// A 64-bit tagged value.
///
/// For v1, we support:
/// - Int: 64-bit signed integer
/// - Float: 64-bit IEEE 754 double
/// - Bool: true/false
/// - Nil: null value
/// - Ptr: pointer to heap object (String, Array, Object)
#[derive(Clone, Copy)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Nil,
    Ptr(GcRef),
}

impl Value {
    pub fn is_nil(&self) -> bool {
        matches!(self, Value::Nil)
    }

    pub fn is_int(&self) -> bool {
        matches!(self, Value::Int(_))
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Value::Float(_))
    }

    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    pub fn is_ptr(&self) -> bool {
        matches!(self, Value::Ptr(_))
    }

    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
            _ => None,
        }
    }

    pub fn as_float(&self) -> Option<f64> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Int(n) => *n != 0,
            Value::Float(f) => *f != 0.0,
            Value::Bool(b) => *b,
            Value::Nil => false,
            Value::Ptr(_) => true, // Objects are truthy
        }
    }

    pub fn is_truthy(&self) -> bool {
        self.as_bool()
    }

    pub fn as_ptr(&self) -> Option<GcRef> {
        match self {
            Value::Ptr(r) => Some(*r),
            _ => None,
        }
    }

    /// Get the type name of this value.
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Int(_) => "int",
            Value::Float(_) => "float",
            Value::Bool(_) => "bool",
            Value::Nil => "nil",
            Value::Ptr(_) => "object", // Will be refined based on heap object type
        }
    }

    /// Check if two values are equal.
    pub fn eq(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
            (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::Nil, Value::Nil) => true,
            (Value::Ptr(a), Value::Ptr(b)) => a.index == b.index,
            _ => false,
        }
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
            Value::Int(n) => write!(f, "Int({})", n),
            Value::Float(n) => write!(f, "Float({})", n),
            Value::Bool(b) => write!(f, "Bool({})", b),
            Value::Nil => write!(f, "Nil"),
            Value::Ptr(r) => write!(f, "Ptr({})", r.index),
        }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Float(n) => {
                if n.fract() == 0.0 {
                    write!(f, "{}.0", n)
                } else {
                    write!(f, "{}", n)
                }
            }
            Value::Bool(b) => write!(f, "{}", b),
            Value::Nil => write!(f, "nil"),
            Value::Ptr(_) => write!(f, "<object>"), // Will be refined when heap is accessible
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_int_as_int() {
        assert_eq!(Value::Int(42).as_int(), Some(42));
    }

    #[test]
    fn test_float_as_float() {
        assert_eq!(Value::Float(3.14).as_float(), Some(3.14));
    }

    #[test]
    fn test_int_as_float() {
        assert_eq!(Value::Int(42).as_float(), Some(42.0));
    }

    #[test]
    fn test_bool_as_bool() {
        assert!(Value::Bool(true).as_bool());
        assert!(!Value::Bool(false).as_bool());
    }

    #[test]
    fn test_nil_is_falsy() {
        assert!(!Value::Nil.is_truthy());
    }

    #[test]
    fn test_int_truthiness() {
        assert!(Value::Int(1).is_truthy());
        assert!(Value::Int(-1).is_truthy());
        assert!(!Value::Int(0).is_truthy());
    }

    #[test]
    fn test_float_truthiness() {
        assert!(Value::Float(1.0).is_truthy());
        assert!(!Value::Float(0.0).is_truthy());
    }

    #[test]
    fn test_equality() {
        assert!(Value::Int(42).eq(&Value::Int(42)));
        assert!(Value::Float(3.14).eq(&Value::Float(3.14)));
        assert!(Value::Int(42).eq(&Value::Float(42.0)));
        assert!(Value::Nil.eq(&Value::Nil));
        assert!(!Value::Int(1).eq(&Value::Nil));
    }
}
