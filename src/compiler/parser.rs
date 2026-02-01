use crate::compiler::ast::*;
use crate::compiler::lexer::{Span, Token, TokenKind};
use crate::compiler::types::TypeAnnotation;

/// Identifiers for asm block built-in functions.
const ASM_EMIT: &str = "__emit";
const ASM_SAFEPOINT: &str = "__safepoint";
const ASM_GC_HINT: &str = "__gc_hint";

/// A recursive descent parser for moca.
pub struct Parser<'a> {
    filename: &'a str,
    tokens: Vec<Token>,
    current: usize,
}

impl<'a> Parser<'a> {
    pub fn new(filename: &'a str, tokens: Vec<Token>) -> Self {
        Self {
            filename,
            tokens,
            current: 0,
        }
    }

    pub fn parse(&mut self) -> Result<Program, String> {
        let mut items = Vec::new();

        while !self.is_at_end() {
            items.push(self.item()?);
        }

        Ok(Program { items })
    }

    fn item(&mut self) -> Result<Item, String> {
        if self.check(&TokenKind::Import) {
            Ok(Item::Import(self.import_stmt()?))
        } else if self.check(&TokenKind::Fun) {
            Ok(Item::FnDef(self.fn_def()?))
        } else if self.check(&TokenKind::Struct) {
            Ok(Item::StructDef(self.struct_def()?))
        } else if self.check(&TokenKind::Impl) {
            Ok(Item::ImplBlock(self.impl_block()?))
        } else {
            Ok(Item::Statement(self.statement()?))
        }
    }

    fn import_stmt(&mut self) -> Result<Import, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Import)?;

        let mut path = Vec::new();
        let mut relative = false;

        // Check for relative import: import .local_mod;
        if self.match_token(&TokenKind::Dot) {
            relative = true;
        }

        // Parse module path: utils.http or local_mod
        path.push(self.expect_ident()?);
        while self.match_token(&TokenKind::Dot) {
            path.push(self.expect_ident()?);
        }

        self.expect(&TokenKind::Semi)?;

        Ok(Import {
            path,
            relative,
            span,
        })
    }

    /// Parse type parameters: `<T>`, `<T, U>`, etc.
    /// Returns an empty Vec if no type parameters are present.
    fn parse_type_params(&mut self) -> Result<Vec<String>, String> {
        if !self.match_token(&TokenKind::Lt) {
            return Ok(Vec::new());
        }

        let mut params = Vec::new();
        params.push(self.expect_ident()?);

        while self.match_token(&TokenKind::Comma) {
            params.push(self.expect_ident()?);
        }

        self.expect(&TokenKind::Gt)?;
        Ok(params)
    }

    fn fn_def(&mut self) -> Result<FnDef, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Fun)?;

        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            params.push(self.parse_param()?);
            while self.match_token(&TokenKind::Comma) {
                params.push(self.parse_param()?);
            }
        }
        self.expect(&TokenKind::RParen)?;

        // Parse optional return type: -> Type
        let return_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        let body = self.block()?;

        Ok(FnDef {
            name,
            type_params,
            params,
            return_type,
            body,
            span,
        })
    }

    /// Parse a struct definition: `struct Point { x: int, y: int }` or `struct Container<T> { value: T }`
    fn struct_def(&mut self) -> Result<StructDef, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Struct)?;

        let name = self.expect_ident()?;
        let type_params = self.parse_type_params()?;
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) {
            fields.push(self.struct_field()?);
            while self.match_token(&TokenKind::Comma) {
                if self.check(&TokenKind::RBrace) {
                    break; // Allow trailing comma
                }
                fields.push(self.struct_field()?);
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(StructDef {
            name,
            type_params,
            fields,
            span,
        })
    }

    /// Parse a struct field: `name: Type`
    fn struct_field(&mut self) -> Result<StructField, String> {
        let span = self.current_span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Colon)?;
        let type_annotation = self.parse_type_annotation()?;

        Ok(StructField {
            name,
            type_annotation,
            span,
        })
    }

    /// Parse a struct literal expression: `Point { x: 1, y: 2 }`
    /// The identifier has already been consumed.
    fn struct_literal(&mut self, name: String, span: Span) -> Result<Expr, String> {
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) {
            let field_name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let value = self.expression()?;
            fields.push((field_name, value));

            while self.match_token(&TokenKind::Comma) {
                if self.check(&TokenKind::RBrace) {
                    break; // Allow trailing comma
                }
                let field_name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let value = self.expression()?;
                fields.push((field_name, value));
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Expr::StructLiteral {
            name,
            type_args: Vec::new(), // TODO: Parse type args in Phase 3
            fields,
            span,
        })
    }

    /// Parse an impl block: `impl Point { ... }` or `impl<T> Container<T> { ... }`
    fn impl_block(&mut self) -> Result<ImplBlock, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Impl)?;

        // Parse optional type parameters: impl<T> ...
        let type_params = self.parse_type_params()?;

        let struct_name = self.expect_ident()?;

        // Parse optional type arguments for the struct: Container<T>
        let struct_type_args = self.parse_type_args()?;

        self.expect(&TokenKind::LBrace)?;

        let mut methods = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            methods.push(self.fn_def()?);
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(ImplBlock {
            type_params,
            struct_name,
            struct_type_args,
            methods,
            span,
        })
    }

    /// Parse type arguments: `<int, string>`, `<T>`, etc.
    /// Returns an empty Vec if no type arguments are present.
    fn parse_type_args(&mut self) -> Result<Vec<TypeAnnotation>, String> {
        if !self.match_token(&TokenKind::Lt) {
            return Ok(Vec::new());
        }

        let mut args = Vec::new();
        args.push(self.parse_type_annotation()?);

        while self.match_token(&TokenKind::Comma) {
            args.push(self.parse_type_annotation()?);
        }

        self.expect(&TokenKind::Gt)?;
        Ok(args)
    }

    fn parse_param(&mut self) -> Result<Param, String> {
        let param_span = self.current_span();
        let param_name = self.expect_ident()?;

        // Parse optional type annotation: : Type
        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        Ok(Param {
            name: param_name,
            type_annotation,
            span: param_span,
        })
    }

    fn block(&mut self) -> Result<Block, String> {
        let span = self.current_span();
        self.expect(&TokenKind::LBrace)?;

        let mut statements = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            statements.push(self.statement()?);
        }

        self.expect(&TokenKind::RBrace)?;

        Ok(Block { statements, span })
    }

    fn statement(&mut self) -> Result<Statement, String> {
        if self.check(&TokenKind::Let) {
            self.let_stmt()
        } else if self.check(&TokenKind::Var) {
            self.var_stmt()
        } else if self.check(&TokenKind::If) {
            self.if_stmt()
        } else if self.check(&TokenKind::While) {
            self.while_stmt()
        } else if self.check(&TokenKind::For) {
            self.for_stmt()
        } else if self.check(&TokenKind::Return) {
            self.return_stmt()
        } else if self.check(&TokenKind::Throw) {
            self.throw_stmt()
        } else if self.check(&TokenKind::Try) {
            self.try_stmt()
        } else if self.check_ident() && self.check_ahead(&TokenKind::Eq, 1) {
            self.assign_stmt()
        } else {
            // Could be expression statement or complex assignment
            self.expr_or_assign_stmt()
        }
    }

    fn let_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Let)?;

        let name = self.expect_ident()?;

        // Parse optional type annotation: : Type
        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        self.expect(&TokenKind::Eq)?;
        let init = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Let {
            name,
            mutable: false,
            type_annotation,
            init,
            span,
        })
    }

    fn var_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Var)?;

        let name = self.expect_ident()?;

        // Parse optional type annotation: : Type
        let type_annotation = if self.match_token(&TokenKind::Colon) {
            Some(self.parse_type_annotation()?)
        } else {
            None
        };

        self.expect(&TokenKind::Eq)?;
        let init = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Let {
            name,
            mutable: true,
            type_annotation,
            init,
            span,
        })
    }

    fn assign_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let value = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Assign { name, value, span })
    }

    fn expr_or_assign_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        let expr = self.expression()?;

        // Check if this is an assignment
        if self.match_token(&TokenKind::Eq) {
            let value = self.expression()?;
            self.expect(&TokenKind::Semi)?;

            // Determine what kind of assignment this is
            match expr {
                Expr::Index { object, index, .. } => Ok(Statement::IndexAssign {
                    object: *object,
                    index: *index,
                    value,
                    span,
                    object_type: None,
                }),
                Expr::Field { object, field, .. } => Ok(Statement::FieldAssign {
                    object: *object,
                    field,
                    value,
                    span,
                }),
                _ => Err(self.error("invalid assignment target")),
            }
        } else {
            self.expect(&TokenKind::Semi)?;
            Ok(Statement::Expr { expr, span })
        }
    }

    fn if_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::If)?;

        let condition = self.expression()?;
        let then_block = self.block()?;

        let else_block = if self.match_token(&TokenKind::Else) {
            if self.check(&TokenKind::If) {
                // else if -> treat as else { if ... }
                let inner_if = self.if_stmt()?;
                Some(Block {
                    statements: vec![inner_if],
                    span: self.current_span(),
                })
            } else {
                Some(self.block()?)
            }
        } else {
            None
        };

        Ok(Statement::If {
            condition,
            then_block,
            else_block,
            span,
        })
    }

    fn while_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::While)?;

        let condition = self.expression()?;
        let body = self.block()?;

        Ok(Statement::While {
            condition,
            body,
            span,
        })
    }

    fn for_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::For)?;

        let var = self.expect_ident()?;
        self.expect(&TokenKind::In)?;
        let iterable = self.expression()?;
        let body = self.block()?;

        Ok(Statement::ForIn {
            var,
            iterable,
            body,
            span,
        })
    }

    fn return_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Return)?;

        let value = if self.check(&TokenKind::Semi) {
            None
        } else {
            Some(self.expression()?)
        };

        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Return { value, span })
    }

    fn throw_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Throw)?;

        let value = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Throw { value, span })
    }

    fn try_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Try)?;

        let try_block = self.block()?;

        self.expect(&TokenKind::Catch)?;
        let catch_var = self.expect_ident()?;
        let catch_block = self.block()?;

        Ok(Statement::Try {
            try_block,
            catch_var,
            catch_block,
            span,
        })
    }

    // Type annotation parsing

    /// Parse a type annotation.
    /// Type ::= PrimaryType ('?')?
    /// PrimaryType ::= NamedType | ArrayType | ObjectType | FunctionType
    fn parse_type_annotation(&mut self) -> Result<TypeAnnotation, String> {
        let base_type = self.parse_primary_type()?;

        // Check for nullable suffix: T?
        if self.match_token(&TokenKind::Question) {
            Ok(TypeAnnotation::Nullable(Box::new(base_type)))
        } else {
            Ok(base_type)
        }
    }

    fn parse_primary_type(&mut self) -> Result<TypeAnnotation, String> {
        // Check for function type: (T1, T2) -> R
        if self.check(&TokenKind::LParen) {
            return self.parse_function_type();
        }

        // Check for object type: {field: T, ...}
        if self.check(&TokenKind::LBrace) {
            return self.parse_object_type();
        }

        // Named type (int, float, bool, string, nil) or array<T>
        let name = self.expect_ident()?;

        // Check for array<T>
        if name == "array" && self.match_token(&TokenKind::Lt) {
            let element_type = self.parse_type_annotation()?;
            self.expect(&TokenKind::Gt)?;
            return Ok(TypeAnnotation::Array(Box::new(element_type)));
        }

        // Check for vec<T>
        if name == "vec" && self.match_token(&TokenKind::Lt) {
            let element_type = self.parse_type_annotation()?;
            self.expect(&TokenKind::Gt)?;
            return Ok(TypeAnnotation::Vec(Box::new(element_type)));
        }

        // Check for map<K, V>
        if name == "map" && self.match_token(&TokenKind::Lt) {
            let key_type = self.parse_type_annotation()?;
            self.expect(&TokenKind::Comma)?;
            let value_type = self.parse_type_annotation()?;
            self.expect(&TokenKind::Gt)?;
            return Ok(TypeAnnotation::Map(
                Box::new(key_type),
                Box::new(value_type),
            ));
        }

        Ok(TypeAnnotation::Named(name))
    }

    fn parse_function_type(&mut self) -> Result<TypeAnnotation, String> {
        self.expect(&TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            params.push(self.parse_type_annotation()?);
            while self.match_token(&TokenKind::Comma) {
                params.push(self.parse_type_annotation()?);
            }
        }
        self.expect(&TokenKind::RParen)?;

        self.expect(&TokenKind::Arrow)?;
        let ret = self.parse_type_annotation()?;

        Ok(TypeAnnotation::Function {
            params,
            ret: Box::new(ret),
        })
    }

    fn parse_object_type(&mut self) -> Result<TypeAnnotation, String> {
        self.expect(&TokenKind::LBrace)?;

        let mut fields = Vec::new();
        if !self.check(&TokenKind::RBrace) {
            let field_name = self.expect_ident()?;
            self.expect(&TokenKind::Colon)?;
            let field_type = self.parse_type_annotation()?;
            fields.push((field_name, field_type));

            while self.match_token(&TokenKind::Comma) {
                if self.check(&TokenKind::RBrace) {
                    break; // Allow trailing comma
                }
                let field_name = self.expect_ident()?;
                self.expect(&TokenKind::Colon)?;
                let field_type = self.parse_type_annotation()?;
                fields.push((field_name, field_type));
            }
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(TypeAnnotation::Object(fields))
    }

    // Expression parsing with precedence climbing

    fn expression(&mut self) -> Result<Expr, String> {
        self.or_expr()
    }

    fn or_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.and_expr()?;

        while self.match_token(&TokenKind::OrOr) {
            let span = left.span();
            let right = self.and_expr()?;
            left = Expr::Binary {
                op: BinaryOp::Or,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn and_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.eq_expr()?;

        while self.match_token(&TokenKind::AndAnd) {
            let span = left.span();
            let right = self.eq_expr()?;
            left = Expr::Binary {
                op: BinaryOp::And,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn eq_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.cmp_expr()?;

        loop {
            let op = if self.match_token(&TokenKind::EqEq) {
                BinaryOp::Eq
            } else if self.match_token(&TokenKind::NotEq) {
                BinaryOp::Ne
            } else {
                break;
            };

            let span = left.span();
            let right = self.cmp_expr()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn cmp_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.add_expr()?;

        loop {
            let op = if self.match_token(&TokenKind::Lt) {
                BinaryOp::Lt
            } else if self.match_token(&TokenKind::Le) {
                BinaryOp::Le
            } else if self.match_token(&TokenKind::Gt) {
                BinaryOp::Gt
            } else if self.match_token(&TokenKind::Ge) {
                BinaryOp::Ge
            } else {
                break;
            };

            let span = left.span();
            let right = self.add_expr()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn add_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.mul_expr()?;

        loop {
            let op = if self.match_token(&TokenKind::Plus) {
                BinaryOp::Add
            } else if self.match_token(&TokenKind::Minus) {
                BinaryOp::Sub
            } else {
                break;
            };

            let span = left.span();
            let right = self.mul_expr()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn mul_expr(&mut self) -> Result<Expr, String> {
        let mut left = self.unary_expr()?;

        loop {
            let op = if self.match_token(&TokenKind::Star) {
                BinaryOp::Mul
            } else if self.match_token(&TokenKind::Slash) {
                BinaryOp::Div
            } else if self.match_token(&TokenKind::Percent) {
                BinaryOp::Mod
            } else {
                break;
            };

            let span = left.span();
            let right = self.unary_expr()?;
            left = Expr::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
                span,
            };
        }

        Ok(left)
    }

    fn unary_expr(&mut self) -> Result<Expr, String> {
        if self.match_token(&TokenKind::Bang) {
            let span = self.previous_span();
            let operand = self.unary_expr()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
                span,
            });
        }

        if self.match_token(&TokenKind::Minus) {
            let span = self.previous_span();
            let operand = self.unary_expr()?;
            return Ok(Expr::Unary {
                op: UnaryOp::Neg,
                operand: Box::new(operand),
                span,
            });
        }

        self.postfix_expr()
    }

    fn postfix_expr(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;

        loop {
            if self.match_token(&TokenKind::LParen) {
                // Function call
                if let Expr::Ident { name, span } = &expr {
                    let mut args = Vec::new();

                    if !self.check(&TokenKind::RParen) {
                        args.push(self.expression()?);
                        while self.match_token(&TokenKind::Comma) {
                            args.push(self.expression()?);
                        }
                    }

                    self.expect(&TokenKind::RParen)?;

                    expr = Expr::Call {
                        callee: name.clone(),
                        type_args: Vec::new(), // TODO: Parse type args in Phase 3
                        args,
                        span: *span,
                    };
                } else {
                    return Err(self.error("expected function name before '('"));
                }
            } else if self.match_token(&TokenKind::LBracket) {
                // Index access
                let span = expr.span();
                let index = self.expression()?;
                self.expect(&TokenKind::RBracket)?;

                expr = Expr::Index {
                    object: Box::new(expr),
                    index: Box::new(index),
                    span,
                    object_type: None,
                };
            } else if self.match_token(&TokenKind::Dot) {
                // Field access or method call
                let span = expr.span();
                let field = self.expect_ident()?;

                // Check if this is a method call
                if self.match_token(&TokenKind::LParen) {
                    let mut args = Vec::new();

                    if !self.check(&TokenKind::RParen) {
                        args.push(self.expression()?);
                        while self.match_token(&TokenKind::Comma) {
                            args.push(self.expression()?);
                        }
                    }

                    self.expect(&TokenKind::RParen)?;

                    expr = Expr::MethodCall {
                        object: Box::new(expr),
                        method: field,
                        type_args: Vec::new(), // TODO: Parse type args in Phase 3
                        args,
                        span,
                    };
                } else {
                    expr = Expr::Field {
                        object: Box::new(expr),
                        field,
                        span,
                    };
                }
            } else if self.match_token(&TokenKind::ColonColon) {
                // Associated function call: Type::func()
                if let Expr::Ident {
                    name: type_name,
                    span,
                } = &expr
                {
                    let function = self.expect_ident()?;
                    self.expect(&TokenKind::LParen)?;

                    let mut args = Vec::new();
                    if !self.check(&TokenKind::RParen) {
                        args.push(self.expression()?);
                        while self.match_token(&TokenKind::Comma) {
                            args.push(self.expression()?);
                        }
                    }

                    self.expect(&TokenKind::RParen)?;

                    expr = Expr::AssociatedFunctionCall {
                        type_name: type_name.clone(),
                        type_args: Vec::new(), // TODO: Parse type args in Phase 3
                        function,
                        fn_type_args: Vec::new(), // TODO: Parse fn type args in Phase 3
                        args,
                        span: *span,
                    };
                } else {
                    return Err(self.error("expected type name before '::'"));
                }
            } else {
                break;
            }
        }

        Ok(expr)
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let span = self.current_span();

        if let Some(TokenKind::Int(value)) = self.peek_kind() {
            let value = *value;
            self.advance();
            return Ok(Expr::Int { value, span });
        }

        if let Some(TokenKind::Float(value)) = self.peek_kind() {
            let value = *value;
            self.advance();
            return Ok(Expr::Float { value, span });
        }

        if let Some(TokenKind::Str(value)) = self.peek_kind() {
            let value = value.clone();
            self.advance();
            return Ok(Expr::Str { value, span });
        }

        if self.match_token(&TokenKind::True) {
            return Ok(Expr::Bool { value: true, span });
        }

        if self.match_token(&TokenKind::False) {
            return Ok(Expr::Bool { value: false, span });
        }

        if self.match_token(&TokenKind::Nil) {
            return Ok(Expr::Nil { span });
        }

        if let Some(TokenKind::Ident(name)) = self.peek_kind() {
            let name = name.clone();
            self.advance();

            // Check if this is a struct literal: Name { field: value, ... }
            // Use lookahead to distinguish from blocks: { must be followed by ident :
            if self.check(&TokenKind::LBrace) && self.is_struct_literal_start() {
                return self.struct_literal(name, span);
            }

            return Ok(Expr::Ident { name, span });
        }

        if self.match_token(&TokenKind::LParen) {
            let expr = self.expression()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(expr);
        }

        if self.match_token(&TokenKind::LBracket) {
            // Array literal
            let mut elements = Vec::new();

            if !self.check(&TokenKind::RBracket) {
                elements.push(self.expression()?);
                while self.match_token(&TokenKind::Comma) {
                    if self.check(&TokenKind::RBracket) {
                        break; // Allow trailing comma
                    }
                    elements.push(self.expression()?);
                }
            }

            self.expect(&TokenKind::RBracket)?;
            return Ok(Expr::Array { elements, span });
        }

        // Inline assembly block: asm { ... } or asm(inputs) { ... } or asm(inputs) -> type { ... }
        if self.match_token(&TokenKind::Asm) {
            return self.asm_block(span);
        }

        Err(self.error("expected expression"))
    }

    /// Parse an inline assembly block.
    /// asm { ... }
    /// asm(inputs) { ... }
    /// asm(inputs) -> type { ... }
    fn asm_block(&mut self, span: Span) -> Result<Expr, String> {
        // Parse optional inputs: asm(x, y)
        let inputs = if self.match_token(&TokenKind::LParen) {
            let mut inputs = Vec::new();
            if !self.check(&TokenKind::RParen) {
                inputs.push(self.expect_ident()?);
                while self.match_token(&TokenKind::Comma) {
                    inputs.push(self.expect_ident()?);
                }
            }
            self.expect(&TokenKind::RParen)?;
            inputs
        } else {
            Vec::new()
        };

        // Parse optional output type: -> type
        let output_type = if self.match_token(&TokenKind::Arrow) {
            Some(self.expect_ident()?)
        } else {
            None
        };

        // Parse body: { ... }
        self.expect(&TokenKind::LBrace)?;
        let mut body = Vec::new();
        while !self.check(&TokenKind::RBrace) && !self.is_at_end() {
            body.push(self.asm_instruction()?);
        }
        self.expect(&TokenKind::RBrace)?;

        Ok(Expr::Asm(AsmBlock {
            inputs,
            output_type,
            body,
            span,
        }))
    }

    /// Parse a single asm instruction.
    /// __emit("OpName", args...);
    /// __safepoint();
    /// __gc_hint(size);
    fn asm_instruction(&mut self) -> Result<AsmInstruction, String> {
        let span = self.current_span();

        // Check for identifier (must be one of the asm builtins)
        let name = self.expect_ident()?;

        match name.as_str() {
            ASM_EMIT => {
                self.expect(&TokenKind::LParen)?;

                // First argument must be a string (op name)
                let op_name = match self.peek_kind() {
                    Some(TokenKind::Str(s)) => {
                        let s = s.clone();
                        self.advance();
                        s
                    }
                    _ => return Err(self.error("__emit requires a string as first argument")),
                };

                // Optional additional arguments
                let mut args = Vec::new();
                while self.match_token(&TokenKind::Comma) {
                    args.push(self.asm_arg()?);
                }

                self.expect(&TokenKind::RParen)?;
                self.expect(&TokenKind::Semi)?;

                Ok(AsmInstruction::Emit {
                    op_name,
                    args,
                    span,
                })
            }
            ASM_SAFEPOINT => {
                self.expect(&TokenKind::LParen)?;
                self.expect(&TokenKind::RParen)?;
                self.expect(&TokenKind::Semi)?;
                Ok(AsmInstruction::Safepoint { span })
            }
            ASM_GC_HINT => {
                self.expect(&TokenKind::LParen)?;

                // Expect an integer argument
                let size = match self.peek_kind() {
                    Some(TokenKind::Int(n)) => {
                        let n = *n;
                        self.advance();
                        n
                    }
                    _ => return Err(self.error("__gc_hint requires an integer argument")),
                };

                self.expect(&TokenKind::RParen)?;
                self.expect(&TokenKind::Semi)?;

                Ok(AsmInstruction::GcHint { size, span })
            }
            _ => Err(self.error(&format!(
                "unknown asm instruction '{}' (expected __emit, __safepoint, or __gc_hint)",
                name
            ))),
        }
    }

    /// Parse an asm argument (int, float, or string).
    fn asm_arg(&mut self) -> Result<AsmArg, String> {
        match self.peek_kind() {
            Some(TokenKind::Int(n)) => {
                let n = *n;
                self.advance();
                Ok(AsmArg::Int(n))
            }
            Some(TokenKind::Float(f)) => {
                let f = *f;
                self.advance();
                Ok(AsmArg::Float(f))
            }
            Some(TokenKind::Str(s)) => {
                let s = s.clone();
                self.advance();
                Ok(AsmArg::String(s))
            }
            _ => Err(self.error("expected int, float, or string as asm argument")),
        }
    }

    // Helper methods

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.current)
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|t| &t.kind)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Eof) | None)
    }

    fn check(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == Some(kind)
    }

    fn check_ident(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::Ident(_)))
    }

    fn check_ahead(&self, kind: &TokenKind, offset: usize) -> bool {
        self.tokens.get(self.current + offset).map(|t| &t.kind) == Some(kind)
    }

    /// Check if the current position looks like the start of a struct literal.
    /// We're at `{` and need to distinguish `Struct { field: value }` from a block `{ stmt; }`.
    /// Returns true if it looks like `{ }` (empty struct) or `{ ident : ...`
    fn is_struct_literal_start(&self) -> bool {
        // Current is at `{`
        // Check: `{ }` (empty struct) or `{ ident :`
        if self.check_ahead(&TokenKind::RBrace, 1) {
            // Empty struct literal `{}`
            return true;
        }

        // Check for `{ ident :` pattern
        if let Some(token) = self.tokens.get(self.current + 1)
            && matches!(&token.kind, TokenKind::Ident(_))
        {
            return self.check_ahead(&TokenKind::Colon, 2);
        }

        false
    }

    fn advance(&mut self) -> Option<&Token> {
        if !self.is_at_end() {
            self.current += 1;
        }
        self.tokens.get(self.current - 1)
    }

    fn match_token(&mut self, kind: &TokenKind) -> bool {
        if self.check(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, kind: &TokenKind) -> Result<(), String> {
        if self.check(kind) {
            self.advance();
            Ok(())
        } else {
            Err(self.error(&format!("expected {:?}", kind)))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        if let Some(TokenKind::Ident(name)) = self.peek_kind() {
            let name = name.clone();
            self.advance();
            Ok(name)
        } else {
            Err(self.error("expected identifier"))
        }
    }

    fn current_span(&self) -> Span {
        self.peek().map(|t| t.span).unwrap_or(Span::new(1, 1))
    }

    fn previous_span(&self) -> Span {
        self.tokens
            .get(self.current.saturating_sub(1))
            .map(|t| t.span)
            .unwrap_or(Span::new(1, 1))
    }

    fn error(&self, message: &str) -> String {
        let span = self.current_span();
        format!(
            "error: {}\n  --> {}:{}:{}",
            message, self.filename, span.line, span.column
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compiler::lexer::Lexer;

    fn parse(source: &str) -> Result<Program, String> {
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mc", tokens);
        parser.parse()
    }

    #[test]
    fn test_let_statement() {
        let program = parse("let x = 42;").unwrap();
        assert_eq!(program.items.len(), 1);
        match &program.items[0] {
            Item::Statement(Statement::Let { name, mutable, .. }) => {
                assert_eq!(name, "x");
                assert!(!mutable);
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_var_statement() {
        let program = parse("var x = 0;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { name, mutable, .. }) => {
                assert_eq!(name, "x");
                assert!(mutable);
            }
            _ => panic!("expected var statement"),
        }
    }

    #[test]
    fn test_function_definition() {
        let program = parse("fun add(a, b) { return a + b; }").unwrap();
        match &program.items[0] {
            Item::FnDef(FnDef { name, params, .. }) => {
                assert_eq!(name, "add");
                let param_names: Vec<&str> = params.iter().map(|p| p.name.as_str()).collect();
                assert_eq!(param_names, vec!["a", "b"]);
            }
            _ => panic!("expected function definition"),
        }
    }

    #[test]
    fn test_if_else() {
        let program = parse("if x > 0 { print(x); } else { print(0); }").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::If { else_block, .. }) => {
                assert!(else_block.is_some());
            }
            _ => panic!("expected if statement"),
        }
    }

    #[test]
    fn test_while_loop() {
        let program = parse("while i < 10 { i = i + 1; }").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::While { .. }) => {}
            _ => panic!("expected while statement"),
        }
    }

    #[test]
    fn test_binary_expression() {
        let program = parse("let x = 1 + 2 * 3;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => {
                // Should be 1 + (2 * 3) due to precedence
                match init {
                    Expr::Binary {
                        op: BinaryOp::Add,
                        right,
                        ..
                    } => match right.as_ref() {
                        Expr::Binary {
                            op: BinaryOp::Mul, ..
                        } => {}
                        _ => panic!("expected multiplication"),
                    },
                    _ => panic!("expected binary expression"),
                }
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_function_call() {
        let program = parse("print(42);").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Expr { expr, .. }) => match expr {
                Expr::Call { callee, args, .. } => {
                    assert_eq!(callee, "print");
                    assert_eq!(args.len(), 1);
                }
                _ => panic!("expected call expression"),
            },
            _ => panic!("expected expression statement"),
        }
    }

    #[test]
    fn test_float_literal() {
        let program = parse("let x = 3.14;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::Float { value, .. } => {
                    assert_eq!(*value, 3.14);
                }
                _ => panic!("expected float"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_string_literal() {
        let program = parse(r#"let s = "hello";"#).unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::Str { value, .. } => {
                    assert_eq!(value, "hello");
                }
                _ => panic!("expected string"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_nil_literal() {
        let program = parse("let x = nil;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => {
                assert!(matches!(init, Expr::Nil { .. }));
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_array_literal() {
        let program = parse("let arr = [1, 2, 3];").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::Array { elements, .. } => {
                    assert_eq!(elements.len(), 3);
                }
                _ => panic!("expected array"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_index_access() {
        let program = parse("let x = arr[0];").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => {
                assert!(matches!(init, Expr::Index { .. }));
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_field_access() {
        let program = parse("let x = obj.field;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::Field { field, .. } => {
                    assert_eq!(field, "field");
                }
                _ => panic!("expected field access"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_for_in() {
        let program = parse("for x in arr { print(x); }").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::ForIn { var, .. }) => {
                assert_eq!(var, "x");
            }
            _ => panic!("expected for-in statement"),
        }
    }

    #[test]
    fn test_try_catch() {
        let program = parse("try { throw x; } catch e { print(e); }").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Try { catch_var, .. }) => {
                assert_eq!(catch_var, "e");
            }
            _ => panic!("expected try statement"),
        }
    }

    #[test]
    fn test_throw() {
        let program = parse(r#"throw "error";"#).unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Throw { value, .. }) => {
                assert!(matches!(value, Expr::Str { .. }));
            }
            _ => panic!("expected throw statement"),
        }
    }

    #[test]
    fn test_import_simple() {
        let program = parse("import utils;").unwrap();
        match &program.items[0] {
            Item::Import(Import { path, relative, .. }) => {
                assert_eq!(path, &["utils"]);
                assert!(!relative);
            }
            _ => panic!("expected import statement"),
        }
    }

    #[test]
    fn test_import_nested() {
        let program = parse("import utils.http.client;").unwrap();
        match &program.items[0] {
            Item::Import(Import { path, relative, .. }) => {
                assert_eq!(path, &["utils", "http", "client"]);
                assert!(!relative);
            }
            _ => panic!("expected import statement"),
        }
    }

    #[test]
    fn test_import_relative() {
        let program = parse("import .local_mod;").unwrap();
        match &program.items[0] {
            Item::Import(Import { path, relative, .. }) => {
                assert_eq!(path, &["local_mod"]);
                assert!(relative);
            }
            _ => panic!("expected import statement"),
        }
    }

    // Type annotation tests

    #[test]
    fn test_let_with_type_annotation() {
        let program = parse("let x: int = 42;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                name,
                type_annotation,
                ..
            }) => {
                assert_eq!(name, "x");
                assert!(type_annotation.is_some());
                assert_eq!(type_annotation.as_ref().unwrap().to_string(), "int");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_let_with_nullable_type() {
        let program = parse("let x: string? = nil;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                type_annotation, ..
            }) => {
                assert!(type_annotation.is_some());
                assert_eq!(type_annotation.as_ref().unwrap().to_string(), "string?");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_let_with_array_type() {
        let program = parse("let arr: array<int> = [1, 2, 3];").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                type_annotation, ..
            }) => {
                assert!(type_annotation.is_some());
                assert_eq!(type_annotation.as_ref().unwrap().to_string(), "array<int>");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_let_with_vec_type() {
        let program = parse("let v: vec<int> = vec::new();").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                type_annotation, ..
            }) => {
                assert!(type_annotation.is_some());
                assert_eq!(type_annotation.as_ref().unwrap().to_string(), "vec<int>");
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_let_with_map_type() {
        let program = parse("let m: map<string, int> = map::new();").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                type_annotation, ..
            }) => {
                assert!(type_annotation.is_some());
                assert_eq!(
                    type_annotation.as_ref().unwrap().to_string(),
                    "map<string, int>"
                );
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_nested_vec_type() {
        let program = parse("let v: vec<vec<int>> = vec::new();").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let {
                type_annotation, ..
            }) => {
                assert!(type_annotation.is_some());
                assert_eq!(
                    type_annotation.as_ref().unwrap().to_string(),
                    "vec<vec<int>>"
                );
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_function_with_type_annotations() {
        let program = parse("fun add(a: int, b: int) -> int { return a + b; }").unwrap();
        match &program.items[0] {
            Item::FnDef(FnDef {
                name,
                params,
                return_type,
                ..
            }) => {
                assert_eq!(name, "add");
                assert_eq!(params.len(), 2);
                assert!(params[0].type_annotation.is_some());
                assert_eq!(
                    params[0].type_annotation.as_ref().unwrap().to_string(),
                    "int"
                );
                assert!(params[1].type_annotation.is_some());
                assert_eq!(
                    params[1].type_annotation.as_ref().unwrap().to_string(),
                    "int"
                );
                assert!(return_type.is_some());
                assert_eq!(return_type.as_ref().unwrap().to_string(), "int");
            }
            _ => panic!("expected function definition"),
        }
    }

    #[test]
    fn test_function_without_type_annotations() {
        let program = parse("fun add(a, b) { return a + b; }").unwrap();
        match &program.items[0] {
            Item::FnDef(FnDef {
                params,
                return_type,
                ..
            }) => {
                assert!(params[0].type_annotation.is_none());
                assert!(params[1].type_annotation.is_none());
                assert!(return_type.is_none());
            }
            _ => panic!("expected function definition"),
        }
    }

    // Struct tests

    #[test]
    fn test_struct_definition() {
        let program = parse("struct Point { x: int, y: int }").unwrap();
        match &program.items[0] {
            Item::StructDef(StructDef { name, fields, .. }) => {
                assert_eq!(name, "Point");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[0].name, "x");
                assert_eq!(fields[1].name, "y");
            }
            _ => panic!("expected struct definition"),
        }
    }

    #[test]
    fn test_struct_with_trailing_comma() {
        let program = parse("struct Point { x: int, y: int, }").unwrap();
        match &program.items[0] {
            Item::StructDef(StructDef { fields, .. }) => {
                assert_eq!(fields.len(), 2);
            }
            _ => panic!("expected struct definition"),
        }
    }

    #[test]
    fn test_struct_with_nullable_field() {
        let program = parse("struct Node { value: int, next: Node? }").unwrap();
        match &program.items[0] {
            Item::StructDef(StructDef { name, fields, .. }) => {
                assert_eq!(name, "Node");
                assert_eq!(fields.len(), 2);
                assert_eq!(fields[1].name, "next");
                assert_eq!(fields[1].type_annotation.to_string(), "Node?");
            }
            _ => panic!("expected struct definition"),
        }
    }

    #[test]
    fn test_impl_block() {
        let program = parse("impl Point { fun get_x(self) -> int { return 0; } }").unwrap();
        match &program.items[0] {
            Item::ImplBlock(ImplBlock {
                struct_name,
                methods,
                ..
            }) => {
                assert_eq!(struct_name, "Point");
                assert_eq!(methods.len(), 1);
                assert_eq!(methods[0].name, "get_x");
            }
            _ => panic!("expected impl block"),
        }
    }

    #[test]
    fn test_impl_with_self_method() {
        let program =
            parse("impl Rectangle { fun area(self) -> int { return self.width * self.height; } }")
                .unwrap();
        match &program.items[0] {
            Item::ImplBlock(ImplBlock { methods, .. }) => {
                assert_eq!(methods.len(), 1);
                assert_eq!(methods[0].name, "area");
                assert_eq!(methods[0].params.len(), 1);
                assert_eq!(methods[0].params[0].name, "self");
            }
            _ => panic!("expected impl block"),
        }
    }

    #[test]
    fn test_struct_literal() {
        let program = parse("let p = Point { x: 1, y: 2 };").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::StructLiteral { name, fields, .. } => {
                    assert_eq!(name, "Point");
                    assert_eq!(fields.len(), 2);
                    assert_eq!(fields[0].0, "x");
                    assert_eq!(fields[1].0, "y");
                }
                _ => panic!("expected struct literal"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_struct_literal_trailing_comma() {
        let program = parse("let p = Point { x: 1, };").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::StructLiteral { fields, .. } => {
                    assert_eq!(fields.len(), 1);
                }
                _ => panic!("expected struct literal"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_method_call() {
        let program = parse("let a = rect.area();").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::MethodCall { method, args, .. } => {
                    assert_eq!(method, "area");
                    assert_eq!(args.len(), 0);
                }
                _ => panic!("expected method call"),
            },
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_method_call_with_args() {
        let program = parse("let scaled = rect.scale(2, 3);").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { init, .. }) => match init {
                Expr::MethodCall { method, args, .. } => {
                    assert_eq!(method, "scale");
                    assert_eq!(args.len(), 2);
                }
                _ => panic!("expected method call"),
            },
            _ => panic!("expected let statement"),
        }
    }
}
