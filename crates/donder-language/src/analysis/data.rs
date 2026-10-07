//! Data document tokens, classified from the tokens alone: a data document's
//! grammar says what each name is without resolving it.
use super::{SemanticToken, TokenClass};
use crate::data::tree::is_pascal_case;
use crate::dsl::syntax::lexer::{Keyword, LexMode, TokenKind, lex_mode};

pub fn data_tokens(source: &str) -> Vec<SemanticToken> {
    let tokens = lex_mode(source, LexMode::Data);
    let text = |index: usize| {
        let span = tokens[index].span;
        &source[span.start..span.end]
    };
    let aliases = tokens
        .windows(2)
        .enumerate()
        .filter(|(_, pair)| {
            pair[0].kind == TokenKind::Keyword(Keyword::Import)
                && pair[1].kind == TokenKind::Identifier
        })
        .map(|(index, _)| text(index + 1))
        .collect::<Vec<_>>();
    let mut result = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        let kind = |offset: isize| {
            index
                .checked_add_signed(offset)
                .and_then(|index| tokens.get(index))
                .map(|token| token.kind)
        };
        let mut declaration = false;
        let class = match token.kind {
            TokenKind::Keyword(_) => TokenClass::Keyword,
            TokenKind::Integer | TokenKind::Float | TokenKind::Duration | TokenKind::Distance => {
                TokenClass::Number
            }
            TokenKind::String | TokenKind::Path | TokenKind::Color => TokenClass::String,
            TokenKind::Minus
                if matches!(
                    kind(1),
                    Some(TokenKind::Integer | TokenKind::Float | TokenKind::Distance)
                ) =>
            {
                TokenClass::Number
            }
            TokenKind::Identifier => {
                let word = text(index);
                if is_pascal_case(word) {
                    TokenClass::Type
                } else if kind(1) == Some(TokenKind::Colon) {
                    TokenClass::Property
                } else if kind(-1) == Some(TokenKind::Keyword(Keyword::Import)) {
                    declaration = true;
                    TokenClass::Namespace
                } else if kind(-1) == Some(TokenKind::Identifier) && is_pascal_case(text(index - 1))
                {
                    // `Type name { ... }`: a declaration or an owned member.
                    declaration = true;
                    TokenClass::Variable
                } else if kind(-1) == Some(TokenKind::Colon)
                    && index >= 2
                    && text(index - 2) == "name"
                {
                    declaration = true;
                    TokenClass::Variable
                } else if kind(1) == Some(TokenKind::Dot)
                    && kind(-1) != Some(TokenKind::Dot)
                    && aliases.contains(&word)
                {
                    TokenClass::Namespace
                } else {
                    TokenClass::Variable
                }
            }
            _ => continue,
        };
        result.push(SemanticToken {
            span: token.span,
            class,
            declaration,
        });
    }
    result
}

/// A data token as completion reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DataTokenKind {
    Name,
    Keyword,
    Literal,
    Open(char),
    Close(char),
    Colon,
    Comma,
    Dot,
    Semicolon,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DataToken {
    pub kind: DataTokenKind,
    pub span: crate::dsl::TextSpan,
}

/// The tokens of a data document, without the end marker.
pub fn data_token_stream(source: &str) -> Vec<DataToken> {
    lex_mode(source, LexMode::Data)
        .into_iter()
        .filter(|token| token.kind != TokenKind::Eof)
        .map(|token| DataToken {
            span: token.span,
            kind: match token.kind {
                TokenKind::Identifier => DataTokenKind::Name,
                TokenKind::Keyword(_) => DataTokenKind::Keyword,
                TokenKind::Integer
                | TokenKind::Float
                | TokenKind::Duration
                | TokenKind::Distance
                | TokenKind::String
                | TokenKind::Path
                | TokenKind::Color => DataTokenKind::Literal,
                TokenKind::LeftBrace => DataTokenKind::Open('{'),
                TokenKind::LeftBracket => DataTokenKind::Open('['),
                TokenKind::LeftParen => DataTokenKind::Open('('),
                TokenKind::RightBrace => DataTokenKind::Close('}'),
                TokenKind::RightBracket => DataTokenKind::Close(']'),
                TokenKind::RightParen => DataTokenKind::Close(')'),
                TokenKind::Colon => DataTokenKind::Colon,
                TokenKind::Comma => DataTokenKind::Comma,
                TokenKind::Dot => DataTokenKind::Dot,
                TokenKind::Semicolon => DataTokenKind::Semicolon,
                _ => DataTokenKind::Other,
            },
        })
        .collect()
}
