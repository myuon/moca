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
    BinaryOp, Block, Expr, FnDef, ImplBlock, Item, Program, Statement, StructDef, UnaryOp,
};
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation, TypeVarId};
use std::collections::{BTreeMap, HashMap};

/// Information about a struct definition.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub name: String,
    /// Fields in declaration order: (name, type)
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
            Type::Array(elem) => Type::Array(Box::new(self.apply(elem))),
            Type::Vector(elem) => Type::Vector(Box::new(self.apply(elem))),
            Type::Map(key, value) => {
                Type::Map(Box::new(self.apply(key)), Box::new(self.apply(value)))
            }
            Type::Nullable(inner) => Type::Nullable(Box::new(self.apply(inner))),
            Type::Object(fields) => {
                let new_fields: BTreeMap<String, Type> = fields
                    .iter()
                    .map(|(k, v)| (k.clone(), self.apply(v)))
                    .collect();
                Type::Object(new_fields)
            }
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
            // Primitive types and Any are unchanged
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Nil | Type::Any => {
                ty.clone()
            }
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

/// The type checker with inference support.
pub struct TypeChecker {
    filename: String,
    next_var_id: TypeVarId,
    errors: Vec<TypeError>,
    /// Function signatures (name -> type)
    functions: HashMap<String, Type>,
    /// Struct definitions (name -> struct info)
    structs: HashMap<String, StructInfo>,
    /// Substitution accumulated during inference
    substitution: Substitution,
    /// Type of objects in index expressions (Span -> Type)
    /// Used by codegen to differentiate between array and vector access
    index_object_types: HashMap<Span, Type>,
}

impl TypeChecker {
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            next_var_id: 0,
            errors: Vec::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            substitution: Substitution::new(),
            index_object_types: HashMap::new(),
        }
    }

    /// Get the index object types map (for codegen)
    pub fn index_object_types(&self) -> &HashMap<Span, Type> {
        &self.index_object_types
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
                    "string" => Ok(Type::String),
                    "nil" => Ok(Type::Nil),
                    "any" => Ok(Type::Any),
                    _ => {
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
            TypeAnnotation::Object(fields) => {
                let mut type_fields = BTreeMap::new();
                for (name, ann) in fields {
                    type_fields.insert(name.clone(), self.resolve_type_annotation(ann, span)?);
                }
                Ok(Type::Object(type_fields))
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
            | (Type::String, Type::String)
            | (Type::Nil, Type::Nil) => Ok(Substitution::new()),

            // Any type unifies with any other type
            // any ~ T -> T (any adapts to the other type)
            // any ~ any -> any
            (Type::Any, _) | (_, Type::Any) => Ok(Substitution::new()),

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

            // Array types
            (Type::Array(elem1), Type::Array(elem2)) => self.unify(elem1, elem2, span),

            // Vector types
            (Type::Vector(elem1), Type::Vector(elem2)) => self.unify(elem1, elem2, span),

            // Map types
            (Type::Map(k1, v1), Type::Map(k2, v2)) => {
                let s1 = self.unify(k1, k2, span)?;
                let s2 = self.unify(v1, v2, span)?;
                Ok(s1.compose(&s2))
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

            // Object types - must have same fields
            (Type::Object(fields1), Type::Object(fields2)) => {
                if fields1.keys().collect::<Vec<_>>() != fields2.keys().collect::<Vec<_>>() {
                    return Err(TypeError::mismatch(t1.clone(), t2.clone(), span));
                }
                let mut result = Substitution::new();
                for (k, v1) in fields1 {
                    if let Some(v2) = fields2.get(k) {
                        let s = self.unify(v1, v2, span)?;
                        result = result.compose(&s);
                    }
                }
                Ok(result)
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

            // Mismatch
            _ => Err(TypeError::mismatch(t1, t2, span)),
        }
    }

    /// Type check a program.
    pub fn check_program(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        // First pass: collect struct definitions
        for item in &program.items {
            if let Item::StructDef(struct_def) = item {
                self.register_struct(struct_def);
            }
        }

        // Second pass: collect function signatures and impl block methods
        for item in &program.items {
            match item {
                Item::FnDef(fn_def) => {
                    let fn_type = self.infer_function_signature(fn_def);
                    self.functions.insert(fn_def.name.clone(), fn_type);
                }
                Item::ImplBlock(impl_block) => {
                    self.register_impl_methods(impl_block);
                }
                _ => {}
            }
        }

        // Third pass: type check function bodies and statements
        let mut main_env = TypeEnv::new();
        for item in &program.items {
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
                Item::Import(_) => {
                    // Imports are handled elsewhere
                }
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
    fn infer_function_signature(&mut self, fn_def: &FnDef) -> Type {
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

        Type::function(param_types, ret_type)
    }

    /// Type check a function definition.
    fn check_function(&mut self, fn_def: &FnDef) {
        let mut env = TypeEnv::new();

        // Get function type
        let fn_type = self.functions.get(&fn_def.name).cloned();
        let (param_types, expected_ret) = match fn_type {
            Some(Type::Function { params, ret }) => (params, *ret),
            _ => return,
        };

        // Bind parameters
        for (param, param_type) in fn_def.params.iter().zip(param_types.iter()) {
            env.bind(param.name.clone(), param_type.clone());
        }

        // Infer body type
        let body_type = self.infer_block(&fn_def.body, &mut env);

        // Unify return type
        if let Err(e) = self.unify(&body_type, &expected_ret, fn_def.span) {
            self.errors.push(e);
        }
    }

    /// Register a struct definition.
    fn register_struct(&mut self, struct_def: &StructDef) {
        let mut fields = Vec::new();
        for field in &struct_def.fields {
            match field.type_annotation.to_type() {
                Ok(ty) => fields.push((field.name.clone(), ty)),
                Err(msg) => {
                    self.errors.push(TypeError::new(msg, field.span));
                    fields.push((field.name.clone(), self.fresh_var()));
                }
            }
        }

        let info = StructInfo {
            name: struct_def.name.clone(),
            fields,
            methods: HashMap::new(),
        };
        self.structs.insert(struct_def.name.clone(), info);
    }

    /// Register methods from an impl block.
    fn register_impl_methods(&mut self, impl_block: &ImplBlock) {
        let struct_name = &impl_block.struct_name;

        // Check if struct exists
        if !self.structs.contains_key(struct_name) {
            self.errors.push(TypeError::new(
                format!("impl for undefined struct `{}`", struct_name),
                impl_block.span,
            ));
            return;
        }

        for method in &impl_block.methods {
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

            let fn_type = Type::function(param_types, ret_type);

            // Add method to struct's method table
            if let Some(struct_info) = self.structs.get_mut(struct_name) {
                struct_info.methods.insert(method.name.clone(), fn_type);
            }
        }
    }

    /// Type check an impl block.
    fn check_impl_block(&mut self, impl_block: &ImplBlock) {
        let struct_name = &impl_block.struct_name;

        // Get struct type for 'self'
        let self_type = if let Some(info) = self.structs.get(struct_name) {
            Type::Struct {
                name: info.name.clone(),
                fields: info.fields.clone(),
            }
        } else {
            return; // Error already reported in register_impl_methods
        };

        for method in &impl_block.methods {
            let mut env = TypeEnv::new();

            // Get method signature from struct info
            let method_type = self
                .structs
                .get(struct_name)
                .and_then(|info| info.methods.get(&method.name))
                .cloned();

            let (param_types, expected_ret) = match method_type {
                Some(Type::Function { params, ret }) => (params, *ret),
                _ => continue,
            };

            // Bind 'self' parameter
            let mut param_iter = param_types.iter();
            for param in &method.params {
                if param.name == "self" {
                    env.bind("self".to_string(), self_type.clone());
                } else if let Some(param_type) = param_iter.next() {
                    env.bind(param.name.clone(), param_type.clone());
                }
            }

            // Infer body type
            let body_type = self.infer_block(&method.body, &mut env);

            // Unify return type
            if let Err(e) = self.unify(&body_type, &expected_ret, method.span) {
                self.errors.push(e);
            }
        }
    }

    /// Infer the type of a block (returns the type of the last expression).
    fn infer_block(&mut self, block: &Block, env: &mut TypeEnv) -> Type {
        env.enter_scope();
        let mut result_type = Type::Nil;

        for stmt in &block.statements {
            result_type = self.infer_statement(stmt, env);
        }

        env.exit_scope();
        result_type
    }

    /// Infer the type of a statement.
    fn infer_statement(&mut self, stmt: &Statement, env: &mut TypeEnv) -> Type {
        match stmt {
            Statement::Let {
                name,
                type_annotation,
                init,
                span,
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
                    Type::Array(elem) => *elem,
                    Type::Var(_) => {
                        // Create fresh element type and unify
                        let elem = self.fresh_var();
                        let arr_type = Type::Array(Box::new(elem.clone()));
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

            Statement::Return { value, span: _ } => {
                if let Some(expr) = value {
                    self.infer_expr(expr, env)
                } else {
                    Type::Nil
                }
            }

            Statement::Expr { expr, .. } => self.infer_expr(expr, env),

            Statement::IndexAssign {
                object,
                index,
                value,
                span,
                ..
            } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);
                let val_type = self.infer_expr(value, env);

                // Index should be int
                if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                    self.errors.push(e);
                }

                // Record the object type for codegen
                let resolved_obj_type = self.substitution.apply(&obj_type);
                self.index_object_types
                    .insert(*span, resolved_obj_type.clone());

                // Object can be array<T>, Vector<T>, or VectorAny struct
                match resolved_obj_type {
                    Type::Array(elem) | Type::Vector(elem) => {
                        if let Err(e) = self.unify(&val_type, &elem, *span) {
                            self.errors.push(e);
                        }
                    }
                    Type::Struct { ref name, .. } if name == "VectorAny" => {
                        // VectorAny allows any element type (untyped)
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
                    Type::Object(fields) => {
                        if let Some(field_type) = fields.get(field)
                            && let Err(e) = self.unify(&val_type, field_type, *span)
                        {
                            self.errors.push(e);
                        }
                        // Allow dynamic field addition (no error for unknown fields)
                    }
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

            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                self.infer_block(try_block, env);
                env.enter_scope();
                // Catch variable is string (error message)
                env.bind(catch_var.clone(), Type::String);
                self.infer_block(catch_block, env);
                env.exit_scope();
                Type::Nil
            }
        }
    }

    /// Infer the type of an expression.
    fn infer_expr(&mut self, expr: &Expr, env: &mut TypeEnv) -> Type {
        match expr {
            Expr::Int { .. } => Type::Int,
            Expr::Float { .. } => Type::Float,
            Expr::Bool { .. } => Type::Bool,
            Expr::Str { .. } => Type::String,
            Expr::Nil { .. } => Type::Nil,

            Expr::Ident { name, span } => {
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

            Expr::Array { elements, span } => {
                if elements.is_empty() {
                    // Empty array has unknown element type
                    Type::Array(Box::new(self.fresh_var()))
                } else {
                    let first_type = self.infer_expr(&elements[0], env);
                    for elem in elements.iter().skip(1) {
                        let elem_type = self.infer_expr(elem, env);
                        if let Err(e) = self.unify(&first_type, &elem_type, *span) {
                            self.errors.push(e);
                        }
                    }
                    Type::Array(Box::new(self.substitution.apply(&first_type)))
                }
            }

            Expr::Index {
                object,
                index,
                span,
                ..
            } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);

                // Index should be int
                if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                    self.errors.push(e);
                }

                // Record the object type for codegen
                let resolved_obj_type = self.substitution.apply(&obj_type);
                self.index_object_types
                    .insert(*span, resolved_obj_type.clone());

                // Object can be array<T>, Vector<T>, string, or struct (structs are compiled as arrays)
                match resolved_obj_type {
                    Type::Array(elem) => self.substitution.apply(&elem),
                    Type::Vector(elem) => self.substitution.apply(&elem),
                    Type::String => Type::Int, // String index returns byte value as int
                    Type::Struct { fields, .. } => {
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
                    Type::Var(_) => {
                        // Unknown type, could be array or struct
                        self.fresh_var()
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!(
                                "expected array, Vector, string or struct, found `{}`",
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
            } => {
                let obj_type = self.infer_expr(object, env);

                match self.substitution.apply(&obj_type) {
                    Type::Object(fields) => {
                        if let Some(field_type) = fields.get(field) {
                            field_type.clone()
                        } else {
                            // Allow dynamic field access (returns unknown type)
                            self.fresh_var()
                        }
                    }
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

            Expr::Unary { op, operand, span } => {
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
                        } else if self.unify(&left_type, &Type::String, *span).is_ok()
                            && self.unify(&right_type, &Type::String, *span).is_ok()
                        {
                            Type::String
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

            Expr::Call { callee, args, span } => {
                // Check for builtin functions
                if let Some(result_type) = self.check_builtin(callee, args, env, *span) {
                    return result_type;
                }

                // User-defined function
                if let Some(fn_type) = self.functions.get(callee).cloned() {
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

                            for (arg, param_type) in args.iter().zip(params.iter()) {
                                let arg_type = self.infer_expr(arg, env);
                                if let Err(e) = self.unify(&arg_type, param_type, arg.span()) {
                                    self.errors.push(e);
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
                } else {
                    self.errors.push(TypeError::new(
                        format!("undefined function `{}`", callee),
                        *span,
                    ));
                    self.fresh_var()
                }
            }

            Expr::StructLiteral { name, fields, span } => {
                // Look up struct definition
                let struct_info = match self.structs.get(name) {
                    Some(info) => info.clone(),
                    None => {
                        self.errors.push(TypeError::new(
                            format!("undefined struct `{}`", name),
                            *span,
                        ));
                        // Still infer field types to find nested errors
                        for (_, expr) in fields {
                            self.infer_expr(expr, env);
                        }
                        return self.fresh_var();
                    }
                };

                // Check that all required fields are provided
                let provided_fields: HashMap<&str, &Expr> =
                    fields.iter().map(|(n, e)| (n.as_str(), e)).collect();

                for (field_name, expected_type) in &struct_info.fields {
                    match provided_fields.get(field_name.as_str()) {
                        Some(expr) => {
                            let actual_type = self.infer_expr(expr, env);
                            if let Err(e) = self.unify(&actual_type, expected_type, expr.span()) {
                                self.errors.push(e);
                            }
                        }
                        None => {
                            self.errors.push(TypeError::new(
                                format!("missing field `{}` in struct `{}`", field_name, name),
                                *span,
                            ));
                        }
                    }
                }

                // Check for extra fields not in the struct definition
                let struct_field_names: std::collections::HashSet<&str> =
                    struct_info.fields.iter().map(|(n, _)| n.as_str()).collect();

                for (field_name, expr) in fields {
                    if !struct_field_names.contains(field_name.as_str()) {
                        self.errors.push(TypeError::new(
                            format!("unknown field `{}` in struct `{}`", field_name, name),
                            expr.span(),
                        ));
                        self.infer_expr(expr, env);
                    }
                }

                Type::Struct {
                    name: name.clone(),
                    fields: struct_info.fields.clone(),
                }
            }

            Expr::MethodCall {
                object,
                method,
                args,
                span,
            } => {
                let obj_type = self.infer_expr(object, env);
                let resolved_obj_type = self.substitution.apply(&obj_type);

                // Handle vec<T> methods
                if let Type::Vector(elem_type) = &resolved_obj_type {
                    return self.check_vec_method(method, args, elem_type, env, *span);
                }

                // Handle map<K, V> methods
                if let Type::Map(key_type, value_type) = &resolved_obj_type {
                    return self.check_map_method(method, args, key_type, value_type, env, *span);
                }

                // Get struct name from object type
                let struct_name = match &resolved_obj_type {
                    Type::Struct { name, .. } => name.clone(),
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
                let method_type = self
                    .structs
                    .get(&struct_name)
                    .and_then(|info| info.methods.get(method))
                    .cloned();

                match method_type {
                    Some(Type::Function { params, ret }) => {
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
                        for (arg, param_type) in args.iter().zip(params.iter()) {
                            let arg_type = self.infer_expr(arg, env);
                            if let Err(e) = self.unify(&arg_type, param_type, arg.span()) {
                                self.errors.push(e);
                            }
                        }

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
                    Some("string") => Type::String,
                    Some("nil") => Type::Nil,
                    _ => self.fresh_var(), // Any/unknown type
                }
            }
        }
    }

    /// Check builtin function calls.
    fn check_builtin(
        &mut self,
        name: &str,
        args: &[Expr],
        env: &mut TypeEnv,
        span: Span,
    ) -> Option<Type> {
        match name {
            "print" | "print_debug" => {
                // print/print_debug accepts any type
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Nil)
            }
            "__syscall" => {
                // __syscall(num, ...args) -> Int | String
                // First argument must be syscall number (Int), rest depends on syscall
                if args.is_empty() {
                    self.errors.push(TypeError::new(
                        "__syscall expects at least 1 argument (syscall number)",
                        span,
                    ));
                    return Some(self.fresh_var());
                }
                // First arg must be Int (syscall number)
                let num_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&num_type, &Type::Int, span) {
                    self.errors.push(e);
                }
                // Infer types for remaining arguments (no strict checking)
                for arg in args.iter().skip(1) {
                    self.infer_expr(arg, env);
                }
                // Return type depends on syscall (can be Int or String for read)
                Some(self.fresh_var())
            }
            "len" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("len expects 1 argument", span));
                    return Some(Type::Int);
                }
                let arg_type = self.infer_expr(&args[0], env);
                // len works on array or string
                match self.substitution.apply(&arg_type) {
                    Type::Array(_) | Type::String => {}
                    Type::Var(_) => {}
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("len expects array or string, got `{}`", arg_type),
                            span,
                        ));
                    }
                }
                Some(Type::Int)
            }
            "push" => {
                if args.len() != 2 {
                    self.errors
                        .push(TypeError::new("push expects 2 arguments", span));
                    return Some(Type::Nil);
                }
                let arr_type = self.infer_expr(&args[0], env);
                let val_type = self.infer_expr(&args[1], env);

                let elem_type = self.fresh_var();
                let expected = Type::Array(Box::new(elem_type.clone()));
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
                let arr_type = self.infer_expr(&args[0], env);
                let elem_type = self.fresh_var();
                let expected = Type::Array(Box::new(elem_type.clone()));
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
                Some(Type::String)
            }
            "to_string" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("to_string expects 1 argument", span));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::String)
            }
            "parse_int" => {
                if args.len() != 1 {
                    self.errors
                        .push(TypeError::new("parse_int expects 1 argument", span));
                    return Some(Type::Int);
                }
                let arg_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&arg_type, &Type::String, span) {
                    self.errors.push(e);
                }
                Some(Type::Int)
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
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Any)
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
                Some(Type::Any) // Returns a reference (opaque)
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
                    return Some(Type::String);
                }
                let arg_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&arg_type, &Type::Int, span) {
                    self.errors.push(e);
                }
                Some(Type::String)
            }
            "args" => {
                if !args.is_empty() {
                    self.errors
                        .push(TypeError::new("args expects 0 arguments", span));
                }
                Some(Type::Array(Box::new(Type::String)))
            }
            _ => None,
        }
    }

    /// Type check method calls on vec<T>.
    fn check_vec_method(
        &mut self,
        method: &str,
        args: &[Expr],
        elem_type: &Type,
        env: &mut TypeEnv,
        span: Span,
    ) -> Type {
        match method {
            "push" => {
                // push(value: T) -> nil
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("vec.push expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return Type::Nil;
                }
                let arg_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&arg_type, elem_type, args[0].span()) {
                    self.errors.push(e);
                }
                Type::Nil
            }
            "pop" => {
                // pop() -> T
                if !args.is_empty() {
                    self.errors.push(TypeError::new(
                        format!("vec.pop expects 0 arguments, got {}", args.len()),
                        span,
                    ));
                }
                self.substitution.apply(elem_type)
            }
            "get" => {
                // get(index: int) -> T
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("vec.get expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return self.substitution.apply(elem_type);
                }
                let index_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&index_type, &Type::Int, args[0].span()) {
                    self.errors.push(e);
                }
                self.substitution.apply(elem_type)
            }
            "set" => {
                // set(index: int, value: T) -> nil
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        format!("vec.set expects 2 arguments, got {}", args.len()),
                        span,
                    ));
                    return Type::Nil;
                }
                let index_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&index_type, &Type::Int, args[0].span()) {
                    self.errors.push(e);
                }
                let value_type = self.infer_expr(&args[1], env);
                if let Err(e) = self.unify(&value_type, elem_type, args[1].span()) {
                    self.errors.push(e);
                }
                Type::Nil
            }
            "len" => {
                // len() -> int
                if !args.is_empty() {
                    self.errors.push(TypeError::new(
                        format!("vec.len expects 0 arguments, got {}", args.len()),
                        span,
                    ));
                }
                Type::Int
            }
            _ => {
                self.errors.push(TypeError::new(
                    format!("undefined method `{}` on vec<{}>", method, elem_type),
                    span,
                ));
                for arg in args {
                    self.infer_expr(arg, env);
                }
                self.fresh_var()
            }
        }
    }

    /// Type check method calls on map<K, V>.
    fn check_map_method(
        &mut self,
        method: &str,
        args: &[Expr],
        key_type: &Type,
        value_type: &Type,
        env: &mut TypeEnv,
        span: Span,
    ) -> Type {
        match method {
            "put" => {
                // put(key: K, value: V) -> nil
                if args.len() != 2 {
                    self.errors.push(TypeError::new(
                        format!("map.put expects 2 arguments, got {}", args.len()),
                        span,
                    ));
                    return Type::Nil;
                }
                let k_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&k_type, key_type, args[0].span()) {
                    self.errors.push(e);
                }
                let v_type = self.infer_expr(&args[1], env);
                if let Err(e) = self.unify(&v_type, value_type, args[1].span()) {
                    self.errors.push(e);
                }
                Type::Nil
            }
            "get" => {
                // get(key: K) -> V
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("map.get expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return self.substitution.apply(value_type);
                }
                let k_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&k_type, key_type, args[0].span()) {
                    self.errors.push(e);
                }
                self.substitution.apply(value_type)
            }
            "contains" => {
                // contains(key: K) -> bool
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("map.contains expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return Type::Bool;
                }
                let k_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&k_type, key_type, args[0].span()) {
                    self.errors.push(e);
                }
                Type::Bool
            }
            "remove" => {
                // remove(key: K) -> bool
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        format!("map.remove expects 1 argument, got {}", args.len()),
                        span,
                    ));
                    return Type::Bool;
                }
                let k_type = self.infer_expr(&args[0], env);
                if let Err(e) = self.unify(&k_type, key_type, args[0].span()) {
                    self.errors.push(e);
                }
                Type::Bool
            }
            "keys" => {
                // keys() -> vec<K>
                if !args.is_empty() {
                    self.errors.push(TypeError::new(
                        format!("map.keys expects 0 arguments, got {}", args.len()),
                        span,
                    ));
                }
                Type::Vector(Box::new(self.substitution.apply(key_type)))
            }
            "values" => {
                // values() -> vec<V>
                if !args.is_empty() {
                    self.errors.push(TypeError::new(
                        format!("map.values expects 0 arguments, got {}", args.len()),
                        span,
                    ));
                }
                Type::Vector(Box::new(self.substitution.apply(value_type)))
            }
            "len" => {
                // len() -> int
                if !args.is_empty() {
                    self.errors.push(TypeError::new(
                        format!("map.len expects 0 arguments, got {}", args.len()),
                        span,
                    ));
                }
                Type::Int
            }
            _ => {
                self.errors.push(TypeError::new(
                    format!(
                        "undefined method `{}` on map<{}, {}>",
                        method, key_type, value_type
                    ),
                    span,
                ));
                for arg in args {
                    self.infer_expr(arg, env);
                }
                self.fresh_var()
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;

    fn check(source: &str) -> Result<(), Vec<TypeError>> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();
        let mut parser = Parser::new("test.mc", tokens);
        let program = parser.parse().unwrap();
        let mut checker = TypeChecker::new("test.mc");
        checker.check_program(&program)
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
