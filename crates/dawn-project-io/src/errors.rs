use crate::{IoDiagnostic, TextRange};
use camino::Utf8PathBuf;
use std::{fmt, io};

#[derive(Debug)]
pub enum LoadProjectError {
    InvalidImports {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
    InvalidEntrypoint {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    ParseYaml {
        path: Utf8PathBuf,
        message: String,
        range: Option<TextRange>,
    },
    InvalidDocument {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        message: String,
    },
    InvalidReference {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        reference: String,
    },
    InvalidEffect {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
    InvalidOperator {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
}

#[derive(Debug)]
pub enum ExportProjectError {
    OutputRootIsFile {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Serialize {
        path: Utf8PathBuf,
        source: yaml_serde::Error,
    },
    InvalidReference {
        path: Utf8PathBuf,
        reference: String,
        message: String,
    },
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntrypoint { path } => write!(formatter, "invalid entrypoint {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::ParseYaml { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidDocument { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidReference {
                path, reference, ..
            } => {
                write!(formatter, "{path}: invalid reference {reference}")
            }
            Self::InvalidEffect { path, diagnostics }
            | Self::InvalidImports { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid document: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::InvalidOperator { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid operator: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for LoadProjectError {}

impl fmt::Display for ExportProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputRootIsFile { path } => write!(formatter, "output root is a file: {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::Serialize { path, source } => write!(formatter, "{path}: {source}"),
            Self::InvalidReference {
                path,
                reference,
                message,
            } => write!(
                formatter,
                "{path}: invalid reference {reference}: {message}"
            ),
        }
    }
}

impl std::error::Error for ExportProjectError {}
