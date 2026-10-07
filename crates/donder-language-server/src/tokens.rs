//! Semantic tokens: the only highlighting, so every editor colors Donder
//! from the language's own lexer.
use donder_language::analysis::{SemanticToken, TokenClass};
use lsp_types::{SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend};

use crate::text::LineIndex;

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

const DECLARATION: u32 = 1;
const DEFAULT_LIBRARY: u32 = 1 << 1;
const READONLY: u32 = 1 << 2;

pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: TYPES.to_vec(),
        token_modifiers: vec![
            SemanticTokenModifier::DECLARATION,
            SemanticTokenModifier::DEFAULT_LIBRARY,
            SemanticTokenModifier::READONLY,
        ],
    }
}

fn encoding(class: TokenClass) -> (u32, u32) {
    match class {
        TokenClass::Comment => (0, 0),
        TokenClass::Keyword => (1, 0),
        TokenClass::Type => (2, 0),
        TokenClass::Number => (3, 0),
        TokenClass::String => (4, 0),
        TokenClass::Operator => (5, 0),
        TokenClass::Namespace => (6, 0),
        TokenClass::Function => (7, 0),
        TokenClass::Parameter => (8, 0),
        TokenClass::Variable => (9, 0),
        TokenClass::Property => (10, 0),
        TokenClass::EnumMember => (11, 0),
        TokenClass::Builtin => (7, DEFAULT_LIBRARY),
        TokenClass::Context => (9, DEFAULT_LIBRARY | READONLY),
    }
}

pub fn encode(text: &str, tokens: &[SemanticToken]) -> SemanticTokens {
    let lines = LineIndex::new(text);
    let mut data = Vec::with_capacity(tokens.len());
    let (mut previous_line, mut previous_start) = (0, 0);
    let mut tokens = tokens.to_vec();
    tokens.sort_by_key(|token| token.span.start);
    for token in tokens {
        let start = lines.position(text, token.span.start);
        let end = lines.position(text, token.span.end);
        // Tokens never span lines; a stray one is skipped rather than split.
        if end.line != start.line || end.character <= start.character {
            continue;
        }
        let (token_type, mut modifiers) = encoding(token.class);
        if token.declaration {
            modifiers |= DECLARATION;
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
