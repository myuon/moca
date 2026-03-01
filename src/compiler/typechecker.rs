//! Type checker with Hindley-Milner type inference (Algorithm W).
//!
//! This module implements:
//! - Type variable management
//! - Substitution (mapping from type variables to types)
//! - Unification algorithm
//! - Type inference for expressions and statements

// TypeError contains detailed error information, hence it's large
#![allow(clippy::result_large_err)]

use crate::compiler::ast::{
    BinaryOp, Block, Expr, FnDef, ImplBlock, InterfaceDef, Item, NewLiteralElement, Param, Program,
    Statement, StructDef, UnaryOp,
};
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation, TypeVarId};
use std::collections::{HashMap, HashSet};
use std::mem;

/// Information about a struct definition.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    /// Type parameters for generic structs: `struct Container<T> { ... }`
    pub type_params: Vec<String>,
    /// Fields in declaration order: (name, type)
    /// For generic structs, field types may contain Type::Param
    pub fields: Vec<(String, Type)>,
    /// Methods from impl blocks: method_name -> function type
    pub methods: HashMap<String, Type>,
}

/// A type error with location information.
#[derive(Debug, Clone)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
    pub expected: Option<Type>,
    pub found: Option<Type>,
}

impl TypeError {
    pub fn new(message: impl Into<String>, span: Span) -> Self {
        Self {
            message: message.into(),
            span,
            expected: None,
            found: None,
        }
    }

    pub fn mismatch(expected: Type, found: Type, span: Span) -> Self {
        Self {
            message: format!("expected `{}`, found `{}`", expected, found),
            span,
            expected: Some(expected),
            found: Some(found),
        }
    }
}

/// Substitution: a mapping from type variables to types.
#[derive(Debug, Clone, Default)]
pub struct Substitution {
    mapping: HashMap<TypeVarId, Type>,
}

impl Substitution {
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply this substitution to a type.
    pub fn apply(&self, ty: &Type) -> Type {
        match ty {
            Type::Var(id) => {
                if let Some(t) = self.mapping.get(id) {
                    // Recursively apply in case t contains other type variables
                    self.apply(t)
                } else {
                    ty.clone()
                }
            }
            Type::Ptr(elem) => Type::Ptr(Box::new(self.apply(elem))),
            Type::Nullable(inner) => Type::Nullable(Box::new(self.apply(inner))),
            Type::Function { params, ret } => Type::Function {
                params: params.iter().map(|p| self.apply(p)).collect(),
                ret: Box::new(self.apply(ret)),
            },
            Type::Struct { name, fields } => Type::Struct {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), self.apply(t)))
                    .collect(),
            },
            Type::GenericStruct {
                name,
                type_args,
                fields,
            } => Type::GenericStruct {
                name: name.clone(),
                type_args: type_args.iter().map(|t| self.apply(t)).collect(),
                fields: fields
                    .iter()
                    .map(|(n, t)| (n.clone(), self.apply(t)))
                    .collect(),
            },
            // Type parameters and interface bounds are unchanged
            Type::Param { .. } | Type::InterfaceBound { .. } => ty.clone(),
            // Primitive types, Any, and Dyn are unchanged
            Type::Int
            | Type::Float
            | Type::Bool
            | Type::Byte
            | Type::Char
            | Type::Nil
            | Type::Any
            | Type::Dyn => ty.clone(),
        }
    }

    /// Extend this substitution with a new mapping.
    pub fn extend(&mut self, var: TypeVarId, ty: Type) {
        self.mapping.insert(var, ty);
    }

    /// Compose two substitutions: apply s2 after s1.
    pub fn compose(&self, other: &Substitution) -> Substitution {
        let mut result = Substitution::new();

        // Apply other to all types in self
        for (k, v) in &self.mapping {
            result.mapping.insert(*k, other.apply(v));
        }

        // Add mappings from other that aren't in self
        for (k, v) in &other.mapping {
            result.mapping.entry(*k).or_insert_with(|| v.clone());
        }

        result
    }
}

/// Type environment: maps variable names to types.
#[derive(Debug, Clone, Default)]
pub struct TypeEnv {
    bindings: Vec<HashMap<String, Type>>,
}

impl TypeEnv {
    pub fn new() -> Self {
        Self {
            bindings: vec![HashMap::new()],
        }
    }

    pub fn enter_scope(&mut self) {
        self.bindings.push(HashMap::new());
    }

    pub fn exit_scope(&mut self) {
        self.bindings.pop();
    }

    pub fn bind(&mut self, name: String, ty: Type) {
        if let Some(scope) = self.bindings.last_mut() {
            scope.insert(name, ty);
        }
    }

    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for scope in self.bindings.iter().rev() {
            if let Some(ty) = scope.get(name) {
                return Some(ty);
            }
        }
        None
    }

    /// Apply a substitution to all types in the environment.
    pub fn apply_substitution(&mut self, subst: &Substitution) {
        for scope in &mut self.bindings {
            for ty in scope.values_mut() {
                *ty = subst.apply(ty);
            }
        }
    }
}

/// Information about a generic function
#[derive(Debug, Clone)]
pub struct GenericFunctionInfo {
    /// Type parameters: T, U, etc.
    pub type_params: Vec<String>,
    /// Interface bounds for each type parameter (parallel to `type_params`)
    pub type_param_bounds: Vec<Vec<String>>,
    /// Function type (with Type::Param for generic parameters)
    pub fn_type: Type,
    /// Internal name used for distinguishing overloaded variants.
    /// For non-overloaded functions this equals the function name.
    pub internal_name: String,
}

/// Information about an interface definition
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    /// Method signatures: method_name -> function type (params excluding self -> ret)
    pub methods: HashMap<String, Type>,
}

/// The type checker with inference support.
pub struct TypeChecker {
    filename: String,
    next_var_id: TypeVarId,
    errors: Vec<TypeError>,
    /// Function signatures (name -> type)
    functions: HashMap<String, Type>,
    /// Generic function signatures (name -> overload variants)
    generic_functions: HashMap<String, Vec<GenericFunctionInfo>>,
    /// Struct definitions (name -> struct info)
    structs: HashMap<String, StructInfo>,
    /// Primitive type methods: type_name -> (method_name -> method_type)
    primitive_methods: HashMap<String, HashMap<String, Type>>,
    /// Interface definitions: name -> InterfaceInfo
    interfaces: HashMap<String, InterfaceInfo>,
    /// Interface implementations: (interface_name, type_name)
    interface_impls: HashSet<(String, String)>,
    /// Substitution accumulated during inference
    substitution: Substitution,
    /// Current type parameters in scope (during function signature inference)
    current_type_params: Vec<String>,
    /// Current type parameter bounds in scope: param_name -> interface names
    current_type_param_bounds: HashMap<String, Vec<String>>,
    /// Name of the function currently being type-checked (None for top-level)
    current_function_name: Option<String>,
}

impl TypeChecker {
    pub fn new(filename: &str) -> Self {
        let mut structs = HashMap::new();

        // Pre-register builtin generic types so impl block methods
        // are stored in their StructInfo (just like user-defined structs).
        structs.insert(
            "Vec".to_string(),
            StructInfo {
                name: "Vec".to_string(),
                type_params: vec!["T".to_string()],
                fields: vec![],
                methods: HashMap::new(),
            },
        );
        structs.insert(
            "Map".to_string(),
            StructInfo {
                name: "Map".to_string(),
                type_params: vec!["K".to_string(), "V".to_string()],
                fields: vec![],
                methods: HashMap::new(),
            },
        );

        Self {
            filename: filename.to_string(),
            next_var_id: 0,
            errors: Vec::new(),
            functions: HashMap::new(),
            generic_functions: HashMap::new(),
            structs,
            primitive_methods: HashMap::new(),
            interfaces: HashMap::new(),
            interface_impls: HashSet::new(),
            substitution: Substitution::new(),
            current_type_params: Vec::new(),
            current_type_param_bounds: HashMap::new(),
            current_function_name: None,
        }
    }

    /// Generate a fresh type variable.
    fn fresh_var(&mut self) -> Type {
        let id = self.next_var_id;
        self.next_var_id += 1;
        Type::Var(id)
    }

    /// Convert a type annotation to a Type, resolving struct names.
    fn resolve_type_annotation(&self, ann: &TypeAnnotation, span: Span) -> Result<Type, TypeError> {
        match ann {
            TypeAnnotation::Named(name) => {
                // First try primitive types
                match name.as_str() {
                    "int" => Ok(Type::Int),
                    "float" => Ok(Type::Float),
                    "bool" => Ok(Type::Bool),
                    "byte" => Ok(Type::Byte),
                    "char" => Ok(Type::Char),
                    "string" => Ok(Type::string()),
                    "nil" => Ok(Type::Nil),
                    "any" => Ok(Type::Any),
                    "dyn" => Ok(Type::Dyn),
                    _ => {
                        // Check if it's a type parameter in scope
                        if self.current_type_params.contains(name) {
                            return Ok(Type::Param { name: name.clone() });
                        }
                        // Try to find a struct with this name
                        if let Some(info) = self.structs.get(name) {
                            Ok(Type::Struct {
                                name: info.name.clone(),
                                fields: info.fields.clone(),
                            })
                        } else {
                            Err(TypeError::new(format!("unknown type: {}", name), span))
                        }
                    }
                }
            }
            TypeAnnotation::Array(elem) => {
                let elem_type = self.resolve_type_annotation(elem, span)?;
                Ok(Type::array(elem_type))
            }
            TypeAnnotation::Vec(elem) => {
                let elem_type = self.resolve_type_annotation(elem, span)?;
                Ok(Type::vector(elem_type))
            }
            TypeAnnotation::Map(key, value) => {
                let key_type = self.resolve_type_annotation(key, span)?;
                let value_type = self.resolve_type_annotation(value, span)?;
                Ok(Type::map(key_type, value_type))
            }
            TypeAnnotation::Nullable(inner) => {
                let inner_type = self.resolve_type_annotation(inner, span)?;
                Ok(Type::nullable(inner_type))
            }
            TypeAnnotation::Function { params, ret } => {
                let param_types: Result<Vec<_>, _> = params
                    .iter()
                    .map(|p| self.resolve_type_annotation(p, span))
                    .collect();
                Ok(Type::function(
                    param_types?,
                    self.resolve_type_annotation(ret, span)?,
                ))
            }
            TypeAnnotation::Generic { name, type_args } => {
                // Handle ptr<T>
                if name == "ptr" {
                    if type_args.len() != 1 {
                        return Err(TypeError::new("ptr expects exactly 1 type argument", span));
                    }
                    let elem = self.resolve_type_annotation(&type_args[0], span)?;
                    return Ok(Type::Ptr(Box::new(elem)));
                }

                // Look up struct definition
                if let Some(struct_info) = self.structs.get(name).cloned() {
                    // Check type argument count
                    if type_args.len() != struct_info.type_params.len() {
                        return Err(TypeError::new(
                            format!(
                                "struct `{}` expects {} type arguments, got {}",
                                name,
                                struct_info.type_params.len(),
                                type_args.len()
                            ),
                            span,
                        ));
                    }

                    // Resolve type arguments
                    let resolved_type_args: Vec<Type> = type_args
                        .iter()
                        .map(|ta| self.resolve_type_annotation(ta, span))
                        .collect::<Result<Vec<_>, _>>()?;

                    // Substitute type params in field types
                    let instantiated_fields: Vec<(String, Type)> = struct_info
                        .fields
                        .iter()
                        .map(|(fname, ftype)| {
                            let mut substituted = ftype.clone();
                            for (param_name, type_arg) in struct_info
                                .type_params
                                .iter()
                                .zip(resolved_type_args.iter())
                            {
                                substituted = substituted.substitute_param(param_name, type_arg);
                            }
                            (fname.clone(), substituted)
                        })
                        .collect();

                    Ok(Type::GenericStruct {
                        name: name.clone(),
                        type_args: resolved_type_args,
                        fields: instantiated_fields,
                    })
                } else {
                    Err(TypeError::new(
                        format!("unknown generic type: {}", name),
                        span,
                    ))
                }
            }
        }
    }

    /// Unify two types, returning a substitution that makes them equal.
    fn unify(&mut self, t1: &Type, t2: &Type, span: Span) -> Result<Substitution, TypeError> {
        let t1 = self.substitution.apply(t1);
        let t2 = self.substitution.apply(t2);

        match (&t1, &t2) {
            // Same types unify trivially
            (Type::Int, Type::Int)
            | (Type::Float, Type::Float)
            | (Type::Bool, Type::Bool)
            | (Type::Byte, Type::Byte)
            | (Type::Char, Type::Char)
            | (Type::Nil, Type::Nil) => Ok(Substitution::new()),

            // byte and int are implicitly convertible (byte is stored as i64 at runtime)
            (Type::Byte, Type::Int) | (Type::Int, Type::Byte) => Ok(Substitution::new()),

            // char and int are implicitly convertible (char is stored as i64 at runtime)
            (Type::Char, Type::Int) | (Type::Int, Type::Char) => Ok(Substitution::new()),

            // Ptr<T> unification
            (Type::Ptr(a), Type::Ptr(b)) => self.unify(a, b, span),

            // Any type unifies with any other type
            // any ~ T -> T (any adapts to the other type)
            // any ~ any -> any
            (Type::Any, _) | (_, Type::Any) => Ok(Substitution::new()),

            // Dyn only unifies with Dyn (unlike Any which unifies with everything)
            (Type::Dyn, Type::Dyn) => Ok(Substitution::new()),

            // Type variable unification
            (Type::Var(id), other) | (other, Type::Var(id)) => {
                if let Type::Var(other_id) = other
                    && id == other_id
                {
                    return Ok(Substitution::new());
                }
                // Occurs check: prevent infinite types
                if other.free_type_vars().contains(id) {
                    return Err(TypeError::new(
                        format!("infinite type: ?T{} occurs in {}", id, other),
                        span,
                    ));
                }
                let mut subst = Substitution::new();
                subst.extend(*id, other.clone());
                self.substitution = self.substitution.compose(&subst);
                Ok(subst)
            }

            // Type parameter unification: same name = same type
            (Type::Param { name: n1 }, Type::Param { name: n2 }) => {
                if n1 == n2 {
                    Ok(Substitution::new())
                } else {
                    Err(TypeError::mismatch(t1, t2, span))
                }
            }

            // Interface-bounded types: same interface name = same type
            (
                Type::InterfaceBound { interface_name: n1 },
                Type::InterfaceBound { interface_name: n2 },
            ) => {
                if n1 == n2 {
                    Ok(Substitution::new())
                } else {
                    Err(TypeError::mismatch(t1, t2, span))
                }
            }

            // Nullable types
            (Type::Nullable(inner1), Type::Nullable(inner2)) => self.unify(inner1, inner2, span),

            // Nil can be assigned to nullable types: nil -> T?
            (Type::Nullable(_), Type::Nil) | (Type::Nil, Type::Nullable(_)) => {
                Ok(Substitution::new())
            }

            // T can be assigned to T?: T -> T?
            (Type::Nullable(inner), other) | (other, Type::Nullable(inner))
                if !matches!(other, Type::Nullable(_)) && !matches!(other, Type::Nil) =>
            {
                self.unify(inner, other, span)
            }

            // Function types
            (
                Type::Function {
                    params: p1,
                    ret: r1,
                },
                Type::Function {
                    params: p2,
                    ret: r2,
                },
            ) => {
                if p1.len() != p2.len() {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                let mut result = Substitution::new();
                for (param1, param2) in p1.iter().zip(p2.iter()) {
                    let s = self.unify(param1, param2, span)?;
                    result = result.compose(&s);
                }
                let s = self.unify(r1, r2, span)?;
                result = result.compose(&s);
                Ok(result)
            }

            // Struct types - nominal typing: names must match exactly
            (
                Type::Struct {
                    name: n1,
                    fields: f1,
                },
                Type::Struct {
                    name: n2,
                    fields: f2,
                },
            ) => {
                if n1 != n2 {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                if f1.len() != f2.len() {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                let mut result = Substitution::new();
                for ((name1, ty1), (name2, ty2)) in f1.iter().zip(f2.iter()) {
                    if name1 != name2 {
                        return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                    }
                    let s = self.unify(ty1, ty2, span)?;
                    result = result.compose(&s);
                }
                Ok(result)
            }

            // Generic struct types - nominal typing with type argument unification
            (
                Type::GenericStruct {
                    name: n1,
                    type_args: a1,
                    ..
                },
                Type::GenericStruct {
                    name: n2,
                    type_args: a2,
                    ..
                },
            ) => {
                if n1 != n2 {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                if a1.len() != a2.len() {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                let mut result = Substitution::new();
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    let s = self.unify(arg1, arg2, span)?;
                    result = result.compose(&s);
                }
                Ok(result)
            }

            // Mismatch
            _ => Err(TypeError::mismatch(t1, t2, span)),
        }
    }

    /// Type check a program.
    pub fn check_program(&mut self, program: &mut Program) -> Result<(), Vec<TypeError>> {
        // First pass: collect struct definitions and interface definitions
        for item in &program.items {
            match item {
                Item::StructDef(struct_def) => {
                    self.register_struct(struct_def);
                }
                Item::InterfaceDef(interface_def) => {
                    self.register_interface(interface_def);
                }
                _ => {}
            }
        }

        // Second pass: collect function signatures and impl block methods
        for item in &program.items {
            match item {
                Item::FnDef(fn_def) => {
                    let (fn_type, is_generic) = self.infer_function_signature(fn_def);
                    if is_generic {
                        let overloads = self
                            .generic_functions
                            .entry(fn_def.name.clone())
                            .or_default();
                        let index = overloads.len();
                        let internal_name = if index == 0 {
                            fn_def.name.clone()
                        } else {
                            // When we add a second overload, rename the first one too
                            if index == 1 {
                                overloads[0].internal_name = format!("{}$$0", fn_def.name);
                            }
                            format!("{}$${}", fn_def.name, index)
                        };
                        overloads.push(GenericFunctionInfo {
                            type_params: fn_def.type_params.clone(),
                            type_param_bounds: fn_def.type_param_bounds.clone(),
                            fn_type: fn_type.clone(),
                            internal_name,
                        });
                    }
                    self.functions.insert(fn_def.name.clone(), fn_type);
                }
                Item::ImplBlock(impl_block) => {
                    self.register_impl_methods(impl_block);
                    self.register_interface_impl(impl_block);
                }
                _ => {}
            }
        }

        // Register overloaded function variants under their internal names.
        // This allows subsequent lookups (e.g., when a call is inferred twice)
        // to find the function by its rewritten name (e.g., show$$0).
        {
            let additional: Vec<(String, GenericFunctionInfo)> = self
                .generic_functions
                .values()
                .filter(|overloads| overloads.len() > 1)
                .flat_map(|overloads| {
                    overloads
                        .iter()
                        .map(|o| (o.internal_name.clone(), o.clone()))
                })
                .collect();
            for (internal_name, info) in additional {
                self.functions
                    .insert(internal_name.clone(), info.fn_type.clone());
                self.generic_functions.insert(internal_name, vec![info]);
            }
        }

        // Pass 2.5: auto-derive ToString for structs without explicit impl
        let struct_names: Vec<String> = self.structs.keys().cloned().collect();
        let mut synthetic_items = Vec::new();
        for struct_name in &struct_names {
            // Skip if explicit impl ToString already exists
            if self
                .interface_impls
                .contains(&("ToString".to_string(), struct_name.clone()))
            {
                continue;
            }
            // Skip internal/builtin/container types (these have their own formatting via dyn)
            if struct_name.starts_with("__")
                || matches!(
                    struct_name.as_str(),
                    "Vec" | "Map" | "Array" | "HashMapEntry"
                )
            {
                continue;
            }
            if let Some(impl_block) = self.generate_tostring_impl(struct_name) {
                self.register_impl_methods(&impl_block);
                self.register_interface_impl(&impl_block);
                synthetic_items.push(Item::ImplBlock(impl_block));
            }
        }
        program.items.extend(synthetic_items);

        // Pass 2.6: auto-derive WriteTo for structs that have ToString.
        // WriteTo wraps to_string() + write_str(), so structs can be used with print<T: WriteTo>.
        let mut writeto_items = Vec::new();
        for struct_name in &struct_names {
            // Skip if WriteTo already explicitly implemented
            if self
                .interface_impls
                .contains(&("WriteTo".to_string(), struct_name.clone()))
            {
                continue;
            }
            // Only derive for types that have ToString
            if !self
                .interface_impls
                .contains(&("ToString".to_string(), struct_name.clone()))
            {
                continue;
            }
            if let Some(impl_block) = self.generate_writeto_from_tostring_impl(struct_name) {
                self.register_impl_methods(&impl_block);
                self.register_interface_impl(&impl_block);
                writeto_items.push(Item::ImplBlock(impl_block));
            }
        }
        program.items.extend(writeto_items);

        // Pass 2.7: Rename overloaded generic function definitions.
        // When the same function name has multiple generic definitions with different
        // bounds, each definition gets a unique internal name (e.g., print$$0, print$$1).
        {
            let mut overload_counters: HashMap<String, usize> = HashMap::new();
            for item in &mut program.items {
                if let Item::FnDef(fn_def) = item
                    && let Some(overloads) = self.generic_functions.get(&fn_def.name)
                    && overloads.len() > 1
                {
                    let counter = overload_counters.entry(fn_def.name.clone()).or_insert(0);
                    if let Some(overload) = overloads.get(*counter) {
                        fn_def.name = overload.internal_name.clone();
                    }
                    *counter += 1;
                }
            }
        }

        // Third pass: type check function bodies and statements (mutable)
        let mut main_env = TypeEnv::new();
        for item in &mut program.items {
            match item {
                Item::FnDef(fn_def) => {
                    self.check_function(fn_def);
                }
                Item::StructDef(_struct_def) => {
                    // Already registered in first pass
                }
                Item::ImplBlock(impl_block) => {
                    self.check_impl_block(impl_block);
                }
                Item::Statement(stmt) => {
                    // Use shared environment for top-level statements
                    self.infer_statement(stmt, &mut main_env);
                }
                Item::InterfaceDef(_) => {
                    // Already handled in first pass
                }
                Item::Import(_) => {
                    // Imports are handled elsewhere
                }
            }
        }

        // Re-apply substitution to all top-level statement types.
        // This resolves type variables created during inference but unified later.
        for item in &mut program.items {
            if let Item::Statement(stmt) = item {
                Self::resolve_stmt_types(&self.substitution, stmt);
            }
        }

        // Apply final substitution and check for unresolved type variables
        self.finalize()?;

        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }

    /// Infer the type signature of a function.
    /// Returns the function type and whether it's a generic function.
    fn infer_function_signature(&mut self, fn_def: &FnDef) -> (Type, bool) {
        // Set current type params for resolving type annotations
        let is_generic = !fn_def.type_params.is_empty();
        self.current_type_params = fn_def.type_params.clone();

        let param_types: Vec<Type> = fn_def
            .params
            .iter()
            .map(|p| {
                if let Some(ann) = &p.type_annotation {
                    self.resolve_type_annotation(ann, p.span)
                        .unwrap_or_else(|e| {
                            self.errors.push(e);
                            self.fresh_var()
                        })
                } else {
                    self.fresh_var()
                }
            })
            .collect();

        let ret_type = if let Some(ann) = &fn_def.return_type {
            self.resolve_type_annotation(ann, fn_def.span)
                .unwrap_or_else(|e| {
                    self.errors.push(e);
                    self.fresh_var()
                })
        } else {
            self.fresh_var()
        };

        // Clear current type params
        self.current_type_params.clear();

        let fn_type = Type::function(param_types, ret_type);
        (fn_type, is_generic)
    }

    /// Type check a function definition.
    fn check_function(&mut self, fn_def: &mut FnDef) {
        let mut env = TypeEnv::new();

        // Set current type params for generic functions
        self.current_type_params = fn_def.type_params.clone();

        // Set current type parameter bounds
        self.current_type_param_bounds.clear();
        for (param, bounds) in fn_def
            .type_params
            .iter()
            .zip(fn_def.type_param_bounds.iter())
        {
            if !bounds.is_empty() {
                self.current_type_param_bounds
                    .insert(param.clone(), bounds.clone());
            }
        }

        // Track current function name for local variable type collection
        self.current_function_name = Some(fn_def.name.clone());

        // Get function type
        let fn_type = self.functions.get(&fn_def.name).cloned();
        let (param_types, expected_ret) = match fn_type {
            Some(Type::Function { params, ret }) => (params, *ret),
            _ => {
                self.current_type_params.clear();
                self.current_function_name = None;
                return;
            }
        };

        // Bind parameters
        for (param, param_type) in fn_def.params.iter().zip(param_types.iter()) {
            env.bind(param.name.clone(), param_type.clone());
        }

        // Infer body type
        let body_type = self.infer_block(&mut fn_def.body, &mut env);

        // Unify return type
        if let Err(e) = self.unify(&body_type, &expected_ret, fn_def.span) {
            self.errors.push(e);
        }

        // Re-apply substitution to all Let inferred_type fields.
        // This resolves type variables that were created during inference
        // but only unified later in the function body.
        Self::resolve_let_types(&self.substitution, &mut fn_def.body.statements);

        // Clear current type params, bounds, and function name
        self.current_type_params.clear();
        self.current_type_param_bounds.clear();
        self.current_function_name = None;
    }

    /// Walk statements and apply substitution to Let inferred_type fields
    /// and object_type fields on Index/IndexAssign/MethodCall expressions.
    fn resolve_let_types(subst: &Substitution, stmts: &mut [Statement]) {
        for stmt in stmts.iter_mut() {
            Self::resolve_stmt_types(subst, stmt);
        }
    }

    fn resolve_stmt_types(subst: &Substitution, stmt: &mut Statement) {
        match stmt {
            Statement::Let {
                inferred_type: Some(ty),
                init,
                ..
            } => {
                *ty = subst.apply(ty);
                Self::resolve_expr_types(subst, init);
            }
            Statement::Assign { value, .. } => {
                Self::resolve_expr_types(subst, value);
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                object_type,
                ..
            } => {
                if let Some(ty) = object_type {
                    *ty = subst.apply(ty);
                }
                Self::resolve_expr_types(subst, object);
                Self::resolve_expr_types(subst, index);
                Self::resolve_expr_types(subst, value);
            }
            Statement::FieldAssign { object, value, .. } => {
                Self::resolve_expr_types(subst, object);
                Self::resolve_expr_types(subst, value);
            }
            Statement::Expr { expr, .. } => {
                Self::resolve_expr_types(subst, expr);
            }
            Statement::Return { value: Some(v), .. } => {
                Self::resolve_expr_types(subst, v);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                Self::resolve_expr_types(subst, condition);
                Self::resolve_let_types(subst, &mut then_block.statements);
                if let Some(else_block) = else_block {
                    Self::resolve_let_types(subst, &mut else_block.statements);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                Self::resolve_expr_types(subst, condition);
                Self::resolve_let_types(subst, &mut body.statements);
            }
            Statement::ForIn { body, .. } | Statement::ForRange { body, .. } => {
                Self::resolve_let_types(subst, &mut body.statements);
            }
            Statement::Try {
                try_block,
                catch_block,
                ..
            } => {
                Self::resolve_let_types(subst, &mut try_block.statements);
                Self::resolve_let_types(subst, &mut catch_block.statements);
            }
            Statement::MatchDyn {
                arms,
                default_block,
                ..
            } => {
                for arm in arms {
                    Self::resolve_let_types(subst, &mut arm.body.statements);
                }
                Self::resolve_let_types(subst, &mut default_block.statements);
            }
            _ => {}
        }
    }

    fn resolve_expr_types(subst: &Substitution, expr: &mut Expr) {
        match expr {
            Expr::Index {
                object,
                index,
                object_type,
                ..
            } => {
                if let Some(ty) = object_type {
                    *ty = subst.apply(ty);
                }
                Self::resolve_expr_types(subst, object);
                Self::resolve_expr_types(subst, index);
            }
            Expr::MethodCall {
                object,
                args,
                object_type,
                ..
            } => {
                if let Some(ty) = object_type {
                    *ty = subst.apply(ty);
                }
                Self::resolve_expr_types(subst, object);
                for arg in args {
                    Self::resolve_expr_types(subst, arg);
                }
            }
            Expr::Call { args, .. } => {
                for arg in args {
                    Self::resolve_expr_types(subst, arg);
                }
            }
            Expr::Binary { left, right, .. } => {
                Self::resolve_expr_types(subst, left);
                Self::resolve_expr_types(subst, right);
            }
            Expr::Unary { operand, .. } => {
                Self::resolve_expr_types(subst, operand);
            }
            Expr::Block {
                statements, expr, ..
            } => {
                Self::resolve_let_types(subst, statements);
                Self::resolve_expr_types(subst, expr);
            }
            Expr::Field { object, .. } => {
                Self::resolve_expr_types(subst, object);
            }
            Expr::Array { elements, .. } => {
                for elem in elements {
                    Self::resolve_expr_types(subst, elem);
                }
            }
            _ => {}
        }
    }

    /// Register a struct definition.
    fn register_struct(&mut self, struct_def: &StructDef) {
        // Phase 1: Insert struct with placeholder fields so self-referencing types can resolve
        let placeholder_info = StructInfo {
            name: struct_def.name.clone(),
            type_params: struct_def.type_params.clone(),
            fields: Vec::new(),
            methods: HashMap::new(),
        };
        self.structs
            .insert(struct_def.name.clone(), placeholder_info);

        // Phase 2: Resolve field types (now self-references like ptr<HashMapEntry<K, V>> work)
        self.current_type_params = struct_def.type_params.clone();

        let mut fields = Vec::new();
        for field in &struct_def.fields {
            match self.resolve_type_annotation(&field.type_annotation, field.span) {
                Ok(ty) => fields.push((field.name.clone(), ty)),
                Err(e) => {
                    self.errors.push(e);
                    fields.push((field.name.clone(), self.fresh_var()));
                }
            }
        }

        // Clear current type params
        self.current_type_params.clear();

        // Update struct with resolved fields
        if let Some(info) = self.structs.get_mut(&struct_def.name) {
            info.fields = fields;
        }
    }

    /// Register an interface definition.
    fn register_interface(&mut self, interface_def: &InterfaceDef) {
        let mut methods = HashMap::new();
        for method_sig in &interface_def.methods {
            // Build the method type (params excluding self -> return type)
            let param_types: Vec<Type> = method_sig
                .params
                .iter()
                .filter(|p| p.name != "self")
                .map(|p| {
                    if let Some(ann) = &p.type_annotation {
                        self.resolve_type_annotation(ann, p.span)
                            .unwrap_or_else(|e| {
                                self.errors.push(e);
                                self.fresh_var()
                            })
                    } else {
                        self.fresh_var()
                    }
                })
                .collect();

            let ret_type = if let Some(ann) = &method_sig.return_type {
                self.resolve_type_annotation(ann, method_sig.span)
                    .unwrap_or_else(|e| {
                        self.errors.push(e);
                        self.fresh_var()
                    })
            } else {
                Type::Nil
            };

            let fn_type = Type::function(param_types, ret_type);
            methods.insert(method_sig.name.clone(), fn_type);
        }

        self.interfaces.insert(
            interface_def.name.clone(),
            InterfaceInfo {
                name: interface_def.name.clone(),
                methods,
            },
        );
    }

    /// Register and validate an interface implementation.
    /// Checks that all required methods are provided with matching signatures.
    fn register_interface_impl(&mut self, impl_block: &ImplBlock) {
        let interface_name = match &impl_block.interface_name {
            Some(name) => name.clone(),
            None => return,
        };
        let type_name = &impl_block.struct_name;

        // Check that the interface exists
        let interface_info = match self.interfaces.get(&interface_name) {
            Some(info) => info.clone(),
            None => {
                self.errors.push(TypeError::new(
                    format!("undefined interface `{}`", interface_name),
                    impl_block.span,
                ));
                return;
            }
        };

        // Check that all interface methods are implemented
        let impl_method_names: HashSet<String> =
            impl_block.methods.iter().map(|m| m.name.clone()).collect();

        for method_name in interface_info.methods.keys() {
            if !impl_method_names.contains(method_name) {
                self.errors.push(TypeError::new(
                    format!(
                        "missing method `{}` in impl `{}` for `{}`",
                        method_name, interface_name, type_name
                    ),
                    impl_block.span,
                ));
            }
        }

        // Register the implementation
        self.interface_impls
            .insert((interface_name, type_name.clone()));
    }

    /// Generate a synthetic `impl ToString for StructName` block for auto-derive.
    fn generate_tostring_impl(&self, struct_name: &str) -> Option<ImplBlock> {
        let struct_info = self.structs.get(struct_name)?;
        let span = Span::new(0, 0);

        // Determine if this is a generic struct
        let type_params = struct_info.type_params.clone();

        // Build the to_string method body:
        // return "StructName { field1: " + self.field1.to_string() + ", field2: " + ...  + " }";
        let return_expr = if struct_info.fields.is_empty() {
            // Empty struct: return "StructName {}"
            Expr::Str {
                value: format!("{} {{}}", struct_name),
                span,
                inferred_type: None,
            }
        } else {
            // Build concatenation chain
            let mut expr = Expr::Str {
                value: format!("{} {{ ", struct_name),
                span,
                inferred_type: None,
            };

            for (i, (field_name, field_type)) in struct_info.fields.iter().enumerate() {
                // Add separator for subsequent fields
                if i > 0 {
                    expr = Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(expr),
                        right: Box::new(Expr::Str {
                            value: ", ".to_string(),
                            span,
                            inferred_type: None,
                        }),
                        span,
                        inferred_type: None,
                    };
                }

                // Add "field_name: "
                expr = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(expr),
                    right: Box::new(Expr::Str {
                        value: format!("{}: ", field_name),
                        span,
                        inferred_type: None,
                    }),
                    span,
                    inferred_type: None,
                };

                // Build field access: self.field_name
                let field_access = Expr::Field {
                    object: Box::new(Expr::Ident {
                        name: "self".to_string(),
                        span,
                        inferred_type: None,
                    }),
                    field: field_name.clone(),
                    span,
                    inferred_type: None,
                };

                // Determine formatting method based on field type
                let field_str = if self.field_has_tostring(field_type) {
                    // Use self.field.to_string()
                    Expr::MethodCall {
                        object: Box::new(field_access),
                        method: "to_string".to_string(),
                        type_args: Vec::new(),
                        args: Vec::new(),
                        span,
                        object_type: None,
                        inferred_type: None,
                    }
                } else {
                    // Use debug(self.field as dyn) — dyn fallback
                    Expr::Call {
                        callee: "debug".to_string(),
                        type_args: Vec::new(),
                        args: vec![Expr::AsDyn {
                            expr: Box::new(field_access),
                            span,
                            inferred_type: None,
                            is_implicit: true,
                        }],
                        span,
                        inferred_type: None,
                    }
                };

                // Concatenate
                expr = Expr::Binary {
                    op: BinaryOp::Add,
                    left: Box::new(expr),
                    right: Box::new(field_str),
                    span,
                    inferred_type: None,
                };
            }

            // Add closing " }"
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(expr),
                right: Box::new(Expr::Str {
                    value: " }".to_string(),
                    span,
                    inferred_type: None,
                }),
                span,
                inferred_type: None,
            }
        };

        let body = Block {
            statements: vec![Statement::Return {
                value: Some(return_expr),
                span,
            }],
            span,
        };

        let to_string_fn = FnDef {
            name: "to_string".to_string(),
            type_params: Vec::new(),
            type_param_bounds: Vec::new(),
            params: vec![Param {
                name: "self".to_string(),
                type_annotation: None,
                span,
            }],
            return_type: Some(TypeAnnotation::Named("string".to_string())),
            body,
            attributes: Vec::new(),
            span,
        };

        Some(ImplBlock {
            type_params,
            interface_name: Some("ToString".to_string()),
            struct_name: struct_name.to_string(),
            struct_type_args: Vec::new(),
            methods: vec![to_string_fn],
            span,
        })
    }

    /// Generate a synthetic `impl WriteTo for StructName` that wraps ToString.
    /// Body: `let s = self.to_string(); write_str(fd, s, len(s));`
    fn generate_writeto_from_tostring_impl(&self, struct_name: &str) -> Option<ImplBlock> {
        let struct_info = self.structs.get(struct_name)?;
        let span = Span::new(0, 0);
        let type_params = struct_info.type_params.clone();

        // Build: let s = self.to_string();
        let let_s = Statement::Let {
            name: "s".to_string(),
            type_annotation: None,
            init: Expr::MethodCall {
                object: Box::new(Expr::Ident {
                    name: "self".to_string(),
                    span,
                    inferred_type: None,
                }),
                method: "to_string".to_string(),
                type_args: Vec::new(),
                args: Vec::new(),
                span,
                object_type: None,
                inferred_type: None,
            },
            span,
            inferred_type: None,
        };

        // Build: write_str(fd, s, len(s));
        let write_call = Statement::Expr {
            expr: Expr::Call {
                callee: "write_str".to_string(),
                type_args: Vec::new(),
                args: vec![
                    Expr::Ident {
                        name: "fd".to_string(),
                        span,
                        inferred_type: None,
                    },
                    Expr::Ident {
                        name: "s".to_string(),
                        span,
                        inferred_type: None,
                    },
                    Expr::Call {
                        callee: "len".to_string(),
                        type_args: Vec::new(),
                        args: vec![Expr::Ident {
                            name: "s".to_string(),
                            span,
                            inferred_type: None,
                        }],
                        span,
                        inferred_type: None,
                    },
                ],
                span,
                inferred_type: None,
            },
            span,
        };

        let body = Block {
            statements: vec![let_s, write_call],
            span,
        };

        let write_to_fn = FnDef {
            name: "write_to".to_string(),
            type_params: Vec::new(),
            type_param_bounds: Vec::new(),
            params: vec![
                Param {
                    name: "self".to_string(),
                    type_annotation: None,
                    span,
                },
                Param {
                    name: "fd".to_string(),
                    type_annotation: Some(TypeAnnotation::Named("int".to_string())),
                    span,
                },
            ],
            return_type: None,
            body,
            attributes: Vec::new(),
            span,
        };

        Some(ImplBlock {
            type_params,
            interface_name: Some("WriteTo".to_string()),
            struct_name: struct_name.to_string(),
            struct_type_args: Vec::new(),
            methods: vec![write_to_fn],
            span,
        })
    }

    /// Check if a field type has a ToString implementation.
    /// Returns true for primitives, structs (auto-derived), and other types with known ToString.
    fn field_has_tostring(&self, field_type: &Type) -> bool {
        match field_type {
            Type::Int | Type::Float | Type::Bool => true,
            t if t.is_string() => true,
            Type::Struct { name, .. } | Type::GenericStruct { name, .. } => {
                // All structs are auto-derived, so they have ToString
                // (unless they're internal types, but those would be checked separately)
                self.structs.contains_key(name)
            }
            _ => false,
        }
    }

    /// Register methods from an impl block.
    fn register_impl_methods(&mut self, impl_block: &ImplBlock) {
        let struct_name = &impl_block.struct_name;

        // Allow impl blocks for builtin types (vec, map), primitive types, or defined structs
        let is_builtin_type = struct_name == "vec" || struct_name == "map";
        let is_primitive_type = matches!(struct_name.as_str(), "int" | "float" | "bool" | "string");
        if !is_builtin_type && !is_primitive_type && !self.structs.contains_key(struct_name) {
            self.errors.push(TypeError::new(
                format!("impl for undefined struct `{}`", struct_name),
                impl_block.span,
            ));
            return;
        }

        for method in &impl_block.methods {
            // Set current type params: impl block's params + method's params
            self.current_type_params = impl_block.type_params.clone();
            self.current_type_params.extend(method.type_params.clone());

            // Infer method signature (skip 'self' parameter in type)
            let param_types: Vec<Type> = method
                .params
                .iter()
                .filter(|p| p.name != "self")
                .map(|p| {
                    if let Some(ann) = &p.type_annotation {
                        self.resolve_type_annotation(ann, p.span)
                            .unwrap_or_else(|e| {
                                self.errors.push(e);
                                self.fresh_var()
                            })
                    } else {
                        self.fresh_var()
                    }
                })
                .collect();

            let ret_type = if let Some(ann) = &method.return_type {
                self.resolve_type_annotation(ann, method.span)
                    .unwrap_or_else(|e| {
                        self.errors.push(e);
                        self.fresh_var()
                    })
            } else {
                self.fresh_var()
            };

            // Clear current type params
            self.current_type_params.clear();

            let fn_type = Type::function(param_types.clone(), ret_type);

            // Check if this is an associated function (no 'self' parameter)
            let has_self = method.params.iter().any(|p| p.name == "self");

            if is_primitive_type {
                // Store methods for primitive types in dedicated map
                self.primitive_methods
                    .entry(struct_name.clone())
                    .or_default()
                    .insert(method.name.clone(), fn_type);
            } else if has_self {
                // Add method to struct's method table
                if let Some(struct_info) = self.structs.get_mut(struct_name) {
                    struct_info.methods.insert(method.name.clone(), fn_type);
                }
            } else {
                // Associated function - add to struct's method table if struct exists,
                // or register as a standalone function for builtin types
                if let Some(struct_info) = self.structs.get_mut(struct_name) {
                    struct_info.methods.insert(method.name.clone(), fn_type);
                } else {
                    // For builtin types (vec, map), register as {type}_{func} function
                    let func_name = format!("{}_{}", struct_name, method.name);
                    self.functions.insert(func_name, fn_type);
                }
            }
        }
    }

    /// Type check an impl block.
    fn check_impl_block(&mut self, impl_block: &mut ImplBlock) {
        let struct_name = &impl_block.struct_name;
        let is_builtin_type = matches!(struct_name.as_str(), "vec" | "map" | "Vec" | "Map");
        let is_primitive_type = matches!(struct_name.as_str(), "int" | "float" | "bool" | "string");

        // Get struct type for 'self'
        let self_type = if is_primitive_type {
            Some(match struct_name.as_str() {
                "int" => Type::Int,
                "float" => Type::Float,
                "bool" => Type::Bool,
                "string" => Type::string(),
                _ => unreachable!(),
            })
        } else if is_builtin_type && impl_block.interface_name.is_none() {
            None // Builtin types without interface impl don't need self_type
        } else if let Some(info) = self.structs.get(struct_name).cloned() {
            // For generic structs, create GenericStruct with type params
            if !impl_block.type_params.is_empty() {
                let type_args: Vec<Type> = impl_block
                    .type_params
                    .iter()
                    .map(|name| Type::Param { name: name.clone() })
                    .collect();
                Some(Type::GenericStruct {
                    name: info.name.clone(),
                    type_args,
                    fields: info.fields.clone(),
                })
            } else {
                Some(Type::Struct {
                    name: info.name.clone(),
                    fields: info.fields.clone(),
                })
            }
        } else {
            return; // Error already reported in register_impl_methods
        };

        for method in &mut impl_block.methods {
            let mut env = TypeEnv::new();
            let has_self = method.params.iter().any(|p| p.name == "self");

            // Set current type params: impl block's params + method's params
            self.current_type_params = impl_block.type_params.clone();
            self.current_type_params.extend(method.type_params.clone());

            // Get method signature
            let method_type = if is_primitive_type {
                // For primitive types, look up in dedicated primitive_methods map
                self.primitive_methods
                    .get(struct_name)
                    .and_then(|methods| methods.get(&method.name))
                    .cloned()
            } else if is_builtin_type && impl_block.interface_name.is_none() {
                // For builtin types (Vec/Map) without an interface impl,
                // skip body checking entirely.
                // Bodies use low-level intrinsics that aren't type-checkable.
                // Method signatures are already registered via register_impl_methods.
                None
            } else {
                // For struct methods, look up in the struct's method table
                self.structs
                    .get(struct_name)
                    .and_then(|info| info.methods.get(&method.name))
                    .cloned()
            };

            let (param_types, expected_ret) = match method_type {
                Some(Type::Function { params, ret }) => (params, *ret),
                _ => {
                    self.current_type_params.clear();
                    continue;
                }
            };

            // Bind 'self' parameter if present
            let mut param_iter = param_types.iter();
            for param in &method.params {
                if param.name == "self" {
                    if let Some(ref self_ty) = self_type {
                        env.bind("self".to_string(), self_ty.clone());
                    }
                } else if let Some(param_type) = param_iter.next() {
                    env.bind(param.name.clone(), param_type.clone());
                }
            }

            // Infer body type
            let body_type = self.infer_block(&mut method.body, &mut env);

            // Unify return type
            // Skip type checking for builtin type associated functions
            // (vec/map use Vec<T>/Map<K,V> generic structs internally)
            if (!is_builtin_type || has_self)
                && let Err(e) = self.unify(&body_type, &expected_ret, method.span)
            {
                self.errors.push(e);
            }

            // Re-apply substitution to resolve type variables in Let inferred_type fields
            Self::resolve_let_types(&self.substitution, &mut method.body.statements);

            // Clear current type params
            self.current_type_params.clear();
        }
    }

    /// Infer the type of a block (returns the type of the last expression).
    fn infer_block(&mut self, block: &mut Block, env: &mut TypeEnv) -> Type {
        env.enter_scope();
        let mut result_type = Type::Nil;

        for stmt in &mut block.statements {
            result_type = self.infer_statement(stmt, env);
        }

        env.exit_scope();
        result_type
    }

    /// Infer the type of a statement.
    fn infer_statement(&mut self, stmt: &mut Statement, env: &mut TypeEnv) -> Type {
        match stmt {
            Statement::Let {
                name,
                type_annotation,
                init,
                span,
                inferred_type,
                ..
            } => {
                let init_type = self.infer_expr(init, env);

                if let Some(ann) = type_annotation {
                    match self.resolve_type_annotation(ann, *span) {
                        Ok(declared_type) => {
                            if let Err(e) = self.unify(&init_type, &declared_type, *span) {
                                self.errors.push(e);
                            }
                            env.bind(name.clone(), declared_type);
                        }
                        Err(e) => {
                            self.errors.push(e);
                            env.bind(name.clone(), init_type);
                        }
                    }
                } else {
                    env.bind(name.clone(), init_type);
                }

                // Write the resolved type directly to the AST node
                if let Some(ty) = env.lookup(name).cloned() {
                    *inferred_type = Some(self.substitution.apply(&ty));
                }

                Type::Nil
            }

            Statement::Assign { name, value, span } => {
                let value_type = self.infer_expr(value, env);
                if let Some(var_type) = env.lookup(name).cloned()
                    && let Err(e) = self.unify(&value_type, &var_type, *span)
                {
                    self.errors.push(e);
                }
                Type::Nil
            }

            Statement::If {
                condition,
                then_block,
                else_block,
                span,
            } => {
                let cond_type = self.infer_expr(condition, env);
                if let Err(e) = self.unify(&cond_type, &Type::Bool, *span) {
                    self.errors.push(e);
                }

                let then_type = self.infer_block(then_block, env);

                if let Some(else_block) = else_block {
                    let else_type = self.infer_block(else_block, env);
                    if let Err(e) = self.unify(&then_type, &else_type, *span) {
                        self.errors.push(e);
                    }
                }

                then_type
            }

            Statement::While {
                condition,
                body,
                span,
            } => {
                let cond_type = self.infer_expr(condition, env);
                if let Err(e) = self.unify(&cond_type, &Type::Bool, *span) {
                    self.errors.push(e);
                }
                self.infer_block(body, env);
                Type::Nil
            }

            Statement::ForIn {
                var,
                iterable,
                body,
                span,
            } => {
                let iter_type = self.infer_expr(iterable, env);

                // Iterable should be array<T>, and var has type T
                let elem_type = match self.substitution.apply(&iter_type) {
                    ref t if t.is_array() => t.collection_element_type().unwrap().clone(),
                    Type::Var(_) => {
                        // Create fresh element type and unify
                        let elem = self.fresh_var();
                        let arr_type = Type::array(elem.clone());
                        if let Err(e) = self.unify(&iter_type, &arr_type, *span) {
                            self.errors.push(e);
                        }
                        elem
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected array, found `{}`", iter_type),
                            *span,
                        ));
                        self.fresh_var()
                    }
                };

                env.enter_scope();
                env.bind(var.clone(), elem_type);
                self.infer_block(body, env);
                env.exit_scope();
                Type::Nil
            }

            Statement::ForRange {
                var,
                start,
                end,
                body,
                span,
                ..
            } => {
                let start_type = self.infer_expr(start, env);
                let end_type = self.infer_expr(end, env);

                // Both start and end must be int
                if let Err(e) = self.unify(&start_type, &Type::Int, *span) {
                    self.errors.push(e);
                }
                if let Err(e) = self.unify(&end_type, &Type::Int, *span) {
                    self.errors.push(e);
                }

                env.enter_scope();
                env.bind(var.clone(), Type::Int);
                self.infer_block(body, env);
                env.exit_scope();
                Type::Nil
            }

            Statement::Return { value, span: _ } => {
                if let Some(expr) = value {
                    self.infer_expr(expr, env)
                } else {
                    Type::Nil
                }
            }

            Statement::Const { name, init, .. } => {
                let init_type = self.infer_expr(init, env);
                env.bind(name.clone(), init_type.clone());
                init_type
            }

            Statement::Expr { expr, .. } => self.infer_expr(expr, env),

            Statement::IndexAssign {
                object,
                index,
                value,
                span,
                object_type,
            } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);
                let val_type = self.infer_expr(value, env);

                // Write the object type directly to the AST node
                let resolved_obj_type = self.substitution.apply(&obj_type);
                *object_type = Some(resolved_obj_type.clone());

                // Object can be array<T>, Vec<T>, or Map<K,V> generic struct
                match resolved_obj_type {
                    ref t @ Type::GenericStruct { .. } if t.is_array() || t.is_vec() => {
                        // Index should be int for Array/Vec
                        if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                            self.errors.push(e);
                        }
                        if let Some(elem) = t.collection_element_type()
                            && let Err(e) = self.unify(&val_type, elem, *span)
                        {
                            self.errors.push(e);
                        }
                    }
                    Type::GenericStruct {
                        ref name,
                        ref type_args,
                        ..
                    } if name == "Map" => {
                        // Map<K,V> - index should be key type K, check value type V
                        if let Some(key_type) = type_args.first()
                            && let Err(e) = self.unify(&idx_type, key_type, *span)
                        {
                            self.errors.push(e);
                        }
                        if let Some(val_elem) = type_args.get(1)
                            && let Err(e) = self.unify(&val_type, val_elem, *span)
                        {
                            self.errors.push(e);
                        }
                    }
                    Type::Ptr(ref elem) => {
                        // ptr<T> - index should be int, check element type T
                        if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                            self.errors.push(e);
                        }
                        if let Err(e) = self.unify(&val_type, elem, *span) {
                            self.errors.push(e);
                        }
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!(
                                "cannot index assign to `{}`",
                                self.substitution.apply(&obj_type)
                            ),
                            *span,
                        ));
                    }
                }

                Type::Nil
            }

            Statement::FieldAssign {
                object,
                field,
                value,
                span,
            } => {
                let obj_type = self.infer_expr(object, env);
                let val_type = self.infer_expr(value, env);

                // Check field exists and type matches
                match self.substitution.apply(&obj_type) {
                    Type::Struct { name, fields } => {
                        // Look up field in struct definition
                        let mut found = false;
                        for (field_name, field_type) in &fields {
                            if field_name == field {
                                if let Err(e) = self.unify(&val_type, field_type, *span) {
                                    self.errors.push(e);
                                }
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            self.errors.push(TypeError::new(
                                format!("struct `{}` has no field `{}`", name, field),
                                *span,
                            ));
                        }
                    }
                    Type::GenericStruct { name, fields, .. } => {
                        // Look up field in generic struct (fields have type params substituted)
                        let mut found = false;
                        for (field_name, field_type) in &fields {
                            if field_name == field {
                                if let Err(e) = self.unify(&val_type, field_type, *span) {
                                    self.errors.push(e);
                                }
                                found = true;
                                break;
                            }
                        }
                        if !found {
                            self.errors.push(TypeError::new(
                                format!("struct `{}` has no field `{}`", name, field),
                                *span,
                            ));
                        }
                    }
                    Type::Var(_) => {
                        // Can't infer field assignment on unknown object type
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected object or struct, found `{}`", obj_type),
                            *span,
                        ));
                    }
                }
                Type::Nil
            }

            Statement::Throw { value, .. } => {
                self.infer_expr(value, env);
                Type::Nil
            }

            Statement::MatchDyn {
                expr,
                arms,
                default_block,
                span,
            } => {
                // The matched expression must be of type Dyn
                let expr_type = self.infer_expr(expr, env);
                if let Err(e) = self.unify(&expr_type, &Type::Dyn, *span) {
                    self.errors.push(e);
                }

                // Type-check each arm and collect block types for unification
                let mut result_type: Option<Type> = None;
                for arm in arms.iter_mut() {
                    // Check if the type annotation is an interface name
                    let is_interface = if let TypeAnnotation::Named(name) = &arm.type_annotation {
                        self.interfaces.contains_key(name)
                    } else {
                        false
                    };

                    let arm_type = if is_interface {
                        // Interface match arm: variable carries interface bound for method resolution
                        if let TypeAnnotation::Named(name) = &arm.type_annotation {
                            Type::InterfaceBound {
                                interface_name: name.clone(),
                            }
                        } else {
                            Type::Any
                        }
                    } else {
                        match self.resolve_type_annotation(&arm.type_annotation, arm.span) {
                            Ok(ty) => ty,
                            Err(e) => {
                                self.errors.push(e);
                                self.fresh_var()
                            }
                        }
                    };
                    env.enter_scope();
                    env.bind(arm.var_name.clone(), arm_type);
                    let block_type = self.infer_block(&mut arm.body, env);
                    env.exit_scope();

                    if let Some(ref prev) = result_type
                        && let Err(e) = self.unify(prev, &block_type, *span)
                    {
                        self.errors.push(e);
                    }
                    result_type = Some(block_type);
                }

                // Type-check the default block and unify with arm types
                let default_type = self.infer_block(default_block, env);
                if let Some(ref prev) = result_type
                    && let Err(e) = self.unify(prev, &default_type, *span)
                {
                    self.errors.push(e);
                }
                result_type = Some(default_type);

                result_type.unwrap_or(Type::Nil)
            }

            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                self.infer_block(try_block, env);
                env.enter_scope();
                // Catch variable is string (error message)
                env.bind(catch_var.clone(), Type::string());
                self.infer_block(catch_block, env);
                env.exit_scope();
                Type::Nil
            }
        }
    }

    /// Infer the type of an expression and set it on the AST node.
    fn infer_expr(&mut self, expr: &mut Expr, env: &mut TypeEnv) -> Type {
        let ty = self.infer_expr_inner(expr, env);
        expr.set_inferred_type(ty.clone());
        ty
    }

    /// Inner implementation of expression type inference.
    fn infer_expr_inner(&mut self, expr: &mut Expr, env: &mut TypeEnv) -> Type {
        match expr {
            Expr::Int { .. } => Type::Int,
            Expr::Float { .. } => Type::Float,
            Expr::Bool { .. } => Type::Bool,
            Expr::Str { .. } => Type::string(),
            Expr::StringInterpolation { parts, .. } => {
                for part in parts.iter_mut() {
                    if let crate::compiler::ast::StringInterpPart::Expr(e) = part {
                        self.infer_expr(e, env);
                    }
                }
                Type::string()
            }
            Expr::Nil { .. } => Type::Nil,

            Expr::Ident { name, span, .. } => {
                if let Some(ty) = env.lookup(name) {
                    self.substitution.apply(ty)
                } else if let Some(fn_type) = self.functions.get(name) {
                    // Function reference (used in spawn, etc.)
                    fn_type.clone()
                } else {
                    self.errors.push(TypeError::new(
                        format!("undefined variable `{}`", name),
                        *span,
                    ));
                    self.fresh_var()
                }
            }

            Expr::Array { elements, span, .. } => {
                if elements.is_empty() {
                    // Empty array has unknown element type
                    Type::array(self.fresh_var())
                } else {
                    let first_type = self.infer_expr(&mut elements[0], env);
                    for elem in elements.iter_mut().skip(1) {
                        let elem_type = self.infer_expr(elem, env);
                        if let Err(e) = self.unify(&first_type, &elem_type, *span) {
                            self.errors.push(e);
                        }
                    }
                    Type::array(self.substitution.apply(&first_type))
                }
            }

            Expr::Index {
                object,
                index,
                span,
                object_type,
                ..
            } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);

                // Write the object type directly to the AST node
                let resolved_obj_type = self.substitution.apply(&obj_type);
                *object_type = Some(resolved_obj_type.clone());

                // Object can be array<T>, Vec<T>, Map<K,V>, string, or struct
                match resolved_obj_type {
                    ref t @ Type::GenericStruct { .. } if t.is_array() || t.is_vec() => {
                        // Index should be int for Array/Vec
                        if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                            self.errors.push(e);
                        }
                        t.collection_element_type()
                            .map(|e| self.substitution.apply(e))
                            .unwrap_or_else(|| self.fresh_var())
                    }
                    Type::GenericStruct {
                        ref name,
                        ref type_args,
                        ..
                    } if name == "Map" => {
                        // Map<K,V> - index should be key type K, return value type V
                        if let Some(key_type) = type_args.first()
                            && let Err(e) = self.unify(&idx_type, key_type, *span)
                        {
                            self.errors.push(e);
                        }
                        type_args
                            .get(1)
                            .cloned()
                            .unwrap_or_else(|| self.fresh_var())
                    }
                    Type::Struct { fields, .. } => {
                        // Index should be int for Struct
                        if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                            self.errors.push(e);
                        }
                        // For structs, index access returns a type variable
                        // (we'd need to know the index value at compile time to be precise)
                        // All fields may have different types, so return a fresh var
                        if fields.is_empty() {
                            self.fresh_var()
                        } else {
                            // Return first field type as approximation (or unify all)
                            self.fresh_var()
                        }
                    }
                    Type::Ptr(ref elem) => {
                        // ptr<T> - index should be int, return element type T
                        if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                            self.errors.push(e);
                        }
                        self.substitution.apply(elem)
                    }
                    Type::Var(_) => {
                        // Unknown type, could be array or struct
                        self.fresh_var()
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!(
                                "expected array, Vector, Vec, Map, string, struct or ptr, found `{}`",
                                obj_type
                            ),
                            *span,
                        ));
                        self.fresh_var()
                    }
                }
            }

            Expr::Field {
                object,
                field,
                span,
                ..
            } => {
                let obj_type = self.infer_expr(object, env);

                match self.substitution.apply(&obj_type) {
                    Type::Struct { name, fields } => {
                        // Look up field in struct definition
                        for (field_name, field_type) in &fields {
                            if field_name == field {
                                return field_type.clone();
                            }
                        }
                        self.errors.push(TypeError::new(
                            format!("struct `{}` has no field `{}`", name, field),
                            *span,
                        ));
                        self.fresh_var()
                    }
                    Type::GenericStruct {
                        name,
                        fields,
                        type_args,
                    } => {
                        // string (array<byte>) and array<T> have .data and .len fields
                        let t = Type::GenericStruct {
                            name: name.clone(),
                            fields: fields.clone(),
                            type_args: type_args.clone(),
                        };
                        if t.is_array() || t.is_string() {
                            let elem_type = type_args
                                .first()
                                .cloned()
                                .unwrap_or_else(|| self.fresh_var());
                            match field.as_str() {
                                "data" => return Type::Ptr(Box::new(elem_type)),
                                "len" => return Type::Int,
                                _ => {
                                    self.errors.push(TypeError::new(
                                        format!("string has no field `{}`", field),
                                        *span,
                                    ));
                                    return self.fresh_var();
                                }
                            }
                        }
                        // Look up field in generic struct (fields already have type params substituted)
                        for (field_name, field_type) in &fields {
                            if field_name == field {
                                return field_type.clone();
                            }
                        }
                        self.errors.push(TypeError::new(
                            format!("struct `{}` has no field `{}`", name, field),
                            *span,
                        ));
                        self.fresh_var()
                    }
                    Type::Var(_) => {
                        // Can't infer field access on unknown object type
                        self.fresh_var()
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected object or struct, found `{}`", obj_type),
                            *span,
                        ));
                        self.fresh_var()
                    }
                }
            }

            Expr::Unary {
                op, operand, span, ..
            } => {
                let operand_type = self.infer_expr(operand, env);

                match op {
                    UnaryOp::Neg => {
                        // Negation works on int or float
                        let _result = self.fresh_var();
                        // Try to unify with int first
                        if self.unify(&operand_type, &Type::Int, *span).is_ok() {
                            Type::Int
                        } else if self.unify(&operand_type, &Type::Float, *span).is_ok() {
                            Type::Float
                        } else {
                            self.errors.push(TypeError::new(
                                format!("cannot negate `{}`", operand_type),
                                *span,
                            ));
                            self.fresh_var()
                        }
                    }
                    UnaryOp::Not => {
                        if let Err(e) = self.unify(&operand_type, &Type::Bool, *span) {
                            self.errors.push(e);
                        }
                        Type::Bool
                    }
                }
            }

            Expr::Binary {
                op,
                left,
                right,
                span,
                ..
            } => {
                let left_type = self.infer_expr(left, env);
                let right_type = self.infer_expr(right, env);

                match op {
                    // Arithmetic operations: int/float -> int/float
                    BinaryOp::Add => {
                        // + can be int+int, float+float, or string+string
                        if self.unify(&left_type, &Type::Int, *span).is_ok()
                            && self.unify(&right_type, &Type::Int, *span).is_ok()
                        {
                            Type::Int
                        } else if self.unify(&left_type, &Type::Float, *span).is_ok()
                            && self.unify(&right_type, &Type::Float, *span).is_ok()
                        {
                            Type::Float
                        } else if self.unify(&left_type, &Type::string(), *span).is_ok()
                            && self.unify(&right_type, &Type::string(), *span).is_ok()
                        {
                            Type::string()
                        } else {
                            // Unify left and right, require numeric or string
                            if let Err(e) = self.unify(&left_type, &right_type, *span) {
                                self.errors.push(e);
                            }
                            self.substitution.apply(&left_type)
                        }
                    }

                    BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                        // These only work on numeric types
                        if self.unify(&left_type, &Type::Int, *span).is_ok()
                            && self.unify(&right_type, &Type::Int, *span).is_ok()
                        {
                            Type::Int
                        } else if self.unify(&left_type, &Type::Float, *span).is_ok()
                            && self.unify(&right_type, &Type::Float, *span).is_ok()
                        {
                            Type::Float
                        } else {
                            if let Err(e) = self.unify(&left_type, &right_type, *span) {
                                self.errors.push(e);
                            }
                            self.substitution.apply(&left_type)
                        }
                    }

                    // Comparison: same type -> bool
                    BinaryOp::Eq | BinaryOp::Ne => {
                        if let Err(e) = self.unify(&left_type, &right_type, *span) {
                            self.errors.push(e);
                        }
                        Type::Bool
                    }

                    BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                        // Only numeric types
                        let is_int_comparison = self.unify(&left_type, &Type::Int, *span).is_ok()
                            && self.unify(&right_type, &Type::Int, *span).is_ok();
                        let is_float_comparison =
                            self.unify(&left_type, &Type::Float, *span).is_ok()
                                && self.unify(&right_type, &Type::Float, *span).is_ok();

                        if !is_int_comparison && !is_float_comparison {
                            self.errors.push(TypeError::new(
                                format!("cannot compare `{}` and `{}`", left_type, right_type),
                                *span,
                            ));
                        }
                        Type::Bool
                    }

                    // Bitwise: int -> int
                    BinaryOp::BitwiseAnd
                    | BinaryOp::BitwiseOr
                    | BinaryOp::BitwiseXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => {
                        if self.unify(&left_type, &Type::Int, *span).is_ok()
                            && self.unify(&right_type, &Type::Int, *span).is_ok()
                        {
                            Type::Int
                        } else {
                            self.errors.push(TypeError::new(
                                format!(
                                    "bitwise operations require integer operands, got `{}` and `{}`",
                                    left_type, right_type
                                ),
                                *span,
                            ));
                            Type::Int
                        }
                    }

                    // Logical: bool -> bool
                    BinaryOp::And | BinaryOp::Or => {
                        if let Err(e) = self.unify(&left_type, &Type::Bool, *span) {
                            self.errors.push(e);
                        }
                        if let Err(e) = self.unify(&right_type, &Type::Bool, *span) {
                            self.errors.push(e);
                        }
                        Type::Bool
                    }
                }
            }

            Expr::Call {
                callee,
                type_args,
                args,
                span,
                ..
            } => {
                // Check for builtin functions
                if let Some(result_type) = self.check_builtin(callee, args, env, *span) {
                    return result_type;
                }

                // print fallback for types that can't resolve through the generic path:
                // `any` unifies with everything (empty substitution), `Var` is unresolved,
                // `Nullable` doesn't implement WriteTo. Redirect to _print_dyn.
                if callee == "print" && args.len() == 1 && type_args.is_empty() {
                    let arg_type = self.infer_expr(&mut args[0], env);
                    let resolved_arg_type = self.substitution.apply(&arg_type);
                    if matches!(
                        resolved_arg_type,
                        Type::Any | Type::Nullable(_) | Type::Var(_)
                    ) {
                        let span = args[0].span();
                        let inner = mem::replace(
                            &mut args[0],
                            Expr::Nil {
                                span,
                                inferred_type: None,
                            },
                        );
                        args[0] = Expr::AsDyn {
                            expr: Box::new(inner),
                            span,
                            inferred_type: Some(Type::Dyn),
                            is_implicit: true,
                        };
                        *callee = "_print_dyn".to_string();
                        return Type::Nil;
                    }
                }

                // Check if it's a generic function with explicit type arguments
                if let Some(overloads) = self.generic_functions.get(callee.as_str()).cloned() {
                    // Use the first overload for type inference.
                    // All overloads share compatible function types.
                    let generic_info = overloads[0].clone();
                    let has_overloads = overloads.len() > 1;

                    // Track fresh type variables for implicit generic calls
                    let mut fresh_vars: Vec<Type> = Vec::new();
                    // Instantiate the generic function with the provided type arguments
                    let fn_type = if !type_args.is_empty() {
                        // Check that the number of type arguments matches
                        if type_args.len() != generic_info.type_params.len() {
                            self.errors.push(TypeError::new(
                                format!(
                                    "function `{}` expects {} type arguments, got {}",
                                    callee,
                                    generic_info.type_params.len(),
                                    type_args.len()
                                ),
                                *span,
                            ));
                            generic_info.fn_type.clone()
                        } else {
                            // Substitute type parameters with type arguments
                            let mut instantiated = generic_info.fn_type.clone();
                            let mut resolved_args = Vec::new();
                            for (param_name, type_arg) in
                                generic_info.type_params.iter().zip(type_args.iter())
                            {
                                let resolved_arg =
                                    match self.resolve_type_annotation(type_arg, *span) {
                                        Ok(t) => t,
                                        Err(e) => {
                                            self.errors.push(e);
                                            self.fresh_var()
                                        }
                                    };
                                resolved_args.push(resolved_arg.clone());
                                instantiated =
                                    instantiated.substitute_param(param_name, &resolved_arg);
                            }

                            // Overload resolution or bounds checking
                            if has_overloads {
                                let selected_idx =
                                    self.resolve_overload(&overloads, &resolved_args);
                                let selected = &overloads[selected_idx];
                                *callee = selected.internal_name.clone();
                            } else {
                                // Check interface bounds (single overload)
                                for (i, bounds) in generic_info.type_param_bounds.iter().enumerate()
                                {
                                    if bounds.is_empty() {
                                        continue;
                                    }
                                    let concrete_type = &resolved_args[i];
                                    let type_name = self.type_to_impl_name(concrete_type);
                                    for bound in bounds {
                                        if !self
                                            .interface_impls
                                            .contains(&(bound.clone(), type_name.clone()))
                                        {
                                            self.errors.push(TypeError::new(
                                                format!(
                                                    "type `{}` does not implement interface `{}`",
                                                    concrete_type, bound
                                                ),
                                                *span,
                                            ));
                                        }
                                    }
                                }
                            }

                            instantiated
                        }
                    } else {
                        // No explicit type args - substitute type params with fresh type variables
                        // This allows Hindley-Milner inference to work out the types
                        let mut instantiated = generic_info.fn_type.clone();
                        for param_name in &generic_info.type_params {
                            let fresh = self.fresh_var();
                            fresh_vars.push(fresh.clone());
                            instantiated = instantiated.substitute_param(param_name, &fresh);
                        }
                        instantiated
                    };

                    match fn_type {
                        Type::Function { params, ret } => {
                            if args.len() != params.len() {
                                self.errors.push(TypeError::new(
                                    format!(
                                        "function `{}` expects {} arguments, got {}",
                                        callee,
                                        params.len(),
                                        args.len()
                                    ),
                                    *span,
                                ));
                                return self.substitution.apply(&ret);
                            }

                            self.check_call_args(args, &params, env);

                            // Write back inferred type arguments for implicit generic calls
                            if type_args.is_empty() && !fresh_vars.is_empty() {
                                let mut inferred_type_args = Vec::new();
                                let mut all_resolved = true;
                                for fresh in &fresh_vars {
                                    let resolved = self.substitution.apply(fresh);
                                    if let Some(ta) = resolved.to_type_annotation() {
                                        inferred_type_args.push(ta);
                                    } else {
                                        all_resolved = false;
                                        break;
                                    }
                                }
                                if all_resolved {
                                    *type_args = inferred_type_args;

                                    // Overload resolution or bounds checking for inferred types
                                    let resolved_args: Vec<Type> = fresh_vars
                                        .iter()
                                        .map(|f| self.substitution.apply(f))
                                        .collect();

                                    if has_overloads {
                                        let selected_idx =
                                            self.resolve_overload(&overloads, &resolved_args);
                                        let selected = &overloads[selected_idx];
                                        *callee = selected.internal_name.clone();
                                    } else {
                                        for (i, bounds) in
                                            generic_info.type_param_bounds.iter().enumerate()
                                        {
                                            if bounds.is_empty() {
                                                continue;
                                            }
                                            let concrete_type = &resolved_args[i];
                                            let type_name = self.type_to_impl_name(concrete_type);
                                            for bound in bounds {
                                                if !self
                                                    .interface_impls
                                                    .contains(&(bound.clone(), type_name.clone()))
                                                {
                                                    self.errors.push(TypeError::new(
                                                        format!(
                                                            "type `{}` does not implement interface `{}`",
                                                            concrete_type, bound
                                                        ),
                                                        *span,
                                                    ));
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            self.substitution.apply(&ret)
                        }
                        _ => {
                            self.errors.push(TypeError::new(
                                format!("`{}` is not a function", callee),
                                *span,
                            ));
                            self.fresh_var()
                        }
                    }
                } else if let Some(fn_type) = self.functions.get(callee).cloned() {
                    // Non-generic user-defined function
                    match fn_type {
                        Type::Function { params, ret } => {
                            if args.len() != params.len() {
                                self.errors.push(TypeError::new(
                                    format!(
                                        "function `{}` expects {} arguments, got {}",
                                        callee,
                                        params.len(),
                                        args.len()
                                    ),
                                    *span,
                                ));
                                return self.substitution.apply(&ret);
                            }

                            self.check_call_args(args, &params, env);

                            self.substitution.apply(&ret)
                        }
                        _ => {
                            self.errors.push(TypeError::new(
                                format!("`{}` is not a function", callee),
                                *span,
                            ));
                            self.fresh_var()
                        }
                    }
                } else if let Some(var_type) = env.lookup(callee).cloned() {
                    // Variable holding a closure / function value
                    let var_type = self.substitution.apply(&var_type);
                    match var_type {
                        Type::Function { params, ret } => {
                            if args.len() != params.len() {
                                self.errors.push(TypeError::new(
                                    format!(
                                        "`{}` expects {} arguments, got {}",
                                        callee,
                                        params.len(),
                                        args.len()
                                    ),
                                    *span,
                                ));
                                return self.substitution.apply(&ret);
                            }
                            self.check_call_args(args, &params, env);
                            self.substitution.apply(&ret)
                        }
                        Type::Var(_) => {
                            // Unresolved type variable — create function type constraint
                            let mut param_types = Vec::new();
                            for arg in args.iter_mut() {
                                param_types.push(self.infer_expr(arg, env));
                            }
                            let ret_type = self.fresh_var();
                            let fn_type = Type::Function {
                                params: param_types,
                                ret: Box::new(ret_type.clone()),
                            };
                            if let Err(e) = self.unify(&var_type, &fn_type, *span) {
                                self.errors.push(e);
                            }
                            self.substitution.apply(&ret_type)
                        }
                        Type::Any => {
                            // Dynamic call: any-typed value used as callable
                            // Infer args but don't constrain types; return any
                            for arg in args.iter_mut() {
                                self.infer_expr(arg, env);
                            }
                            Type::Any
                        }
                        _ => {
                            self.errors.push(TypeError::new(
                                format!("`{}` is not callable", callee),
                                *span,
                            ));
                            self.fresh_var()
                        }
                    }
                } else {
                    self.errors.push(TypeError::new(
                        format!("undefined function `{}`", callee),
                        *span,
                    ));
                    self.fresh_var()
                }
            }

            Expr::StructLiteral {
                name,
                type_args,
                fields,
                span,
                ..
            } => {
                // Look up struct definition
                let struct_info = match self.structs.get(name) {
                    Some(info) => info.clone(),
                    None => {
                        self.errors.push(TypeError::new(
                            format!("undefined struct `{}`", name),
                            *span,
                        ));
                        // Still infer field types to find nested errors
                        for (_, expr) in fields.iter_mut() {
                            self.infer_expr(expr, env);
                        }
                        return self.fresh_var();
                    }
                };

                // Handle generic structs
                let is_generic = !struct_info.type_params.is_empty();
                let instantiated_fields = if is_generic {
                    // Check that type args are provided for generic structs
                    if type_args.is_empty() {
                        // Type inference will happen during unification
                        // For now, create fresh type variables for each type param
                        let type_vars: Vec<Type> = struct_info
                            .type_params
                            .iter()
                            .map(|_| self.fresh_var())
                            .collect();

                        // Substitute type params with fresh type vars in field types
                        struct_info
                            .fields
                            .iter()
                            .map(|(fname, ftype)| {
                                let mut substituted = ftype.clone();
                                for (param_name, type_var) in
                                    struct_info.type_params.iter().zip(type_vars.iter())
                                {
                                    substituted =
                                        substituted.substitute_param(param_name, type_var);
                                }
                                (fname.clone(), substituted)
                            })
                            .collect::<Vec<_>>()
                    } else if type_args.len() != struct_info.type_params.len() {
                        self.errors.push(TypeError::new(
                            format!(
                                "struct `{}` expects {} type arguments, got {}",
                                name,
                                struct_info.type_params.len(),
                                type_args.len()
                            ),
                            *span,
                        ));
                        struct_info.fields.clone()
                    } else {
                        // Resolve type arguments and substitute in field types
                        let resolved_type_args: Vec<Type> = type_args
                            .iter()
                            .map(|ta| {
                                self.resolve_type_annotation(ta, *span).unwrap_or_else(|e| {
                                    self.errors.push(e);
                                    self.fresh_var()
                                })
                            })
                            .collect();

                        struct_info
                            .fields
                            .iter()
                            .map(|(fname, ftype)| {
                                let mut substituted = ftype.clone();
                                for (param_name, type_arg) in struct_info
                                    .type_params
                                    .iter()
                                    .zip(resolved_type_args.iter())
                                {
                                    substituted =
                                        substituted.substitute_param(param_name, type_arg);
                                }
                                (fname.clone(), substituted)
                            })
                            .collect::<Vec<_>>()
                    }
                } else {
                    struct_info.fields.clone()
                };

                // Build expected type map from struct definition
                let expected_types: HashMap<&str, &Type> = instantiated_fields
                    .iter()
                    .map(|(n, t)| (n.as_str(), t))
                    .collect();

                // Check for missing fields
                let provided_names: std::collections::HashSet<&str> =
                    fields.iter().map(|(n, _)| n.as_str()).collect();
                for (field_name, _) in &instantiated_fields {
                    if !provided_names.contains(field_name.as_str()) {
                        self.errors.push(TypeError::new(
                            format!("missing field `{}` in struct `{}`", field_name, name),
                            *span,
                        ));
                    }
                }

                // Type check all provided fields
                let struct_field_names: std::collections::HashSet<&str> = instantiated_fields
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect();
                for (field_name, expr) in fields.iter_mut() {
                    if let Some(expected_type) = expected_types.get(field_name.as_str()) {
                        let actual_type = self.infer_expr(expr, env);
                        if let Err(e) = self.unify(&actual_type, expected_type, expr.span()) {
                            self.errors.push(e);
                        }
                    } else if !struct_field_names.contains(field_name.as_str()) {
                        self.errors.push(TypeError::new(
                            format!("unknown field `{}` in struct `{}`", field_name, name),
                            expr.span(),
                        ));
                        self.infer_expr(expr, env);
                    }
                }

                if is_generic && !type_args.is_empty() {
                    // Return GenericStruct with explicit type args
                    let resolved_type_args: Vec<Type> = type_args
                        .iter()
                        .map(|ta| {
                            self.resolve_type_annotation(ta, *span).unwrap_or_else(|e| {
                                self.errors.push(e);
                                self.fresh_var()
                            })
                        })
                        .collect();
                    Type::GenericStruct {
                        name: name.clone(),
                        type_args: resolved_type_args,
                        fields: instantiated_fields,
                    }
                } else {
                    Type::Struct {
                        name: name.clone(),
                        fields: instantiated_fields,
                    }
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
                span,
                object_type,
                ..
            } => {
                let obj_type = self.infer_expr(object, env);
                let resolved_obj_type = self.substitution.apply(&obj_type);

                // Write the object type directly to the AST node
                *object_type = Some(resolved_obj_type.clone());

                // Handle ptr<T> methods
                if let Type::Ptr(ref elem) = resolved_obj_type {
                    return self.check_ptr_method(method, args, elem, env, *span);
                }

                // Handle primitive type methods (int, float, bool, string)
                let primitive_type_name = match &resolved_obj_type {
                    Type::Int => Some("int"),
                    Type::Float => Some("float"),
                    Type::Bool => Some("bool"),
                    t if t.is_string() => Some("string"),
                    _ => None,
                };
                if let Some(type_name) = primitive_type_name {
                    return self.check_primitive_method(type_name, method, args, env, *span);
                }

                // Handle Type::Param — resolve method via interface bounds
                if let Type::Param { name: param_name } = &resolved_obj_type {
                    if let Some(bounds) = self.current_type_param_bounds.get(param_name).cloned() {
                        // Search through bounds to find the method
                        for bound in &bounds {
                            if let Some(interface_info) = self.interfaces.get(bound).cloned()
                                && let Some(Type::Function { params, ret }) =
                                    interface_info.methods.get(method)
                            {
                                // Check argument count
                                if args.len() != params.len() {
                                    self.errors.push(TypeError::new(
                                        format!(
                                            "method `{}` expects {} arguments, got {}",
                                            method,
                                            params.len(),
                                            args.len()
                                        ),
                                        *span,
                                    ));
                                    return self.substitution.apply(ret);
                                }

                                // Type check arguments
                                self.check_call_args(args, params, env);

                                return self.substitution.apply(ret);
                            }
                        }
                        // Method not found in any bound
                        self.errors.push(TypeError::new(
                            format!(
                                "method `{}` not found in interface bounds {:?} for type parameter `{}`",
                                method, bounds, param_name
                            ),
                            *span,
                        ));
                    } else {
                        self.errors.push(TypeError::new(
                            format!(
                                "cannot call method `{}` on unbounded type parameter `{}`",
                                method, param_name
                            ),
                            *span,
                        ));
                    }
                    for arg in args {
                        self.infer_expr(arg, env);
                    }
                    return self.fresh_var();
                }

                // Handle Type::InterfaceBound — resolve method via interface definition
                if let Type::InterfaceBound { interface_name } = &resolved_obj_type {
                    if let Some(interface_info) = self.interfaces.get(interface_name).cloned() {
                        if let Some(Type::Function { params, ret }) =
                            interface_info.methods.get(method)
                        {
                            if args.len() != params.len() {
                                self.errors.push(TypeError::new(
                                    format!(
                                        "method `{}` expects {} arguments, got {}",
                                        method,
                                        params.len(),
                                        args.len()
                                    ),
                                    *span,
                                ));
                                return self.substitution.apply(ret);
                            }
                            self.check_call_args(args, params, env);
                            return self.substitution.apply(ret);
                        }
                        self.errors.push(TypeError::new(
                            format!(
                                "method `{}` not found in interface `{}`",
                                method, interface_name
                            ),
                            *span,
                        ));
                    } else {
                        self.errors.push(TypeError::new(
                            format!("unknown interface `{}`", interface_name),
                            *span,
                        ));
                    }
                    for arg in args {
                        self.infer_expr(arg, env);
                    }
                    return self.fresh_var();
                }

                // Get struct name and type args from object type
                let (struct_name, type_args) = match &resolved_obj_type {
                    Type::Struct { name, .. } => (name.clone(), Vec::new()),
                    Type::GenericStruct {
                        name, type_args, ..
                    } => (name.clone(), type_args.clone()),
                    Type::Var(_) => {
                        // Can't determine struct type yet
                        for arg in args {
                            self.infer_expr(arg, env);
                        }
                        return self.fresh_var();
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!(
                                "cannot call method `{}` on type `{}`",
                                method, resolved_obj_type
                            ),
                            *span,
                        ));
                        for arg in args {
                            self.infer_expr(arg, env);
                        }
                        return self.fresh_var();
                    }
                };

                // Look up method in struct's method table
                let struct_info = self.structs.get(&struct_name).cloned();
                let mut method_type = struct_info
                    .as_ref()
                    .and_then(|info| info.methods.get(method))
                    .cloned();

                // Fallback for Map generic method names (put, get, contains, remove)
                // The typechecker runs before desugar, so user code uses generic names
                // but prelude only defines specialized versions (put_int, put_string, etc.)
                if method_type.is_none() && struct_name == "Map" && !type_args.is_empty() {
                    let suffix = match &type_args[0] {
                        Type::Int => Some("int"),
                        t if t.is_string() => Some("string"),
                        _ => None,
                    };
                    if let Some(suffix) = suffix {
                        let specialized = match method.as_str() {
                            "put" | "set" => Some(format!("put_{}", suffix)),
                            "get" => Some(format!("get_{}", suffix)),
                            "contains" => Some(format!("contains_{}", suffix)),
                            "remove" => Some(format!("remove_{}", suffix)),
                            _ => None,
                        };
                        if let Some(ref specialized_name) = specialized {
                            method_type = struct_info
                                .as_ref()
                                .and_then(|info| info.methods.get(specialized_name))
                                .cloned();
                        }
                    }
                }

                match method_type {
                    Some(Type::Function { params, ret }) => {
                        // For generic structs, substitute type params with actual type args
                        let (params, ret) = if !type_args.is_empty() {
                            if let Some(ref info) = struct_info {
                                // First, apply current substitution to resolve any Var bindings
                                // (e.g., Var(17) → Param("V") from impl block body checking)
                                let mut substituted_params: Vec<Type> =
                                    params.iter().map(|p| self.substitution.apply(p)).collect();
                                let mut substituted_ret = self.substitution.apply(&ret);
                                for (param_name, type_arg) in
                                    info.type_params.iter().zip(type_args.iter())
                                {
                                    substituted_params = substituted_params
                                        .iter()
                                        .map(|p| p.substitute_param(param_name, type_arg))
                                        .collect();
                                    substituted_ret =
                                        substituted_ret.substitute_param(param_name, type_arg);
                                }
                                (substituted_params, Box::new(substituted_ret))
                            } else {
                                (params, ret)
                            }
                        } else {
                            (params, ret)
                        };

                        // Check argument count
                        if args.len() != params.len() {
                            self.errors.push(TypeError::new(
                                format!(
                                    "method `{}` expects {} arguments, got {}",
                                    method,
                                    params.len(),
                                    args.len()
                                ),
                                *span,
                            ));
                            return self.substitution.apply(&ret);
                        }

                        // Type check arguments
                        self.check_call_args(args, &params, env);

                        self.substitution.apply(&ret)
                    }
                    Some(_) => {
                        self.errors.push(TypeError::new(
                            format!("`{}` is not a method", method),
                            *span,
                        ));
                        self.fresh_var()
                    }
                    None => {
                        self.errors.push(TypeError::new(
                            format!("undefined method `{}` on struct `{}`", method, struct_name),
                            *span,
                        ));
                        for arg in args {
                            self.infer_expr(arg, env);
                        }
                        self.fresh_var()
                    }
                }
            }
            Expr::Asm(asm_block) => {
                // For asm blocks, we just check that input variables exist
                // and return the declared output type (or Any if none)
                for input_name in &asm_block.inputs {
                    if let Some(ty) = env.lookup(input_name) {
                        let _ = ty.clone();
                    }
                }
                // Return type based on output_type annotation or Any
                match asm_block.output_type.as_deref() {
                    Some("i64") => Type::Int,
                    Some("f64") => Type::Float,
                    Some("bool") => Type::Bool,
                    Some("string") => Type::string(),
                    Some("nil") => Type::Nil,
                    _ => self.fresh_var(), // Any/unknown type
                }
            }

            Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                args,
                span,
                ..
            } => {
                // Check if this is an associated function on a struct or builtin type
                self.infer_associated_function_call(
                    type_name, type_args, function, args, env, *span,
                )
            }

            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                span,
                ..
            } => {
                // Type check the elements
                for elem in elements.iter_mut() {
                    match elem {
                        NewLiteralElement::Value(e) => {
                            self.infer_expr(e, env);
                        }
                        NewLiteralElement::KeyValue { key, value } => {
                            self.infer_expr(key, env);
                            self.infer_expr(value, env);
                        }
                    }
                }

                // Resolve type arguments to Types
                let resolved_type_args: Vec<Type> = type_args
                    .iter()
                    .filter_map(|ann| self.resolve_type_annotation(ann, *span).ok())
                    .collect();

                // Look up the struct definition
                if let Some(struct_info) = self.structs.get(type_name).cloned() {
                    // Build the result type as GenericStruct
                    if struct_info.type_params.is_empty() {
                        // Non-generic struct
                        let fields = struct_info
                            .fields
                            .iter()
                            .map(|(name, ty)| (name.clone(), ty.clone()))
                            .collect();
                        Type::Struct {
                            name: type_name.clone(),
                            fields,
                        }
                    } else {
                        // Generic struct - substitute type params
                        let fields: Vec<(String, Type)> = struct_info
                            .fields
                            .iter()
                            .map(|(name, ty)| {
                                let mut substituted = ty.clone();
                                for (param_name, type_arg) in struct_info
                                    .type_params
                                    .iter()
                                    .zip(resolved_type_args.iter())
                                {
                                    substituted =
                                        substituted.substitute_param(param_name, type_arg);
                                }
                                (name.clone(), substituted)
                            })
                            .collect();
                        Type::GenericStruct {
                            name: type_name.clone(),
                            type_args: resolved_type_args,
                            fields,
                        }
                    }
                } else {
                    // Unknown type - report error
                    self.errors.push(TypeError::new(
                        format!("unknown type '{}' in type literal", type_name),
                        *span,
                    ));
                    self.fresh_var()
                }
            }

            Expr::Block {
                statements,
                expr,
                span: _,
                ..
            } => {
                // Block is generated by desugar, which runs after type checking.
                // This should not be encountered during type checking, but we handle it
                // for completeness. Type check all statements and return the type of the final expr.
                for stmt in statements.iter_mut() {
                    self.infer_statement(stmt, env);
                }
                self.infer_expr(expr, env)
            }

            Expr::Lambda {
                params,
                return_type,
                body,
                span,
                ..
            } => {
                // Infer parameter types
                let param_types: Vec<Type> = params
                    .iter()
                    .map(|p| {
                        if let Some(ann) = &p.type_annotation {
                            self.resolve_type_annotation(ann, p.span)
                                .unwrap_or_else(|e| {
                                    self.errors.push(e);
                                    self.fresh_var()
                                })
                        } else {
                            self.fresh_var()
                        }
                    })
                    .collect();

                let expected_ret = if let Some(ann) = return_type {
                    self.resolve_type_annotation(ann, *span)
                        .unwrap_or_else(|e| {
                            self.errors.push(e);
                            self.fresh_var()
                        })
                } else {
                    self.fresh_var()
                };

                // Enter new scope and bind parameters
                env.enter_scope();
                for (param, param_type) in params.iter().zip(param_types.iter()) {
                    env.bind(param.name.clone(), param_type.clone());
                }

                // Infer body type
                let body_type = {
                    let mut result_type = Type::Nil;
                    for stmt in &mut body.statements {
                        result_type = self.infer_statement(stmt, env);
                    }
                    result_type
                };

                env.exit_scope();

                // Unify body type with expected return type
                if let Err(e) = self.unify(&body_type, &expected_ret, *span) {
                    self.errors.push(e);
                }

                Type::Function {
                    params: param_types
                        .iter()
                        .map(|t| self.substitution.apply(t))
                        .collect(),
                    ret: Box::new(self.substitution.apply(&expected_ret)),
                }
            }

            Expr::AsDyn { expr, .. } => {
                // Infer the inner expression type, then wrap as Dyn
                self.infer_expr(expr, env);
                Type::Dyn
            }

            Expr::CallExpr {
                callee, args, span, ..
            } => {
                let callee_type = self.infer_expr(callee, env);
                let resolved = self.substitution.apply(&callee_type);

                match resolved {
                    Type::Function { params, ret } => {
                        if args.len() != params.len() {
                            self.errors.push(TypeError::new(
                                format!(
                                    "closure expects {} arguments, got {}",
                                    params.len(),
                                    args.len()
                                ),
                                *span,
                            ));
                            return self.substitution.apply(&ret);
                        }

                        self.check_call_args(args, &params, env);

                        self.substitution.apply(&ret)
                    }
                    Type::Var(_) => {
                        // Unknown callee type - infer args and create function type constraint
                        let arg_types: Vec<Type> = args
                            .iter_mut()
                            .map(|arg| self.infer_expr(arg, env))
                            .collect();
                        let ret = self.fresh_var();
                        let fn_type = Type::Function {
                            params: arg_types,
                            ret: Box::new(ret.clone()),
                        };
                        if let Err(e) = self.unify(&callee_type, &fn_type, *span) {
                            self.errors.push(e);
                        }
                        self.substitution.apply(&ret)
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected function type, found `{}`", resolved),
                            *span,
                        ));
                        // Still infer args for error reporting
                        for arg in args.iter_mut() {
                            self.infer_expr(arg, env);
                        }
                        self.fresh_var()
                    }
                }
            }
        }
    }

    /// Infer the type of an associated function call (Type::func()).
    fn infer_associated_function_call(
        &mut self,
        type_name: &str,
        struct_type_args: &[crate::compiler::types::TypeAnnotation],
        function: &str,
        args: &mut [Expr],
        env: &mut TypeEnv,
        span: Span,
    ) -> Type {
        // Infer types of all arguments
        let arg_types: Vec<Type> = args
            .iter_mut()
            .map(|arg| self.infer_expr(arg, env))
            .collect();

        // Check if it's an associated function on a struct
        // Clone the struct info to avoid borrow issues
        let struct_info_opt = self.structs.get(type_name).cloned();

        if let Some(struct_info) = struct_info_opt {
            let fn_type_opt = struct_info.methods.get(function).cloned();

            if let Some(Type::Function { params, ret }) = fn_type_opt {
                // For generic structs, substitute type params with provided type args
                let (params, ret) = if !struct_type_args.is_empty()
                    && !struct_info.type_params.is_empty()
                {
                    // Resolve type arguments to Types
                    let resolved_type_args: Vec<Type> = struct_type_args
                        .iter()
                        .map(|ann| {
                            self.resolve_type_annotation(ann, span).unwrap_or_else(|e| {
                                self.errors.push(e);
                                self.fresh_var()
                            })
                        })
                        .collect();

                    // Substitute type parameters with concrete types
                    let mut substituted_params = params;
                    let mut substituted_ret = *ret.clone();
                    for (param_name, type_arg) in struct_info
                        .type_params
                        .iter()
                        .zip(resolved_type_args.iter())
                    {
                        substituted_params = substituted_params
                            .iter()
                            .map(|p| p.substitute_param(param_name, type_arg))
                            .collect();
                        substituted_ret = substituted_ret.substitute_param(param_name, type_arg);
                    }
                    (substituted_params, Box::new(substituted_ret))
                } else {
                    (params, ret)
                };

                // Found the associated function - check argument types
                // Check argument count
                if params.len() != arg_types.len() {
                    self.errors.push(TypeError::new(
                        format!(
                            "{}::{} expects {} arguments, got {}",
                            type_name,
                            function,
                            params.len(),
                            arg_types.len()
                        ),
                        span,
                    ));
                } else {
                    // Check each argument type
                    for (i, (param, arg_type)) in params.iter().zip(arg_types.iter()).enumerate() {
                        if let Err(e) = self.unify(param, arg_type, span) {
                            self.errors.push(TypeError::new(
                                format!(
                                    "{}::{} argument {} type mismatch: {}",
                                    type_name,
                                    function,
                                    i + 1,
                                    e.message
                                ),
                                span,
                            ));
                        }
                    }
                }
                return self.substitution.apply(ret.as_ref());
            }
        }

        // Not found
        self.errors.push(TypeError::new(
            format!(
                "no associated function `{}` found for type `{}`",
                function, type_name
            ),
            span,
        ));
        self.fresh_var()
    }

    /// Type-check call arguments against parameter types.
    /// Inserts implicit `as dyn` coercion when a parameter expects `dyn`
    /// and the argument is a non-dyn concrete type.
    fn check_call_args(&mut self, args: &mut [Expr], params: &[Type], env: &mut TypeEnv) {
        for (arg, param_type) in args.iter_mut().zip(params.iter()) {
            let arg_type = self.infer_expr(arg, env);
            let resolved_param = self.substitution.apply(param_type);
            let resolved_arg = self.substitution.apply(&arg_type);
            // Implicit dyn coercion: only for concrete value types, not for
            // types that are already compatible (dyn, any) or unresolved (Var, Param).
            // Note: `any` and `Var` are excluded because they may already be dyn at runtime
            // (e.g., from _dyn_to_string calling __dyn_type_name). Auto-boxing them would
            // cause double-boxing.
            if matches!(resolved_param, Type::Dyn)
                && !matches!(
                    resolved_arg,
                    Type::Dyn | Type::Any | Type::Var(_) | Type::Param { .. }
                )
            {
                // Implicit dyn coercion: wrap argument in AsDyn
                let span = arg.span();
                let inner = mem::replace(
                    arg,
                    Expr::Nil {
                        span,
                        inferred_type: None,
                    },
                );
                *arg = Expr::AsDyn {
                    expr: Box::new(inner),
                    span,
                    inferred_type: Some(Type::Dyn),
                    is_implicit: true,
                };
            } else if let Err(e) = self.unify(&arg_type, param_type, arg.span()) {
                self.errors.push(e);
            }
        }
    }

    /// Check builtin function calls.
    fn check_builtin(
        &mut self,
        name: &str,
        args: &mut [Expr],
        env: &mut TypeEnv,
        span: Span,
    ) -> Option<Type> {
        match name {
            "__typeof" => {
                // __typeof accepts any type, returns int (type tag)
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Int)
            }
            "__heap_size" => {
                // __heap_size accepts any type (ref), returns int (slot count)
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Int)
            }
            "__hostcall" => {
                // __hostcall(num, ...args) -> Int | String
                // First argument must be hostcall number (Int), rest depends on hostcall
                if args.is_empty() {
                    self.errors.push(TypeError::new(
                        "__hostcall expects at least 1 argument (hostcall number)",
                        span,
                    ));
                    return Some(self.fresh_var());
                }
                // First arg must be Int (hostcall number)
                let num_type = self.infer_expr(&mut args[0], env);
                if let Err(e) = self.unify(&num_type, &Type::Int, span) {
                    self.errors.push(e);
                }
                // Infer types for remaining arguments (no strict checking)
                for arg in args.iter_mut().skip(1) {
                    self.infer_expr(arg, env);
                }
                // Return type depends on hostcall (can be Int or String for read)
                Some(self.fresh_var())
            }
            "len" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("len expects 1 argument", span));
                    return Some(Type::Int);
                }
                let arg_type = self.infer_expr(&mut args[0], env);
                let resolved = self.substitution.apply(&arg_type);
                // len works on array or string
                match &resolved {
                    t if t.is_array() => {}
                    Type::Var(_) => {}
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("len expects array or string, got `{}`", arg_type),
                            span,
                        ));
                    }
                }
                // The resolved type is now on the argument's inferred_type via infer_expr
                Some(Type::Int)
            }
            "push" => {
                if args.len() != 2 {
                    self.errors
                        .push(TypeError::new("push expects 2 arguments", span));
                    return Some(Type::Nil);
                }
                let arr_type = self.infer_expr(&mut args[0], env);
                let val_type = self.infer_expr(&mut args[1], env);

                let elem_type = self.fresh_var();
                let expected = Type::array(elem_type.clone());
                if let Err(e) = self.unify(&arr_type, &expected, span) {
                    self.errors.push(e);
                }
                if let Err(e) = self.unify(&val_type, &elem_type, span) {
                    self.errors.push(e);
                }
                Some(Type::Nil)
            }
            "pop" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("pop expects 1 argument", span));
                    return Some(self.fresh_var());
                }
                let arr_type = self.infer_expr(&mut args[0], env);
                let elem_type = self.fresh_var();
                let expected = Type::array(elem_type.clone());
                if let Err(e) = self.unify(&arr_type, &expected, span) {
                    self.errors.push(e);
                }
                Some(self.substitution.apply(&elem_type))
            }
            "type_of" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("type_of expects 1 argument", span));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::string())
            }
            // Thread operations - for now just return appropriate types
            "spawn" | "channel" | "send" | "recv" | "join" => {
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(self.fresh_var())
            }
            // Low-level heap intrinsics (for stdlib implementation)
            "__heap_load" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        "__heap_load expects 2 arguments (ref, index)",
                        span,
                    ));
                }
                for arg in args.iter_mut() {
                    self.infer_expr(arg, env);
                }
                // Try to infer return type from first argument's struct type and literal offset
                let first_type = args
                    .first()
                    .and_then(|a| a.inferred_type())
                    .map(|t| self.substitution.apply(t));
                let offset = args.get(1).and_then(|a| match a {
                    Expr::Int { value, .. } => Some(*value as usize),
                    _ => None,
                });
                match (first_type.as_ref(), offset) {
                    (Some(Type::GenericStruct { fields, .. }), Some(idx)) if idx < fields.len() => {
                        Some(fields[idx].1.clone())
                    }
                    (Some(Type::Struct { fields, .. }), Some(idx)) if idx < fields.len() => {
                        Some(fields[idx].1.clone())
                    }
                    _ => Some(Type::Any),
                }
            }
            "__heap_store" => {
                if args.len() != 3 {
                    self.errors.push(TypeError::new(
                        "__heap_store expects 3 arguments (ref, index, value)",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Nil)
            }
            "__alloc_heap" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        "__alloc_heap expects 1 argument (size)",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                // Returns a raw heap pointer with fresh type variable
                let elem = self.fresh_var();
                Some(Type::Ptr(Box::new(elem)))
            }
            "__null_ptr" => {
                if !args.is_empty() {
                    self.errors
                        .push(TypeError::new("__null_ptr expects 0 arguments", span));
                }
                // Returns a null pointer with fresh type variable
                let elem = self.fresh_var();
                Some(Type::Ptr(Box::new(elem)))
            }
            "__ptr_offset" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        "__ptr_offset expects 2 arguments (ptr, offset)",
                        span,
                    ));
                }
                let ptr_type = if !args.is_empty() {
                    self.infer_expr(&mut args[0], env)
                } else {
                    self.fresh_var()
                };
                if args.len() > 1 {
                    let offset_type = self.infer_expr(&mut args[1], env);
                    if let Err(e) = self.unify(&offset_type, &Type::Int, span) {
                        self.errors.push(e);
                    }
                }
                // ptr_type should be ptr<T>, return same ptr<T>
                let elem = self.fresh_var();
                let expected_ptr = Type::Ptr(Box::new(elem));
                if let Err(e) = self.unify(&ptr_type, &expected_ptr, span) {
                    self.errors.push(e);
                }
                Some(self.substitution.apply(&expected_ptr))
            }
            "__umul128_hi" => {
                if args.len() != 2 {
                    self.errors
                        .push(TypeError::new("__umul128_hi expects 2 arguments", span));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Int)
            }
            "__alloc_string" => {
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        "__alloc_string expects 2 arguments (data_ref, len)",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Any) // Returns a string reference
            }
            "__call_func" => {
                // __call_func(func_idx, arg) → any
                // Calls function by dynamic index with one argument
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        "__call_func expects 2 arguments (func_idx, arg)",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Any)
            }
            // CLI argument operations
            "argc" => {
                if !args.is_empty() {
                    self.errors
                        .push(TypeError::new("argc expects 0 arguments", span));
                }
                Some(Type::Int)
            }
            "argv" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("argv expects 1 argument (index)", span));
                    return Some(Type::string());
                }
                let arg_type = self.infer_expr(&mut args[0], env);
                if let Err(e) = self.unify(&arg_type, &Type::Int, span) {
                    self.errors.push(e);
                }
                Some(Type::string())
            }
            "args" => {
                if !args.is_empty() {
                    self.errors
                        .push(TypeError::new("args expects 0 arguments", span));
                }
                Some(Type::array(Type::string()))
            }
            // vec_new and map_new are now associated functions (vec::new(), map::new())
            // They are defined in prelude.mc using impl vec/map blocks
            _ => None,
        }
    }

    /// Type check method calls on ptr<T>.
    fn check_ptr_method(
        &mut self,
        method: &str,
        args: &mut [Expr],
        elem_type: &Type,
        env: &mut TypeEnv,
        span: Span,
    ) -> Type {
        match method {
            "offset" => {
                // offset(n: int) -> ptr<T>
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("ptr.offset expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return Type::Ptr(Box::new(self.substitution.apply(elem_type)));
                }
                let arg_type = self.infer_expr(&mut args[0], env);
                if let Err(e) = self.unify(&arg_type, &Type::Int, args[0].span()) {
                    self.errors.push(e);
                }
                Type::Ptr(Box::new(self.substitution.apply(elem_type)))
            }
            _ => {
                self.errors.push(TypeError::new(
                    format!("undefined method `{}` on ptr<{}>", method, elem_type),
                    span,
                ));
                for arg in args {
                    self.infer_expr(arg, env);
                }
                self.fresh_var()
            }
        }
    }

    /// Type check method calls on primitive types (int, float, bool, string).
    fn check_primitive_method(
        &mut self,
        type_name: &str,
        method: &str,
        args: &mut [Expr],
        env: &mut TypeEnv,
        span: Span,
    ) -> Type {
        let method_type = self
            .primitive_methods
            .get(type_name)
            .and_then(|methods| methods.get(method))
            .cloned();

        match method_type {
            Some(Type::Function { params, ret }) => {
                if args.len() != params.len() {
                    self.errors.push(TypeError::new(
                        format!(
                            "method `{}` on `{}` expects {} arguments, got {}",
                            method,
                            type_name,
                            params.len(),
                            args.len()
                        ),
                        span,
                    ));
                    return self.substitution.apply(&ret);
                }
                self.check_call_args(args, &params, env);
                self.substitution.apply(&ret)
            }
            Some(_) => {
                self.errors.push(TypeError::new(
                    format!("`{}` is not a method on `{}`", method, type_name),
                    span,
                ));
                self.fresh_var()
            }
            None => {
                self.errors.push(TypeError::new(
                    format!("undefined method `{}` on `{}`", method, type_name),
                    span,
                ));
                for arg in args {
                    self.infer_expr(arg, env);
                }
                self.fresh_var()
            }
        }
    }

    /// Convert a concrete Type to the name used in interface_impls lookup.
    /// Resolve the best overload for a function call based on concrete type arguments.
    /// Returns the index of the overload whose bounds are all satisfied and has the
    /// highest specificity (most total bounds). Falls back to an unbounded overload
    /// if no bounded overload is fully satisfied.
    fn resolve_overload(&self, overloads: &[GenericFunctionInfo], resolved_args: &[Type]) -> usize {
        let mut best_index = 0;
        let mut best_score: i32 = -1;

        for (i, overload) in overloads.iter().enumerate() {
            let mut all_satisfied = true;
            let mut score: i32 = 0;

            for (j, bounds) in overload.type_param_bounds.iter().enumerate() {
                if bounds.is_empty() {
                    continue;
                }
                if j >= resolved_args.len() {
                    all_satisfied = false;
                    break;
                }
                let concrete_type = &resolved_args[j];
                let type_name = self.type_to_impl_name(concrete_type);
                for bound in bounds {
                    if self
                        .interface_impls
                        .contains(&(bound.clone(), type_name.clone()))
                    {
                        score += 1;
                    } else {
                        all_satisfied = false;
                        break;
                    }
                }
                if !all_satisfied {
                    break;
                }
            }

            if all_satisfied && score > best_score {
                best_score = score;
                best_index = i;
            }
        }

        best_index
    }

    fn type_to_impl_name(&self, ty: &Type) -> String {
        match ty {
            Type::Int => "int".to_string(),
            Type::Float => "float".to_string(),
            Type::Bool => "bool".to_string(),
            t if t.is_string() => "string".to_string(),
            Type::Struct { name, .. } => name.clone(),
            Type::GenericStruct { name, .. } => name.clone(),
            _ => ty.to_string(),
        }
    }

    /// Finalize type checking: apply substitution and check for unresolved vars.
    fn finalize(&mut self) -> Result<(), Vec<TypeError>> {
        // For now, we don't require all type variables to be resolved
        // This allows for polymorphic code
        Ok(())
    }

    /// Get all collected errors.
    pub fn errors(&self) -> &[TypeError] {
        &self.errors
    }

    /// Get interface implementations: set of (interface_name, type_name).
    pub fn interface_impls(&self) -> &HashSet<(String, String)> {
        &self.interface_impls
    }

    /// Get interface definitions: name -> method names (in canonical order).
    pub fn interface_method_names(&self) -> HashMap<String, Vec<String>> {
        self.interfaces
            .iter()
            .map(|(name, info)| {
                let mut methods: Vec<String> = info.methods.keys().cloned().collect();
                methods.sort(); // canonical order
                (name.clone(), methods)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;
    use crate::compiler::prepend_stdlib;

    fn check(source: &str) -> Result<(), Vec<TypeError>> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();
        let mut parser = Parser::new("test.mc", tokens);
        let parsed_program = parser.parse().unwrap();
        let mut program = prepend_stdlib(parsed_program).unwrap();
        let mut checker = TypeChecker::new("test.mc");
        checker.check_program(&mut program)
    }

    #[test]
    fn test_literal_types() {
        assert!(check("let x = 42;").is_ok());
        assert!(check("let x = 3.14;").is_ok());
        assert!(check("let x = true;").is_ok());
        assert!(check(r#"let x = "hello";"#).is_ok());
        assert!(check("let x = nil;").is_ok());
    }

    #[test]
    fn test_type_annotation_match() {
        assert!(check("let x: int = 42;").is_ok());
        assert!(check("let x: float = 3.14;").is_ok());
        assert!(check("let x: bool = true;").is_ok());
        assert!(check(r#"let x: string = "hello";"#).is_ok());
    }

    #[test]
    fn test_type_annotation_mismatch() {
        let result = check(r#"let x: int = "hello";"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_array_type() {
        assert!(check("let arr = [1, 2, 3];").is_ok());
        assert!(check("let arr: array<int> = [1, 2, 3];").is_ok());
    }

    #[test]
    fn test_array_element_mismatch() {
        let result = check(r#"let arr = [1, "hello"];"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_function_inference() {
        assert!(check("fun add(a, b) { return a + b; } let r = add(1, 2);").is_ok());
    }

    #[test]
    fn test_function_with_types() {
        assert!(check("fun add(a: int, b: int) -> int { return a + b; }").is_ok());
    }

    #[test]
    fn test_nullable_type() {
        assert!(check("let x: string? = nil;").is_ok());
        assert!(check(r#"let x: string? = "hello";"#).is_ok());
    }

    #[test]
    fn test_non_nullable_nil() {
        let result = check("let x: string = nil;");
        assert!(result.is_err());
    }

    // Acceptance Criteria tests

    #[test]
    fn test_ac1_let_infers_int() {
        // AC1: `let x = 1;` infers x as int
        assert!(check("let x = 1; let y: int = x;").is_ok());
    }

    #[test]
    fn test_ac3_function_inference() {
        // AC3: `fun f(a, b) { a + b }` called with f(1, 2) infers int
        assert!(check("fun f(a, b) { return a + b; } let r: int = f(1, 2);").is_ok());
    }

    #[test]
    fn test_ac4_function_arg_mismatch() {
        // AC4: `fun f(a, b) { a + b }` called with f(1, "x") is type error
        let result = check(r#"fun f(a, b) { return a + b; } f(1, "x");"#);
        assert!(result.is_err());
    }

    #[test]
    fn test_ac8_array_type_inferred() {
        // AC8: `[1, 2, 3]` has type `array<int>`
        assert!(check("let arr: array<int> = [1, 2, 3];").is_ok());
    }

    #[test]
    fn test_binary_ops_type_check() {
        // Arithmetic requires same numeric types
        assert!(check("let x = 1 + 2;").is_ok());
        assert!(check("let x = 1.0 + 2.0;").is_ok());
        assert!(check(r#"let x = "a" + "b";"#).is_ok());

        // Comparison returns bool
        assert!(check("let x: bool = 1 < 2;").is_ok());
        assert!(check("let x: bool = 1 == 2;").is_ok());

        // Logical operators
        assert!(check("let x: bool = true && false;").is_ok());
        assert!(check("let x: bool = true || false;").is_ok());
    }

    #[test]
    fn test_if_condition_must_be_bool() {
        assert!(check("if true { let x = 1; }").is_ok());
        let result = check("if 1 { let x = 1; }");
        assert!(result.is_err());
    }

    #[test]
    fn test_while_condition_must_be_bool() {
        assert!(check("while false { let x = 1; }").is_ok());
        let result = check("while 1 { let x = 1; }");
        assert!(result.is_err());
    }

    // Struct type checking tests

    #[test]
    fn test_struct_definition() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_struct_literal() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2 };
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_struct_literal_wrong_field_type() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: "hello", y: 2 };
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_struct_literal_missing_field() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1 };
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_struct_literal_unknown_field() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2, z: 3 };
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_struct_field_access() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2 };
            let x: int = p.x;
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_struct_field_access_unknown_field() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2 };
            let z = p.z;
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_struct_type_annotation() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            let p: Point = Point { x: 1, y: 2 };
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_struct_type_annotation_mismatch() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            struct Other { a: int }
            let p: Point = Other { a: 1 };
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_struct_nullable_field() {
        assert!(
            check(
                r#"
            struct User { name: string, age: int? }
            let u = User { name: "Alice", age: nil };
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_impl_block_method() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            impl Point {
                fun sum(self) -> int {
                    return self.x + self.y;
                }
            }
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_method_call() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            impl Point {
                fun sum(self) -> int {
                    return self.x + self.y;
                }
            }
            let p = Point { x: 1, y: 2 };
            let s: int = p.sum();
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_method_call_with_args() {
        assert!(
            check(
                r#"
            struct Point { x: int, y: int }
            impl Point {
                fun scale(self, factor: int) -> int {
                    return (self.x + self.y) * factor;
                }
            }
            let p = Point { x: 1, y: 2 };
            let s: int = p.scale(3);
        "#
            )
            .is_ok()
        );
    }

    #[test]
    fn test_method_call_wrong_arg_type() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            impl Point {
                fun scale(self, factor: int) -> int {
                    return (self.x + self.y) * factor;
                }
            }
            let p = Point { x: 1, y: 2 };
            let s = p.scale("hello");
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_undefined_struct() {
        let result = check(
            r#"
            let p = Unknown { x: 1 };
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_undefined_method() {
        let result = check(
            r#"
            struct Point { x: int, y: int }
            let p = Point { x: 1, y: 2 };
            p.unknown_method();
        "#,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_impl_for_undefined_struct() {
        let result = check(
            r#"
            impl Unknown {
                fun foo(self) { }
            }
        "#,
        );
        assert!(result.is_err());
    }
}
