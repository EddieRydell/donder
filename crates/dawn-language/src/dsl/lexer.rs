pub fn lex(source: &str) -> Vec<Token> {
    Lexer::new(source).collect()
}

pub struct Lexer<'source> {
    source: &'source str,
    cursor: usize,
    emitted_eof: bool,
}

impl<'source> Lexer<'source> {
    pub fn new(source: &'source str) -> Self {
        Self {
            source,
            cursor: 0,
            emitted_eof: false,
        }
    }

    fn skip_whitespace_and_comments(&mut self) -> Option<Token> {
        loop {
            while self.peek().is_some_and(char::is_whitespace) {
                self.bump();
            }

            if self.remaining().starts_with("//") {
                self.cursor += 2;
                while self.peek().is_some_and(|candidate| candidate != '\n') {
                    self.bump();
                }
                continue;
            }

            if self.remaining().starts_with("/*") {
                let start = self.cursor;
                self.cursor += 2;

                while !self.is_at_end() && !self.remaining().starts_with("*/") {
                    self.bump();
                }

                if self.is_at_end() {
                    return Some(Token {
                        kind: TokenKind::Error(LexErrorKind::UnterminatedBlockComment),
                        span: TextSpan {
                            start,
                            end: self.cursor,
                        },
                    });
                }

                self.cursor += 2;
                continue;
            }

            return None;
        }
    }

    fn lex_identifier_or_keyword(&mut self, start: usize) -> Token {
        while self.peek().is_some_and(is_identifier_continue) {
            self.bump();
        }

        let text = &self.source[start..self.cursor];
        let kind = match text {
            "import" => TokenKind::Keyword(Keyword::Import),
            "from" => TokenKind::Keyword(Keyword::From),
            "effect" => TokenKind::Keyword(Keyword::Effect),
            "operator" => TokenKind::Keyword(Keyword::Operator),
            "input" => TokenKind::Keyword(Keyword::Input),
            "fixed" => TokenKind::Keyword(Keyword::Fixed),
            "param" => TokenKind::Keyword(Keyword::Param),
            "int" => TokenKind::Keyword(Keyword::Int),
            "float" => TokenKind::Keyword(Keyword::Float),
            "bool" => TokenKind::Keyword(Keyword::Bool),
            "color" => TokenKind::Keyword(Keyword::Color),
            "curve" => TokenKind::Keyword(Keyword::Curve),
            "array" => TokenKind::Keyword(Keyword::Array),
            "enum" => TokenKind::Keyword(Keyword::Enum),
            "void" => TokenKind::Keyword(Keyword::Void),
            "return" => TokenKind::Keyword(Keyword::Return),
            "if" => TokenKind::Keyword(Keyword::If),
            "else" => TokenKind::Keyword(Keyword::Else),
            "for" => TokenKind::Keyword(Keyword::For),
            "true" => TokenKind::Keyword(Keyword::True),
            "false" => TokenKind::Keyword(Keyword::False),
            _ => TokenKind::Identifier,
        };

        self.token(kind, start)
    }

    fn lex_number(&mut self, start: usize) -> Token {
        while self
            .peek()
            .is_some_and(|candidate| candidate.is_ascii_digit())
        {
            self.bump();
        }

        if self.peek() == Some('.')
            && self
                .peek_next()
                .is_some_and(|candidate| candidate.is_ascii_digit())
        {
            self.bump();
            while self
                .peek()
                .is_some_and(|candidate| candidate.is_ascii_digit())
            {
                self.bump();
            }
            return self.token(TokenKind::FloatLiteral, start);
        }

        self.token(TokenKind::IntegerLiteral, start)
    }

    fn lex_string(&mut self, start: usize) -> Token {
        while let Some(candidate) = self.peek() {
            match candidate {
                '"' => {
                    self.bump();
                    return self.token(TokenKind::StringLiteral, start);
                }
                '\\' => {
                    self.bump();
                    if self.peek().is_some() {
                        self.bump();
                    }
                }
                _ => {
                    self.bump();
                }
            }
        }

        self.token(TokenKind::Error(LexErrorKind::UnterminatedString), start)
    }

    fn lex_color(&mut self, start: usize) -> Token {
        let mut digits = 0;
        while self
            .peek()
            .is_some_and(|candidate| candidate.is_ascii_hexdigit())
            && digits < 6
        {
            self.bump();
            digits += 1;
        }

        if digits == 6 {
            self.token(TokenKind::ColorLiteral, start)
        } else {
            self.token(TokenKind::Error(LexErrorKind::InvalidColorLiteral), start)
        }
    }

    fn token(&self, kind: TokenKind, start: usize) -> Token {
        Token {
            kind,
            span: TextSpan {
                start,
                end: self.cursor,
            },
        }
    }

    fn remaining(&self) -> &str {
        &self.source[self.cursor..]
    }

    fn is_at_end(&self) -> bool {
        self.cursor >= self.source.len()
    }

    fn peek(&self) -> Option<char> {
        self.remaining().chars().next()
    }

    fn peek_next(&self) -> Option<char> {
        self.remaining().chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let candidate = self.peek()?;
        self.cursor += candidate.len_utf8();
        Some(candidate)
    }
}

impl Iterator for Lexer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Self::Item> {
        if self.emitted_eof {
            return None;
        }

        if let Some(token) = self.skip_whitespace_and_comments() {
            return Some(token);
        }

        let start = self.cursor;

        let Some(candidate) = self.bump() else {
            self.emitted_eof = true;
            return Some(Token {
                kind: TokenKind::Eof,
                span: TextSpan { start, end: start },
            });
        };

        let kind = match candidate {
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            '<' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::LessEqual
                } else {
                    TokenKind::LessThan
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::GreaterThan
                }
            }
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            '.' => TokenKind::Dot,
            '=' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::EqualEqual
                } else {
                    TokenKind::Equals
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.bump();
                    TokenKind::BangEqual
                } else {
                    TokenKind::Bang
                }
            }
            '&' => {
                if self.peek() == Some('&') {
                    self.bump();
                    TokenKind::AmpAmp
                } else {
                    TokenKind::Error(LexErrorKind::UnexpectedCharacter)
                }
            }
            '|' => {
                if self.peek() == Some('|') {
                    self.bump();
                    TokenKind::PipePipe
                } else {
                    TokenKind::Error(LexErrorKind::UnexpectedCharacter)
                }
            }
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '#' => return Some(self.lex_color(start)),
            '"' => return Some(self.lex_string(start)),
            candidate if is_identifier_start(candidate) => {
                return Some(self.lex_identifier_or_keyword(start));
            }
            candidate if candidate.is_ascii_digit() => return Some(self.lex_number(start)),
            _ => TokenKind::Error(LexErrorKind::UnexpectedCharacter),
        };

        Some(self.token(kind, start))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: TextSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TokenKind {
    Identifier,
    IntegerLiteral,
    FloatLiteral,
    ColorLiteral,
    StringLiteral,
    Keyword(Keyword),
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    LessThan,
    GreaterThan,
    LessEqual,
    GreaterEqual,
    Colon,
    Semicolon,
    Comma,
    Dot,
    Equals,
    EqualEqual,
    Bang,
    BangEqual,
    AmpAmp,
    PipePipe,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eof,
    Error(LexErrorKind),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Keyword {
    Import,
    From,
    Effect,
    Operator,
    Input,
    Param,
    Fixed,
    Int,
    Float,
    Bool,
    Color,
    Curve,
    Array,
    Enum,
    Void,
    Return,
    If,
    Else,
    For,
    True,
    False,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LexErrorKind {
    UnexpectedCharacter,
    InvalidColorLiteral,
    UnterminatedString,
    UnterminatedBlockComment,
}

fn is_identifier_start(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphabetic()
}

fn is_identifier_continue(candidate: char) -> bool {
    candidate == '_' || candidate.is_ascii_alphanumeric()
}
