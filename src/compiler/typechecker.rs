//! Type checker with Hindley-Milner type inference (Algorithm W).
//!
//! This module implements:
//! - Type variable management
//! - Substitution (mapping from type variables to types)
//! - Unification algorithm
//! - Type inference for expressions and statements

use crate::compiler::ast::{BinaryOp, Block, Expr, FnDef, Item, Program, Statement, UnaryOp};
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation, TypeVarId};
use std::collections::{BTreeMap, HashMap};

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
            // Primitive types are unchanged
            Type::Int | Type::Float | Type::Bool | Type::String | Type::Nil => ty.clone(),
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
    /// Substitution accumulated during inference
    substitution: Substitution,
}

impl TypeChecker {
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            next_var_id: 0,
            errors: Vec::new(),
            functions: HashMap::new(),
            substitution: Substitution::new(),
        }
    }

    /// Generate a fresh type variable.
    fn fresh_var(&mut self) -> Type {
        let id = self.next_var_id;
        self.next_var_id += 1;
        Type::Var(id)
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

            // Type variable unification
            (Type::Var(id), other) | (other, Type::Var(id)) => {
                if let Type::Var(other_id) = other {
                    if id == other_id {
                        return Ok(Substitution::new());
                    }
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

            // Mismatch
            _ => Err(TypeError::mismatch(t1, t2, span)),
        }
    }

    /// Type check a program.
    pub fn check_program(&mut self, program: &Program) -> Result<(), Vec<TypeError>> {
        // First pass: collect function signatures
        for item in &program.items {
            if let Item::FnDef(fn_def) = item {
                let fn_type = self.infer_function_signature(fn_def);
                self.functions.insert(fn_def.name.clone(), fn_type);
            }
        }

        // Second pass: type check function bodies and statements
        for item in &program.items {
            match item {
                Item::FnDef(fn_def) => {
                    self.check_function(fn_def);
                }
                Item::Statement(stmt) => {
                    let mut env = TypeEnv::new();
                    self.infer_statement(stmt, &mut env);
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
                    ann.to_type().unwrap_or_else(|_| self.fresh_var())
                } else {
                    self.fresh_var()
                }
            })
            .collect();

        let ret_type = if let Some(ann) = &fn_def.return_type {
            ann.to_type().unwrap_or_else(|_| self.fresh_var())
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
                    match ann.to_type() {
                        Ok(declared_type) => {
                            if let Err(e) = self.unify(&init_type, &declared_type, *span) {
                                self.errors.push(e);
                            }
                            env.bind(name.clone(), declared_type);
                        }
                        Err(msg) => {
                            self.errors.push(TypeError::new(msg, *span));
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
                if let Some(var_type) = env.lookup(name).cloned() {
                    if let Err(e) = self.unify(&value_type, &var_type, *span) {
                        self.errors.push(e);
                    }
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

            Statement::Return { value, span } => {
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
            } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);
                let val_type = self.infer_expr(value, env);

                // Object should be array<T>
                let elem_type = self.fresh_var();
                let arr_type = Type::Array(Box::new(elem_type.clone()));
                if let Err(e) = self.unify(&obj_type, &arr_type, *span) {
                    self.errors.push(e);
                }
                if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                    self.errors.push(e);
                }
                if let Err(e) = self.unify(&val_type, &elem_type, *span) {
                    self.errors.push(e);
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

                // Check field exists in object type
                match self.substitution.apply(&obj_type) {
                    Type::Object(fields) => {
                        if let Some(field_type) = fields.get(field) {
                            if let Err(e) = self.unify(&val_type, field_type, *span) {
                                self.errors.push(e);
                            }
                        } else {
                            self.errors.push(TypeError::new(
                                format!("unknown field `{}` on type `{}`", field, obj_type),
                                *span,
                            ));
                        }
                    }
                    Type::Var(_) => {
                        // Can't infer field assignment on unknown object type
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected object, found `{}`", obj_type),
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
                } else {
                    self.errors
                        .push(TypeError::new(format!("undefined variable `{}`", name), *span));
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

            Expr::Object { fields, .. } => {
                let type_fields: BTreeMap<String, Type> = fields
                    .iter()
                    .map(|(name, expr)| (name.clone(), self.infer_expr(expr, env)))
                    .collect();
                Type::Object(type_fields)
            }

            Expr::Index { object, index, span } => {
                let obj_type = self.infer_expr(object, env);
                let idx_type = self.infer_expr(index, env);

                // Index should be int
                if let Err(e) = self.unify(&idx_type, &Type::Int, *span) {
                    self.errors.push(e);
                }

                // Object should be array<T>
                let elem_type = self.fresh_var();
                let arr_type = Type::Array(Box::new(elem_type.clone()));
                if let Err(e) = self.unify(&obj_type, &arr_type, *span) {
                    self.errors.push(e);
                    return self.fresh_var();
                }

                self.substitution.apply(&elem_type)
            }

            Expr::Field { object, field, span } => {
                let obj_type = self.infer_expr(object, env);

                match self.substitution.apply(&obj_type) {
                    Type::Object(fields) => {
                        if let Some(field_type) = fields.get(field) {
                            field_type.clone()
                        } else {
                            self.errors.push(TypeError::new(
                                format!("unknown field `{}` on type `{}`", field, obj_type),
                                *span,
                            ));
                            self.fresh_var()
                        }
                    }
                    Type::Var(_) => {
                        // Can't infer field access on unknown object type
                        self.fresh_var()
                    }
                    _ => {
                        self.errors.push(TypeError::new(
                            format!("expected object, found `{}`", obj_type),
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
                        let result = self.fresh_var();
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
                        if self.unify(&left_type, &Type::Int, *span).is_ok()
                            && self.unify(&right_type, &Type::Int, *span).is_ok()
                        {
                            // OK
                        } else if self.unify(&left_type, &Type::Float, *span).is_ok()
                            && self.unify(&right_type, &Type::Float, *span).is_ok()
                        {
                            // OK
                        } else {
                            self.errors.push(TypeError::new(
                                format!(
                                    "cannot compare `{}` and `{}`",
                                    left_type, right_type
                                ),
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
            "print" => {
                // print accepts any type
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::Nil)
            }
            "len" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        "len expects 1 argument",
                        span,
                    ));
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
                    self.errors.push(TypeError::new(
                        "push expects 2 arguments",
                        span,
                    ));
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
                    self.errors.push(TypeError::new(
                        "pop expects 1 argument",
                        span,
                    ));
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
                    self.errors.push(TypeError::new(
                        "type_of expects 1 argument",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::String)
            }
            "to_string" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        "to_string expects 1 argument",
                        span,
                    ));
                }
                for arg in args {
                    self.infer_expr(arg, env);
                }
                Some(Type::String)
            }
            "parse_int" => {
                if args.len() != 1 {
                    self.errors.push(TypeError::new(
                        "parse_int expects 1 argument",
                        span,
                    ));
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
            _ => None,
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
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens().unwrap();
        let mut parser = Parser::new("test.mica", tokens);
        let program = parser.parse().unwrap();
        let mut checker = TypeChecker::new("test.mica");
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
        assert!(check(
            "fun add(a, b) { return a + b; } let r = add(1, 2);"
        ).is_ok());
    }

    #[test]
    fn test_function_with_types() {
        assert!(check(
            "fun add(a: int, b: int) -> int { return a + b; }"
        ).is_ok());
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

    #[test]
    fn test_object_type() {
        assert!(check(r#"let obj = {x: 1, y: "a"};"#).is_ok());
    }
}
