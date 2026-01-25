use crate::compiler::ast::*;
use crate::compiler::lexer::Span;
use std::collections::HashMap;

/// Resolved program with variable indices and function references.
#[derive(Debug, Clone)]
pub struct ResolvedProgram {
    pub functions: Vec<ResolvedFunction>,
    pub main_body: Vec<ResolvedStatement>,
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
}

/// The resolver performs name resolution and variable slot assignment.
pub struct Resolver<'a> {
    filename: &'a str,
    functions: HashMap<String, usize>,
    builtins: Vec<String>,
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
        }
    }

    pub fn resolve(&mut self, program: Program) -> Result<ResolvedProgram, String> {
        // First pass: collect all function names
        let mut func_defs = Vec::new();
        let mut main_stmts = Vec::new();

        for item in program.items {
            match item {
                Item::Import(_import) => {
                    // Imports are handled in module resolution phase
                    // For now, we just skip them during local resolution
                }
                Item::FnDef(fn_def) => {
                    let index = func_defs.len();
                    if self.functions.contains_key(&fn_def.name) {
                        return Err(self.error(
                            &format!("function '{}' already defined", fn_def.name),
                            fn_def.span,
                        ));
                    }
                    self.functions.insert(fn_def.name.clone(), index);
                    func_defs.push(fn_def);
                }
                Item::Statement(stmt) => {
                    main_stmts.push(stmt);
                }
            }
        }

        // Second pass: resolve function bodies
        let mut resolved_functions = Vec::new();
        for fn_def in func_defs {
            let resolved = self.resolve_function(fn_def)?;
            resolved_functions.push(resolved);
        }

        // Resolve main body
        let mut scope = Scope::new();
        let resolved_main = self.resolve_statements(main_stmts, &mut scope)?;

        Ok(ResolvedProgram {
            functions: resolved_functions,
            main_body: resolved_main,
        })
    }

    fn resolve_function(&self, fn_def: FnDef) -> Result<ResolvedFunction, String> {
        let mut scope = Scope::new();

        // Add parameters to scope
        for param in &fn_def.params {
            scope.declare(param.clone(), false);
        }

        let body = self.resolve_statements(fn_def.body.statements, &mut scope)?;

        Ok(ResolvedFunction {
            name: fn_def.name,
            params: fn_def.params,
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
                init,
                span: _,
            } => {
                let init = self.resolve_expr(init, scope)?;
                let slot = scope.declare(name.clone(), mutable);
                Ok(ResolvedStatement::Let { slot, init })
            }
            Statement::Assign { name, value, span } => {
                let (slot, mutable) = scope.lookup(&name).ok_or_else(|| {
                    self.error(&format!("undefined variable '{}'", name), span)
                })?;

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
                var, iterable, body, ..
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
                let (slot, _) = scope.lookup(&name).ok_or_else(|| {
                    self.error(&format!("undefined variable '{}'", name), span)
                })?;
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
                        return Err(self.error("spawn takes exactly 1 argument (function name)", span));
                    }

                    // Check if the argument is an identifier referring to a function
                    if let Expr::Ident { name, span: arg_span } = &args[0] {
                        if let Some(&func_index) = self.functions.get(name) {
                            return Ok(ResolvedExpr::SpawnFunc { func_index });
                        } else {
                            return Err(self.error(&format!("spawn: '{}' is not a function", name), *arg_span));
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
        self.locals.last_mut().unwrap().insert(name, (slot, mutable));
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
