//! Type definitions for the moca type system.
//!
//! This module defines the core type representations used for
//! Hindley-Milner type inference (Algorithm W).

use std::collections::BTreeMap;
use std::fmt;

/// A unique identifier for type variables during inference.
pub type TypeVarId = u32;

/// Core type representation for the moca type system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Type {
    /// Integer type: `int`
    Int,
    /// Floating-point type: `float`
    Float,
    /// Boolean type: `bool`
    Bool,
    /// String type: `string`
    String,
    /// Nil type: `nil`
    Nil,
    /// Array type: `array<T>`
    Array(Box<Type>),
    /// Vector type: `vec<T>` (dynamic array)
    Vector(Box<Type>),
    /// Map type: `map<K, V>` (hash map)
    Map(Box<Type>, Box<Type>),
    /// Object type with named fields: `{field1: T1, field2: T2, ...}`
    /// Uses BTreeMap for deterministic ordering.
    Object(BTreeMap<std::string::String, Type>),
    /// Nullable type: `T?` (equivalent to T | nil)
    Nullable(Box<Type>),
    /// Function type: `(T1, T2, ...) -> R`
    Function { params: Vec<Type>, ret: Box<Type> },
    /// Struct type: a named type with fixed fields in declaration order.
    /// Unlike Object, structs use nominal typing (name must match).
    Struct {
        name: std::string::String,
        /// Fields in declaration order (name, type)
        fields: Vec<(std::string::String, Type)>,
    },
    /// Type variable for inference (unresolved type).
    /// These are resolved during unification in Algorithm W.
    Var(TypeVarId),
    /// Raw heap pointer type: `ptr<T>`
    Ptr(Box<Type>),
    /// Any type: bypasses type checking, unifies with any other type.
    /// When unified with another type T, the result is T.
    Any,
    /// Type parameter (generic type variable): `T`, `U`, etc.
    /// Used in generic function/struct definitions.
    Param { name: std::string::String },
    /// Generic struct instantiation: `Container<int>`, `Pair<T, U>`
    GenericStruct {
        name: std::string::String,
        /// Type arguments (concrete types or type parameters)
        type_args: Vec<Type>,
        /// Fields in declaration order (name, type) with type params substituted
        fields: Vec<(std::string::String, Type)>,
    },
}

impl Type {
    /// Create a new array type.
    pub fn array(element: Type) -> Type {
        Type::Array(Box::new(element))
    }

    /// Create a new vector type.
    pub fn vector(element: Type) -> Type {
        Type::Vector(Box::new(element))
    }

    /// Create a new map type.
    pub fn map(key: Type, value: Type) -> Type {
        Type::Map(Box::new(key), Box::new(value))
    }

    /// Create a new nullable type.
    pub fn nullable(inner: Type) -> Type {
        Type::Nullable(Box::new(inner))
    }

    /// Create a new function type.
    pub fn function(params: Vec<Type>, ret: Type) -> Type {
        Type::Function {
            params,
            ret: Box::new(ret),
        }
    }

    /// Create an empty object type.
    pub fn empty_object() -> Type {
        Type::Object(BTreeMap::new())
    }

    /// Create an object type from field definitions.
    pub fn object(fields: impl IntoIterator<Item = (std::string::String, Type)>) -> Type {
        Type::Object(fields.into_iter().collect())
    }

    /// Check if this type contains any type variables.
    pub fn has_type_vars(&self) -> bool {
        match self {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Nil | Type::Any => false,
            Type::Var(_) => true,
            Type::Param { .. } => false, // Type params are not inference variables
            Type::Ptr(elem) | Type::Array(elem) | Type::Vector(elem) => elem.has_type_vars(),
            Type::Map(key, value) => key.has_type_vars() || value.has_type_vars(),
            Type::Nullable(inner) => inner.has_type_vars(),
            Type::Object(fields) => fields.values().any(|t| t.has_type_vars()),
            Type::Struct { fields, .. } => fields.iter().any(|(_, t)| t.has_type_vars()),
            Type::GenericStruct {
                type_args, fields, ..
            } => {
                type_args.iter().any(|t| t.has_type_vars())
                    || fields.iter().any(|(_, t)| t.has_type_vars())
            }
            Type::Function { params, ret } => {
                params.iter().any(|t| t.has_type_vars()) || ret.has_type_vars()
            }
        }
    }

    /// Collect all type variable IDs in this type.
    pub fn free_type_vars(&self) -> Vec<TypeVarId> {
        let mut vars = Vec::new();
        self.collect_type_vars(&mut vars);
        vars
    }

    fn collect_type_vars(&self, vars: &mut Vec<TypeVarId>) {
        match self {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Nil | Type::Any => {}
            Type::Param { .. } => {} // Type params are not inference variables
            Type::Var(id) => {
                if !vars.contains(id) {
                    vars.push(*id);
                }
            }
            Type::Ptr(elem) | Type::Array(elem) | Type::Vector(elem) => {
                elem.collect_type_vars(vars)
            }
            Type::Map(key, value) => {
                key.collect_type_vars(vars);
                value.collect_type_vars(vars);
            }
            Type::Nullable(inner) => inner.collect_type_vars(vars),
            Type::Object(fields) => {
                for t in fields.values() {
                    t.collect_type_vars(vars);
                }
            }
            Type::Struct { fields, .. } => {
                for (_, t) in fields {
                    t.collect_type_vars(vars);
                }
            }
            Type::GenericStruct {
                type_args, fields, ..
            } => {
                for t in type_args {
                    t.collect_type_vars(vars);
                }
                for (_, t) in fields {
                    t.collect_type_vars(vars);
                }
            }
            Type::Function { params, ret } => {
                for t in params {
                    t.collect_type_vars(vars);
                }
                ret.collect_type_vars(vars);
            }
        }
    }

    /// Substitute a type parameter with a concrete type.
    /// Returns a new type with all occurrences of `Type::Param { name }` replaced with `replacement`.
    pub fn substitute_param(&self, param_name: &str, replacement: &Type) -> Type {
        match self {
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Nil | Type::Any => {
                self.clone()
            }
            Type::Var(_) => self.clone(),
            Type::Param { name } => {
                if name == param_name {
                    replacement.clone()
                } else {
                    self.clone()
                }
            }
            Type::Ptr(elem) => Type::Ptr(Box::new(elem.substitute_param(param_name, replacement))),
            Type::Array(elem) => {
                Type::Array(Box::new(elem.substitute_param(param_name, replacement)))
            }
            Type::Vector(elem) => {
                Type::Vector(Box::new(elem.substitute_param(param_name, replacement)))
            }
            Type::Map(key, value) => Type::Map(
                Box::new(key.substitute_param(param_name, replacement)),
                Box::new(value.substitute_param(param_name, replacement)),
            ),
            Type::Nullable(inner) => {
                Type::Nullable(Box::new(inner.substitute_param(param_name, replacement)))
            }
            Type::Object(fields) => {
                let new_fields: BTreeMap<std::string::String, Type> = fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.substitute_param(param_name, replacement)))
                    .collect();
                Type::Object(new_fields)
            }
            Type::Struct { name, fields } => Type::Struct {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), t.substitute_param(param_name, replacement)))
                    .collect(),
            },
            Type::GenericStruct {
                name,
                type_args,
                fields,
            } => Type::GenericStruct {
                name: name.clone(),
                type_args: type_args
                    .iter()
                    .map(|t| t.substitute_param(param_name, replacement))
                    .collect(),
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), t.substitute_param(param_name, replacement)))
                    .collect(),
            },
            Type::Function { params, ret } => Type::Function {
                params: params
                    .iter()
                    .map(|t| t.substitute_param(param_name, replacement))
                    .collect(),
                ret: Box::new(ret.substitute_param(param_name, replacement)),
            },
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Type::Int => write!(f, "int"),
            Type::Float => write!(f, "float"),
            Type::Bool => write!(f, "bool"),
            Type::String => write!(f, "string"),
            Type::Nil => write!(f, "nil"),
            Type::Array(elem) => write!(f, "array<{}>", elem),
            Type::Vector(elem) => write!(f, "vec<{}>", elem),
            Type::Map(key, value) => write!(f, "map<{}, {}>", key, value),
            Type::Nullable(inner) => write!(f, "{}?", inner),
            Type::Object(fields) => {
                write!(f, "{{")?;
                let mut first = true;
                for (name, ty) in fields {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", name, ty)?;
                    first = false;
                }
                write!(f, "}}")
            }
            Type::Function { params, ret } => {
                write!(f, "(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") -> {}", ret)
            }
            Type::Struct { name, .. } => write!(f, "{}", name),
            Type::Var(id) => write!(f, "?T{}", id),
            Type::Ptr(elem) => write!(f, "ptr<{}>", elem),
            Type::Any => write!(f, "any"),
            Type::Param { name } => write!(f, "{}", name),
            Type::GenericStruct {
                name, type_args, ..
            } => {
                write!(f, "{}<", name)?;
                for (i, arg) in type_args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
        }
    }
}

/// Type annotation as it appears in source code (AST representation).
/// This is parsed from source and later converted to Type during type checking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeAnnotation {
    /// Simple named type: `int`, `float`, `bool`, `string`, `nil`
    Named(std::string::String),
    /// Array type: `array<T>`
    Array(Box<TypeAnnotation>),
    /// Vector type: `vec<T>`
    Vec(Box<TypeAnnotation>),
    /// Map type: `map<K, V>`
    Map(Box<TypeAnnotation>, Box<TypeAnnotation>),
    /// Object type: `{field1: T1, field2: T2}`
    Object(Vec<(std::string::String, TypeAnnotation)>),
    /// Nullable type: `T?`
    Nullable(Box<TypeAnnotation>),
    /// Function type: `(T1, T2) -> R`
    Function {
        params: Vec<TypeAnnotation>,
        ret: Box<TypeAnnotation>,
    },
    /// Generic type with type arguments: `Container<int>`, `Pair<T, U>`
    Generic {
        name: std::string::String,
        type_args: Vec<TypeAnnotation>,
    },
}

impl TypeAnnotation {
    /// Convert a type annotation to a concrete Type.
    /// Returns an error if the type name is unknown.
    /// Note: This basic conversion doesn't handle struct types or generic instantiation.
    /// Those require context from the typechecker.
    pub fn to_type(&self) -> Result<Type, String> {
        match self {
            TypeAnnotation::Named(name) => match name.as_str() {
                "int" => Ok(Type::Int),
                "float" => Ok(Type::Float),
                "bool" => Ok(Type::Bool),
                "string" => Ok(Type::String),
                "nil" => Ok(Type::Nil),
                "any" => Ok(Type::Any),
                _ => Err(format!("unknown type: {}", name)),
            },
            TypeAnnotation::Array(elem) => Ok(Type::array(elem.to_type()?)),
            TypeAnnotation::Vec(elem) => Ok(Type::vector(elem.to_type()?)),
            TypeAnnotation::Map(key, value) => Ok(Type::map(key.to_type()?, value.to_type()?)),
            TypeAnnotation::Object(fields) => {
                let mut type_fields = BTreeMap::new();
                for (name, ann) in fields {
                    type_fields.insert(name.clone(), ann.to_type()?);
                }
                Ok(Type::Object(type_fields))
            }
            TypeAnnotation::Nullable(inner) => Ok(Type::nullable(inner.to_type()?)),
            TypeAnnotation::Function { params, ret } => {
                let param_types: Result<Vec<_>, _> = params.iter().map(|p| p.to_type()).collect();
                Ok(Type::function(param_types?, ret.to_type()?))
            }
            TypeAnnotation::Generic { name, type_args } => {
                if name == "ptr" {
                    if type_args.len() != 1 {
                        return Err("ptr expects exactly 1 type argument".to_string());
                    }
                    Ok(Type::Ptr(Box::new(type_args[0].to_type()?)))
                } else {
                    // Generic types need context from typechecker to resolve
                    Err(format!(
                        "generic type '{}' requires typechecker context",
                        name
                    ))
                }
            }
        }
    }
}

impl fmt::Display for TypeAnnotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeAnnotation::Named(name) => write!(f, "{}", name),
            TypeAnnotation::Array(elem) => write!(f, "array<{}>", elem),
            TypeAnnotation::Vec(elem) => write!(f, "vec<{}>", elem),
            TypeAnnotation::Map(key, value) => write!(f, "map<{}, {}>", key, value),
            TypeAnnotation::Object(fields) => {
                write!(f, "{{")?;
                let mut first = true;
                for (name, ty) in fields {
                    if !first {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}: {}", name, ty)?;
                    first = false;
                }
                write!(f, "}}")
            }
            TypeAnnotation::Nullable(inner) => write!(f, "{}?", inner),
            TypeAnnotation::Function { params, ret } => {
                write!(f, "(")?;
                for (i, param) in params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", param)?;
                }
                write!(f, ") -> {}", ret)
            }
            TypeAnnotation::Generic { name, type_args } => {
                write!(f, "{}<", name)?;
                for (i, arg) in type_args.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", arg)?;
                }
                write!(f, ">")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_type_display() {
        assert_eq!(Type::Int.to_string(), "int");
        assert_eq!(Type::Float.to_string(), "float");
        assert_eq!(Type::Bool.to_string(), "bool");
        assert_eq!(Type::String.to_string(), "string");
        assert_eq!(Type::Nil.to_string(), "nil");
        assert_eq!(Type::Any.to_string(), "any");
        assert_eq!(Type::array(Type::Int).to_string(), "array<int>");
        assert_eq!(Type::vector(Type::Int).to_string(), "vec<int>");
        assert_eq!(
            Type::map(Type::String, Type::Int).to_string(),
            "map<string, int>"
        );
        assert_eq!(Type::nullable(Type::String).to_string(), "string?");
        assert_eq!(Type::Var(0).to_string(), "?T0");

        let obj = Type::object([
            ("x".to_string(), Type::Int),
            ("y".to_string(), Type::String),
        ]);
        assert_eq!(obj.to_string(), "{x: int, y: string}");

        let func = Type::function(vec![Type::Int, Type::Int], Type::Int);
        assert_eq!(func.to_string(), "(int, int) -> int");
    }

    #[test]
    fn test_type_annotation_to_type() {
        assert_eq!(
            TypeAnnotation::Named("int".to_string()).to_type().unwrap(),
            Type::Int
        );
        assert_eq!(
            TypeAnnotation::Named("string".to_string())
                .to_type()
                .unwrap(),
            Type::String
        );
        assert_eq!(
            TypeAnnotation::Named("any".to_string()).to_type().unwrap(),
            Type::Any
        );
        assert_eq!(
            TypeAnnotation::Array(Box::new(TypeAnnotation::Named("int".to_string())))
                .to_type()
                .unwrap(),
            Type::array(Type::Int)
        );
        assert_eq!(
            TypeAnnotation::Vec(Box::new(TypeAnnotation::Named("int".to_string())))
                .to_type()
                .unwrap(),
            Type::vector(Type::Int)
        );
        assert_eq!(
            TypeAnnotation::Map(
                Box::new(TypeAnnotation::Named("string".to_string())),
                Box::new(TypeAnnotation::Named("int".to_string()))
            )
            .to_type()
            .unwrap(),
            Type::map(Type::String, Type::Int)
        );
        assert_eq!(
            TypeAnnotation::Nullable(Box::new(TypeAnnotation::Named("string".to_string())))
                .to_type()
                .unwrap(),
            Type::nullable(Type::String)
        );

        // Unknown type should error
        assert!(
            TypeAnnotation::Named("unknown".to_string())
                .to_type()
                .is_err()
        );
    }

    #[test]
    fn test_has_type_vars() {
        assert!(!Type::Int.has_type_vars());
        assert!(!Type::Any.has_type_vars());
        assert!(!Type::array(Type::Int).has_type_vars());
        assert!(!Type::vector(Type::Int).has_type_vars());
        assert!(!Type::map(Type::String, Type::Int).has_type_vars());
        assert!(Type::Var(0).has_type_vars());
        assert!(Type::array(Type::Var(0)).has_type_vars());
        assert!(Type::vector(Type::Var(0)).has_type_vars());
        assert!(Type::map(Type::Var(0), Type::Int).has_type_vars());
        assert!(Type::map(Type::String, Type::Var(0)).has_type_vars());
        assert!(Type::function(vec![Type::Var(0)], Type::Int).has_type_vars());
    }

    #[test]
    fn test_free_type_vars() {
        assert!(Type::Int.free_type_vars().is_empty());
        assert_eq!(Type::Var(0).free_type_vars(), vec![0]);
        assert_eq!(Type::Var(5).free_type_vars(), vec![5]);

        let func = Type::function(vec![Type::Var(1), Type::Var(2)], Type::Var(1));
        let vars = func.free_type_vars();
        assert!(vars.contains(&1));
        assert!(vars.contains(&2));

        let map_type = Type::map(Type::Var(3), Type::Var(4));
        let map_vars = map_type.free_type_vars();
        assert!(map_vars.contains(&3));
        assert!(map_vars.contains(&4));
    }
}
