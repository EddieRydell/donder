//! Semantic tokens: the only highlighting, so every editor colors Donder
//! from the language's own lexer.
use donder_language::analysis::{SemanticToken, TokenClass};
use lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend};

use crate::document::Document;

const TYPES: [SemanticTokenType; 12] = [
    SemanticTokenType::COMMENT,
    SemanticTokenType::KEYWORD,
    SemanticTokenType::TYPE,
    SemanticTokenType::NUMBER,
    SemanticTokenType::STRING,
    SemanticTokenType::OPERATOR,
    SemanticTokenType::NAMESPACE,
    SemanticTokenType::FUNCTION,
    SemanticTokenType::PARAMETER,
    SemanticTokenType::VARIABLE,
    SemanticTokenType::PROPERTY,
    SemanticTokenType::ENUM_MEMBER,
];

const MODIFIERS: [SemanticTokenModifier; 3] = [
    SemanticTokenModifier::DECLARATION,
    SemanticTokenModifier::DEFAULT_LIBRARY,
    SemanticTokenModifier::READONLY,
];

pub(crate) fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.to_vec(),
        token_modifiers: MODIFIERS.to_vec(),
    }
}

/// A modifier's bit, by its place in the legend.
fn modifier(modifier: &SemanticTokenModifier) -> u32 {
    let index = MODIFIERS
        .iter()
        .position(|listed| listed == modifier)
        .unwrap_or_else(|| unreachable!("the legend lists every modifier"));
    1 << index
}

/// A class's token type, by its place in the legend, and modifiers.
fn encoding(class: TokenClass) -> (u32, u32) {
    let (ty, modifiers) = match class {
        TokenClass::Comment => (SemanticTokenType::COMMENT, 0),
        TokenClass::Keyword => (SemanticTokenType::KEYWORD, 0),
        TokenClass::Type => (SemanticTokenType::TYPE, 0),
        TokenClass::Number => (SemanticTokenType::NUMBER, 0),
        TokenClass::String => (SemanticTokenType::STRING, 0),
        TokenClass::Operator => (SemanticTokenType::OPERATOR, 0),
        TokenClass::Namespace => (SemanticTokenType::NAMESPACE, 0),
        TokenClass::Function => (SemanticTokenType::FUNCTION, 0),
        TokenClass::Parameter => (SemanticTokenType::PARAMETER, 0),
        TokenClass::Variable => (SemanticTokenType::VARIABLE, 0),
        TokenClass::Property => (SemanticTokenType::PROPERTY, 0),
        TokenClass::EnumMember => (SemanticTokenType::ENUM_MEMBER, 0),
        TokenClass::Builtin => (
            SemanticTokenType::FUNCTION,
            modifier(&SemanticTokenModifier::DEFAULT_LIBRARY),
        ),
        TokenClass::Context => (
            SemanticTokenType::VARIABLE,
            modifier(&SemanticTokenModifier::DEFAULT_LIBRARY)
                | modifier(&SemanticTokenModifier::READONLY),
        ),
    };
    let index = TYPES
        .iter()
        .position(|listed| *listed == ty)
        .unwrap_or_else(|| unreachable!("the legend lists every token type"));
    (index as u32, modifiers)
}

/// `tokens`, in source order, as LSP encodes them: each relative to the one
/// before, the first relative to the document start.
pub(crate) fn encode(document: &Document, tokens: &[SemanticToken]) -> SemanticTokens {
    let mut data = Vec::with_capacity(tokens.len());
    let (mut previous_line, mut previous_start) = (0, 0);
    for token in tokens {
        let start = document.position(token.span.start);
        let end = document.position(token.span.end);
        // Tokens never span lines; a stray one is skipped rather than split.
        if end.line != start.line || end.character <= start.character {
            continue;
        }
        let (token_type, mut modifiers) = encoding(token.class);
        if token.declaration {
            modifiers |= modifier(&SemanticTokenModifier::DECLARATION);
        }
        let delta_line = start.line - previous_line;
        let delta_start = if delta_line == 0 {
            start.character - previous_start
        } else {
            start.character
        };
        data.push(lsp_types::SemanticToken {
            delta_line,
            delta_start,
            length: end.character - start.character,
            token_type,
            token_modifiers_bitset: modifiers,
        });
        previous_line = start.line;
        previous_start = start.character;
    }
    SemanticTokens {
        result_id: None,
        data,
    }
}
