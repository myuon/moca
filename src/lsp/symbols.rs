use crate::compiler::ast::{
    Block, Expr, FnDef, ImplBlock, InterfaceDef, Item, Program, Statement, StructDef,
};
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
    Struct,
    Interface,
    Method,
    Field,
}

/// A document symbol with optional children (for textDocument/documentSymbol).
#[derive(Debug, Clone)]
pub struct DocSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub span: Span,
    pub children: Vec<DocSymbol>,
}

/// Symbol table built from the AST.
#[derive(Debug, Default)]
pub struct SymbolTable {
    /// All symbol definitions: name -> list of definitions (may have multiple scopes)
    pub definitions: HashMap<String, Vec<SymbolInfo>>,
    /// All references: span -> symbol name (for finding what's at a position)
    pub references: Vec<(Span, String)>,
    /// Top-level document symbols (for textDocument/documentSymbol)
    pub doc_symbols: Vec<DocSymbol>,
    /// impl blocks: struct_name -> list of (interface_name, methods)
    pub impl_blocks: Vec<ImplInfo>,
}

/// Information about an impl block.
#[derive(Debug, Clone)]
pub struct ImplInfo {
    pub struct_name: String,
    pub interface_name: Option<String>,
    pub methods: Vec<SymbolInfo>,
    pub span: Span,
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
                    self.doc_symbols.push(DocSymbol {
                        name: fn_def.name.clone(),
                        kind: SymbolKind::Function,
                        span: fn_def.span,
                        children: vec![],
                    });
                }
                Item::StructDef(struct_def) => {
                    self.collect_struct_def(struct_def);
                }
                Item::ImplBlock(impl_block) => {
                    self.collect_impl_block(impl_block);
                }
                Item::InterfaceDef(interface_def) => {
                    self.collect_interface_def(interface_def);
                }
                Item::Statement(stmt) => {
                    self.collect_statement(stmt);
                    // Add top-level let/const to doc_symbols
                    match stmt {
                        Statement::Let { name, span, .. } | Statement::Const { name, span, .. } => {
                            self.doc_symbols.push(DocSymbol {
                                name: name.clone(),
                                kind: SymbolKind::Variable,
                                span: *span,
                                children: vec![],
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    fn collect_struct_def(&mut self, struct_def: &StructDef) {
        let info = SymbolInfo {
            name: struct_def.name.clone(),
            kind: SymbolKind::Struct,
            def_span: struct_def.span,
        };
        self.definitions
            .entry(struct_def.name.clone())
            .or_default()
            .push(info);

        let mut children = Vec::new();
        for field in &struct_def.fields {
            let field_info = SymbolInfo {
                name: field.name.clone(),
                kind: SymbolKind::Field,
                def_span: field.span,
            };
            // Register as "StructName.field_name" for lookup
            let qualified = format!("{}.{}", struct_def.name, field.name);
            self.definitions
                .entry(qualified)
                .or_default()
                .push(field_info);

            children.push(DocSymbol {
                name: field.name.clone(),
                kind: SymbolKind::Field,
                span: field.span,
                children: vec![],
            });
        }

        self.doc_symbols.push(DocSymbol {
            name: struct_def.name.clone(),
            kind: SymbolKind::Struct,
            span: struct_def.span,
            children,
        });
    }

    fn collect_impl_block(&mut self, impl_block: &ImplBlock) {
        let mut method_infos = Vec::new();
        let mut children = Vec::new();

        for method in &impl_block.methods {
            let qualified = format!("{}.{}", impl_block.struct_name, method.name);
            let info = SymbolInfo {
                name: qualified.clone(),
                kind: SymbolKind::Method,
                def_span: method.span,
            };
            self.definitions
                .entry(qualified)
                .or_default()
                .push(info.clone());

            // Also register just the method name for simple lookup
            let simple_info = SymbolInfo {
                name: method.name.clone(),
                kind: SymbolKind::Method,
                def_span: method.span,
            };
            self.definitions
                .entry(method.name.clone())
                .or_default()
                .push(simple_info);

            method_infos.push(info);
            children.push(DocSymbol {
                name: method.name.clone(),
                kind: SymbolKind::Method,
                span: method.span,
                children: vec![],
            });

            // Collect references inside method bodies
            for param in &method.params {
                let param_info = SymbolInfo {
                    name: param.name.clone(),
                    kind: SymbolKind::Parameter,
                    def_span: param.span,
                };
                self.definitions
                    .entry(param.name.clone())
                    .or_default()
                    .push(param_info);
            }
            self.collect_block(&method.body);
        }

        self.impl_blocks.push(ImplInfo {
            struct_name: impl_block.struct_name.clone(),
            interface_name: impl_block.interface_name.clone(),
            methods: method_infos,
            span: impl_block.span,
        });

        let label = if let Some(ref iface) = impl_block.interface_name {
            format!("impl {} for {}", iface, impl_block.struct_name)
        } else {
            format!("impl {}", impl_block.struct_name)
        };

        self.doc_symbols.push(DocSymbol {
            name: label,
            kind: SymbolKind::Method, // closest kind for impl blocks
            span: impl_block.span,
            children,
        });
    }

    fn collect_interface_def(&mut self, interface_def: &InterfaceDef) {
        let info = SymbolInfo {
            name: interface_def.name.clone(),
            kind: SymbolKind::Interface,
            def_span: interface_def.span,
        };
        self.definitions
            .entry(interface_def.name.clone())
            .or_default()
            .push(info);

        let mut children = Vec::new();
        for method_sig in &interface_def.methods {
            let qualified = format!("{}.{}", interface_def.name, method_sig.name);
            let method_info = SymbolInfo {
                name: qualified.clone(),
                kind: SymbolKind::Method,
                def_span: method_sig.span,
            };
            self.definitions
                .entry(qualified)
                .or_default()
                .push(method_info);

            children.push(DocSymbol {
                name: method_sig.name.clone(),
                kind: SymbolKind::Method,
                span: method_sig.span,
                children: vec![],
            });
        }

        self.doc_symbols.push(DocSymbol {
            name: interface_def.name.clone(),
            kind: SymbolKind::Interface,
            span: interface_def.span,
            children,
        });
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

    /// Find all references to a symbol name (both definitions and usages).
    pub fn find_references(&self, name: &str) -> Vec<Span> {
        let mut spans = Vec::new();

        // Add definition spans
        if let Some(defs) = self.definitions.get(name) {
            for def in defs {
                spans.push(def.def_span);
            }
        }

        // Add reference spans
        for (span, ref_name) in &self.references {
            if ref_name == name {
                spans.push(*span);
            }
        }

        spans
    }

    /// Find all implementations of a given interface name.
    pub fn find_implementations(&self, interface_name: &str) -> Vec<&ImplInfo> {
        self.impl_blocks
            .iter()
            .filter(|info| info.interface_name.as_deref() == Some(interface_name))
            .collect()
    }
}
