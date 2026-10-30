use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::{Diagnostic as DslDiagnostic, compile_script};
use serde::{Deserialize, Serialize};
use specta::Type;

use crate::{ProjectRecovery, ProjectSession};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCheckReport {
    pub session: Option<ProjectSession>,
    pub recovery: ProjectRecovery,
    pub diagnostics: Vec<IoDiagnostic>,
    /// Every reference resolved before loading finished or failed.
    pub index: crate::ProjectIndex,
}

impl ProjectCheckReport {
    /// The loaded project, or why it did not load.
    pub fn into_session(self) -> Result<ProjectSession, Vec<IoDiagnostic>> {
        self.session.ok_or(self.diagnostics)
    }
}

/// A problem in a project document, as every editor shows it.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IoDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: IoDiagnosticSeverity,
    pub code: IoDiagnosticCode,
    pub message: String,
    pub(crate) detail: Option<String>,
    pub fix: Option<IoFix>,
    pub related: Vec<IoRelatedLocation>,
}

/// `path:line:column: message`.
impl std::fmt::Display for IoDiagnostic {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}", self.path)?;
        if let Some(range) = &self.range {
            write!(
                formatter,
                ":{}:{}",
                range.start.line + 1,
                range.start.character + 1
            )?;
        }
        write!(formatter, ": {}", self.message)
    }
}

/// How to fix a diagnostic.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IoFix {
    /// Text that replaces the diagnostic's range.
    Replace(String),
    /// Import the unreferenced document from the project root.
    Include(crate::Inclusion),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct IoRelatedLocation {
    pub(crate) path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum IoDiagnosticSeverity {
    Error,
    Warning,
}

/// What kind of problem a diagnostic reports, spelled as editors show it.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
pub enum IoDiagnosticCode {
    #[serde(rename = "donder.load")]
    DonderLoad,
    #[serde(rename = "donder.reference")]
    DonderReference,
    #[serde(rename = "data.syntax")]
    DataSyntax,
    #[serde(rename = "script.compile")]
    ScriptCompile,
    #[serde(rename = "io.read")]
    IoRead,
    #[serde(rename = "donder.unreferenced")]
    UnreferencedDocument,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize, Type)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

impl IoDiagnostic {
    /// An error with no fix and no related locations.
    pub(crate) fn error(
        path: impl Into<Utf8PathBuf>,
        range: Option<TextRange>,
        code: IoDiagnosticCode,
        message: impl Into<String>,
    ) -> Self {
        Self {
            path: path.into(),
            range,
            severity: IoDiagnosticSeverity::Error,
            code,
            message: message.into(),
            detail: None,
            fix: None,
            related: Vec::new(),
        }
    }
}

/// Why loading stopped: the diagnostics of the document where it did.
pub(crate) type LoadError = Vec<IoDiagnostic>;

pub(crate) fn invalid_document(
    path: impl Into<Utf8PathBuf>,
    range: Option<TextRange>,
    message: impl Into<String>,
) -> LoadError {
    vec![IoDiagnostic::error(
        path,
        range,
        IoDiagnosticCode::DonderLoad,
        message,
    )]
}

pub(crate) fn invalid_reference(
    path: impl Into<Utf8PathBuf>,
    range: Option<TextRange>,
    reference: impl std::fmt::Display,
) -> LoadError {
    vec![IoDiagnostic::error(
        path,
        range,
        IoDiagnosticCode::DonderReference,
        format!("invalid reference {reference}"),
    )]
}

pub(crate) fn read_error(path: impl Into<Utf8PathBuf>, source: &std::io::Error) -> LoadError {
    vec![IoDiagnostic::error(
        path,
        None,
        IoDiagnosticCode::IoRead,
        source.to_string(),
    )]
}

pub(crate) fn script_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match compile_script(text) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| {
                dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::ScriptCompile)
            })
            .collect(),
    }
}

pub(crate) fn data_diagnostic(
    path: &Utf8Path,
    text: &str,
    diagnostic: DslDiagnostic,
) -> IoDiagnostic {
    dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::DataSyntax)
}

pub(crate) fn dsl_diagnostic(
    path: &Utf8Path,
    text: &str,
    diagnostic: DslDiagnostic,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    IoDiagnostic {
        fix: diagnostic.fix.map(IoFix::Replace),
        ..IoDiagnostic::error(
            path,
            Some(byte_range(text, diagnostic.span.start, diagnostic.span.end)),
            code,
            diagnostic.message,
        )
    }
}

pub(crate) fn push_diagnostic(diagnostics: &mut Vec<IoDiagnostic>, diagnostic: IoDiagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

pub(crate) fn byte_range(text: &str, start: usize, end: usize) -> TextRange {
    let start = byte_position(text, start);
    let mut end = byte_position(text, end);
    if end == start {
        end.character = end.character.saturating_add(1);
    }
    TextRange { start, end }
}

fn byte_position(text: &str, byte_offset: usize) -> TextPosition {
    let clamped = byte_offset.min(text.len());
    let mut line = 0;
    let mut line_start = 0;
    for (index, character) in text.char_indices() {
        if index >= clamped {
            break;
        }
        if character == '\n' {
            line += 1;
            line_start = index + character.len_utf8();
        }
    }
    TextPosition {
        line,
        character: text[line_start..clamped].chars().count() as u32,
    }
}
