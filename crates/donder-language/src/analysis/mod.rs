//! Editor analysis of single documents: semantic tokens, symbols, references,
//! hover text, completion and signature help, for the language server.
//! Positions are byte offsets; the server converts them to its encoding.
//! Script analysis works on what parses, and completion works on tokens, so
//! both keep answering while a document is half-typed.
mod data;
mod script;

pub use data::{DataToken, DataTokenKind, data_token_stream, data_tokens};
pub use script::{
    ScriptAnalysis, ScriptReference, ScriptTarget, analyze_script, script_completions,
    script_signature, type_text,
};

use crate::compiler::TextSpan;
use donder_runtime_types::Type;

/// What a token is, for highlighting.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum TokenClass {
    Comment,
    Keyword,
    Type,
    Number,
    String,
    Operator,
    Namespace,
    Function,
    Parameter,
    Variable,
    Property,
    EnumMember,
    /// A builtin function or reducer.
    Builtin,
    /// A context value: `time`, `pixel.index`, `PI`.
    Context,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SemanticToken {
    pub span: TextSpan,
    pub class: TokenClass,
    /// The token names the object it declares.
    pub declaration: bool,
}

/// What a name means, for hover, navigation and rename.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SymbolKind {
    Effect,
    Operator,
    Function,
    Param,
    Input,
    Argument,
    Let,
    Index,
    EnumOption,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// The declaring name.
    pub span: TextSpan,
    /// The whole declaration, for outlines.
    pub extent: TextSpan,
    /// A one-line summary, such as the declaration's source.
    pub detail: String,
    pub description: Option<String>,
    /// The type of a parameter, argument, `let` or loop index, when it is
    /// known.
    pub ty: Option<Type>,
    /// The enclosing symbol.
    pub parent: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum CompletionKind {
    Keyword,
    Type,
    Function,
    Variable,
    Field,
    EnumMember,
    Reference,
    Snippet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
    pub detail: Option<String>,
    pub documentation: Option<String>,
    /// Text to insert instead of the label.
    pub insert: Option<String>,
}

impl Completion {
    pub fn new(label: impl Into<String>, kind: CompletionKind) -> Self {
        Self {
            label: label.into(),
            kind,
            detail: None,
            documentation: None,
            insert: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn documentation(mut self, documentation: impl Into<String>) -> Self {
        self.documentation = Some(documentation.into());
        self
    }

    pub fn insert(mut self, insert: impl Into<String>) -> Self {
        self.insert = Some(insert.into());
        self
    }
}

/// A call's signatures and the argument the cursor is in.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignatureHelp {
    pub signatures: Vec<Signature>,
    pub active_parameter: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Signature {
    pub label: String,
    /// Each parameter's label, a substring of `label`.
    pub parameters: Vec<String>,
    pub documentation: Option<String>,
}

pub(crate) fn contains(span: TextSpan, offset: usize) -> bool {
    span.start <= offset && offset <= span.end
}
