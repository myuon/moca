/// A 64-bit tagged value.
///
/// For v0, we use a simple representation:
/// - Integers are stored directly (SMI - small integers)
/// - Bools are stored as 0 or 1
///
/// In future versions, we'll use proper tagging with the lower bits.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Bool(b) => Some(if *b { 1 } else { 0 }),
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            Value::Int(n) => *n != 0,
            Value::Bool(b) => *b,
        }
    }

    pub fn is_truthy(&self) -> bool {
        self.as_bool()
    }
}

impl std::fmt::Display for Value {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Int(n) => write!(f, "{}", n),
            Value::Bool(b) => write!(f, "{}", b),
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
    fn test_bool_as_bool() {
        assert!(Value::Bool(true).as_bool());
        assert!(!Value::Bool(false).as_bool());
    }

    #[test]
    fn test_int_truthiness() {
        assert!(Value::Int(1).is_truthy());
        assert!(Value::Int(-1).is_truthy());
        assert!(!Value::Int(0).is_truthy());
    }
}
