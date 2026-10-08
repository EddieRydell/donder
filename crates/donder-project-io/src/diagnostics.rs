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

/// The syntax and schema diagnostics of a data document.
pub(crate) fn data_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    crate::document::read(text)
        .1
        .into_iter()
        .map(|diagnostic| data_diagnostic(path, text, diagnostic))
        .collect()
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
        path: path.to_path_buf(),
        range: Some(byte_range(text, diagnostic.span.start, diagnostic.span.end)),
        severity: IoDiagnosticSeverity::Error,
        code,
        message: diagnostic.message,
        detail: None,
        fix: diagnostic.fix.map(IoFix::Replace),
        related: Vec::new(),
    }
}

pub(crate) fn load_error_diagnostic(error: LoadProjectError) -> IoDiagnostic {
    match error {
        LoadProjectError::Io { path, source } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::IoRead,
            message: source.to_string(),
            detail: None,
            fix: None,
            related: Vec::new(),
        },
        LoadProjectError::InvalidDocument {
            path,
            range,
            message,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DonderLoad,
            message,
            detail: None,
            fix: None,
            related: Vec::new(),
        },
        LoadProjectError::InvalidReference {
            path,
            range,
            reference,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DonderReference,
            message: format!("invalid reference {reference}"),
            detail: None,
            fix: None,
            related: Vec::new(),
        },
        LoadProjectError::InvalidScript { path, diagnostics }
        | LoadProjectError::InvalidData { path, diagnostics }
        | LoadProjectError::InvalidImports { path, diagnostics } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: diagnostics
                .first()
                .map_or(IoDiagnosticCode::DonderLoad, |diagnostic| {
                    diagnostic.code.clone()
                }),
            message: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join(", "),
            detail: None,
            fix: None,
            related: Vec::new(),
        },
    }
}

pub(crate) fn push_diagnostic(diagnostics: &mut Vec<IoDiagnostic>, diagnostic: IoDiagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

pub(crate) fn push_load_error_diagnostics(
    diagnostics: &mut Vec<IoDiagnostic>,
    error: LoadProjectError,
) {
    match error {
        LoadProjectError::InvalidImports {
            diagnostics: listed,
            ..
        }
        | LoadProjectError::InvalidScript {
            diagnostics: listed,
            ..
        }
        | LoadProjectError::InvalidData {
            diagnostics: listed,
            ..
        } => {
            for diagnostic in listed {
                push_diagnostic(diagnostics, diagnostic);
            }
        }
        other => push_diagnostic(diagnostics, load_error_diagnostic(other)),
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

pub(crate) fn byte_position(text: &str, byte_offset: usize) -> TextPosition {
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
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::{Diagnostic as DslDiagnostic, compile_script};

use crate::{LoadProjectError, ProjectRecovery, ProjectSession};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCheckReport {
    pub session: Option<ProjectSession>,
    pub recovery: ProjectRecovery,
    pub diagnostics: Vec<IoDiagnostic>,
    /// Every reference resolved before loading finished or failed.
    pub index: crate::ProjectIndex,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: IoDiagnosticSeverity,
    pub code: IoDiagnosticCode,
    pub message: String,
    pub detail: Option<String>,
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
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoFix {
    /// Text that replaces the diagnostic's range.
    Replace(String),
    /// Import the unreferenced document from the project root.
    Include(crate::Inclusion),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoRelatedLocation {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticCode {
    DonderLoad,
    DonderReference,
    DataSyntax,
    ScriptCompile,
    IoRead,
    UnreferencedDocument,
}

impl IoDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DonderLoad => "donder.load",
            Self::DonderReference => "donder.reference",
            Self::DataSyntax => "data.syntax",
            Self::ScriptCompile => "script.compile",
            Self::IoRead => "io.read",
            Self::UnreferencedDocument => "donder.unreferenced",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}
