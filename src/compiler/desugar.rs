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

/// A part of a string interpolation with its original type information.
enum InterpPart {
    Literal(String),
    TypedExpr(Box<Expr>, Option<Type>),
}

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

    /// Extract the key type from a Map type (if known and concrete).
    fn extract_map_key_type(obj_type: &Option<Type>) -> Option<&Type> {
        match obj_type.as_ref()? {
            Type::Map(key_type, _) => Some(key_type),
            Type::GenericStruct {
                name, type_args, ..
            } if name == "Map" && !type_args.is_empty() => Some(&type_args[0]),
            _ => None,
        }
    }

    /// Specialize a Map method name based on the key type.
    /// Returns Some(specialized_name) if the method can be specialized, None otherwise.
    fn specialize_map_method(obj_type: &Option<Type>, method: &str) -> Option<String> {
        let key_type = Self::extract_map_key_type(obj_type)?;
        let suffix = match key_type {
            Type::Int => "int",
            Type::String => "string",
            _ => return None,
        };
        match method {
            "put" | "set" => Some(format!("put_{}", suffix)),
            "get" => Some(format!("get_{}", suffix)),
            "contains" => Some(format!("contains_{}", suffix)),
            "remove" => Some(format!("remove_{}", suffix)),
            _ => None,
        }
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
                    // For Map types, specialize to put_int/put_string if key type is known
                    let method = Self::specialize_map_method(&object_type, "set")
                        .unwrap_or_else(|| "set".to_string());

                    // Transform to method call: object.set(index, value)
                    // or object.put_int(index, value) / object.put_string(index, value)
                    return Statement::Expr {
                        expr: Expr::MethodCall {
                            object: Box::new(desugared_object),
                            method,
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
                    // For Map types, specialize to get_int/get_string if key type is known
                    let method = Self::specialize_map_method(&object_type, "get")
                        .unwrap_or_else(|| "get".to_string());

                    // Transform to method call: object.get(index)
                    // or object.get_int(index) / object.get_string(index)
                    return Expr::MethodCall {
                        object: Box::new(desugared_object),
                        method,
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
            } => {
                // Desugar ptr<T>.offset(n) to __ptr_offset(ptr, n)
                if let Some(Type::Ptr(_)) = &object_type
                    && method == "offset"
                {
                    let mut builtin_args = vec![self.desugar_expr(*object)];
                    builtin_args.extend(args.into_iter().map(|e| self.desugar_expr(e)));
                    return Expr::Call {
                        callee: "__ptr_offset".to_string(),
                        type_args: vec![],
                        args: builtin_args,
                        span,
                        inferred_type,
                    };
                }

                // Desugar Map method calls to type-specific variants
                // e.g., m.put(k, v) → m.put_int(k, v) when K=int
                let method =
                    if let Some(specialized) = Self::specialize_map_method(&object_type, &method) {
                        specialized
                    } else {
                        method
                    };

                Expr::MethodCall {
                    object: Box::new(self.desugar_expr(*object)),
                    method,
                    type_args,
                    args: args.into_iter().map(|e| self.desugar_expr(e)).collect(),
                    span,
                    object_type,
                    inferred_type,
                }
            }

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

            // StringInterpolation - desugar to direct buffer write for 3+ parts,
            // binary Add for 2 parts, or single expr/literal for 0-1 parts.
            Expr::StringInterpolation { parts, span, .. } => {
                use crate::compiler::ast::StringInterpPart;
                use crate::compiler::types::Type;

                // Collect parts with their original types (not wrapped in to_string)
                let typed_parts: Vec<InterpPart> = parts
                    .into_iter()
                    .map(|part| match part {
                        StringInterpPart::Literal(s) => InterpPart::Literal(s),
                        StringInterpPart::Expr(expr) => {
                            let expr = self.desugar_expr(*expr);
                            let ty = expr.inferred_type().cloned();
                            InterpPart::TypedExpr(Box::new(expr), ty)
                        }
                    })
                    // Filter out empty literals
                    .filter(|p| !matches!(p, InterpPart::Literal(s) if s.is_empty()))
                    .collect();

                if typed_parts.is_empty() {
                    return Expr::Str {
                        value: String::new(),
                        span,
                        inferred_type: Some(Type::String),
                    };
                }

                // Convert to string-typed exprs for 1-2 part cases
                if typed_parts.len() <= 2 {
                    let exprs: Vec<Expr> = typed_parts
                        .into_iter()
                        .map(|p| match p {
                            InterpPart::Literal(s) => Expr::Str {
                                value: s,
                                span,
                                inferred_type: Some(Type::String),
                            },
                            InterpPart::TypedExpr(expr, Some(Type::String)) => *expr,
                            InterpPart::TypedExpr(expr, _) => Expr::Call {
                                callee: "to_string".to_string(),
                                type_args: vec![],
                                args: vec![*expr],
                                span,
                                inferred_type: Some(Type::String),
                            },
                        })
                        .collect();

                    if exprs.len() == 1 {
                        return exprs.into_iter().next().unwrap();
                    }

                    // 2 parts: binary Add (string_concat)
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

                // 3+ parts: generate inline length-compute + buffer-write code
                self.desugar_string_interp_direct(typed_parts, span)
            }
        }
    }

    /// Desugar 3+ part string interpolation into direct buffer write.
    ///
    /// Generates a Block expression that:
    /// 1. Evaluates each expression into a temp var
    /// 2. Pre-converts float expressions to strings (Rust needed for shortest repr)
    /// 3. Computes total length by summing per-part lengths
    /// 4. Allocates a single buffer with __alloc_heap(total)
    /// 5. Writes each part directly into the buffer
    /// 6. Returns __alloc_string(buf, total)
    fn desugar_string_interp_direct(&mut self, typed_parts: Vec<InterpPart>, span: Span) -> Expr {
        use crate::compiler::types::Type;

        let mut stmts: Vec<Statement> = Vec::new();

        // Phase 0: Evaluate expressions into temp vars and pre-convert floats
        // Each part becomes either:
        // - PartInfo::Literal { value, len } for string literals
        // - PartInfo::String { var } for string/float/unknown (already a string ref)
        // - PartInfo::Int { var } for int values
        // - PartInfo::Bool { var } for bool values
        // - PartInfo::Nil for nil values
        enum PartInfo {
            Literal(String),
            String(String), // var name holding a string ref
            Int(String),    // var name holding an int
            Float(String),  // var name holding a float
            Bool(String),   // var name holding a bool
            Nil,
        }

        let mut part_infos: Vec<PartInfo> = Vec::new();

        for part in typed_parts {
            match part {
                InterpPart::Literal(s) => {
                    part_infos.push(PartInfo::Literal(s));
                }
                InterpPart::TypedExpr(expr, ty) => {
                    let var = self.fresh_var();
                    let expr = *expr;
                    match ty.as_ref() {
                        Some(Type::Int) => {
                            // let _var = expr;
                            stmts.push(Statement::Let {
                                name: var.clone(),
                                type_annotation: None,
                                init: expr,
                                span,
                                inferred_type: Some(Type::Int),
                            });
                            part_infos.push(PartInfo::Int(var));
                        }
                        Some(Type::Bool) => {
                            stmts.push(Statement::Let {
                                name: var.clone(),
                                type_annotation: None,
                                init: expr,
                                span,
                                inferred_type: Some(Type::Bool),
                            });
                            part_infos.push(PartInfo::Bool(var));
                        }
                        Some(Type::Nil) => {
                            // Just evaluate the expression (for side effects), ignore the value
                            stmts.push(Statement::Expr { expr, span });
                            part_infos.push(PartInfo::Nil);
                        }
                        Some(Type::String) => {
                            stmts.push(Statement::Let {
                                name: var.clone(),
                                type_annotation: None,
                                init: expr,
                                span,
                                inferred_type: Some(Type::String),
                            });
                            part_infos.push(PartInfo::String(var));
                        }
                        Some(Type::Float) => {
                            // Store float directly for _float_digit_count/_float_write_to
                            stmts.push(Statement::Let {
                                name: var.clone(),
                                type_annotation: None,
                                init: expr,
                                span,
                                inferred_type: Some(Type::Float),
                            });
                            part_infos.push(PartInfo::Float(var));
                        }
                        _ => {
                            // Unknown type: fallback to to_string
                            stmts.push(Statement::Let {
                                name: var.clone(),
                                type_annotation: None,
                                init: Expr::Call {
                                    callee: "to_string".to_string(),
                                    type_args: vec![],
                                    args: vec![expr],
                                    span,
                                    inferred_type: Some(Type::String),
                                },
                                span,
                                inferred_type: Some(Type::String),
                            });
                            part_infos.push(PartInfo::String(var));
                        }
                    }
                }
            }
        }

        // Phase 1: Build the total length expression
        // total = lit0_len + _int_digit_count(v0) + lit1_len + len(v1) + ...
        let mut len_parts: Vec<Expr> = Vec::new();
        for info in &part_infos {
            match info {
                PartInfo::Literal(s) => {
                    len_parts.push(Expr::Int {
                        value: s.len() as i64,
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
                PartInfo::String(var) => {
                    // len(var)
                    len_parts.push(Expr::Call {
                        callee: "len".to_string(),
                        type_args: vec![],
                        args: vec![Expr::Ident {
                            name: var.clone(),
                            span,
                            inferred_type: Some(Type::String),
                        }],
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
                PartInfo::Int(var) => {
                    // _int_digit_count(var)
                    len_parts.push(Expr::Call {
                        callee: "_int_digit_count".to_string(),
                        type_args: vec![],
                        args: vec![Expr::Ident {
                            name: var.clone(),
                            span,
                            inferred_type: Some(Type::Int),
                        }],
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
                PartInfo::Float(var) => {
                    // _float_digit_count(var)
                    len_parts.push(Expr::Call {
                        callee: "_float_digit_count".to_string(),
                        type_args: vec![],
                        args: vec![Expr::Ident {
                            name: var.clone(),
                            span,
                            inferred_type: Some(Type::Float),
                        }],
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
                PartInfo::Bool(var) => {
                    // _bool_str_len(var) returns 4 for true, 5 for false
                    len_parts.push(Expr::Call {
                        callee: "_bool_str_len".to_string(),
                        type_args: vec![],
                        args: vec![Expr::Ident {
                            name: var.clone(),
                            span,
                            inferred_type: Some(Type::Bool),
                        }],
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
                PartInfo::Nil => {
                    len_parts.push(Expr::Int {
                        value: 3,
                        span,
                        inferred_type: Some(Type::Int),
                    });
                }
            }
        }

        // Sum all length parts: len_parts[0] + len_parts[1] + ...
        let total_expr = len_parts
            .into_iter()
            .reduce(|acc, e| Expr::Binary {
                op: BinaryOp::Add,
                left: Box::new(acc),
                right: Box::new(e),
                span,
                inferred_type: Some(Type::Int),
            })
            .unwrap();

        // let __interp_total = <total_expr>;
        let total_var = self.fresh_var();
        stmts.push(Statement::Let {
            name: total_var.clone(),
            type_annotation: None,
            init: total_expr,
            span,
            inferred_type: Some(Type::Int),
        });

        // Phase 2: let __interp_buf = __alloc_heap(__interp_total);
        let buf_var = self.fresh_var();
        stmts.push(Statement::Let {
            name: buf_var.clone(),
            type_annotation: None,
            init: Expr::Call {
                callee: "__alloc_heap".to_string(),
                type_args: vec![],
                args: vec![Expr::Ident {
                    name: total_var.clone(),
                    span,
                    inferred_type: Some(Type::Int),
                }],
                span,
                inferred_type: None,
            },
            span,
            inferred_type: None,
        });

        // let __interp_off = 0;
        let off_var = self.fresh_var();
        stmts.push(Statement::Let {
            name: off_var.clone(),
            type_annotation: None,
            init: Expr::Int {
                value: 0,
                span,
                inferred_type: Some(Type::Int),
            },
            span,
            inferred_type: Some(Type::Int),
        });

        // Phase 3: Write each part into the buffer
        let buf_ident = || Expr::Ident {
            name: buf_var.clone(),
            span,
            inferred_type: None,
        };
        let off_ident = || Expr::Ident {
            name: off_var.clone(),
            span,
            inferred_type: Some(Type::Int),
        };

        for info in &part_infos {
            let write_expr = match info {
                PartInfo::Literal(s) => {
                    // _str_copy_to(buf, off, "literal")
                    Expr::Call {
                        callee: "_str_copy_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Str {
                                value: s.clone(),
                                span,
                                inferred_type: Some(Type::String),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
                PartInfo::String(var) => {
                    // _str_copy_to(buf, off, var)
                    Expr::Call {
                        callee: "_str_copy_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Ident {
                                name: var.clone(),
                                span,
                                inferred_type: Some(Type::String),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
                PartInfo::Int(var) => {
                    // _int_write_to(buf, off, var)
                    Expr::Call {
                        callee: "_int_write_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Ident {
                                name: var.clone(),
                                span,
                                inferred_type: Some(Type::Int),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
                PartInfo::Float(var) => {
                    // _float_write_to(buf, off, var)
                    Expr::Call {
                        callee: "_float_write_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Ident {
                                name: var.clone(),
                                span,
                                inferred_type: Some(Type::Float),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
                PartInfo::Bool(var) => {
                    // _bool_write_to(buf, off, var)
                    Expr::Call {
                        callee: "_bool_write_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Ident {
                                name: var.clone(),
                                span,
                                inferred_type: Some(Type::Bool),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
                PartInfo::Nil => {
                    // Inline: write 'n', 'i', 'l' bytes
                    // We use _str_copy_to(buf, off, "nil")
                    Expr::Call {
                        callee: "_str_copy_to".to_string(),
                        type_args: vec![],
                        args: vec![
                            buf_ident(),
                            off_ident(),
                            Expr::Str {
                                value: "nil".to_string(),
                                span,
                                inferred_type: Some(Type::String),
                            },
                        ],
                        span,
                        inferred_type: Some(Type::Int),
                    }
                }
            };

            // __interp_off = <write_expr>;
            stmts.push(Statement::Assign {
                name: off_var.clone(),
                value: write_expr,
                span,
            });
        }

        // Final expression: __alloc_string(buf, total)
        let final_expr = Expr::Call {
            callee: "__alloc_string".to_string(),
            type_args: vec![],
            args: vec![
                buf_ident(),
                Expr::Ident {
                    name: total_var,
                    span,
                    inferred_type: Some(Type::Int),
                },
            ],
            span,
            inferred_type: Some(Type::String),
        };

        Expr::Block {
            statements: stmts,
            expr: Box::new(final_expr),
            span,
            inferred_type: Some(Type::String),
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

        // Determine the specialized put method based on key type annotation
        let put_method = if let Some(key_ta) = type_args.first() {
            match key_ta {
                TypeAnnotation::Named(name) if name == "int" => "put_int".to_string(),
                TypeAnnotation::Named(name) if name == "string" => "put_string".to_string(),
                _ => "put".to_string(),
            }
        } else {
            "put".to_string()
        };

        // __new_literal_N.put_int(k, v); or __new_literal_N.put_string(k, v);
        for elem in elements.into_iter() {
            if let NewLiteralElement::KeyValue { key, value } = elem {
                statements.push(Statement::Expr {
                    expr: Expr::MethodCall {
                        object: Box::new(Expr::Ident {
                            name: var_name.clone(),
                            span,
                            inferred_type: None,
                        }),
                        method: put_method.clone(),
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
