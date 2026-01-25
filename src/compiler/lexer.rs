/// Token kinds for the mica language.
#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // Keywords
    Let,
    Mut,
    Fn,
    If,
    Else,
    While,
    Return,
    True,
    False,

    // Literals
    Int(i64),
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

    // Delimiters
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Semi,

    // Special
    Eof,
}

/// Source location information.
#[derive(Debug, Clone, Copy, PartialEq)]
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

/// The lexer for mica source code.
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
                '(' => { self.advance(); TokenKind::LParen }
                ')' => { self.advance(); TokenKind::RParen }
                '{' => { self.advance(); TokenKind::LBrace }
                '}' => { self.advance(); TokenKind::RBrace }
                ',' => { self.advance(); TokenKind::Comma }
                ';' => { self.advance(); TokenKind::Semi }
                '+' => { self.advance(); TokenKind::Plus }
                '-' => { self.advance(); TokenKind::Minus }
                '*' => { self.advance(); TokenKind::Star }
                '/' => { self.advance(); TokenKind::Slash }
                '%' => { self.advance(); TokenKind::Percent }
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
                '0'..='9' => self.scan_number()?,
                'a'..='z' | 'A'..='Z' | '_' => self.scan_identifier(),
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

        while let Some((_, ch)) = self.peek() {
            if ch.is_ascii_digit() {
                self.advance();
            } else {
                break;
            }
        }

        let end = self.peek().map(|(i, _)| i).unwrap_or(self.source.len());
        let num_str = &self.source[start..end];

        let value: i64 = num_str
            .parse()
            .map_err(|_| self.error(&format!("invalid number '{}'", num_str)))?;

        Ok(TokenKind::Int(value))
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
            "mut" => TokenKind::Mut,
            "fn" => TokenKind::Fn,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "return" => TokenKind::Return,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
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
        let mut lexer = Lexer::new("test.mica", source);
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
        let mut lexer = Lexer::new("test.mica", source);
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
        let source = "let mut fn if else while return true false";
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens().unwrap();

        let expected = vec![
            TokenKind::Let,
            TokenKind::Mut,
            TokenKind::Fn,
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
        let mut lexer = Lexer::new("test.mica", source);
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
        let source = "fn add(a, b) { return a + b; }";
        let mut lexer = Lexer::new("test.mica", source);
        let tokens = lexer.scan_tokens().unwrap();

        assert_eq!(tokens[0].kind, TokenKind::Fn);
        assert_eq!(tokens[1].kind, TokenKind::Ident("add".to_string()));
        assert_eq!(tokens[2].kind, TokenKind::LParen);
        assert_eq!(tokens[3].kind, TokenKind::Ident("a".to_string()));
        assert_eq!(tokens[4].kind, TokenKind::Comma);
        assert_eq!(tokens[5].kind, TokenKind::Ident("b".to_string()));
        assert_eq!(tokens[6].kind, TokenKind::RParen);
        assert_eq!(tokens[7].kind, TokenKind::LBrace);
    }
}
