//! Desugar phase: transforms syntax sugar into core AST constructs.
//!
//! This phase runs after type checking and before monomorphisation.
//! Currently handles:
//! - NewLiteral (`new Vec<T> {...}`) → uninit + index assignments
//! - Index (`vec[i]`) → `vec.get(i)` for Vec/Map types
//! - IndexAssign (`vec[i] = v`) → `vec.set(i, v)` for Vec/Map types
//! - ForRange (`for i in start..end { body }`) → let + while loop

use crate::compiler::ast::{
    AsmBlock, BinaryOp, Block, Expr, FnDef, ImplBlock, Item, NewLiteralElement, Param, Program,
    Statement, StructDef, StructField,
};
use crate::compiler::lexer::Span;
use crate::compiler::types::{Type, TypeAnnotation};

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

    /// Check if the type should have its index operations desugared to method calls.
    /// Returns true for Vec<T> and Map<K,V>, false for Array<T> and other types.
    /// Array<T> is handled directly in codegen (not desugared) to support chained access.
    fn should_desugar_index(&self, obj_type: &Type) -> bool {
        match obj_type {
            // Map<K,V> generic structs should be desugared to get/set method calls
            Type::GenericStruct { name, .. } if name == "Map" => true,
            // Vec<T> uses ptr-based layout and is handled directly in codegen
            // with HeapLoad2/HeapStore2 for better performance
            Type::GenericStruct { name, .. } if name == "Vec" => false,
            // Legacy Vector type - handled directly in codegen (ptr-based layout)
            Type::Vector(_) => false,
            // Array, String, and other types should NOT be desugared
            Type::Array(_) | Type::String => false,
            // Struct types should NOT be desugared
            Type::Struct { .. } => false,
            // Default: don't desugar
            _ => false,
        }
    }

    /// Generate a unique variable name for for-range end bound.
    fn fresh_for_end_var(&mut self) -> String {
        let name = format!("__for_end_{}", self.counter);
        self.counter += 1;
        name
    }

    /// Desugar a program.
    fn desugar_program(&mut self, program: Program) -> Program {
        Program {
            items: program
                .items
                .into_iter()
                .flat_map(|item| self.desugar_item_to_items(item))
                .collect(),
        }
    }

    /// Desugar an item, potentially producing multiple items (e.g. ForRange → multiple statements).
    fn desugar_item_to_items(&mut self, item: Item) -> Vec<Item> {
        match item {
            Item::Import(import) => vec![Item::Import(import)],
            Item::FnDef(fn_def) => vec![Item::FnDef(self.desugar_fn_def(fn_def))],
            Item::StructDef(struct_def) => {
                vec![Item::StructDef(self.desugar_struct_def(struct_def))]
            }
            Item::ImplBlock(impl_block) => {
                vec![Item::ImplBlock(self.desugar_impl_block(impl_block))]
            }
            Item::Statement(stmt) => self
                .desugar_statement_to_stmts(stmt)
                .into_iter()
                .map(Item::Statement)
                .collect(),
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
            attributes: fn_def.attributes,
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
        match stmt {
            Statement::ForRange {
                var,
                start,
                end,
                inclusive,
                body,
                span,
            } => self.desugar_for_range(var, start, end, inclusive, body, span),
            _ => vec![self.desugar_statement(stmt)],
        }
    }

    /// Desugar ForRange into: let __for_end = end; let var = start; while var < __for_end { body; var = var + 1; }
    fn desugar_for_range(
        &mut self,
        var: String,
        start: Expr,
        end: Expr,
        inclusive: bool,
        body: Block,
        span: Span,
    ) -> Vec<Statement> {
        let end_var = self.fresh_for_end_var();
        let desugared_start = self.desugar_expr(start);
        let desugared_end = self.desugar_expr(end);
        let desugared_body = self.desugar_block(body);

        // let __for_end_N = end;
        let let_end = Statement::Let {
            name: end_var.clone(),
            type_annotation: None,
            init: desugared_end,
            span,
            inferred_type: Some(Type::Int),
        };

        // let var = start;
        let let_var = Statement::Let {
            name: var.clone(),
            type_annotation: None,
            init: desugared_start,
            span,
            inferred_type: Some(Type::Int),
        };

        // var < __for_end_N  (or var <= for inclusive)
        let cmp_op = if inclusive {
            BinaryOp::Le
        } else {
            BinaryOp::Lt
        };
        let condition = Expr::Binary {
            op: cmp_op,
            left: Box::new(Expr::Ident {
                name: var.clone(),
                span,
                inferred_type: Some(Type::Int),
            }),
            right: Box::new(Expr::Ident {
                name: end_var,
                span,
                inferred_type: Some(Type::Int),
            }),
            span,
            inferred_type: Some(Type::Bool),
        };

        // var = var + 1;
        let increment = Statement::Assign {
            name: var.clone(),
            value: Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(Expr::Ident {
                    name: var,
                    span,
                    inferred_type: Some(Type::Int),
                }),
                right: Box::new(Expr::Int {
                    value: 1,
                    span,
                    inferred_type: Some(Type::Int),
                }),
                span,
                inferred_type: Some(Type::Int),
            },
            span,
        };

        // while condition { body; var = var + 1; }
        let mut while_body_stmts = desugared_body.statements;
        while_body_stmts.push(increment);
        let while_stmt = Statement::While {
            condition,
            body: Block {
                statements: while_body_stmts,
                span,
            },
            span,
        };

        vec![let_end, let_var, while_stmt]
    }

    /// Desugar a statement.
    fn desugar_statement(&mut self, stmt: Statement) -> Statement {
        match stmt {
            Statement::Let {
                name,
                type_annotation,
                init,
                span,
                inferred_type,
            } => Statement::Let {
                name,
                type_annotation,
                init: self.desugar_expr(init),
                span,
                inferred_type,
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
            } => {
                let desugared_object = self.desugar_expr(object);
                let desugared_index = self.desugar_expr(index);
                let desugared_value = self.desugar_expr(value);

                // Check if this is a Vec or Map type that should be desugared
                if let Some(ref obj_type) = object_type
                    && self.should_desugar_index(obj_type)
                {
                    // Transform to method call: object.set(index, value)
                    return Statement::Expr {
                        expr: Expr::MethodCall {
                            object: Box::new(desugared_object),
                            method: "set".to_string(),
                            type_args: vec![],
                            args: vec![desugared_index, desugared_value],
                            span,
                            object_type: None,
                            inferred_type: None,
                        },
                        span,
                    };
                }

                // Keep as IndexAssign for Array and other types
                Statement::IndexAssign {
                    object: desugared_object,
                    index: desugared_index,
                    value: desugared_value,
                    span,
                    object_type,
                }
            }
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
            Statement::ForRange { .. } => {
                unreachable!("ForRange should be handled in desugar_statement_to_stmts")
            }
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
            Statement::Const { name, init, span } => Statement::Const { name, init, span },
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
            Expr::Array {
                elements,
                span,
                inferred_type,
            } => Expr::Array {
                elements: elements.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
                inferred_type,
            },

            // Index - desugar to method call for Vec/Map types
            Expr::Index {
                object,
                index,
                span,
                object_type,
                inferred_type,
            } => {
                let desugared_object = self.desugar_expr(*object);
                let desugared_index = self.desugar_expr(*index);

                // Check if this is a Vec or Map type that should be desugared
                if let Some(ref obj_type) = object_type
                    && self.should_desugar_index(obj_type)
                {
                    // Transform to method call: object.get(index)
                    return Expr::MethodCall {
                        object: Box::new(desugared_object),
                        method: "get".to_string(),
                        type_args: vec![],
                        args: vec![desugared_index],
                        span,
                        object_type: None,
                        inferred_type,
                    };
                }

                // Keep as Index for Array and other types
                Expr::Index {
                    object: Box::new(desugared_object),
                    index: Box::new(desugared_index),
                    span,
                    object_type,
                    inferred_type,
                }
            }

            // Field access - desugar object
            Expr::Field {
                object,
                field,
                span,
                inferred_type,
            } => Expr::Field {
                object: Box::new(self.desugar_expr(*object)),
                field,
                span,
                inferred_type,
            },

            // Unary - desugar operand
            Expr::Unary {
                op,
                operand,
                span,
                inferred_type,
            } => Expr::Unary {
                op,
                operand: Box::new(self.desugar_expr(*operand)),
                span,
                inferred_type,
            },

            // Binary - desugar both sides
            Expr::Binary {
                op,
                left,
                right,
                span,
                inferred_type,
            } => Expr::Binary {
                op,
                left: Box::new(self.desugar_expr(*left)),
                right: Box::new(self.desugar_expr(*right)),
                span,
                inferred_type,
            },

            // Call - desugar arguments
            Expr::Call {
                callee,
                type_args,
                args,
                span,
                inferred_type,
            } => Expr::Call {
                callee,
                type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
                inferred_type,
            },

            // Struct literal - desugar field values
            Expr::StructLiteral {
                name,
                type_args,
                fields,
                span,
                inferred_type,
            } => Expr::StructLiteral {
                name,
                type_args,
                fields: fields
                    .into_iter()
                    .map(|(name, value)| (name, self.desugar_expr(value)))
                    .collect(),
                span,
                inferred_type,
            },

            // Method call - desugar object and arguments
            Expr::MethodCall {
                object,
                method,
                type_args,
                args,
                span,
                object_type,
                inferred_type,
            } => Expr::MethodCall {
                object: Box::new(self.desugar_expr(*object)),
                method,
                type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
                object_type,
                inferred_type,
            },

            // Associated function call - desugar arguments
            Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                fn_type_args,
                args,
                span,
                inferred_type,
            } => Expr::AssociatedFunctionCall {
                type_name,
                type_args,
                function,
                fn_type_args,
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
                inferred_type,
            },

            // Asm - desugar (inputs are just variable names, no expressions)
            Expr::Asm(asm_block) => Expr::Asm(self.desugar_asm_block(asm_block)),

            // NewLiteral - this is the main desugar target!
            Expr::NewLiteral {
                type_name,
                type_args,
                elements,
                span,
                ..
            } => self.desugar_new_literal(type_name, type_args, elements, span),

            // Block - desugar statements and the final expression
            Expr::Block {
                statements,
                expr,
                span,
                inferred_type,
            } => Expr::Block {
                statements: statements
                    .into_iter()
                    .map(|stmt| self.desugar_statement(stmt))
                    .collect(),
                expr: Box::new(self.desugar_expr(*expr)),
                span,
                inferred_type,
            },

            // Lambda - desugar body
            Expr::Lambda {
                params,
                return_type,
                body,
                span,
                inferred_type,
            } => Expr::Lambda {
                params,
                return_type,
                body: self.desugar_block(body),
                span,
                inferred_type,
            },

            // CallExpr - desugar callee and arguments
            Expr::CallExpr {
                callee,
                args,
                span,
                inferred_type,
            } => Expr::CallExpr {
                callee: Box::new(self.desugar_expr(*callee)),
                args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                span,
                inferred_type,
            },

            // StringInterpolation - desugar to single-allocation concat block for 3+ parts,
            // or binary Add for 2 parts, or single expr/literal for 0-1 parts.
            Expr::StringInterpolation { parts, span, .. } => {
                use crate::compiler::ast::StringInterpPart;
                use crate::compiler::types::Type;

                let exprs: Vec<Expr> = parts
                    .into_iter()
                    .map(|part| match part {
                        StringInterpPart::Literal(s) => Expr::Str {
                            value: s,
                            span,
                            inferred_type: Some(Type::String),
                        },
                        StringInterpPart::Expr(expr) => {
                            let expr = self.desugar_expr(*expr);
                            // If the expression is already a string, no need to wrap in to_string
                            if expr.inferred_type() == Some(&Type::String) {
                                expr
                            } else {
                                Expr::Call {
                                    callee: "to_string".to_string(),
                                    type_args: vec![],
                                    args: vec![expr],
                                    span,
                                    inferred_type: Some(Type::String),
                                }
                            }
                        }
                    })
                    // Filter out empty string literals
                    .filter(|e| !matches!(e, Expr::Str { value, .. } if value.is_empty()))
                    .collect();

                if exprs.is_empty() {
                    return Expr::Str {
                        value: String::new(),
                        span,
                        inferred_type: Some(Type::String),
                    };
                }

                if exprs.len() == 1 {
                    return exprs.into_iter().next().unwrap();
                }

                if exprs.len() == 2 {
                    // For 2 parts, use binary Add (existing string_concat inline)
                    let mut result = exprs.into_iter();
                    let first = result.next().unwrap();
                    return result.fold(first, |acc, expr| Expr::Binary {
                        op: BinaryOp::Add,
                        left: Box::new(acc),
                        right: Box::new(expr),
                        span,
                        inferred_type: Some(Type::String),
                    });
                }

                // 3+ parts: single-allocation concat via block expression
                self.desugar_string_interp_concat(exprs, span)
            }
        }
    }

    /// Desugar an asm block (no change to the block itself).
    fn desugar_asm_block(&mut self, asm_block: AsmBlock) -> AsmBlock {
        asm_block
    }

    /// Desugar string interpolation with 3+ parts into a single-allocation concat.
    ///
    /// Generates a block expression equivalent to:
    /// ```text
    /// {
    ///     let __si_0 = part0;
    ///     let __si_1 = part1;
    ///     ...
    ///     let __si_total = len(__si_0) + len(__si_1) + ...;
    ///     let __si_data = __alloc_heap(__si_total);
    ///     let __si_off = 0;
    ///     // For each part: copy its character data
    ///     let __si_p0 = __heap_load(__si_0, 0);   // data pointer
    ///     let __si_l0 = __heap_load(__si_0, 1);   // length
    ///     let __si_j = 0;
    ///     while __si_j < __si_l0 {
    ///         __heap_store(__si_data, __si_off, __heap_load(__si_p0, __si_j));
    ///         __si_off = __si_off + 1;
    ///         __si_j = __si_j + 1;
    ///     }
    ///     ... (repeat for each part)
    ///     __alloc_string(__si_data, __si_total)
    /// }
    /// ```
    fn desugar_string_interp_concat(&mut self, exprs: Vec<Expr>, span: Span) -> Expr {
        use crate::compiler::types::Type;

        let n = exprs.len();
        let base = self.counter;
        self.counter += 1;

        // Helper to make an identifier expression
        let ident = |name: &str| -> Expr {
            Expr::Ident {
                name: name.to_string(),
                span,
                inferred_type: None,
            }
        };
        let int_lit = |v: i64| -> Expr {
            Expr::Int {
                value: v,
                span,
                inferred_type: None,
            }
        };
        let call = |callee: &str, args: Vec<Expr>| -> Expr {
            Expr::Call {
                callee: callee.to_string(),
                type_args: vec![],
                args,
                span,
                inferred_type: None,
            }
        };
        let add = |left: Expr, right: Expr| -> Expr {
            Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(left),
                right: Box::new(right),
                span,
                inferred_type: None,
            }
        };
        let lt = |left: Expr, right: Expr| -> Expr {
            Expr::Binary {
                op: BinaryOp::Lt,
                left: Box::new(left),
                right: Box::new(right),
                span,
                inferred_type: None,
            }
        };
        let let_stmt = |name: &str, init: Expr| -> Statement {
            Statement::Let {
                name: name.to_string(),
                type_annotation: None,
                init,
                span,
                inferred_type: None,
            }
        };
        let assign_stmt = |name: &str, value: Expr| -> Statement {
            Statement::Assign {
                name: name.to_string(),
                value,
                span,
            }
        };
        let expr_stmt = |expr: Expr| -> Statement { Statement::Expr { expr, span } };

        let mut statements = Vec::new();

        // Part variable names: __si_{base}_0, __si_{base}_1, ...
        let part_names: Vec<String> = (0..n).map(|i| format!("__si_{}_{}", base, i)).collect();
        let total_name = format!("__si_{}_total", base);
        let data_name = format!("__si_{}_data", base);
        let off_name = format!("__si_{}_off", base);
        let j_name = format!("__si_{}_j", base);

        // Step 1: let __si_N_i = part_i; for each part
        for (i, expr) in exprs.into_iter().enumerate() {
            statements.push(let_stmt(&part_names[i], expr));
        }

        // Step 2: let __si_N_total = len(__si_N_0) + len(__si_N_1) + ...;
        let len_sum = part_names
            .iter()
            .map(|name| call("len", vec![ident(name)]))
            .reduce(&add)
            .unwrap();
        statements.push(let_stmt(&total_name, len_sum));

        // Step 3: let __si_N_data = __alloc_heap(__si_N_total);
        statements.push(let_stmt(
            &data_name,
            call("__alloc_heap", vec![ident(&total_name)]),
        ));

        // Step 4: let __si_N_off = 0;
        statements.push(let_stmt(&off_name, int_lit(0)));

        // Step 5: For each part, generate copy loop
        for (i, part_name) in part_names.iter().enumerate() {
            let ptr_name = format!("__si_{}_p{}", base, i);
            let len_name = format!("__si_{}_l{}", base, i);

            // let __si_N_pI = __heap_load(__si_N_I, 0);
            statements.push(let_stmt(
                &ptr_name,
                call("__heap_load", vec![ident(part_name), int_lit(0)]),
            ));

            // let __si_N_lI = __heap_load(__si_N_I, 1);
            statements.push(let_stmt(
                &len_name,
                call("__heap_load", vec![ident(part_name), int_lit(1)]),
            ));

            // __si_N_j = 0;  (reuse j variable; first time is let, subsequent are assign)
            if i == 0 {
                statements.push(let_stmt(&j_name, int_lit(0)));
            } else {
                statements.push(assign_stmt(&j_name, int_lit(0)));
            }

            // while __si_N_j < __si_N_lI { ... }
            let while_body = vec![
                // __heap_store(__si_N_data, __si_N_off, __heap_load(__si_N_pI, __si_N_j));
                expr_stmt(call(
                    "__heap_store",
                    vec![
                        ident(&data_name),
                        ident(&off_name),
                        call("__heap_load", vec![ident(&ptr_name), ident(&j_name)]),
                    ],
                )),
                // __si_N_off = __si_N_off + 1;
                assign_stmt(&off_name, add(ident(&off_name), int_lit(1))),
                // __si_N_j = __si_N_j + 1;
                assign_stmt(&j_name, add(ident(&j_name), int_lit(1))),
            ];

            statements.push(Statement::While {
                condition: lt(ident(&j_name), ident(&len_name)),
                body: Block {
                    statements: while_body,
                    span,
                },
                span,
            });
        }

        // Final expression: __alloc_string(__si_N_data, __si_N_total)
        Expr::Block {
            statements,
            expr: Box::new(call(
                "__alloc_string",
                vec![ident(&data_name), ident(&total_name)],
            )),
            span,
            inferred_type: Some(Type::String),
        }
    }

    /// Desugar a NewLiteral expression.
    ///
    /// Transforms `new Vec<T> {e1, e2, ...}` into a block expression:
    /// ```text
    /// {
    ///     let __new_literal_N: Vec<T> = Vec<T>::uninit(count);
    ///     __new_literal_N.set(0, e1);
    ///     __new_literal_N.set(1, e2);
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
                inferred_type: None,
            }],
            span,
            inferred_type: None,
        };

        statements.push(Statement::Let {
            name: var_name.clone(),
            type_annotation: Some(type_annotation),
            init: init_expr,
            span,
            inferred_type: None,
        });

        // __new_literal_N.set(i, e_i);
        for (i, elem) in elements.into_iter().enumerate() {
            if let NewLiteralElement::Value(value) = elem {
                statements.push(Statement::Expr {
                    expr: Expr::MethodCall {
                        object: Box::new(Expr::Ident {
                            name: var_name.clone(),
                            span,
                            inferred_type: None,
                        }),
                        method: "set".to_string(),
                        type_args: vec![],
                        args: vec![
                            Expr::Int {
                                value: i as i64,
                                span,
                                inferred_type: None,
                            },
                            value,
                        ],
                        span,
                        object_type: None,
                        inferred_type: None,
                    },
                    span,
                });
            }
        }

        // Return Block with the final expression being the variable reference
        Expr::Block {
            statements,
            expr: Box::new(Expr::Ident {
                name: var_name,
                span,
                inferred_type: None,
            }),
            span,
            inferred_type: None,
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
            inferred_type: None,
        };

        statements.push(Statement::Let {
            name: var_name.clone(),
            type_annotation: Some(type_annotation),
            init: init_expr,
            span,
            inferred_type: None,
        });

        // __new_literal_N.put(k, v);
        for elem in elements.into_iter() {
            if let NewLiteralElement::KeyValue { key, value } = elem {
                statements.push(Statement::Expr {
                    expr: Expr::MethodCall {
                        object: Box::new(Expr::Ident {
                            name: var_name.clone(),
                            span,
                            inferred_type: None,
                        }),
                        method: "put".to_string(),
                        type_args: vec![],
                        args: vec![key, value],
                        span,
                        object_type: None,
                        inferred_type: None,
                    },
                    span,
                });
            }
        }

        // Return Block with the final expression being the variable reference
        Expr::Block {
            statements,
            expr: Box::new(Expr::Ident {
                name: var_name,
                span,
                inferred_type: None,
            }),
            span,
            inferred_type: None,
        }
    }
}

/// Desugar a program, expanding syntax sugar into core constructs.
pub fn desugar_program(program: Program) -> Program {
    let mut desugar = Desugar::new();
    desugar.desugar_program(program)
}
