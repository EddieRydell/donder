//! One document's text and what the server derives from it. Each derivation is
//! computed the first time a request needs it and kept until the text changes,
//! so requests on a large document do not lex or parse it again.
use std::sync::OnceLock;

use donder_language::analysis::{
    DataToken, ScriptAnalysis, SemanticToken, analyze_script, data_token_stream, data_tokens,
};
use donder_language::compiler::{Diagnostic, TextSpan};
use donder_language::data::DataDocument;
use lsp_types::{Position, Range};

use crate::text::LineIndex;
use crate::workspace::Kind;

pub(crate) struct Document {
    kind: Kind,
    text: String,
    lines: OnceLock<LineIndex>,
    tree: OnceLock<(DataDocument, Vec<Diagnostic>)>,
    stream: OnceLock<Vec<DataToken>>,
    tokens: OnceLock<Vec<SemanticToken>>,
    script: OnceLock<ScriptAnalysis>,
}

impl Document {
    pub(crate) fn new(kind: Kind, text: String) -> Self {
        Self {
            kind,
            text,
            lines: OnceLock::new(),
            tree: OnceLock::new(),
            stream: OnceLock::new(),
            tokens: OnceLock::new(),
            script: OnceLock::new(),
        }
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn lines(&self) -> &LineIndex {
        self.lines.get_or_init(|| LineIndex::new(&self.text))
    }

    pub(crate) fn position(&self, offset: usize) -> Position {
        self.lines().position(&self.text, offset)
    }

    pub(crate) fn offset(&self, position: Position) -> usize {
        self.lines().offset(&self.text, position)
    }

    pub(crate) fn range(&self, span: TextSpan) -> Range {
        self.lines().range(&self.text, span)
    }

    /// The data syntax tree and its syntax errors.
    pub(crate) fn parsed(&self) -> &(DataDocument, Vec<Diagnostic>) {
        self.tree
            .get_or_init(|| donder_language::data::parse(&self.text))
    }

    pub(crate) fn tree(&self) -> &DataDocument {
        &self.parsed().0
    }

    /// The data tokens, without trivia.
    pub(crate) fn stream(&self) -> &[DataToken] {
        self.stream.get_or_init(|| data_token_stream(&self.text))
    }

    pub(crate) fn script(&self) -> &ScriptAnalysis {
        self.script.get_or_init(|| analyze_script(&self.text))
    }

    /// The semantic tokens, in source order.
    pub(crate) fn tokens(&self) -> &[SemanticToken] {
        self.tokens.get_or_init(|| {
            let mut tokens = match self.kind {
                Kind::Data => data_tokens(&self.text),
                Kind::Script => self.script().tokens.clone(),
                Kind::Other => Vec::new(),
            };
            tokens.sort_by_key(|token| token.span.start);
            tokens
        })
    }

    /// The semantic token covering `offset`.
    pub(crate) fn token_at(&self, offset: usize) -> Option<&SemanticToken> {
        let tokens = self.tokens();
        let after = tokens.partition_point(|token| token.span.start <= offset);
        tokens[..after]
            .iter()
            .rev()
            .take_while(|token| token.span.end >= offset)
            .last()
    }
}
