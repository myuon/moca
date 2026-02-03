//! Desugar phase: transforms syntax sugar into core AST constructs.
//!
//! This phase runs after type checking and before monomorphisation.
//! Currently handles:
//! - NewLiteral (`new Vec<T> {...}`) → uninit + index assignments

use crate::compiler::ast::{
    AsmBlock, Block, Expr, FnDef, ImplBlock, Item, NewLiteralElement, Param, Program, Statement,
    StructDef, StructField,
};
use crate::compiler::lexer::Span;
use crate::compiler::types::TypeAnnotation;

/// Counter for generating unique variable names.
struct Desugar {
    counter: usize,
}

impl Desugar {
    fn new() -> Self {
        Self { counter: 0 }
    }

    /// Generate a unique variable name for desugared temporaries.
    fn fresh_var(&mut self) -> String {
        let name = format!("__new_literal_{}", self.counter);
        self.counter += 1;
        name
    }

    /// Desugar a program.
    fn desugar_program(&mut self, program: Program) -> Program {
        Program {
            items: program
                .items
                .into_iter()
                .map(|item| self.desugar_item(item))
                .collect(),
        }
    }

    /// Desugar an item.
    fn desugar_item(&mut self, item: Item) -> Item {
        match item {
            Item::Import(import) => Item::Import(import),
            Item::FnDef(fn_def) => Item::FnDef(self.desugar_fn_def(fn_def)),
            Item::StructDef(struct_def) => Item::StructDef(self.desugar_struct_def(struct_def)),
            Item::ImplBlock(impl_block) => Item::ImplBlock(self.desugar_impl_block(impl_block)),
            Item::Statement(stmt) => Item::Statement(self.desugar_statement(stmt)),
        }
    }

    /// Desugar a function definition.
    fn desugar_fn_def(&mut self, fn_def: FnDef) -> FnDef {
        FnDef {
            name: fn_def.name,
            type_params: fn_def.type_params,
            params: fn_def
                .params
                .into_iter()
                .map(|p| self.desugar_param(p))
                .collect(),
            return_type: fn_def.return_type,
            body: self.desugar_block(fn_def.body),
            span: fn_def.span,
        }
    }

    /// Desugar a parameter (no-op for now).
    fn desugar_param(&mut self, param: Param) -> Param {
        param
    }

    /// Desugar a struct definition (no-op).
    fn desugar_struct_def(&mut self, struct_def: StructDef) -> StructDef {
        StructDef {
            name: struct_def.name,
            type_params: struct_def.type_params,
            fields: struct_def
                .fields
                .into_iter()
                .map(|f| self.desugar_struct_field(f))
                .collect(),
            span: struct_def.span,
        }
    }

    /// Desugar a struct field (no-op).
    fn desugar_struct_field(&mut self, field: StructField) -> StructField {
        field
    }

    /// Desugar an impl block.
    fn desugar_impl_block(&mut self, impl_block: ImplBlock) -> ImplBlock {
        ImplBlock {
            type_params: impl_block.type_params,
            struct_name: impl_block.struct_name,
            struct_type_args: impl_block.struct_type_args,
            methods: impl_block
                .methods
                .into_iter()
                .map(|m| self.desugar_fn_def(m))
                .collect(),
            span: impl_block.span,
        }
    }

    /// Desugar a block.
    fn desugar_block(&mut self, block: Block) -> Block {
        Block {
            statements: block
                .statements
                .into_iter()
                .flat_map(|stmt| self.desugar_statement_to_stmts(stmt))
                .collect(),
            span: block.span,
        }
    }

    /// Desugar a statement, potentially producing multiple statements.
    fn desugar_statement_to_stmts(&mut self, stmt: Statement) -> Vec<Statement> {
        vec![self.desugar_statement(stmt)]
    }

    /// Desugar a statement.
    fn desugar_statement(&mut self, stmt: Statement) -> Statement {
        match stmt {
            Statement::Let {
                name,
                mutable,
                type_annotation,
                init,
                span,
            } => Statement::Let {
                name,
                mutable,
                type_annotation,
                init: self.desugar_expr(init),
                span,
            },
            Statement::Assign { name, value, span } => Statement::Assign {
                name,
                value: self.desugar_expr(value),
                span,
            },
            Statement::IndexAssign {
                object,
                index,
                value,
                span,
                object_type,
            } => Statement::IndexAssign {
                object: self.desugar_expr(object),
                index: self.desugar_expr(index),
                value: self.desugar_expr(value),
                span,
                object_type,
            },
            Statement::FieldAssign {
                object,
                field,
                value,
                span,
            } => Statement::FieldAssign {
                object: self.desugar_expr(object),
                field,
                value: self.desugar_expr(value),
                span,
            },
            Statement::If {
                condition,
                then_block,
                else_block,
                span,
            } => Statement::If {
                condition: self.desugar_expr(condition),
                then_block: self.desugar_block(then_block),
                else_block: else_block.map(|b| self.desugar_block(b)),
                span,
            },
            Statement::While {
                condition,
                body,
                span,
            } => Statement::While {
                condition: self.desugar_expr(condition),
                body: self.desugar_block(body),
                span,
            },
            Statement::ForIn {
                var,
                iterable,
                body,
                span,
            } => Statement::ForIn {
                var,
                iterable: self.desugar_expr(iterable),
                body: self.desugar_block(body),
                span,
            },
            Statement::Return { value, span } => Statement::Return {
                value: value.map(|e| self.desugar_expr(e)),
                span,
            },
            Statement::Throw { value, span } => Statement::Throw {
                value: self.desugar_expr(value),
                span,
            },
            Statement::Try {
                try_block,
                catch_var,
                catch_block,
                span,
            } => Statement::Try {
                try_block: self.desugar_block(try_block),
                catch_var,
                catch_block: self.desugar_block(catch_block),
                span,
            },
            Statement::Expr { expr, span } => Statement::Expr {
                expr: self.desugar_expr(expr),
                span,
            },
        }
    }

    /// Desugar an expression.
    fn desugar_expr(&mut self, expr: Expr) -> Expr {
        match expr {
            // Literals - no change
            Expr::Int { .. }
            | Expr::Float { .. }
            | Expr::Bool { .. }
            | Expr::Str { .. }
            | Expr::Nil { .. }
            | Expr::Ident { .. } => expr,

            // Array - desugar elements
            Expr::Array { elements, span } => Expr::Array {
                elements: elements.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
            },

            // Index - desugar object and index
            Expr::Index {
                object,
                index,
                span,
                object_type,
            } => Expr::Index {
                object: Box::new(self.desugar_expr(*object)),
                index: Box::new(self.desugar_expr(*index)),
                span,
                object_type,
            },

            // Field access - desugar object
            Expr::Field {
                object,
                field,
                span,
            } => Expr::Field {
                object: Box::new(self.desugar_expr(*object)),
                field,
                span,
            },

            // Unary - desugar operand
            Expr::Unary { op, operand, span } => Expr::Unary {
                op,
                operand: Box::new(self.desugar_expr(*operand)),
                span,
            },

            // Binary - desugar both sides
            Expr::Binary {
                op,
                left,
                right,
                span,
            } => Expr::Binary {
                op,
                left: Box::new(self.desugar_expr(*left)),
                right: Box::new(self.desugar_expr(*right)),
                span,
            },

            // Call - desugar arguments
            Expr::Call {
                callee,
                type_args,
                args,
                span,
            } => Expr::Call {
                callee,
                type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
            },

            // Struct literal - desugar field values
            Expr::StructLiteral {
                name,
                type_args,
                fields,
                span,
            } => Expr::StructLiteral {
                name,
                type_args,
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name, self.desugar_expr(value)))
                    .collect(),
                span,
            },

            // Method call - desugar object and arguments
            Expr::MethodCall {
                object,
                method,
                type_args,
                args,
                span,
            } => Expr::MethodCall {
                object: Box::new(self.desugar_expr(*object)),
                method,
                type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
            },

            // Associated function call - desugar arguments
            Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                fn_type_args,
                args,
                span,
            } => Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                fn_type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
            },

            // Asm - desugar (inputs are just variable names, no expressions)
            Expr::Asm(asm_block) => Expr::Asm(self.desugar_asm_block(asm_block)),

            // NewLiteral - this is the main desugar target!
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                span,
            } => self.desugar_new_literal(type_name, type_args, elements, span),

            // BlockExpr - desugar statements and the final expression
            Expr::BlockExpr {
                statements,
                expr,
                span,
            } => Expr::BlockExpr {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.desugar_statement(stmt))
                    .collect(),
                expr: Box::new(self.desugar_expr(*expr)),
                span,
            },
        }
    }

    /// Desugar an asm block (no change to the block itself).
    fn desugar_asm_block(&mut self, asm_block: AsmBlock) -> AsmBlock {
        asm_block
    }

    /// Desugar a NewLiteral expression.
    ///
    /// Transforms `new Vec<T> {e1, e2, ...}` into a block expression:
    /// ```text
    /// {
    ///     let __new_literal_N: Vec<T> = Vec<T>::uninit(count);
    ///     __new_literal_N[0] = e1;
    ///     __new_literal_N[1] = e2;
    ///     ...
    ///     __new_literal_N
    /// }
    /// ```
    ///
    /// And `new Map<K,V> {k1: v1, k2: v2, ...}` into:
    /// ```text
    /// {
    ///     let __new_literal_N: Map<K,V> = Map<K,V>::uninit();
    ///     __new_literal_N.put(k1, v1);
    ///     __new_literal_N.put(k2, v2);
    ///     ...
    ///     __new_literal_N
    /// }
    /// ```
    fn desugar_new_literal(
        &mut self,
        type_name: String,
        type_args: Vec<TypeAnnotation>,
        elements: Vec<NewLiteralElement>,
        span: Span,
    ) -> Expr {
        // First, desugar any nested expressions in the elements
        let elements: Vec<NewLiteralElement> = elements
            .into_iter()
            .map(|elem| match elem {
                NewLiteralElement::Value(e) => NewLiteralElement::Value(self.desugar_expr(e)),
                NewLiteralElement::KeyValue { key, value } => NewLiteralElement::KeyValue {
                    key: self.desugar_expr(key),
                    value: self.desugar_expr(value),
                },
            })
            .collect();

        // Determine if this is a Map (KeyValue elements) or Vec (Value elements)
        let is_map = elements
            .first()
            .is_some_and(|e| matches!(e, NewLiteralElement::KeyValue { .. }));

        if is_map {
            self.desugar_map_literal(type_name, type_args, elements, span)
        } else {
            self.desugar_vec_literal(type_name, type_args, elements, span)
        }
    }

    /// Desugar a Vec literal.
    fn desugar_vec_literal(
        &mut self,
        type_name: String,
        type_args: Vec<TypeAnnotation>,
        elements: Vec<NewLiteralElement>,
        span: Span,
    ) -> Expr {
        let var_name = self.fresh_var();
        let count = elements.len();

        // Build the type annotation: Vec<T> or vec<T>
        let type_annotation = if type_args.is_empty() {
            TypeAnnotation::Named(type_name.clone())
        } else {
            TypeAnnotation::Generic {
                name: type_name.clone(),
                type_args: type_args.clone(),
            }
        };

        // Create statements
        let mut statements = Vec::new();

        // let __new_literal_N: Vec<T> = Vec<T>::uninit(count);
        let init_expr = Expr::AssociatedFunctionCall {
            type_name: type_name.clone(),
            type_args: type_args.clone(),
            function: "uninit".to_string(),
            fn_type_args: vec![],
            args: vec![Expr::Int {
                value: count as i64,
                span,
            }],
            span,
        };

        statements.push(Statement::Let {
            name: var_name.clone(),
            mutable: false,
            type_annotation: Some(type_annotation),
            init: init_expr,
            span,
        });

        // __new_literal_N[i] = e_i;
        for (i, elem) in elements.into_iter().enumerate() {
            if let NewLiteralElement::Value(value) = elem {
                statements.push(Statement::IndexAssign {
                    object: Expr::Ident {
                        name: var_name.clone(),
                        span,
                    },
                    index: Expr::Int {
                        value: i as i64,
                        span,
                    },
                    value,
                    span,
                    object_type: None,
                });
            }
        }

        // Return BlockExpr with the final expression being the variable reference
        Expr::BlockExpr {
            statements,
            expr: Box::new(Expr::Ident {
                name: var_name,
                span,
            }),
            span,
        }
    }

    /// Desugar a Map literal.
    ///
    /// Transforms `new Map<K,V> {k1: v1, k2: v2, ...}` into:
    /// ```text
    /// {
    ///     let __new_literal_N: Map<K,V> = Map<K,V>::uninit();
    ///     __new_literal_N.put(k1, v1);
    ///     __new_literal_N.put(k2, v2);
    ///     ...
    ///     __new_literal_N
    /// }
    /// ```
    fn desugar_map_literal(
        &mut self,
        type_name: String,
        type_args: Vec<TypeAnnotation>,
        elements: Vec<NewLiteralElement>,
        span: Span,
    ) -> Expr {
        let var_name = self.fresh_var();

        // Build the type annotation: Map<K,V>
        let type_annotation = if type_args.is_empty() {
            TypeAnnotation::Named(type_name.clone())
        } else {
            TypeAnnotation::Generic {
                name: type_name.clone(),
                type_args: type_args.clone(),
            }
        };

        // Create statements
        let mut statements = Vec::new();

        // let __new_literal_N: Map<K,V> = Map<K,V>::uninit();
        let init_expr = Expr::AssociatedFunctionCall {
            type_name: type_name.clone(),
            type_args: type_args.clone(),
            function: "uninit".to_string(),
            fn_type_args: vec![],
            args: vec![],
            span,
        };

        statements.push(Statement::Let {
            name: var_name.clone(),
            mutable: false,
            type_annotation: Some(type_annotation),
            init: init_expr,
            span,
        });

        // __new_literal_N.put(k, v);
        for elem in elements.into_iter() {
            if let NewLiteralElement::KeyValue { key, value } = elem {
                statements.push(Statement::Expr {
                    expr: Expr::MethodCall {
                        object: Box::new(Expr::Ident {
                            name: var_name.clone(),
                            span,
                        }),
                        method: "put".to_string(),
                        type_args: vec![],
                        args: vec![key, value],
                        span,
                    },
                    span,
                });
            }
        }

        // Return BlockExpr with the final expression being the variable reference
        Expr::BlockExpr {
            statements,
            expr: Box::new(Expr::Ident {
                name: var_name,
                span,
            }),
            span,
        }
    }
}


/// Desugar a program, expanding syntax sugar into core constructs.
pub fn desugar_program(program: Program) -> Program {
    let mut desugar = Desugar::new();
    desugar.desugar_program(program)
}
