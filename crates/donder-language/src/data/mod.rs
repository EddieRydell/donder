//! Data documents: the project, setups, layouts, patches, sequences, curves and
//! gradients, written in the Donder project language
//! (`docs/project_language.md`).
//!
//! [`parse`] turns source into a [`tree::DataDocument`] without knowing any
//! schema, recovering from errors so diagnostics stay useful while a document
//! is being typed. [`print`] writes a tree in the one canonical layout. Every
//! literal must already be in its canonical spelling, so printing a parsed
//! document changes only whitespace.
pub(crate) mod literal;
mod parser;
mod printer;
pub mod schema;
pub mod tree;

pub use literal::{canonical_distance, canonical_duration, canonical_float};
pub use parser::parse;
pub use printer::print;

/// Data documents and scripts share the effect language's file extension;
/// data adds `.data` before it.
pub const DATA_DOCUMENT_SUFFIX: &str = ".data.donder";
pub const SCRIPT_SUFFIX: &str = ".donder";

/// The longest line the printer writes, counting indentation.
pub const LINE_WIDTH: usize = 100;
