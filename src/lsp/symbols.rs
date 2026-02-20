use crate::compiler::ast::{Block, Expr, FnDef, Item, Program, Statement};
use crate::compiler::lexer::Span;
use std::collections::HashMap;

/// Symbol information for go-to-definition and hover.
#[derive(Debug, Clone)]
pub struct SymbolInfo {
    pub name: String,
    pub kind: SymbolKind,
    pub def_span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SymbolKind {
    Function,
    Variable,
    Parameter,
}

/// Symbol table built from the AST.
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// All symbol definitions: name -> list of definitions (may have multiple scopes)
    pub definitions: HashMap<String, Vec<SymbolInfo>>,
    /// All references: span -> symbol name (for finding what's at a position)
    pub references: Vec<(Span, String)>,
}

impl SymbolTable {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a symbol table from a parsed program.
    pub fn from_program(program: &Program) -> Self {
        let mut table = Self::new();
        table.collect_program(program);
        table
    }

    fn collect_program(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::Import(_) => {}
                Item::FnDef(fn_def) => {
                    self.collect_fn_def(fn_def);
                }
                Item::StructDef(_struct_def) => {
                    // TODO: Collect struct definitions for LSP
                }
                Item::ImplBlock(_impl_block) => {
                    // TODO: Collect impl block methods for LSP
                }
                Item::InterfaceDef(_) => {}
                Item::Statement(stmt) => {
                    self.collect_statement(stmt);
                }
            }
        }
    }

    fn collect_fn_def(&mut self, fn_def: &FnDef) {
        // Add function definition
        let info = SymbolInfo {
            name: fn_def.name.clone(),
            kind: SymbolKind::Function,
            def_span: fn_def.span,
        };
        self.definitions
            .entry(fn_def.name.clone())
            .or_default()
            .push(info);

        // Add parameters as definitions
        for param in &fn_def.params {
            let info = SymbolInfo {
                name: param.name.clone(),
                kind: SymbolKind::Parameter,
                def_span: param.span,
            };
            self.definitions
                .entry(param.name.clone())
                .or_default()
                .push(info);
        }

        // Collect from function body
        self.collect_block(&fn_def.body);
    }

    fn collect_block(&mut self, block: &Block) {
        for stmt in &block.statements {
            self.collect_statement(stmt);
        }
    }

    fn collect_statement(&mut self, stmt: &Statement) {
        match stmt {
            Statement::Let {
                name, init, span, ..
            } => {
                // Add variable definition
                let info = SymbolInfo {
                    name: name.clone(),
                    kind: SymbolKind::Variable,
                    def_span: *span,
                };
                self.definitions.entry(name.clone()).or_default().push(info);

                self.collect_expr(init);
            }
            Statement::Assign { name, value, span } => {
                // Add reference to the variable being assigned
                self.references.push((*span, name.clone()));
                self.collect_expr(value);
            }
            Statement::IndexAssign {
                object,
                index,
                value,
                ..
            } => {
                self.collect_expr(object);
                self.collect_expr(index);
                self.collect_expr(value);
            }
            Statement::FieldAssign { object, value, .. } => {
                self.collect_expr(object);
                self.collect_expr(value);
            }
            Statement::If {
                condition,
                then_block,
                else_block,
                ..
            } => {
                self.collect_expr(condition);
                self.collect_block(then_block);
                if let Some(else_block) = else_block {
                    self.collect_block(else_block);
                }
            }
            Statement::While {
                condition, body, ..
            } => {
                self.collect_expr(condition);
                self.collect_block(body);
            }
            Statement::ForIn {
                var,
                iterable,
                body,
                span,
            } => {
                // Loop variable is a definition
                let info = SymbolInfo {
                    name: var.clone(),
                    kind: SymbolKind::Variable,
                    def_span: *span,
                };
                self.definitions.entry(var.clone()).or_default().push(info);

                self.collect_expr(iterable);
                self.collect_block(body);
            }
            Statement::Return { value, .. } => {
                if let Some(value) = value {
                    self.collect_expr(value);
                }
            }
            Statement::Throw { value, .. } => {
                self.collect_expr(value);
            }
            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                span,
            } => {
                self.collect_block(try_block);

                // Catch variable is a definition
                let info = SymbolInfo {
                    name: catch_var.clone(),
                    kind: SymbolKind::Variable,
                    def_span: *span,
                };
                self.definitions
                    .entry(catch_var.clone())
                    .or_default()
                    .push(info);

                self.collect_block(catch_block);
            }
            Statement::Const {
                name, init, span, ..
            } => {
                let info = SymbolInfo {
                    name: name.clone(),
                    kind: SymbolKind::Variable,
                    def_span: *span,
                };
                self.definitions.entry(name.clone()).or_default().push(info);
                self.collect_expr(init);
            }
            Statement::ForRange {
                var,
                start,
                end,
                body,
                span,
                ..
            } => {
                let info = SymbolInfo {
                    name: var.clone(),
                    kind: SymbolKind::Variable,
                    def_span: *span,
                };
                self.definitions.entry(var.clone()).or_default().push(info);

                self.collect_expr(start);
                self.collect_expr(end);
                self.collect_block(body);
            }
            Statement::Expr { expr, .. } => {
                self.collect_expr(expr);
            }
            Statement::MatchDyn {
                expr,
                arms,
                default_block,
                ..
            } => {
                self.collect_expr(expr);
                for arm in arms {
                    self.collect_block(&arm.body);
                }
                self.collect_block(default_block);
            }
        }
    }

    fn collect_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident { name, span, .. } => {
                // This is a reference to a symbol
                self.references.push((*span, name.clone()));
            }
            Expr::Array { elements, .. } => {
                for elem in elements {
                    self.collect_expr(elem);
                }
            }
            Expr::Index { object, index, .. } => {
                self.collect_expr(object);
                self.collect_expr(index);
            }
            Expr::Field { object, .. } => {
                self.collect_expr(object);
            }
            Expr::Unary { operand, .. } => {
                self.collect_expr(operand);
            }
            Expr::Binary { left, right, .. } => {
                self.collect_expr(left);
                self.collect_expr(right);
            }
            Expr::Call {
                callee, args, span, ..
            } => {
                // The callee is a function reference
                self.references.push((*span, callee.clone()));
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, expr) in fields {
                    self.collect_expr(expr);
                }
            }
            Expr::MethodCall { object, args, .. } => {
                self.collect_expr(object);
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::AssociatedFunctionCall {
                type_name,
                function,
                args,
                span,
                ..
            } => {
                // The type and function names are references
                self.references.push((*span, type_name.clone()));
                self.references.push((*span, function.clone()));
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::NewLiteral { elements, .. } => {
                for elem in elements {
                    match elem {
                        crate::compiler::ast::NewLiteralElement::Value(e) => {
                            self.collect_expr(e);
                        }
                        crate::compiler::ast::NewLiteralElement::KeyValue { key, value } => {
                            self.collect_expr(key);
                            self.collect_expr(value);
                        }
                    }
                }
            }
            Expr::Block {
                statements, expr, ..
            } => {
                for stmt in statements {
                    self.collect_statement(stmt);
                }
                self.collect_expr(expr);
            }
            Expr::Lambda { body, .. } => {
                self.collect_block(body);
            }
            Expr::CallExpr { callee, args, .. } => {
                self.collect_expr(callee);
                for arg in args {
                    self.collect_expr(arg);
                }
            }
            Expr::StringInterpolation { parts, .. } => {
                for part in parts {
                    if let crate::compiler::ast::StringInterpPart::Expr(e) = part {
                        self.collect_expr(e);
                    }
                }
            }
            Expr::AsDyn { expr, .. } => {
                self.collect_expr(expr);
            }
            // Literals and asm blocks have no symbol references to collect
            Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. }
            | Expr::Nil { .. }
            | Expr::Asm(_) => {}
        }
    }

    /// Find the symbol at a given position (1-based line, 1-based column).
    pub fn find_at_position(&self, line: u32, column: u32) -> Option<&str> {
        let line = line as usize;
        let column = column as usize;
        for (span, name) in &self.references {
            if span.line == line && column >= span.column && column < span.column + name.len() {
                return Some(name);
            }
        }
        None
    }

    /// Get the definition for a symbol name.
    pub fn get_definition(&self, name: &str) -> Option<&SymbolInfo> {
        self.definitions.get(name).and_then(|defs| defs.first())
    }
}
