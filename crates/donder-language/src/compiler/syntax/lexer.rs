//! Tokens of Donder documents. Scripts (effects, operators and functions)
//! and data documents share one lexer; [`LexMode`] selects the few rules that
//! differ. Type names, reducers and builtins are identifiers; only
//! declaration and statement words are reserved.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct TextSpan {
    pub start: usize,
    pub end: usize,
}

impl TextSpan {
    pub fn to(self, end: Self) -> Self {
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
    /// `"..."`, with the quotes; escapes are decoded by the parser.
    String,
    /// Data only: a number of seconds directly followed by `s`.
    Duration,
    /// Data only: a number of meters directly followed by `m`.
    Distance,
    /// Data only: `<relative/path>`, with the brackets.
    Path,
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
    SlashSlash,
    Arrow,
    Percent,
    Eof,
    Error(LexError),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum Keyword {
    Effect,
    Operator,
    Fn,
    Let,
    Guard,
    If,
    Else,
    For,
    In,
    True,
    False,
    /// Data only, like `From` and `None`.
    Import,
    From,
    None,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) enum LexError {
    UnexpectedCharacter,
    InvalidColor,
    UnterminatedString,
    UnterminatedPath,
    /// `--` in a data document.
    Comment,
}

impl LexError {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::UnexpectedCharacter => "unexpected character",
            Self::InvalidColor => "a color literal has six hexadecimal digits, like #ff8800",
            Self::UnterminatedString => "a string ends with `\"` on the same line",
            Self::UnterminatedPath => "a path ends with `>` on the same line",
            Self::Comment => {
                "data documents have no comments; describe an object with its `description` field"
            }
        }
    }
}

/// Which document kind a source is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LexMode {
    /// Effects, operators and functions: `--` comments.
    Script,
    /// Data documents: no comments; durations, paths and the data keywords.
    Data,
}

pub(crate) fn lex(source: &str) -> Vec<Token> {
    lex_mode(source, LexMode::Script)
}

pub(crate) fn lex_mode(source: &str, mode: LexMode) -> Vec<Token> {
    lex_with_comments(source, mode).0
}

/// The tokens of `source` and the spans of its script comments.
pub(crate) fn lex_with_comments(source: &str, mode: LexMode) -> (Vec<Token>, Vec<TextSpan>) {
    let mut lexer = Lexer {
        source,
        cursor: 0,
        mode,
        comments: Vec::new(),
    };
    let mut tokens = Vec::new();
    loop {
        let token = lexer.next();
        tokens.push(token);
        if token.kind == TokenKind::Eof {
            return (tokens, lexer.comments);
        }
    }
}

struct Lexer<'a> {
    source: &'a str,
    cursor: usize,
    mode: LexMode,
    comments: Vec<TextSpan>,
}

impl Lexer<'_> {
    fn next(&mut self) -> Token {
        if let Some(comment) = self.skip_trivia() {
            return comment;
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
            '-' => self.pair('>', TokenKind::Arrow, TokenKind::Minus),
            '*' => TokenKind::Star,
            '/' => self.pair('/', TokenKind::SlashSlash, TokenKind::Slash),
            '%' => TokenKind::Percent,
            '<' if self.mode == LexMode::Data => {
                self.take_while(|character| !matches!(character, '<' | '>' | '\n' | '\r'));
                if self.eat('>') {
                    TokenKind::Path
                } else {
                    TokenKind::Error(LexError::UnterminatedPath)
                }
            }
            '<' => self.pair('=', TokenKind::LessEqual, TokenKind::Less),
            '"' => {
                let mut escaped = false;
                let mut closed = false;
                while let Some(character) = self.peek() {
                    if character == '\n' {
                        break;
                    }
                    self.bump();
                    match (escaped, character) {
                        (false, '"') => {
                            closed = true;
                            break;
                        }
                        (false, '\\') => escaped = true,
                        _ => escaped = false,
                    }
                }
                if closed {
                    TokenKind::String
                } else {
                    TokenKind::Error(LexError::UnterminatedString)
                }
            }
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
                keyword(&self.source[start..self.cursor], self.mode)
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
                }
                let unit = self.peek().filter(|unit| {
                    self.mode == LexMode::Data
                        && matches!(unit, 's' | 'm')
                        && !self
                            .peek_second()
                            .is_some_and(|next| next == '_' || next.is_ascii_alphanumeric())
                });
                if let Some(unit) = unit {
                    self.bump();
                    if unit == 's' {
                        TokenKind::Duration
                    } else {
                        TokenKind::Distance
                    }
                } else if fraction {
                    TokenKind::Float
                } else {
                    TokenKind::Integer
                }
            }
            _ => TokenKind::Error(LexError::UnexpectedCharacter),
        };
        self.token(kind, start)
    }

    /// Skip whitespace and, in scripts, `--` line comments. A comment in a
    /// data document is an error token covering the comment.
    fn skip_trivia(&mut self) -> Option<Token> {
        loop {
            self.take_while(char::is_whitespace);
            if !self.source[self.cursor..].starts_with("--") {
                return None;
            }
            let start = self.cursor;
            self.take_while(|character| character != '\n');
            if self.mode == LexMode::Data {
                return Some(self.token(TokenKind::Error(LexError::Comment), start));
            }
            self.comments.push(TextSpan {
                start,
                end: self.cursor,
            });
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

impl Keyword {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::Effect => "effect",
            Self::Operator => "operator",
            Self::Fn => "fn",
            Self::Let => "let",
            Self::Guard => "guard",
            Self::If => "if",
            Self::Else => "else",
            Self::For => "for",
            Self::In => "in",
            Self::True => "true",
            Self::False => "false",
            Self::Import => "import",
            Self::From => "from",
            Self::None => "none",
        }
    }
}

impl LexMode {
    /// The words this mode reserves. Data documents reserve only their own,
    /// so fields may be named `effect`, `input` or `in`.
    pub(crate) fn keywords(self) -> &'static [Keyword] {
        match self {
            Self::Script => &[
                Keyword::Effect,
                Keyword::Operator,
                Keyword::Fn,
                Keyword::Let,
                Keyword::Guard,
                Keyword::If,
                Keyword::Else,
                Keyword::For,
                Keyword::In,
                Keyword::True,
                Keyword::False,
            ],
            Self::Data => &[
                Keyword::Import,
                Keyword::From,
                Keyword::None,
                Keyword::True,
                Keyword::False,
            ],
        }
    }
}

fn keyword(text: &str, mode: LexMode) -> Option<Keyword> {
    mode.keywords()
        .iter()
        .copied()
        .find(|keyword| keyword.text() == text)
}

/// Whether `text` is one identifier token in `mode`, as used for authored
/// names: an identifier that is not one of the mode's keywords.
pub(crate) fn is_identifier(text: &str, mode: LexMode) -> bool {
    let tokens = lex_mode(text, mode);
    matches!(tokens.as_slice(), [token, end] if token.kind == TokenKind::Identifier && end.kind == TokenKind::Eof)
}
