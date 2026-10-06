//! Tokens of the effect language. Type names, reducers and builtins are
//! identifiers; only declaration and statement words are reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}

impl TextSpan {
    pub(crate) fn to(self, end: Self) -> Self {
        Self {
            start: self.start,
            end: end.end,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Token {
    pub(crate) kind: TokenKind,
    pub(crate) span: TextSpan,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TokenKind {
    Identifier,
    Integer,
    Float,
    Color,
    Keyword(Keyword),
    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Colon,
    Semicolon,
    Comma,
    Dot,
    DotDot,
    DotDotEqual,
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
    Error(LexError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Keyword {
    Effect,
    Operator,
    Let,
    Guard,
    If,
    Else,
    For,
    In,
    True,
    False,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum LexError {
    UnexpectedCharacter,
    InvalidColor,
    UnterminatedComment,
}

pub(crate) fn lex(source: &str) -> Vec<Token> {
    let mut lexer = Lexer { source, cursor: 0 };
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next();
        tokens.push(token);
        if token.kind == TokenKind::Eof {
            return tokens;
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
}

impl Lexer<'_> {
    fn next(&mut self) -> Token {
        if let Some(error) = self.skip_trivia() {
            return error;
        }
        let start = self.cursor;
        let Some(character) = self.bump() else {
            return self.token(TokenKind::Eof, start);
        };
        let kind = match character {
            '{' => TokenKind::LeftBrace,
            '}' => TokenKind::RightBrace,
            '(' => TokenKind::LeftParen,
            ')' => TokenKind::RightParen,
            '[' => TokenKind::LeftBracket,
            ']' => TokenKind::RightBracket,
            ':' => TokenKind::Colon,
            ';' => TokenKind::Semicolon,
            ',' => TokenKind::Comma,
            '+' => TokenKind::Plus,
            '-' => TokenKind::Minus,
            '*' => TokenKind::Star,
            '/' => TokenKind::Slash,
            '%' => TokenKind::Percent,
            '<' => self.pair('=', TokenKind::LessEqual, TokenKind::Less),
            '>' => self.pair('=', TokenKind::GreaterEqual, TokenKind::Greater),
            '=' => self.pair('=', TokenKind::EqualEqual, TokenKind::Equals),
            '!' => self.pair('=', TokenKind::BangEqual, TokenKind::Bang),
            '&' => self.pair(
                '&',
                TokenKind::AmpAmp,
                TokenKind::Error(LexError::UnexpectedCharacter),
            ),
            '|' => self.pair(
                '|',
                TokenKind::PipePipe,
                TokenKind::Error(LexError::UnexpectedCharacter),
            ),
            '.' if self.eat('.') => {
                if self.eat('=') {
                    TokenKind::DotDotEqual
                } else {
                    TokenKind::DotDot
                }
            }
            '.' => TokenKind::Dot,
            '#' => {
                let digits = self.take_while(|character| character.is_ascii_hexdigit());
                if digits == 6 {
                    TokenKind::Color
                } else {
                    TokenKind::Error(LexError::InvalidColor)
                }
            }
            character if character == '_' || character.is_ascii_alphabetic() => {
                self.take_while(|character| character == '_' || character.is_ascii_alphanumeric());
                keyword(&self.source[start..self.cursor])
                    .map_or(TokenKind::Identifier, TokenKind::Keyword)
            }
            character if character.is_ascii_digit() => {
                self.take_while(|character| character.is_ascii_digit());
                // A range like `0..1` keeps its dots.
                let fraction = self.peek() == Some('.')
                    && self.peek_second().is_some_and(|next| next.is_ascii_digit());
                if fraction {
                    self.bump();
                    self.take_while(|character| character.is_ascii_digit());
                    TokenKind::Float
                } else {
                    TokenKind::Integer
                }
            }
            _ => TokenKind::Error(LexError::UnexpectedCharacter),
        };
        self.token(kind, start)
    }

    fn skip_trivia(&mut self) -> Option<Token> {
        loop {
            self.take_while(char::is_whitespace);
            let rest = &self.source[self.cursor..];
            if rest.starts_with("//") {
                self.take_while(|character| character != '\n');
            } else if rest.starts_with("/*") {
                let start = self.cursor;
                match rest.get(2..).and_then(|comment| comment.find("*/")) {
                    Some(end) => self.cursor += end + 4,
                    None => {
                        self.cursor = self.source.len();
                        let error = TokenKind::Error(LexError::UnterminatedComment);
                        return Some(self.token(error, start));
                    }
                }
            } else {
                return None;
            }
        }
    }

    fn pair(&mut self, next: char, matched: TokenKind, single: TokenKind) -> TokenKind {
        if self.eat(next) { matched } else { single }
    }

    fn eat(&mut self, expected: char) -> bool {
        let matched = self.peek() == Some(expected);
        if matched {
            self.bump();
        }
        matched
    }

    fn take_while(&mut self, accept: impl Fn(char) -> bool) -> usize {
        let mut count = 0;
        while self.peek().is_some_and(&accept) {
            self.bump();
            count += 1;
        }
        count
    }

    fn peek(&self) -> Option<char> {
        self.source[self.cursor..].chars().next()
    }

    fn peek_second(&self) -> Option<char> {
        self.source[self.cursor..].chars().nth(1)
    }

    fn bump(&mut self) -> Option<char> {
        let character = self.peek()?;
        self.cursor += character.len_utf8();
        Some(character)
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
}

fn keyword(text: &str) -> Option<Keyword> {
    Some(match text {
        "effect" => Keyword::Effect,
        "operator" => Keyword::Operator,
        "let" => Keyword::Let,
        "guard" => Keyword::Guard,
        "if" => Keyword::If,
        "else" => Keyword::Else,
        "for" => Keyword::For,
        "in" => Keyword::In,
        "true" => Keyword::True,
        "false" => Keyword::False,
        _ => return None,
    })
}

/// Whether `text` is one identifier token, as used for authored names.
pub(crate) fn is_identifier(text: &str) -> bool {
    let tokens = lex(text);
    matches!(tokens.as_slice(), [token, end] if token.kind == TokenKind::Identifier && end.kind == TokenKind::Eof)
}
