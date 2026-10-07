use super::syntax::lexer::TextSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub span: TextSpan,
    pub message: String,
    /// Text that replaces `span` to fix the error, when one spelling is right.
    pub fix: Option<String>,
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
        self.fix = Some(replacement.into());
        self
    }
}
