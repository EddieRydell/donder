use super::syntax::lexer::TextSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub span: TextSpan,
    pub message: String,
    pub fix: Option<Fix>,
}

/// How to fix a diagnostic.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Fix {
    /// Text that replaces the diagnostic's span, when one spelling is right.
    Replace(String),
    /// A missing field, `name: value`, whose value is the field's empty one.
    /// It is written at byte `at`: before the field there, or after the
    /// value there when `after` is set.
    AddField {
        at: usize,
        field: String,
        after: bool,
    },
}

impl Diagnostic {
    pub(crate) fn new(span: TextSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
            fix: None,
        }
    }

    pub(crate) fn with_fix(mut self, replacement: impl Into<String>) -> Self {
        self.fix = Some(Fix::Replace(replacement.into()));
        self
    }
}
