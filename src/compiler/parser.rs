use crate::compiler::ast::*;
use crate::compiler::lexer::{Span, Token, TokenKind};

/// A recursive descent parser for mica.
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
        if self.check(&TokenKind::Fn) {
            Ok(Item::FnDef(self.fn_def()?))
        } else {
            Ok(Item::Statement(self.statement()?))
        }
    }

    fn fn_def(&mut self) -> Result<FnDef, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Fn)?;

        let name = self.expect_ident()?;
        self.expect(&TokenKind::LParen)?;

        let mut params = Vec::new();
        if !self.check(&TokenKind::RParen) {
            params.push(self.expect_ident()?);
            while self.match_token(&TokenKind::Comma) {
                params.push(self.expect_ident()?);
            }
        }
        self.expect(&TokenKind::RParen)?;

        let body = self.block()?;

        Ok(FnDef {
            name,
            params,
            body,
            span,
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
        } else if self.check(&TokenKind::If) {
            self.if_stmt()
        } else if self.check(&TokenKind::While) {
            self.while_stmt()
        } else if self.check(&TokenKind::Return) {
            self.return_stmt()
        } else if self.check_ident() && self.check_ahead(&TokenKind::Eq, 1) {
            self.assign_stmt()
        } else {
            self.expr_stmt()
        }
    }

    fn let_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        self.expect(&TokenKind::Let)?;

        let mutable = self.match_token(&TokenKind::Mut);
        let name = self.expect_ident()?;
        self.expect(&TokenKind::Eq)?;
        let init = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Let {
            name,
            mutable,
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

    fn expr_stmt(&mut self) -> Result<Statement, String> {
        let span = self.current_span();
        let expr = self.expression()?;
        self.expect(&TokenKind::Semi)?;

        Ok(Statement::Expr { expr, span })
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

        self.call_expr()
    }

    fn call_expr(&mut self) -> Result<Expr, String> {
        let mut expr = self.primary()?;

        if let Expr::Ident { name, span } = &expr {
            if self.match_token(&TokenKind::LParen) {
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
                    args,
                    span: *span,
                };
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

        if self.match_token(&TokenKind::True) {
            return Ok(Expr::Bool { value: true, span });
        }

        if self.match_token(&TokenKind::False) {
            return Ok(Expr::Bool { value: false, span });
        }

        if let Some(TokenKind::Ident(name)) = self.peek_kind() {
            let name = name.clone();
            self.advance();
            return Ok(Expr::Ident { name, span });
        }

        if self.match_token(&TokenKind::LParen) {
            let expr = self.expression()?;
            self.expect(&TokenKind::RParen)?;
            return Ok(expr);
        }

        Err(self.error("expected expression"))
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
        self.tokens
            .get(self.current + offset)
            .map(|t| &t.kind)
            == Some(kind)
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
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens()?;
        let mut parser = Parser::new("test.mica", tokens);
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
    fn test_let_mut_statement() {
        let program = parse("let mut x = 0;").unwrap();
        match &program.items[0] {
            Item::Statement(Statement::Let { name, mutable, .. }) => {
                assert_eq!(name, "x");
                assert!(mutable);
            }
            _ => panic!("expected let statement"),
        }
    }

    #[test]
    fn test_function_definition() {
        let program = parse("fn add(a, b) { return a + b; }").unwrap();
        match &program.items[0] {
            Item::FnDef(FnDef { name, params, .. }) => {
                assert_eq!(name, "add");
                assert_eq!(params, &["a", "b"]);
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
                    Expr::Binary { op: BinaryOp::Add, right, .. } => {
                        match right.as_ref() {
                            Expr::Binary { op: BinaryOp::Mul, .. } => {}
                            _ => panic!("expected multiplication"),
                        }
                    }
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
            Item::Statement(Statement::Expr { expr, .. }) => {
                match expr {
                    Expr::Call { callee, args, .. } => {
                        assert_eq!(callee, "print");
                        assert_eq!(args.len(), 1);
                    }
                    _ => panic!("expected call expression"),
                }
            }
            _ => panic!("expected expression statement"),
        }
    }
}
