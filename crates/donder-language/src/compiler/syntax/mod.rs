//! Source text to syntax tree.
pub(crate) mod ast;
pub(crate) mod lexer;
mod parser;

pub(crate) use parser::{parse, parse_partial, partial_declaration_spans};
