pub(crate) fn parse_yaml_value(path: &Utf8Path, text: &str) -> Result<Value, LoadProjectError> {
    let node = marked_yaml::parse_yaml_with_options(
        0,
        text,
        marked_yaml::LoaderOptions::default().error_on_duplicate_keys(true),
    )
    .map_err(|source| LoadProjectError::ParseYaml {
        path: path.to_path_buf(),
        range: marked_yaml_error_range(&source),
        message: source.to_string(),
    })?;
    let value: Value =
        yaml_serde::from_str(text).map_err(|source| LoadProjectError::ParseYaml {
            path: path.to_path_buf(),
            range: yaml_error_range(&source),
            message: source.to_string(),
        })?;
    let source_index = YamlSourceIndex::from_value_and_node(&value, &node);
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .insert(path.to_path_buf(), source_index);
    });
    Ok(value)
}

pub(crate) fn effect_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match compile_effect_document(text) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| {
                dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::EffectCompile)
            })
            .collect(),
    }
}

pub(crate) fn operator_diagnostics(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match compile_operators(text) {
        Ok(_) => Vec::new(),
        Err(diagnostics) => diagnostics
            .into_iter()
            .map(|diagnostic| {
                dsl_diagnostic(path, text, diagnostic, IoDiagnosticCode::OperatorCompile)
            })
            .collect(),
    }
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
        related: Vec::new(),
    }
}

pub(crate) fn load_error_diagnostic(error: LoadProjectError) -> IoDiagnostic {
    match error {
        LoadProjectError::InvalidEntrypoint { path } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DawnLoad,
            message: "invalid entrypoint".to_string(),
            detail: None,
            related: Vec::new(),
        },
        LoadProjectError::Io { path, source } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::IoRead,
            message: source.to_string(),
            detail: None,
            related: Vec::new(),
        },
        LoadProjectError::ParseYaml {
            path,
            message,
            range,
        } => IoDiagnostic {
            path,
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::YamlParse,
            message,
            detail: None,
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
            code: IoDiagnosticCode::DawnLoad,
            message,
            detail: None,
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
            code: IoDiagnosticCode::DawnReference,
            message: format!("invalid reference {reference}"),
            detail: None,
            related: Vec::new(),
        },
        LoadProjectError::InvalidEffect { path, diagnostics }
        | LoadProjectError::InvalidImports { path, diagnostics } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: diagnostics
                .first()
                .map_or(IoDiagnosticCode::EffectCompile, |diagnostic| {
                    diagnostic.code.clone()
                }),
            message: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join(", "),
            detail: None,
            related: Vec::new(),
        },
        LoadProjectError::InvalidOperator { path, diagnostics } => IoDiagnostic {
            path,
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::OperatorCompile,
            message: diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join(", "),
            detail: None,
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
            diagnostics: effect_diagnostics,
            ..
        }
        | LoadProjectError::InvalidEffect {
            diagnostics: effect_diagnostics,
            ..
        } => {
            for diagnostic in effect_diagnostics {
                push_diagnostic(diagnostics, diagnostic);
            }
        }
        LoadProjectError::InvalidOperator {
            diagnostics: operator_diagnostics,
            ..
        } => {
            for diagnostic in operator_diagnostics {
                push_diagnostic(diagnostics, diagnostic);
            }
        }
        other => push_diagnostic(diagnostics, load_error_diagnostic(other)),
    }
}

pub(crate) fn with_yaml_location(
    error: LoadProjectError,
    path: &Utf8Path,
    range: Option<TextRange>,
) -> LoadProjectError {
    match error {
        LoadProjectError::InvalidDocument { message, .. } => LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range,
            message,
        },
        LoadProjectError::InvalidReference { reference, .. } => {
            LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range,
                reference,
            }
        }
        other => other,
    }
}

pub(crate) fn yaml_error_range(error: &yaml_serde::Error) -> Option<TextRange> {
    let location = error.location()?;
    let line = location.line().saturating_sub(1) as u32;
    let character = location.column().saturating_sub(1) as u32;
    Some(TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(1),
        },
    })
}

pub(crate) fn marked_yaml_error_range(error: &MarkedYamlError) -> Option<TextRange> {
    match error {
        MarkedYamlError::TopLevelMustBeMapping(marker)
        | MarkedYamlError::TopLevelMustBeSequence(marker)
        | MarkedYamlError::UnexpectedAnchor(marker)
        | MarkedYamlError::MappingKeyMustBeScalar(marker)
        | MarkedYamlError::UnexpectedTag(marker)
        | MarkedYamlError::ScanError(marker, _) => Some(marker_range(marker)),
        MarkedYamlError::DuplicateKey(inner) => span_range(inner.key.span()),
    }
}

pub(crate) fn marker_range(marker: &Marker) -> TextRange {
    let line = marker.line().saturating_sub(1) as u32;
    let character = marker.column().saturating_sub(1) as u32;
    TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(1),
        },
    }
}

pub(crate) fn span_range(span: &marked_yaml::Span) -> Option<TextRange> {
    let start = span.start()?;
    let end = span.end().unwrap_or(start);
    let start_line = start.line().saturating_sub(1) as u32;
    let start_character = start.column().saturating_sub(1) as u32;
    let end_line = end.line().saturating_sub(1) as u32;
    let mut end_character = end.column().saturating_sub(1) as u32;
    if start_line == end_line && start_character == end_character {
        end_character = end_character.saturating_add(1);
    }
    Some(TextRange {
        start: TextPosition {
            line: start_line,
            character: start_character,
        },
        end: TextPosition {
            line: end_line,
            character: end_character,
        },
    })
}

pub(crate) fn node_range(node: &Node) -> Option<TextRange> {
    match node {
        Node::Scalar(scalar) => scalar_range(scalar),
        Node::Mapping(mapping) => span_range(mapping.span()),
        Node::Sequence(sequence) => span_range(sequence.span()),
    }
}

pub(crate) fn scalar_range(scalar: &marked_yaml::types::MarkedScalarNode) -> Option<TextRange> {
    let start = scalar.span().start()?;
    let line = start.line().saturating_sub(1) as u32;
    let character = start.column().saturating_sub(1) as u32;
    let width = scalar.as_str().chars().count().max(1) as u32;
    Some(TextRange {
        start: TextPosition { line, character },
        end: TextPosition {
            line,
            character: character.saturating_add(width),
        },
    })
}

pub(crate) fn deserialize_yaml<T: serde::de::DeserializeOwned>(
    path: &Utf8Path,
    value: &Value,
) -> Result<T, LoadProjectError> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let range = YAML_SOURCE_INDICES.with(|indices| {
            let mut indices = indices.borrow_mut();
            let index = indices.get_mut(path)?;
            let mut field_path = index.bound_value_path(value)?;
            for segment in error.path() {
                match segment {
                    serde_path_to_error::Segment::Map { key }
                    | serde_path_to_error::Segment::Enum { variant: key } => {
                        field_path.push(YamlPathSegment::Key(key.clone()));
                    }
                    serde_path_to_error::Segment::Seq { index } => {
                        field_path.push(YamlPathSegment::Index(*index));
                    }
                    serde_path_to_error::Segment::Unknown => {}
                }
            }
            index
                .entries
                .iter()
                .find(|entry| entry.path == field_path)
                .and_then(|entry| entry.range.clone())
        });
        LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range,
            message: error.inner().to_string(),
        }
    })
}

pub(crate) fn source_range_for_value(path: &Utf8Path, value: &Value) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_value(value))
    })
}

pub(crate) fn source_range_for_field_value(
    path: &Utf8Path,
    value: &Value,
    key: &str,
) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_field_value(value, key))
    })
}

pub(crate) fn source_range_for_scalar(path: &Utf8Path, value: &str) -> Option<TextRange> {
    YAML_SOURCE_INDICES.with(|indices| {
        indices
            .borrow_mut()
            .get_mut(path)
            .and_then(|index| index.range_for_scalar(value))
    })
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
use dawn_language::dsl::{Diagnostic as DslDiagnostic, compile_effect_document, compile_operators};
use indexmap::{IndexMap, IndexSet};
use marked_yaml::{LoadError as MarkedYamlError, Marker, Node};
use std::cell::RefCell;
use yaml_serde::Value;

use crate::{LoadProjectError, ProjectRecovery, ProjectSession};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCheckReport {
    pub session: Option<ProjectSession>,
    pub recovery: ProjectRecovery,
    pub diagnostics: Vec<IoDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: IoDiagnosticSeverity,
    pub code: IoDiagnosticCode,
    pub message: String,
    pub detail: Option<String>,
    pub related: Vec<IoRelatedLocation>,
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
    DawnLoad,
    DawnReference,
    EffectCompile,
    OperatorCompile,
    IoRead,
    ManifestField,
    ManifestSyntax,
    LockField,
    LockSyntax,
    SequenceField,
    SequenceItem,
    YamlParse,
}

impl IoDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DawnLoad => "dawn.load",
            Self::DawnReference => "dawn.reference",
            Self::EffectCompile => "effect.compile",
            Self::OperatorCompile => "operator.compile",
            Self::IoRead => "io.read",
            Self::ManifestField => "manifest.field",
            Self::ManifestSyntax => "manifest.syntax",
            Self::LockField => "lock.field",
            Self::LockSyntax => "lock.syntax",
            Self::SequenceField => "sequence.field",
            Self::SequenceItem => "sequence.item",
            Self::YamlParse => "yaml.parse",
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

thread_local! {
    static YAML_SOURCE_INDICES: RefCell<IndexMap<Utf8PathBuf, YamlSourceIndex>> = RefCell::new(IndexMap::new());
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum YamlPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Clone, Debug, Default)]
struct YamlSourceIndex {
    entries: Vec<YamlSourceEntry>,
    value_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_value_paths: IndexSet<Vec<YamlPathSegment>>,
    scalar_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_scalar_paths: IndexSet<Vec<YamlPathSegment>>,
}

#[derive(Clone, Debug)]
struct YamlSourceEntry {
    path: Vec<YamlPathSegment>,
    value: Value,
    range: Option<TextRange>,
}

impl YamlSourceIndex {
    fn from_value_and_node(value: &Value, node: &Node) -> Self {
        let mut index = Self::default();
        let mut path = Vec::new();
        index.push(value, node, &mut path);
        index
    }

    fn push(&mut self, value: &Value, node: &Node, path: &mut Vec<YamlPathSegment>) {
        self.entries.push(YamlSourceEntry {
            path: path.clone(),
            value: value.clone(),
            range: node_range(node),
        });

        match (value, node) {
            (Value::Mapping(mapping), Node::Mapping(marked_mapping)) => {
                for (key, child_value) in mapping {
                    let Some(key) = key.as_str() else {
                        continue;
                    };
                    let Some(child_node) = marked_mapping.get_node(key) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Key(key.to_string()));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            (Value::Sequence(sequence), Node::Sequence(marked_sequence)) => {
                for (index, child_value) in sequence.iter().enumerate() {
                    let Some(child_node) = marked_sequence.get_node(index) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Index(index));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            _ => {}
        }
    }

    fn bound_value_path(&mut self, value: &Value) -> Option<Vec<YamlPathSegment>> {
        let pointer = std::ptr::from_ref(value).addr();
        if let Some(path) = self.value_bindings.get(&pointer) {
            return Some(path.clone());
        }
        let path = self
            .entries
            .iter()
            .filter(|entry| &entry.value == value)
            .map(|entry| &entry.path)
            .find(|path| !self.claimed_value_paths.contains(*path))?
            .clone();
        self.claimed_value_paths.insert(path.clone());
        self.value_bindings.insert(pointer, path.clone());
        Some(path)
    }

    fn range_for_value(&mut self, value: &Value) -> Option<TextRange> {
        let path = self.bound_value_path(value)?;
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_field_value(&mut self, parent: &Value, key: &str) -> Option<TextRange> {
        let parent_path = self.bound_value_path(parent)?;
        let mut field_path = parent_path;
        field_path.push(YamlPathSegment::Key(key.to_string()));
        self.entries
            .iter()
            .find(|entry| entry.path == field_path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_scalar(&mut self, value: &str) -> Option<TextRange> {
        let pointer = value.as_ptr().addr();
        let path = if let Some(path) = self.scalar_bindings.get(&pointer) {
            path.clone()
        } else {
            let path = self
                .entries
                .iter()
                .filter(|entry| entry.value.as_str() == Some(value))
                .map(|entry| &entry.path)
                .find(|path| !self.claimed_scalar_paths.contains(*path))?
                .clone();
            self.claimed_scalar_paths.insert(path.clone());
            self.scalar_bindings.insert(pointer, path.clone());
            path
        };
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }
}
