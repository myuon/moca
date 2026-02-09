use crate::compiler::ast::*;
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};
use std::collections::HashMap;

/// A resolved asm instruction.
#[derive(Debug, Clone)]
pub enum ResolvedAsmInstruction {
    Emit { op_name: String, args: Vec<AsmArg> },
    Safepoint,
    GcHint { size: i64 },
}

/// Resolved program with variable indices and function references.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub functions: Vec<ResolvedFunction>,
    pub main_body: Vec<ResolvedStatement>,
    pub structs: Vec<ResolvedStruct>,
    /// Number of local variables in the main body
    pub main_locals_count: usize,
    /// Type information for main body local variables (indexed by slot)
    pub main_local_types: Vec<Type>,
}

/// Information about a resolved struct.
#[derive(Debug, Clone)]
pub struct ResolvedStruct {
    pub name: String,
    /// Field names in declaration order
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedFunction {
    pub name: String,
    pub params: Vec<String>,
    pub locals_count: usize,
    pub body: Vec<ResolvedStatement>,
    /// Type information for local variables (indexed by slot number)
    pub local_types: Vec<Type>,
    /// Whether this function is marked with @inline
    pub is_inline: bool,
}

#[derive(Debug, Clone)]
pub enum ResolvedStatement {
    Let {
        slot: usize,
        init: ResolvedExpr,
    },
    Assign {
        slot: usize,
        value: ResolvedExpr,
    },
    IndexAssign {
        object: ResolvedExpr,
        index: ResolvedExpr,
        value: ResolvedExpr,
        span: Span,
        /// Type of the object (from typechecker, for codegen)
        object_type: Option<Type>,
    },
    FieldAssign {
        object: ResolvedExpr,
        field: String,
        value: ResolvedExpr,
    },
    If {
        condition: ResolvedExpr,
        then_block: Vec<ResolvedStatement>,
        else_block: Option<Vec<ResolvedStatement>>,
    },
    While {
        condition: ResolvedExpr,
        body: Vec<ResolvedStatement>,
    },
    ForIn {
        slot: usize,
        iterable: ResolvedExpr,
        body: Vec<ResolvedStatement>,
    },
    Return {
        value: Option<ResolvedExpr>,
    },
    Throw {
        value: ResolvedExpr,
    },
    Try {
        try_block: Vec<ResolvedStatement>,
        catch_slot: usize,
        catch_block: Vec<ResolvedStatement>,
    },
    Expr {
        expr: ResolvedExpr,
    },
}

#[derive(Debug, Clone)]
pub enum ResolvedExpr {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Nil,
    Local(usize),
    Array {
        elements: Vec<ResolvedExpr>,
    },
    Index {
        object: Box<ResolvedExpr>,
        index: Box<ResolvedExpr>,
        span: Span,
        /// Type of the object (from typechecker, for codegen)
        object_type: Option<Type>,
    },
    Field {
        object: Box<ResolvedExpr>,
        field: String,
    },
    Unary {
        op: UnaryOp,
        operand: Box<ResolvedExpr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<ResolvedExpr>,
        right: Box<ResolvedExpr>,
    },
    Call {
        func_index: usize,
        args: Vec<ResolvedExpr>,
    },
    Builtin {
        name: String,
        args: Vec<ResolvedExpr>,
        span: Span,
    },
    /// Spawn a thread with a specific function
    SpawnFunc {
        func_index: usize,
    },
    /// Struct literal: `Point { x: 1, y: 2 }`
    /// Fields are resolved to expressions in declaration order (struct field order).
    StructLiteral {
        struct_index: usize,
        /// Field values in declaration order (not named anymore)
        fields: Vec<ResolvedExpr>,
    },
    /// Method call: `obj.method(args)`
    /// Statically dispatched to the resolved function.
    MethodCall {
        object: Box<ResolvedExpr>,
        method: String,
        func_index: usize,
        args: Vec<ResolvedExpr>,
        /// If the method returns a struct, the struct name
        return_struct_name: Option<String>,
    },
    /// Associated function call: `Type::func(args)`
    /// Statically dispatched to the resolved function.
    AssociatedFunctionCall {
        func_index: usize,
        args: Vec<ResolvedExpr>,
        /// If the function returns a struct, the struct name
        return_struct_name: Option<String>,
    },
    /// Inline assembly block.
    AsmBlock {
        /// Resolved input variable slots.
        input_slots: Vec<usize>,
        /// Output type name (for validation).
        output_type: Option<String>,
        /// Resolved asm instructions.
        body: Vec<ResolvedAsmInstruction>,
    },
    /// New literal: `new Vec<int> {1, 2, 3}` or `new Map<string, int> {"a": 1}`
    NewLiteral {
        type_name: String,
        type_args: Vec<crate::compiler::types::TypeAnnotation>,
        elements: Vec<ResolvedNewLiteralElement>,
    },
    /// Block expression: `{ stmt1; stmt2; expr }` - evaluates to the final expression.
    Block {
        statements: Vec<ResolvedStatement>,
        expr: Box<ResolvedExpr>,
    },
}

/// An element in a resolved new literal.
#[derive(Debug, Clone)]
pub enum ResolvedNewLiteralElement {
    /// Simple value: `1`, `"foo"` etc.
    Value(ResolvedExpr),
    /// Key-value pair: `"a": 1`, `key: value`
    KeyValue {
        key: ResolvedExpr,
        value: ResolvedExpr,
    },
}

/// Information about a struct during resolution.
#[derive(Debug, Clone)]
struct StructDefInfo {
    index: usize,
    /// Field names in declaration order
    fields: Vec<String>,
    /// Methods: method_name -> function_index
    methods: HashMap<String, usize>,
    /// Method return types: method_name -> struct_name (if returns a struct)
    method_return_types: HashMap<String, Option<String>>,
}

/// The resolver performs name resolution and variable slot assignment.
pub struct Resolver<'a> {
    filename: &'a str,
    functions: HashMap<String, usize>,
    builtins: Vec<String>,
    /// Struct definitions: struct_name -> info
    structs: HashMap<String, StructDefInfo>,
    /// Resolved struct list (for output)
    resolved_structs: Vec<ResolvedStruct>,
}

impl<'a> Resolver<'a> {
    pub fn new(filename: &'a str) -> Self {
        Self {
            filename,
            functions: HashMap::new(),
            builtins: vec![
                "print".to_string(),
                "print_debug".to_string(),
                "len".to_string(),
                "type_of".to_string(),
                "to_string".to_string(),
                "parse_int".to_string(),
                // Thread operations
                "spawn".to_string(),
                "channel".to_string(),
                "send".to_string(),
                "recv".to_string(),
                "join".to_string(),
                // Syscall operations (generic syscall builtin)
                "__syscall".to_string(),
                // Low-level heap intrinsics (for stdlib implementation)
                "__heap_load".to_string(),
                "__heap_store".to_string(),
                "__alloc_heap".to_string(),
                // CLI argument operations
                "argc".to_string(),
                "argv".to_string(),
                "args".to_string(),
            ],
            structs: HashMap::new(),
            resolved_structs: Vec::new(),
        }
    }

    /// Collect variable types from AST Statement::Let.inferred_type fields.
    fn collect_var_types(stmts: &[Statement]) -> HashMap<String, Type> {
        let mut type_map = HashMap::new();
        Self::collect_var_types_inner(stmts, &mut type_map);
        type_map
    }

    fn collect_var_types_inner(stmts: &[Statement], type_map: &mut HashMap<String, Type>) {
        for stmt in stmts {
            match stmt {
                Statement::Let {
                    name,
                    inferred_type: Some(ty),
                    ..
                } => {
                    type_map.insert(name.clone(), ty.clone());
                }
                Statement::If {
                    then_block,
                    else_block,
                    ..
                } => {
                    Self::collect_var_types_inner(&then_block.statements, type_map);
                    if let Some(else_block) = else_block {
                        Self::collect_var_types_inner(&else_block.statements, type_map);
                    }
                }
                Statement::While { body, .. } | Statement::ForIn { body, .. } => {
                    Self::collect_var_types_inner(&body.statements, type_map);
                }
                Statement::Try {
                    try_block,
                    catch_block,
                    ..
                } => {
                    Self::collect_var_types_inner(&try_block.statements, type_map);
                    Self::collect_var_types_inner(&catch_block.statements, type_map);
                }
                _ => {}
            }
        }
    }

    /// Build a Vec<Type> indexed by slot number for a function,
    /// using the slot_names from Scope and the type_map built from AST nodes.
    fn build_local_types(scope: &Scope, type_map: &HashMap<String, Type>) -> Vec<Type> {
        let mut local_types = vec![Type::Any; scope.locals_count];
        for (slot, name) in scope.slot_names.iter().enumerate() {
            if let Some(ty) = type_map.get(name) {
                local_types[slot] = ty.clone();
            }
        }
        local_types
    }

    pub fn resolve(&mut self, program: Program) -> Result<ResolvedProgram, String> {
        // First pass: collect struct definitions
        let mut struct_defs = Vec::new();
        let mut impl_blocks = Vec::new();
        let mut func_defs = Vec::new();
        let mut main_stmts = Vec::new();

        for item in program.items {
            match item {
                Item::Import(_import) => {
                    // Imports are handled in module resolution phase
                }
                Item::StructDef(struct_def) => {
                    struct_defs.push(struct_def);
                }
                Item::ImplBlock(impl_block) => {
                    impl_blocks.push(impl_block);
                }
                Item::FnDef(fn_def) => {
                    func_defs.push(fn_def);
                }
                Item::Statement(stmt) => {
                    main_stmts.push(stmt);
                }
            }
        }

        // Register struct definitions
        for struct_def in &struct_defs {
            if self.structs.contains_key(&struct_def.name) {
                return Err(self.error(
                    &format!("struct '{}' already defined", struct_def.name),
                    struct_def.span,
                ));
            }
            let index = self.resolved_structs.len();
            let fields: Vec<String> = struct_def.fields.iter().map(|f| f.name.clone()).collect();
            self.structs.insert(
                struct_def.name.clone(),
                StructDefInfo {
                    index,
                    fields: fields.clone(),
                    methods: HashMap::new(),
                    method_return_types: HashMap::new(),
                },
            );
            self.resolved_structs.push(ResolvedStruct {
                name: struct_def.name.clone(),
                fields,
            });
        }

        // Register top-level functions
        for fn_def in &func_defs {
            let index = func_defs
                .iter()
                .position(|f| f.name == fn_def.name)
                .unwrap();
            // Check for builtin name collision
            if self.builtins.contains(&fn_def.name) {
                return Err(self.error(
                    &format!(
                        "cannot define function '{}': name is reserved for builtin",
                        fn_def.name
                    ),
                    fn_def.span,
                ));
            }
            if self.functions.contains_key(&fn_def.name) {
                return Err(self.error(
                    &format!("function '{}' already defined", fn_def.name),
                    fn_def.span,
                ));
            }
            self.functions.insert(fn_def.name.clone(), index);
        }

        // Register impl block methods as functions
        for impl_block in &impl_blocks {
            let is_builtin_type =
                impl_block.struct_name == "vec" || impl_block.struct_name == "map";

            if !is_builtin_type {
                // For struct impls, verify the struct exists
                if !self.structs.contains_key(&impl_block.struct_name) {
                    return Err(self.error(
                        &format!("impl for undefined struct '{}'", impl_block.struct_name),
                        impl_block.span,
                    ));
                }
            }

            for method in &impl_block.methods {
                let has_self = method.params.iter().any(|p| p.name == "self");

                // For associated functions on builtin types, use {type}_{func} naming
                // For methods, use {Type}::{method} naming
                let func_name = if is_builtin_type && !has_self {
                    format!("{}_{}", impl_block.struct_name, method.name)
                } else {
                    format!("{}::{}", impl_block.struct_name, method.name)
                };

                // func_index is the next available index in resolved_functions
                // which will contain func_defs first, then all methods
                let func_index = self.functions.len();

                if self.functions.contains_key(&func_name) {
                    return Err(self.error(
                        &format!(
                            "method '{}' already defined for struct '{}'",
                            method.name, impl_block.struct_name
                        ),
                        method.span,
                    ));
                }
                self.functions.insert(func_name.clone(), func_index);

                // Get return type struct name if method returns a struct
                let return_struct_name = method.return_type.as_ref().and_then(|rt| {
                    if let TypeAnnotation::Named(name) = rt {
                        // Check if this name is a known struct
                        if self.structs.contains_key(name) {
                            Some(name.clone())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });

                // Add method to struct's method table (only for non-builtin types)
                if let Some(struct_info) = self.structs.get_mut(&impl_block.struct_name) {
                    struct_info.methods.insert(method.name.clone(), func_index);
                    struct_info
                        .method_return_types
                        .insert(method.name.clone(), return_struct_name);
                }
            }
        }

        // Second pass: resolve function bodies
        let mut resolved_functions = Vec::new();
        for fn_def in func_defs {
            let resolved = self.resolve_function(fn_def)?;
            resolved_functions.push(resolved);
        }

        // Resolve impl block methods as functions
        for impl_block in impl_blocks {
            for method in impl_block.methods {
                let resolved = self.resolve_method(method, &impl_block.struct_name)?;
                resolved_functions.push(resolved);
            }
        }

        // Resolve main body
        let mut scope = Scope::new();
        let main_type_map = Self::collect_var_types(&main_stmts);
        let resolved_main = self.resolve_statements(main_stmts, &mut scope)?;
        let main_locals_count = scope.locals_count;
        let main_local_types = Self::build_local_types(&scope, &main_type_map);

        Ok(ResolvedProgram {
            functions: resolved_functions,
            main_body: resolved_main,
            structs: self.resolved_structs.clone(),
            main_locals_count,
            main_local_types,
        })
    }

    fn resolve_method(&self, method: FnDef, struct_name: &str) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();
        let has_self = method.params.iter().any(|p| p.name == "self");
        let is_builtin_type = struct_name == "vec" || struct_name == "map";
        let is_inline = method.attributes.iter().any(|a| a.name == "inline");

        let mut param_names: Vec<String> = Vec::new();

        if has_self {
            // Add 'self' as first parameter with struct type information
            param_names.push("self".to_string());
            scope.declare_with_type("self".to_string(), false, Some(struct_name.to_string()));
        }

        // Add other parameters
        for param in &method.params {
            if param.name != "self" {
                param_names.push(param.name.clone());
                let struct_name_for_param =
                    self.struct_name_from_type_annotation(&param.type_annotation);
                scope.declare_with_type(param.name.clone(), false, struct_name_for_param);
            }
        }

        let method_type_map = Self::collect_var_types(&method.body.statements);
        let body = self.resolve_statements(method.body.statements, &mut scope)?;

        // Function name: {Type}::{method} for methods, {type}_{func} for associated functions on builtin types
        let func_name = if is_builtin_type && !has_self {
            format!("{}_{}", struct_name, method.name)
        } else {
            format!("{}::{}", struct_name, method.name)
        };

        // Check for direct recursion in @inline methods
        if is_inline
            && let Some(&func_index) = self.functions.get(&func_name)
            && self.body_calls_function(&body, func_index)
        {
            return Err(self.error(
                &format!("@inline method '{}' cannot be recursive", func_name),
                method.span,
            ));
        }

        let local_types = Self::build_local_types(&scope, &method_type_map);

        Ok(ResolvedFunction {
            name: func_name,
            params: param_names,
            locals_count: scope.locals_count,
            body,
            local_types,
            is_inline,
        })
    }

    fn resolve_function(&self, fn_def: FnDef) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();
        let is_inline = fn_def.attributes.iter().any(|a| a.name == "inline");

        // Add parameters to scope
        let param_names: Vec<String> = fn_def.params.iter().map(|p| p.name.clone()).collect();
        for param in &fn_def.params {
            let struct_name = self.struct_name_from_type_annotation(&param.type_annotation);
            scope.declare_with_type(param.name.clone(), false, struct_name);
        }

        let fn_type_map = Self::collect_var_types(&fn_def.body.statements);
        let body = self.resolve_statements(fn_def.body.statements, &mut scope)?;
        let local_types = Self::build_local_types(&scope, &fn_type_map);

        // Check for direct recursion in @inline functions
        if is_inline
            && let Some(&func_index) = self.functions.get(&fn_def.name)
            && self.body_calls_function(&body, func_index)
        {
            return Err(self.error(
                &format!("@inline function '{}' cannot be recursive", fn_def.name),
                fn_def.span,
            ));
        }

        Ok(ResolvedFunction {
            name: fn_def.name,
            params: param_names,
            locals_count: scope.locals_count,
            body,
            local_types,
            is_inline,
        })
    }

    /// Extract struct name from a type annotation (for function parameters).
    fn struct_name_from_type_annotation(
        &self,
        type_annotation: &Option<crate::compiler::types::TypeAnnotation>,
    ) -> Option<String> {
        match type_annotation {
            Some(crate::compiler::types::TypeAnnotation::Named(type_name)) => {
                if self.structs.contains_key(type_name) {
                    Some(type_name.clone())
                } else {
                    None
                }
            }
            Some(crate::compiler::types::TypeAnnotation::Array(_)) => Some("Array".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Vec(_)) => Some("Vec".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Map(_, _)) => Some("Map".to_string()),
            Some(crate::compiler::types::TypeAnnotation::Generic { name, .. }) => {
                if self.structs.contains_key(name) {
                    Some(name.clone())
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Get struct name from a ResolvedExpr if it evaluates to a struct
    fn get_struct_name(&self, expr: &ResolvedExpr) -> Option<String> {
        match expr {
            ResolvedExpr::StructLiteral { struct_index, .. } => self
                .resolved_structs
                .get(*struct_index)
                .map(|s| s.name.clone()),
            ResolvedExpr::MethodCall {
                return_struct_name, ..
            } => return_struct_name.clone(),
            // Array literals map to Array<T> struct
            ResolvedExpr::Array { .. } => Some("Array".to_string()),
            _ => None,
        }
    }

    fn resolve_statements(
        &self,
        statements: Vec<Statement>,
        scope: &mut Scope,
    ) -> Result<Vec<ResolvedStatement>, String> {
        let mut resolved = Vec::new();

        for stmt in statements {
            resolved.push(self.resolve_statement(stmt, scope)?);
        }

        Ok(resolved)
    }

    fn resolve_statement(
        &self,
        stmt: Statement,
        scope: &mut Scope,
    ) -> Result<ResolvedStatement, String> {
        match stmt {
            Statement::Let {
                name,
                mutable,
                type_annotation,
                init,
                span: _,
                ..
            } => {
                let init = self.resolve_expr(init, scope)?;
                // First try to get struct name from type annotation
                let struct_name = match type_annotation {
                    Some(crate::compiler::types::TypeAnnotation::Named(type_name)) => {
                        if self.structs.contains_key(&type_name) {
                            Some(type_name)
                        } else {
                            self.get_struct_name(&init)
                        }
                    }
                    // array<T> maps to Array<T> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Array(_)) => {
                        Some("Array".to_string())
                    }
                    // vec<T> maps to Vec<T> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Vec(_)) => Some("Vec".to_string()),
                    // map<K, V> maps to Map<K, V> generic struct
                    Some(crate::compiler::types::TypeAnnotation::Map(_, _)) => {
                        Some("Map".to_string())
                    }
                    // Generic type annotation: Vec<int>, Map<string, int>, etc.
                    Some(crate::compiler::types::TypeAnnotation::Generic { name, .. }) => {
                        if self.structs.contains_key(&name) {
                            Some(name)
                        } else {
                            self.get_struct_name(&init)
                        }
                    }
                    _ => self.get_struct_name(&init),
                };
                let slot = scope.declare_with_type(name.clone(), mutable, struct_name);
                Ok(ResolvedStatement::Let { slot, init })
            }
            Statement::Assign { name, value, span } => {
                let (slot, mutable) = scope
                    .lookup(&name)
                    .ok_or_else(|| self.error(&format!("undefined variable '{}'", name), span))?;

                if !mutable {
                    return Err(self.error(
                        &format!("cannot assign to immutable variable '{}'", name),
                        span,
                    ));
                }

                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::Assign { slot, value })
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                let condition = self.resolve_expr(condition, scope)?;

                scope.enter_scope();
                let then_resolved = self.resolve_statements(then_block.statements, scope)?;
                scope.exit_scope();

                let else_resolved = if let Some(else_block) = else_block {
                    scope.enter_scope();
                    let resolved = self.resolve_statements(else_block.statements, scope)?;
                    scope.exit_scope();
                    Some(resolved)
                } else {
                    None
                };

                Ok(ResolvedStatement::If {
                    condition,
                    then_block: then_resolved,
                    else_block: else_resolved,
                })
            }
            Statement::While {
                condition, body, ..
            } => {
                let condition = self.resolve_expr(condition, scope)?;

                scope.enter_scope();
                let body_resolved = self.resolve_statements(body.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::While {
                    condition,
                    body: body_resolved,
                })
            }
            Statement::Return { value, .. } => {
                let value = if let Some(v) = value {
                    Some(self.resolve_expr(v, scope)?)
                } else {
                    None
                };
                Ok(ResolvedStatement::Return { value })
            }
            Statement::Expr { expr, .. } => {
                let expr = self.resolve_expr(expr, scope)?;
                Ok(ResolvedStatement::Expr { expr })
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                span,
                object_type,
            } => {
                let resolved_object = self.resolve_expr(object, scope)?;
                let resolved_index = self.resolve_expr(index, scope)?;
                let resolved_value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::IndexAssign {
                    object: resolved_object,
                    index: resolved_index,
                    value: resolved_value,
                    span,
                    object_type,
                })
            }
            Statement::FieldAssign {
                object,
                field,
                value,
                ..
            } => {
                let object = self.resolve_expr(object, scope)?;
                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::FieldAssign {
                    object,
                    field,
                    value,
                })
            }
            Statement::ForIn {
                var,
                iterable,
                body,
                ..
            } => {
                let iterable = self.resolve_expr(iterable, scope)?;

                scope.enter_scope();
                // Declare loop variable as mutable within the loop
                let slot = scope.declare(var, true);
                // Allocate 2 hidden slots for __idx and __arr used by codegen
                let _idx_slot = scope.declare("__for_idx".to_string(), true);
                let _arr_slot = scope.declare("__for_arr".to_string(), true);
                let body_resolved = self.resolve_statements(body.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::ForIn {
                    slot,
                    iterable,
                    body: body_resolved,
                })
            }
            Statement::Throw { value, .. } => {
                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::Throw { value })
            }
            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                ..
            } => {
                scope.enter_scope();
                let try_resolved = self.resolve_statements(try_block.statements, scope)?;
                scope.exit_scope();

                scope.enter_scope();
                let catch_slot = scope.declare(catch_var, false);
                let catch_resolved = self.resolve_statements(catch_block.statements, scope)?;
                scope.exit_scope();

                Ok(ResolvedStatement::Try {
                    try_block: try_resolved,
                    catch_slot,
                    catch_block: catch_resolved,
                })
            }
        }
    }

    fn resolve_expr(&self, expr: Expr, scope: &mut Scope) -> Result<ResolvedExpr, String> {
        match expr {
            Expr::Int { value, .. } => Ok(ResolvedExpr::Int(value)),
            Expr::Float { value, .. } => Ok(ResolvedExpr::Float(value)),
            Expr::Bool { value, .. } => Ok(ResolvedExpr::Bool(value)),
            Expr::Str { value, .. } => Ok(ResolvedExpr::Str(value)),
            Expr::Nil { .. } => Ok(ResolvedExpr::Nil),
            Expr::Ident { name, span, .. } => {
                let (slot, _) = scope
                    .lookup(&name)
                    .ok_or_else(|| self.error(&format!("undefined variable '{}'", name), span))?;
                Ok(ResolvedExpr::Local(slot))
            }
            Expr::Array { elements, .. } => {
                let resolved: Vec<_> = elements
                    .into_iter()
                    .map(|e| self.resolve_expr(e, scope))
                    .collect::<Result<_, _>>()?;
                Ok(ResolvedExpr::Array { elements: resolved })
            }
            Expr::Index {
                object,
                index,
                span,
                object_type,
                ..
            } => {
                let resolved_object = self.resolve_expr(*object, scope)?;
                let resolved_index = self.resolve_expr(*index, scope)?;
                Ok(ResolvedExpr::Index {
                    object: Box::new(resolved_object),
                    index: Box::new(resolved_index),
                    span,
                    object_type,
                })
            }
            Expr::Field { object, field, .. } => {
                let object = self.resolve_expr(*object, scope)?;
                Ok(ResolvedExpr::Field {
                    object: Box::new(object),
                    field,
                })
            }
            Expr::Unary { op, operand, .. } => {
                let operand = self.resolve_expr(*operand, scope)?;
                Ok(ResolvedExpr::Unary {
                    op,
                    operand: Box::new(operand),
                })
            }
            Expr::Binary {
                op, left, right, ..
            } => {
                let left = self.resolve_expr(*left, scope)?;
                let right = self.resolve_expr(*right, scope)?;
                Ok(ResolvedExpr::Binary {
                    op,
                    left: Box::new(left),
                    right: Box::new(right),
                })
            }
            Expr::Call {
                callee, args, span, ..
            } => {
                // Special handling for spawn - it takes a function name, not a value
                if callee == "spawn" {
                    if args.len() != 1 {
                        return Err(
                            self.error("spawn takes exactly 1 argument (function name)", span)
                        );
                    }

                    // Check if the argument is an identifier referring to a function
                    if let Expr::Ident {
                        name,
                        span: arg_span,
                        ..
                    } = &args[0]
                    {
                        if let Some(&func_index) = self.functions.get(name) {
                            return Ok(ResolvedExpr::SpawnFunc { func_index });
                        } else {
                            return Err(self.error(
                                &format!("spawn: '{}' is not a function", name),
                                *arg_span,
                            ));
                        }
                    } else {
                        return Err(self.error("spawn requires a function name", span));
                    }
                }

                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // Check if it's a builtin
                if self.builtins.contains(&callee) {
                    return Ok(ResolvedExpr::Builtin {
                        name: callee,
                        args: resolved_args,
                        span,
                    });
                }

                // Check if it's a user-defined function
                if let Some(&func_index) = self.functions.get(&callee) {
                    return Ok(ResolvedExpr::Call {
                        func_index,
                        args: resolved_args,
                    });
                }

                Err(self.error(&format!("undefined function '{}'", callee), span))
            }
            Expr::StructLiteral {
                name, fields, span, ..
            } => {
                // Look up struct definition
                let struct_info = self
                    .structs
                    .get(&name)
                    .ok_or_else(|| self.error(&format!("undefined struct '{}'", name), span))?;
                let struct_index = struct_info.index;
                let struct_fields = struct_info.fields.clone();

                // Create a map of provided field names to expressions
                let mut field_map: HashMap<String, Expr> = fields.into_iter().collect();

                // Resolve fields in declaration order
                let mut resolved_fields = Vec::new();
                for field_name in &struct_fields {
                    let expr = field_map.remove(field_name).ok_or_else(|| {
                        self.error(
                            &format!("missing field '{}' in struct '{}'", field_name, name),
                            span,
                        )
                    })?;
                    resolved_fields.push(self.resolve_expr(expr, scope)?);
                }

                // Check for extra fields
                if let Some((extra_field, _)) = field_map.into_iter().next() {
                    return Err(self.error(
                        &format!("unknown field '{}' in struct '{}'", extra_field, name),
                        span,
                    ));
                }

                Ok(ResolvedExpr::StructLiteral {
                    struct_index,
                    fields: resolved_fields,
                })
            }
            Expr::MethodCall {
                object,
                method,
                args,
                span,
                ..
            } => {
                // Get struct name from the object expression before resolving
                let struct_name = match &*object {
                    Expr::Ident { name, .. } => {
                        scope.lookup_with_type(name).and_then(|(_, _, sn)| sn)
                    }
                    Expr::StructLiteral { name, .. } => Some(name.clone()),
                    Expr::NewLiteral { type_name, .. } => Some(type_name.clone()),
                    _ => None,
                };

                let resolved_object = self.resolve_expr(*object, scope)?;
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // If struct_name wasn't found from the unresolved AST,
                // try to get it from the resolved object (e.g., chained method calls)
                let struct_name = struct_name.or_else(|| self.get_struct_name(&resolved_object));

                // Resolve method to function index (static dispatch)
                let (func_index, return_struct_name) = if let Some(sn) = &struct_name {
                    let struct_info = self
                        .structs
                        .get(sn)
                        .ok_or_else(|| self.error(&format!("undefined struct '{}'", sn), span))?;
                    let idx = *struct_info.methods.get(&method).ok_or_else(|| {
                        self.error(
                            &format!("undefined method '{}' for struct '{}'", method, sn),
                            span,
                        )
                    })?;
                    let ret_type = struct_info
                        .method_return_types
                        .get(&method)
                        .cloned()
                        .flatten();
                    (idx, ret_type)
                } else {
                    return Err(self.error(
                        &format!("cannot call method '{}' on non-struct value", method),
                        span,
                    ));
                };

                Ok(ResolvedExpr::MethodCall {
                    object: Box::new(resolved_object),
                    method,
                    func_index,
                    args: resolved_args,
                    return_struct_name,
                })
            }
            Expr::AssociatedFunctionCall {
                type_name,
                function,
                args,
                span,
                ..
            } => {
                // Resolve arguments
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // Look up the associated function
                // First check if it's a struct with that associated function
                if let Some(struct_info) = self.structs.get(&type_name)
                    && let Some(&func_index) = struct_info.methods.get(&function)
                {
                    let return_struct_name = struct_info
                        .method_return_types
                        .get(&function)
                        .cloned()
                        .flatten();
                    return Ok(ResolvedExpr::AssociatedFunctionCall {
                        func_index,
                        args: resolved_args,
                        return_struct_name,
                    });
                }

                // Check for builtin types (vec, map) - they use regular functions
                // Look for function name pattern: type_function (e.g., vec_new)
                let builtin_func_name = format!("{}_{}", type_name, function);
                if let Some(&func_index) = self.functions.get(&builtin_func_name) {
                    return Ok(ResolvedExpr::AssociatedFunctionCall {
                        func_index,
                        args: resolved_args,
                        return_struct_name: None,
                    });
                }

                Err(self.error(
                    &format!(
                        "no associated function '{}' found for type '{}'",
                        function, type_name
                    ),
                    span,
                ))
            }
            Expr::Asm(asm_block) => {
                // Resolve input variable names to slots
                let mut input_slots = Vec::new();
                for input_name in &asm_block.inputs {
                    let (slot, _) = scope.lookup(input_name).ok_or_else(|| {
                        self.error(
                            &format!("undefined variable '{}' in asm block", input_name),
                            asm_block.span,
                        )
                    })?;
                    input_slots.push(slot);
                }

                // Resolve asm instructions (just copy, no variable resolution needed)
                let body: Vec<ResolvedAsmInstruction> = asm_block
                    .body
                    .into_iter()
                    .map(|inst| match inst {
                        AsmInstruction::Emit { op_name, args, .. } => {
                            ResolvedAsmInstruction::Emit { op_name, args }
                        }
                        AsmInstruction::Safepoint { .. } => ResolvedAsmInstruction::Safepoint,
                        AsmInstruction::GcHint { size, .. } => {
                            ResolvedAsmInstruction::GcHint { size }
                        }
                    })
                    .collect();

                Ok(ResolvedExpr::AsmBlock {
                    input_slots,
                    output_type: asm_block.output_type,
                    body,
                })
            }
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                ..
            } => {
                // Resolve each element
                let resolved_elements: Vec<ResolvedNewLiteralElement> = elements
                    .into_iter()
                    .map(|elem| match elem {
                        crate::compiler::ast::NewLiteralElement::Value(e) => {
                            let resolved = self.resolve_expr(e, scope)?;
                            Ok(ResolvedNewLiteralElement::Value(resolved))
                        }
                        crate::compiler::ast::NewLiteralElement::KeyValue { key, value } => {
                            let resolved_key = self.resolve_expr(key, scope)?;
                            let resolved_value = self.resolve_expr(value, scope)?;
                            Ok(ResolvedNewLiteralElement::KeyValue {
                                key: resolved_key,
                                value: resolved_value,
                            })
                        }
                    })
                    .collect::<Result<_, String>>()?;

                Ok(ResolvedExpr::NewLiteral {
                    type_name,
                    type_args,
                    elements: resolved_elements,
                })
            }

            Expr::Block {
                statements, expr, ..
            } => {
                // Create a new scope for the block
                scope.enter_scope();

                // Resolve all statements in the block
                let resolved_stmts: Vec<ResolvedStatement> = statements
                    .into_iter()
                    .map(|stmt| self.resolve_statement(stmt, scope))
                    .collect::<Result<_, _>>()?;

                // Resolve the final expression
                let resolved_expr = self.resolve_expr(*expr, scope)?;

                scope.exit_scope();

                Ok(ResolvedExpr::Block {
                    statements: resolved_stmts,
                    expr: Box::new(resolved_expr),
                })
            }
        }
    }

    /// Check if a resolved function body contains a direct call to a specific function index.
    fn body_calls_function(&self, body: &[ResolvedStatement], target_index: usize) -> bool {
        for stmt in body {
            if self.stmt_calls_function(stmt, target_index) {
                return true;
            }
        }
        false
    }

    fn stmt_calls_function(&self, stmt: &ResolvedStatement, target_index: usize) -> bool {
        match stmt {
            ResolvedStatement::Let { init, .. } => self.expr_calls_function(init, target_index),
            ResolvedStatement::Assign { value, .. } => {
                self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(index, target_index)
                    || self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::FieldAssign { object, value, .. } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(value, target_index)
            }
            ResolvedStatement::If {
                condition,
                then_block,
                else_block,
            } => {
                self.expr_calls_function(condition, target_index)
                    || self.body_calls_function(then_block, target_index)
                    || else_block
                        .as_ref()
                        .is_some_and(|eb| self.body_calls_function(eb, target_index))
            }
            ResolvedStatement::While { condition, body } => {
                self.expr_calls_function(condition, target_index)
                    || self.body_calls_function(body, target_index)
            }
            ResolvedStatement::ForIn { iterable, body, .. } => {
                self.expr_calls_function(iterable, target_index)
                    || self.body_calls_function(body, target_index)
            }
            ResolvedStatement::Return { value } => value
                .as_ref()
                .is_some_and(|v| self.expr_calls_function(v, target_index)),
            ResolvedStatement::Throw { value } => self.expr_calls_function(value, target_index),
            ResolvedStatement::Try {
                try_block,
                catch_block,
                ..
            } => {
                self.body_calls_function(try_block, target_index)
                    || self.body_calls_function(catch_block, target_index)
            }
            ResolvedStatement::Expr { expr } => self.expr_calls_function(expr, target_index),
        }
    }

    fn expr_calls_function(&self, expr: &ResolvedExpr, target_index: usize) -> bool {
        match expr {
            ResolvedExpr::Call { func_index, args } => {
                *func_index == target_index
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::MethodCall {
                object,
                func_index,
                args,
                ..
            } => {
                *func_index == target_index
                    || self.expr_calls_function(object, target_index)
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::AssociatedFunctionCall {
                func_index, args, ..
            } => {
                *func_index == target_index
                    || args
                        .iter()
                        .any(|a| self.expr_calls_function(a, target_index))
            }
            ResolvedExpr::Array { elements } => elements
                .iter()
                .any(|e| self.expr_calls_function(e, target_index)),
            ResolvedExpr::Index { object, index, .. } => {
                self.expr_calls_function(object, target_index)
                    || self.expr_calls_function(index, target_index)
            }
            ResolvedExpr::Field { object, .. } => self.expr_calls_function(object, target_index),
            ResolvedExpr::Unary { operand, .. } => self.expr_calls_function(operand, target_index),
            ResolvedExpr::Binary { left, right, .. } => {
                self.expr_calls_function(left, target_index)
                    || self.expr_calls_function(right, target_index)
            }
            ResolvedExpr::Builtin { args, .. } => args
                .iter()
                .any(|a| self.expr_calls_function(a, target_index)),
            ResolvedExpr::StructLiteral { fields, .. } => fields
                .iter()
                .any(|f| self.expr_calls_function(f, target_index)),
            ResolvedExpr::Block {
                statements, expr, ..
            } => {
                self.body_calls_function(statements, target_index)
                    || self.expr_calls_function(expr, target_index)
            }
            _ => false,
        }
    }

    fn error(&self, message: &str, span: Span) -> String {
        format!(
            "error: {}\n  --> {}:{}:{}",
            message, self.filename, span.line, span.column
        )
    }
}

/// Information about a local variable: (slot, mutable, struct_name).
/// struct_name is Some if the variable holds a struct instance.
type LocalInfo = (usize, bool, Option<String>);

/// A scope for variable resolution.
struct Scope {
    locals: Vec<HashMap<String, LocalInfo>>,
    locals_count: usize,
    /// Maps slot number → variable name (accumulated across all scopes).
    slot_names: Vec<String>,
}

impl Scope {
    fn new() -> Self {
        Self {
            locals: vec![HashMap::new()],
            locals_count: 0,
            slot_names: Vec::new(),
        }
    }

    fn declare(&mut self, name: String, mutable: bool) -> usize {
        self.declare_with_type(name, mutable, None)
    }

    fn declare_with_type(
        &mut self,
        name: String,
        mutable: bool,
        struct_name: Option<String>,
    ) -> usize {
        let slot = self.locals_count;
        self.locals_count += 1;
        self.slot_names.push(name.clone());
        self.locals
            .last_mut()
            .unwrap()
            .insert(name, (slot, mutable, struct_name));
        slot
    }

    fn lookup(&self, name: &str) -> Option<(usize, bool)> {
        self.lookup_with_type(name)
            .map(|(slot, mutable, _)| (slot, mutable))
    }

    fn lookup_with_type(&self, name: &str) -> Option<LocalInfo> {
        for scope in self.locals.iter().rev() {
            if let Some(info) = scope.get(name) {
                return Some(info.clone());
            }
        }
        None
    }

    fn enter_scope(&mut self) {
        self.locals.push(HashMap::new());
    }

    fn exit_scope(&mut self) {
        self.locals.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;
    use crate::compiler::parser::Parser;

    fn resolve(source: &str) -> Result<ResolvedProgram, String> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mc", tokens);
        let program = parser.parse()?;
        let mut resolver = Resolver::new("test.mc");
        resolver.resolve(program)
    }

    #[test]
    fn test_simple_resolution() {
        let program = resolve("let x = 42; print_debug(x);").unwrap();
        assert_eq!(program.main_body.len(), 2);
    }

    #[test]
    fn test_undefined_variable() {
        let result = resolve("print_debug(x);");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("undefined variable"));
    }

    #[test]
    fn test_undefined_function() {
        let result = resolve("foo();");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("undefined function"));
    }

    #[test]
    fn test_immutable_assignment() {
        let result = resolve("let x = 1; x = 2;");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("cannot assign to immutable"));
    }

    #[test]
    fn test_mutable_assignment() {
        let result = resolve("var x = 1; x = 2;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_function_resolution() {
        let program = resolve("fun add(a, b) { return a + b; } let r = add(1, 2);").unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
    }
}
