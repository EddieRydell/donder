use super::lexer::TextSpan;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Diagnostic {
    pub span: TextSpan,
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn new(span: TextSpan, message: impl Into<String>) -> Self {
        Self {
            span,
            message: message.into(),
        }
    }
}
