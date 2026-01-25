use crate::compiler::ast::*;
use crate::compiler::lexer::Span;
use std::collections::HashMap;

/// Resolved program with variable indices and function references.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub functions: Vec<ResolvedFunction>,
    pub main_body: Vec<ResolvedStatement>,
    pub structs: Vec<ResolvedStruct>,
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
    Object {
        fields: Vec<(String, ResolvedExpr)>,
    },
    Index {
        object: Box<ResolvedExpr>,
        index: Box<ResolvedExpr>,
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
    /// Method name is kept for runtime dispatch (type not known statically).
    MethodCall {
        object: Box<ResolvedExpr>,
        method: String,
        args: Vec<ResolvedExpr>,
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
                "len".to_string(),
                "push".to_string(),
                "pop".to_string(),
                "type_of".to_string(),
                "to_string".to_string(),
                "parse_int".to_string(),
                // Thread operations
                "spawn".to_string(),
                "channel".to_string(),
                "send".to_string(),
                "recv".to_string(),
                "join".to_string(),
            ],
            structs: HashMap::new(),
            resolved_structs: Vec::new(),
        }
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
            let struct_info = self.structs.get(&impl_block.struct_name).ok_or_else(|| {
                self.error(
                    &format!("impl for undefined struct '{}'", impl_block.struct_name),
                    impl_block.span,
                )
            })?;
            let _struct_index = struct_info.index;

            for method in &impl_block.methods {
                // Create a unique function name for the method: StructName::method_name
                let func_name = format!("{}::{}", impl_block.struct_name, method.name);
                let func_index = func_defs.len() + self.functions.len();

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

                // Add method to struct's method table
                let struct_info = self.structs.get_mut(&impl_block.struct_name).unwrap();
                struct_info.methods.insert(method.name.clone(), func_index);
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
        let resolved_main = self.resolve_statements(main_stmts, &mut scope)?;

        Ok(ResolvedProgram {
            functions: resolved_functions,
            main_body: resolved_main,
            structs: self.resolved_structs.clone(),
        })
    }

    fn resolve_method(&self, method: FnDef, struct_name: &str) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();

        // Add 'self' as first parameter
        let mut param_names: Vec<String> = vec!["self".to_string()];
        scope.declare("self".to_string(), false);

        // Add other parameters
        for param in &method.params {
            if param.name != "self" {
                param_names.push(param.name.clone());
                scope.declare(param.name.clone(), false);
            }
        }

        let body = self.resolve_statements(method.body.statements, &mut scope)?;

        Ok(ResolvedFunction {
            name: format!("{}::{}", struct_name, method.name),
            params: param_names,
            locals_count: scope.locals_count,
            body,
        })
    }

    fn resolve_function(&self, fn_def: FnDef) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();

        // Add parameters to scope
        let param_names: Vec<String> = fn_def.params.iter().map(|p| p.name.clone()).collect();
        for param_name in &param_names {
            scope.declare(param_name.clone(), false);
        }

        let body = self.resolve_statements(fn_def.body.statements, &mut scope)?;

        Ok(ResolvedFunction {
            name: fn_def.name,
            params: param_names,
            locals_count: scope.locals_count,
            body,
        })
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
                type_annotation: _,
                init,
                span: _,
            } => {
                let init = self.resolve_expr(init, scope)?;
                let slot = scope.declare(name.clone(), mutable);
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
                ..
            } => {
                let object = self.resolve_expr(object, scope)?;
                let index = self.resolve_expr(index, scope)?;
                let value = self.resolve_expr(value, scope)?;
                Ok(ResolvedStatement::IndexAssign {
                    object,
                    index,
                    value,
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
            Expr::Ident { name, span } => {
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
            Expr::Object { fields, .. } => {
                let resolved: Vec<_> = fields
                    .into_iter()
                    .map(|(name, expr)| {
                        let resolved_expr = self.resolve_expr(expr, scope)?;
                        Ok((name, resolved_expr))
                    })
                    .collect::<Result<_, String>>()?;
                Ok(ResolvedExpr::Object { fields: resolved })
            }
            Expr::Index { object, index, .. } => {
                let object = self.resolve_expr(*object, scope)?;
                let index = self.resolve_expr(*index, scope)?;
                Ok(ResolvedExpr::Index {
                    object: Box::new(object),
                    index: Box::new(index),
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
            Expr::Call { callee, args, span } => {
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
            Expr::StructLiteral { name, fields, span } => {
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
                ..
            } => {
                let resolved_object = self.resolve_expr(*object, scope)?;
                let resolved_args: Vec<_> = args
                    .into_iter()
                    .map(|a| self.resolve_expr(a, scope))
                    .collect::<Result<_, _>>()?;

                // Method name is kept for runtime dispatch
                Ok(ResolvedExpr::MethodCall {
                    object: Box::new(resolved_object),
                    method,
                    args: resolved_args,
                })
            }
        }
    }

    fn error(&self, message: &str, span: Span) -> String {
        format!(
            "error: {}\n  --> {}:{}:{}",
            message, self.filename, span.line, span.column
        )
    }
}

/// A scope for variable resolution.
struct Scope {
    locals: Vec<HashMap<String, (usize, bool)>>,
    locals_count: usize,
}

impl Scope {
    fn new() -> Self {
        Self {
            locals: vec![HashMap::new()],
            locals_count: 0,
        }
    }

    fn declare(&mut self, name: String, mutable: bool) -> usize {
        let slot = self.locals_count;
        self.locals_count += 1;
        self.locals
            .last_mut()
            .unwrap()
            .insert(name, (slot, mutable));
        slot
    }

    fn lookup(&self, name: &str) -> Option<(usize, bool)> {
        for scope in self.locals.iter().rev() {
            if let Some(&slot) = scope.get(name) {
                return Some(slot);
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
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mica", tokens);
        let program = parser.parse()?;
        let mut resolver = Resolver::new("test.mica");
        resolver.resolve(program)
    }

    #[test]
    fn test_simple_resolution() {
        let program = resolve("let x = 42; print(x);").unwrap();
        assert_eq!(program.main_body.len(), 2);
    }

    #[test]
    fn test_undefined_variable() {
        let result = resolve("print(x);");
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
