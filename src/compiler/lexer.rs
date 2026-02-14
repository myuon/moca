/// Token kinds for the moca language.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Let,
    Var,
    Fun,
    If,
    Else,
    While,
    For,
    In,
    Return,
    True,
    False,
    Nil,
    Try,
    Catch,
    Throw,
    Import,
    Struct,
    Impl,
    Asm,
    Type,
    New,   // new literal keyword
    Const, // const keyword

    // Literals
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),

    // Operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    AndAnd,
    OrOr,
    Bang,
    Eq,
    Dot,
    Arrow,    // ->
    Question, // ?
    At,       // @

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semi,
    Colon,
    ColonColon, // ::

    // Special
    Eof,
}

/// Source location information.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Span {
    pub line: usize,
    pub column: usize,
}

impl Span {
    pub fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

/// A token with its kind and location.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

impl Token {
    pub fn new(kind: TokenKind, span: Span) -> Self {
        Self { kind, span }
    }
}

/// The lexer for moca source code.
pub struct Lexer<'a> {
    filename: &'a str,
    source: &'a str,
    chars: std::iter::Peekable<std::str::CharIndices<'a>>,
    line: usize,
    column: usize,
    line_start: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(filename: &'a str, source: &'a str) -> Self {
        Self {
            filename,
            source,
            chars: source.char_indices().peekable(),
            line: 1,
            column: 1,
            line_start: 0,
        }
    }

    pub fn scan_tokens(&mut self) -> Result<Vec<Token>, String> {
        let mut tokens = Vec::new();

        loop {
            self.skip_whitespace_and_comments();

            let span = Span::new(self.line, self.column);

            let Some((_, ch)) = self.peek() else {
                tokens.push(Token::new(TokenKind::Eof, span));
                break;
            };

            let kind = match ch {
                '(' => {
                    self.advance();
                    TokenKind::LParen
                }
                ')' => {
                    self.advance();
                    TokenKind::RParen
                }
                '{' => {
                    self.advance();
                    TokenKind::LBrace
                }
                '}' => {
                    self.advance();
                    TokenKind::RBrace
                }
                '[' => {
                    self.advance();
                    TokenKind::LBracket
                }
                ']' => {
                    self.advance();
                    TokenKind::RBracket
                }
                ',' => {
                    self.advance();
                    TokenKind::Comma
                }
                ';' => {
                    self.advance();
                    TokenKind::Semi
                }
                ':' => {
                    self.advance();
                    if self.match_char(':') {
                        TokenKind::ColonColon
                    } else {
                        TokenKind::Colon
                    }
                }
                '.' => {
                    self.advance();
                    TokenKind::Dot
                }
                '+' => {
                    self.advance();
                    TokenKind::Plus
                }
                '-' => {
                    self.advance();
                    if self.match_char('>') {
                        TokenKind::Arrow
                    } else {
                        TokenKind::Minus
                    }
                }
                '?' => {
                    self.advance();
                    TokenKind::Question
                }
                '*' => {
                    self.advance();
                    TokenKind::Star
                }
                '/' => {
                    self.advance();
                    TokenKind::Slash
                }
                '%' => {
                    self.advance();
                    TokenKind::Percent
                }
                '!' => {
                    self.advance();
                    if self.match_char('=') {
                        TokenKind::NotEq
                    } else {
                        TokenKind::Bang
                    }
                }
                '=' => {
                    self.advance();
                    if self.match_char('=') {
                        TokenKind::EqEq
                    } else {
                        TokenKind::Eq
                    }
                }
                '<' => {
                    self.advance();
                    if self.match_char('=') {
                        TokenKind::Le
                    } else {
                        TokenKind::Lt
                    }
                }
                '>' => {
                    self.advance();
                    if self.match_char('=') {
                        TokenKind::Ge
                    } else {
                        TokenKind::Gt
                    }
                }
                '&' => {
                    self.advance();
                    if self.match_char('&') {
                        TokenKind::AndAnd
                    } else {
                        return Err(self.error("expected '&&'"));
                    }
                }
                '|' => {
                    self.advance();
                    if self.match_char('|') {
                        TokenKind::OrOr
                    } else {
                        return Err(self.error("expected '||'"));
                    }
                }
                '"' => self.scan_string()?,
                '0'..='9' => self.scan_number()?,
                'a'..='z' | 'A'..='Z' | '_' => self.scan_identifier(),
                '@' => {
                    self.advance();
                    TokenKind::At
                }
                '`' => self.scan_escaped_identifier()?,
                _ => return Err(self.error(&format!("unexpected character '{}'", ch))),
            };

            tokens.push(Token::new(kind, span));
        }

        Ok(tokens)
    }

    fn peek(&mut self) -> Option<(usize, char)> {
        self.chars.peek().copied()
    }

    fn advance(&mut self) -> Option<(usize, char)> {
        let result = self.chars.next();
        if let Some((_, ch)) = result {
            if ch == '\n' {
                self.line += 1;
                self.column = 1;
            } else {
                self.column += 1;
            }
        }
        result
    }

    fn match_char(&mut self, expected: char) -> bool {
        if self.peek().map(|(_, c)| c) == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.peek() {
                Some((_, ' ' | '\t' | '\r' | '\n')) => {
                    self.advance();
                }
                Some((_, '/')) => {
                    // Check for comment
                    let mut chars = self.chars.clone();
                    chars.next(); // consume '/'
                    if chars.peek().map(|(_, c)| *c) == Some('/') {
                        // Line comment
                        self.advance(); // '/'
                        self.advance(); // '/'
                        while let Some((_, ch)) = self.peek() {
                            if ch == '\n' {
                                break;
                            }
                            self.advance();
                        }
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    fn scan_number(&mut self) -> Result<TokenKind, String> {
        let start = self.peek().map(|(i, _)| i).unwrap_or(0);
        let mut is_float = false;

        while let Some((_, ch)) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        // Check for decimal point
        if let Some((_, '.')) = self.peek() {
            // Look ahead to see if it's followed by a digit
            let mut chars = self.chars.clone();
            chars.next(); // consume '.'
            if let Some((_, ch)) = chars.peek()
                && ch.is_ascii_digit()
            {
                is_float = true;
                self.advance(); // consume '.'
                while let Some((_, ch)) = self.peek() {
                    if ch.is_ascii_digit() {
                        self.advance();
                    } else {
                        break;
                    }
                }
            }
        }

        let end = self.peek().map(|(i, _)| i).unwrap_or(self.source.len());
        let num_str = &self.source[start..end];

        if is_float {
            let value: f64 = num_str
                .parse()
                .map_err(|_| self.error(&format!("invalid float '{}'", num_str)))?;
            Ok(TokenKind::Float(value))
        } else {
            let value: i64 = num_str
                .parse()
                .map_err(|_| self.error(&format!("invalid number '{}'", num_str)))?;
            Ok(TokenKind::Int(value))
        }
    }

    fn scan_string(&mut self) -> Result<TokenKind, String> {
        self.advance(); // consume opening quote

        let mut value = String::new();

        loop {
            match self.peek() {
                None => return Err(self.error("unterminated string")),
                Some((_, '"')) => {
                    self.advance();
                    break;
                }
                Some((_, '\\')) => {
                    self.advance();
                    match self.peek() {
                        Some((_, 'n')) => {
                            self.advance();
                            value.push('\n');
                        }
                        Some((_, 't')) => {
                            self.advance();
                            value.push('\t');
                        }
                        Some((_, 'r')) => {
                            self.advance();
                            value.push('\r');
                        }
                        Some((_, '\\')) => {
                            self.advance();
                            value.push('\\');
                        }
                        Some((_, '"')) => {
                            self.advance();
                            value.push('"');
                        }
                        Some((_, ch)) => {
                            return Err(self.error(&format!("invalid escape sequence '\\{}'", ch)));
                        }
                        None => return Err(self.error("unterminated string")),
                    }
                }
                Some((_, '\n')) => {
                    return Err(self.error("unterminated string (newline in string)"));
                }
                Some((_, ch)) => {
                    self.advance();
                    value.push(ch);
                }
            }
        }

        Ok(TokenKind::Str(value))
    }

    fn scan_escaped_identifier(&mut self) -> Result<TokenKind, String> {
        self.advance(); // consume opening backtick

        let start = self.peek().map(|(i, _)| i).unwrap_or(self.source.len());

        while let Some((_, ch)) = self.peek() {
            if ch == '`' {
                break;
            }
            if ch == '\n' {
                return Err(self.error("unterminated escaped identifier (newline in identifier)"));
            }
            if !ch.is_ascii_alphanumeric() && ch != '_' {
                return Err(
                    self.error(&format!("invalid character '{}' in escaped identifier", ch))
                );
            }
            self.advance();
        }

        let end = self.peek().map(|(i, _)| i).unwrap_or(self.source.len());
        let ident = &self.source[start..end];

        if ident.is_empty() {
            return Err(self.error("empty escaped identifier"));
        }

        match self.peek() {
            Some((_, '`')) => {
                self.advance(); // consume closing backtick
            }
            _ => return Err(self.error("unterminated escaped identifier")),
        }

        Ok(TokenKind::Ident(ident.to_string()))
    }

    fn scan_identifier(&mut self) -> TokenKind {
        let start = self.peek().map(|(i, _)| i).unwrap_or(0);

        while let Some((_, ch)) = self.peek() {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.peek().map(|(i, _)| i).unwrap_or(self.source.len());
        let ident = &self.source[start..end];

        match ident {
            "let" => TokenKind::Let,
            "var" => TokenKind::Var,
            "fun" => TokenKind::Fun,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "nil" => TokenKind::Nil,
            "try" => TokenKind::Try,
            "catch" => TokenKind::Catch,
            "throw" => TokenKind::Throw,
            "import" => TokenKind::Import,
            "struct" => TokenKind::Struct,
            "impl" => TokenKind::Impl,
            "asm" => TokenKind::Asm,
            "type" => TokenKind::Type,
            "new" => TokenKind::New,
            "const" => TokenKind::Const,
            _ => TokenKind::Ident(ident.to_string()),
        }
    }

    fn error(&self, message: &str) -> String {
        format!(
            "error: {}\n  --> {}:{}:{}",
            message, self.filename, self.line, self.column
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_tokens() {
        let source = "let x = 42;";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens.len(), 6);
        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[1].kind, TokenKind::Ident("x".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Eq);
        assert_eq!(tokens[3].kind, TokenKind::Int(42));
        assert_eq!(tokens[4].kind, TokenKind::Semi);
        assert_eq!(tokens[5].kind, TokenKind::Eof);
    }

    #[test]
    fn test_operators() {
        let source = "+ - * / % == != < <= > >= && || !";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        let expected = vec![
            TokenKind::Plus,
            TokenKind::Minus,
            TokenKind::Star,
            TokenKind::Slash,
            TokenKind::Percent,
            TokenKind::EqEq,
            TokenKind::NotEq,
            TokenKind::Lt,
            TokenKind::Le,
            TokenKind::Gt,
            TokenKind::Ge,
            TokenKind::AndAnd,
            TokenKind::OrOr,
            TokenKind::Bang,
            TokenKind::Eof,
        ];

        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(&tokens[i].kind, exp, "mismatch at index {}", i);
        }
    }

    #[test]
    fn test_keywords() {
        let source = "let var fun if else while return true false";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        let expected = vec![
            TokenKind::Let,
            TokenKind::Var,
            TokenKind::Fun,
            TokenKind::If,
            TokenKind::Else,
            TokenKind::While,
            TokenKind::Return,
            TokenKind::True,
            TokenKind::False,
            TokenKind::Eof,
        ];

        for (i, exp) in expected.iter().enumerate() {
            assert_eq!(&tokens[i].kind, exp, "mismatch at index {}", i);
        }
    }

    #[test]
    fn test_line_comment() {
        let source = "let x = 1; // this is a comment\nlet y = 2;";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Let);
        assert_eq!(tokens[1].kind, TokenKind::Ident("x".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Eq);
        assert_eq!(tokens[3].kind, TokenKind::Int(1));
        assert_eq!(tokens[4].kind, TokenKind::Semi);
        assert_eq!(tokens[5].kind, TokenKind::Let);
        assert_eq!(tokens[6].kind, TokenKind::Ident("y".to_string()));
    }

    #[test]
    fn test_function_definition() {
        let source = "fun add(a, b) { return a + b; }";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Fun);
        assert_eq!(tokens[1].kind, TokenKind::Ident("add".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::LParen);
        assert_eq!(tokens[3].kind, TokenKind::Ident("a".to_string()));
        assert_eq!(tokens[4].kind, TokenKind::Comma);
        assert_eq!(tokens[5].kind, TokenKind::Ident("b".to_string()));
        assert_eq!(tokens[6].kind, TokenKind::RParen);
        assert_eq!(tokens[7].kind, TokenKind::LBrace);
    }

    #[test]
    fn test_float_literals() {
        let source = "3.14 0.5 42.0";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Float(3.14));
        assert_eq!(tokens[1].kind, TokenKind::Float(0.5));
        assert_eq!(tokens[2].kind, TokenKind::Float(42.0));
    }

    #[test]
    fn test_string_literals() {
        let source = r#""hello" "world" "line1\nline2""#;
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Str("hello".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Str("world".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Str("line1\nline2".to_string()));
    }

    #[test]
    fn test_nil_keyword() {
        let source = "let x = nil;";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[3].kind, TokenKind::Nil);
    }

    #[test]
    fn test_array_syntax() {
        let source = "[1, 2, 3]";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::LBracket);
        assert_eq!(tokens[1].kind, TokenKind::Int(1));
        assert_eq!(tokens[2].kind, TokenKind::Comma);
        assert_eq!(tokens[3].kind, TokenKind::Int(2));
        assert_eq!(tokens[4].kind, TokenKind::Comma);
        assert_eq!(tokens[5].kind, TokenKind::Int(3));
        assert_eq!(tokens[6].kind, TokenKind::RBracket);
    }

    #[test]
    fn test_object_syntax() {
        let source = "{ x: 10, y: 20 }";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::LBrace);
        assert_eq!(tokens[1].kind, TokenKind::Ident("x".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::Colon);
        assert_eq!(tokens[3].kind, TokenKind::Int(10));
        assert_eq!(tokens[4].kind, TokenKind::Comma);
    }

    #[test]
    fn test_for_in_syntax() {
        let source = "for x in arr { }";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::For);
        assert_eq!(tokens[1].kind, TokenKind::Ident("x".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::In);
        assert_eq!(tokens[3].kind, TokenKind::Ident("arr".to_string()));
    }

    #[test]
    fn test_try_catch_throw() {
        let source = "try { throw x; } catch e { }";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Try);
        assert_eq!(tokens[2].kind, TokenKind::Throw);
        assert_eq!(tokens[6].kind, TokenKind::Catch);
    }

    #[test]
    fn test_dot_operator() {
        let source = "obj.field";
        let mut lexer = Lexer::new("test.mc", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Ident("obj".to_string()));
        assert_eq!(tokens[1].kind, TokenKind::Dot);
        assert_eq!(tokens[2].kind, TokenKind::Ident("field".to_string()));
    }
}
