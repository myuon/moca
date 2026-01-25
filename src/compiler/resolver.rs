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
    If {
        condition: ResolvedExpr,
        then_block: Vec<ResolvedStatement>,
        else_block: Option<Vec<ResolvedStatement>>,
    },
    While {
        condition: ResolvedExpr,
        body: Vec<ResolvedStatement>,
    },
    Return {
        value: Option<ResolvedExpr>,
    },
    Expr {
        expr: ResolvedExpr,
    },
}

#[derive(Debug, Clone)]
pub enum ResolvedExpr {
    Int(i64),
    Bool(bool),
    Local(usize),
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
            builtins: vec!["print".to_string()],
        }
    }

    pub fn resolve(&mut self, program: Program) -> Result<ResolvedProgram, String> {
        // First pass: collect all function names
        let mut func_defs = Vec::new();
        let mut main_stmts = Vec::new();

        for item in program.items {
            match item {
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
        }
    }

    fn resolve_expr(&self, expr: Expr, scope: &mut Scope) -> Result<ResolvedExpr, String> {
        match expr {
            Expr::Int { value, .. } => Ok(ResolvedExpr::Int(value)),
            Expr::Bool { value, .. } => Ok(ResolvedExpr::Bool(value)),
            Expr::Ident { name, span } => {
                let (slot, _) = scope.lookup(&name).ok_or_else(|| {
                    self.error(&format!("undefined variable '{}'", name), span)
                })?;
                Ok(ResolvedExpr::Local(slot))
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
        let result = resolve("let mut x = 1; x = 2;");
        assert!(result.is_ok());
    }

    #[test]
    fn test_function_resolution() {
        let program = resolve("fn add(a, b) { return a + b; } let r = add(1, 2);").unwrap();
        assert_eq!(program.functions.len(), 1);
        assert_eq!(program.functions[0].name, "add");
    }
}
