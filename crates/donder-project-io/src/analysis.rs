use crate::imports::parse_imports;
use std::collections::{BTreeMap, HashSet};
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::dsl::Identifier;
use indexmap::IndexMap;
use yaml_serde::Value;

use crate::diagnostics::{
    load_error_diagnostic, parse_yaml_value, push_diagnostic, source_range_for_field_value,
    source_range_for_value, with_yaml_location,
};
use crate::loader::parse::{
    bool_field, f32_field, mapping, optional_sequence, parse_color, parse_duration,
    parse_duration_as_time, required_field, sequence_values, string_field, u32_field,
};
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, LoadProjectError, SourceDocumentFormat,
    SourceObjectKind, TextPosition, TextRange, source_document_format,
};

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRecovery {
    pub root: Utf8PathBuf,
    pub manifest: Option<donder_package::PackageManifest>,
    pub documents: IndexMap<Utf8PathBuf, RecoveryDocument>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryDocument {
    pub kind: RecoveryDocumentKind,
    pub objects: Vec<RecoveryObject>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryDocumentKind {
    Donder,
    Effect,
    Operator,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryObject {
    pub key: String,
    pub kind: SourceObjectKind,
}

pub(crate) fn analyze_project_documents(
    root: &Utf8Path,
    manifest: Option<donder_package::PackageManifest>,
    overrides: &crate::SourceOverrides,
    checked_dsl_documents: &indexmap::IndexSet<Utf8PathBuf>,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> ProjectRecovery {
    let mut documents = IndexMap::new();
    let paths: std::collections::BTreeSet<_> = project_file_inventory(root)
        .into_iter()
        .chain(overrides.keys().cloned())
        .collect();
    for path in paths {
        let absolute = root.join(&path);
        let kind = document_kind(&path);
        let document = match kind {
            RecoveryDocumentKind::Effect | RecoveryDocumentKind::Operator
                if checked_dsl_documents.contains(&path) =>
            {
                RecoveryDocument {
                    kind,
                    objects: Vec::new(),
                }
            }
            RecoveryDocumentKind::Effect => {
                analyze_dsl_document(&path, &absolute, overrides.get(&path), true, diagnostics)
            }
            RecoveryDocumentKind::Operator => {
                analyze_dsl_document(&path, &absolute, overrides.get(&path), false, diagnostics)
            }
            RecoveryDocumentKind::Donder => {
                analyze_donder_document(&path, &absolute, overrides.get(&path), diagnostics)
            }
            RecoveryDocumentKind::Other => RecoveryDocument {
                kind,
                objects: Vec::new(),
            },
        };
        documents.insert(path, document);
    }
    ProjectRecovery {
        root: root.to_path_buf(),
        manifest,
        documents,
    }
}

pub(crate) fn project_file_inventory(root: &Utf8Path) -> Vec<Utf8PathBuf> {
    let mut pending = vec![Utf8PathBuf::new()];
    let mut files = Vec::new();
    while let Some(relative) = pending.pop() {
        let Ok(entries) = fs::read_dir(root.join(&relative)) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(name) = entry.file_name().into_string() else {
                continue;
            };
            if name == ".git" || name == "target" || name == "node_modules" {
                continue;
            }
            let path = relative.join(name);
            if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
                pending.push(path);
            } else {
                files.push(Utf8PathBuf::from(path.as_str().replace('\\', "/")));
            }
        }
    }
    files.sort();
    files
}

fn document_kind(path: &Utf8Path) -> RecoveryDocumentKind {
    match source_document_format(path) {
        SourceDocumentFormat::Effect => RecoveryDocumentKind::Effect,
        SourceDocumentFormat::Operator => RecoveryDocumentKind::Operator,
        SourceDocumentFormat::Donder => RecoveryDocumentKind::Donder,
        SourceDocumentFormat::Other => RecoveryDocumentKind::Other,
    }
}

fn analyze_dsl_document(
    path: &Utf8Path,
    absolute: &Utf8Path,
    override_text: Option<&String>,
    effect: bool,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let text =
        match override_text.map_or_else(|| fs::read_to_string(absolute), |text| Ok(text.clone())) {
            Ok(text) => text,
            Err(error) => {
                push_diagnostic(
                    diagnostics,
                    IoDiagnostic {
                        path: path.to_path_buf(),
                        range: None,
                        severity: IoDiagnosticSeverity::Error,
                        code: IoDiagnosticCode::IoRead,
                        message: error.to_string(),
                        detail: None,
                        related: Vec::new(),
                    },
                );
                String::new()
            }
        };
    let local = if effect {
        crate::diagnostics::effect_diagnostics(path, &text)
    } else {
        crate::diagnostics::operator_diagnostics(path, &text)
    };
    for diagnostic in local {
        push_diagnostic(diagnostics, diagnostic);
    }
    RecoveryDocument {
        kind: if effect {
            RecoveryDocumentKind::Effect
        } else {
            RecoveryDocumentKind::Operator
        },
        objects: Vec::new(),
    }
}

fn analyze_donder_document(
    path: &Utf8Path,
    absolute: &Utf8Path,
    override_text: Option<&String>,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let text =
        match override_text.map_or_else(|| fs::read_to_string(absolute), |text| Ok(text.clone())) {
            Ok(text) => text,
            Err(error) => {
                push_diagnostic(
                    diagnostics,
                    IoDiagnostic {
                        path: path.to_path_buf(),
                        range: None,
                        severity: IoDiagnosticSeverity::Error,
                        code: IoDiagnosticCode::IoRead,
                        message: error.to_string(),
                        detail: None,
                        related: Vec::new(),
                    },
                );
                return RecoveryDocument {
                    kind: RecoveryDocumentKind::Donder,
                    objects: Vec::new(),
                };
            }
        };
    analyze_donder_text(path, &text, diagnostics)
}

fn analyze_donder_text(
    path: &Utf8Path,
    text: &str,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let value = match parse_yaml_value(path, text) {
        Ok(value) => value,
        Err(error) => {
            push_diagnostic(diagnostics, load_error_diagnostic(error));
            return RecoveryDocument {
                kind: RecoveryDocumentKind::Donder,
                objects: Vec::new(),
            };
        }
    };
    let Some(root) = mapping(&value) else {
        push_schema_error(
            diagnostics,
            path,
            source_range_for_value(path, &value),
            "document root must be a mapping",
            IoDiagnosticCode::DonderLoad,
        );
        return RecoveryDocument {
            kind: RecoveryDocumentKind::Donder,
            objects: Vec::new(),
        };
    };
    analyze_imports(path, root, diagnostics);

    let mut objects = Vec::new();
    for (key, object_value) in root {
        let Some(key) = key.as_str() else {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_value(path, object_value),
                "object keys must be strings",
                IoDiagnosticCode::DonderLoad,
            );
            continue;
        };
        if key == "imports" {
            continue;
        }
        if Identifier::new(key.to_string()).is_err() {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_value(path, object_value),
                format!("invalid object identifier `{key}`"),
                IoDiagnosticCode::DonderLoad,
            );
            continue;
        }
        let object_type = match string_field(path, object_value, "type") {
            Ok(object_type) => object_type,
            Err(error) => {
                push_load_error(diagnostics, error, IoDiagnosticCode::DonderLoad);
                continue;
            }
        };
        let Some(kind) = source_object_kind(object_type) else {
            push_schema_error(
                diagnostics,
                path,
                source_range_for_field_value(path, object_value, "type"),
                format!("unsupported object type `{object_type}`"),
                IoDiagnosticCode::DonderLoad,
            );
            continue;
        };
        if kind == SourceObjectKind::Sequence {
            analyze_sequence(path, object_value, diagnostics);
        }
        objects.push(RecoveryObject {
            key: key.to_string(),
            kind,
        });
    }

    RecoveryDocument {
        kind: RecoveryDocumentKind::Donder,
        objects,
    }
}

fn analyze_imports(
    path: &Utf8Path,
    root: &yaml_serde::Mapping,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    if let Err(error) = parse_imports(path, root) {
        push_load_error(diagnostics, error, IoDiagnosticCode::DonderLoad);
    }
}

pub(crate) fn check_donder_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    let mut diagnostics = Vec::new();
    let _ = analyze_donder_text(path, text, &mut diagnostics);
    sort_diagnostics(&mut diagnostics);
    diagnostics
}

fn source_object_kind(value: &str) -> Option<SourceObjectKind> {
    Some(match value {
        "project" => SourceObjectKind::Project,
        "setup" => SourceObjectKind::Setup,
        "controller" => SourceObjectKind::Controller,
        "layout" => SourceObjectKind::Layout,
        "patch" => SourceObjectKind::Patch,
        "fixture" => SourceObjectKind::FixtureDefinition,
        "curve" => SourceObjectKind::Curve,
        "gradient" => SourceObjectKind::Gradient,
        "sequence" => SourceObjectKind::Sequence,
        _ => return None,
    })
}

fn analyze_sequence(path: &Utf8Path, value: &Value, diagnostics: &mut Vec<IoDiagnostic>) {
    parse_field(
        diagnostics,
        string_field(path, value, "duration")
            .and_then(|duration| {
                parse_duration(duration).map_err(|error| {
                    with_yaml_location(
                        error,
                        path,
                        source_range_for_field_value(path, value, "duration"),
                    )
                })
            })
            .and_then(|duration| {
                (duration.as_seconds_f32() > 0.0)
                    .then_some(duration.as_seconds_f32())
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_field_value(path, value, "duration"),
                        message: "field `duration` must be greater than zero".to_string(),
                    })
            }),
        IoDiagnosticCode::SequenceField,
    );
    parse_field(
        diagnostics,
        f32_field(path, value, "frame_rate").and_then(|frame_rate| {
            if frame_rate.is_finite() && frame_rate > 0.0 {
                Ok(frame_rate)
            } else {
                Err(LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_field_value(path, value, "frame_rate"),
                    message: "field `frame_rate` must be finite and greater than zero".to_string(),
                })
            }
        }),
        IoDiagnosticCode::SequenceField,
    );
    analyze_layers(path, value, diagnostics);
    analyze_mark_collections(path, value, diagnostics);
    analyze_timeline_items(path, value, TimelineItemSchema::Effect, diagnostics);
    analyze_timeline_items(path, value, TimelineItemSchema::AutomationClip, diagnostics);
    analyze_graph_items(path, value, diagnostics);
}

fn analyze_layers(path: &Utf8Path, value: &Value, diagnostics: &mut Vec<IoDiagnostic>) {
    let values = match sequence_values(path, value, "layers") {
        Ok(values) => values,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    let mut ids = BTreeMap::new();
    for layer in values {
        let Some(id) = collect_item_field(diagnostics, u32_field(path, layer, "id")) else {
            continue;
        };
        let Some(_) = collect_item_field(
            diagnostics,
            string_field(path, layer, "name").map(str::to_string),
        ) else {
            continue;
        };
        let Some(_) = collect_item_field(
            diagnostics,
            string_field(path, layer, "color").and_then(|color| {
                parse_color(color)
                    .map(|_| color.to_string())
                    .map_err(|error| {
                        with_yaml_location(
                            error,
                            path,
                            source_range_for_field_value(path, layer, "color"),
                        )
                    })
            }),
        ) else {
            continue;
        };
        let Some(_) = collect_item_field(diagnostics, bool_field(path, layer, "enabled")) else {
            continue;
        };
        if let Some(previous_range) = ids.insert(id, source_range_for_value(path, layer)) {
            let mut diagnostic = schema_diagnostic(
                path,
                source_range_for_field_value(path, layer, "id"),
                format!("duplicate sequence layer id `{id}`"),
                IoDiagnosticCode::SequenceItem,
            );
            diagnostic.related.push(crate::IoRelatedLocation {
                path: path.to_path_buf(),
                range: previous_range,
                message: "first layer with this id".to_string(),
            });
            push_diagnostic(diagnostics, diagnostic);
            continue;
        }
    }
}

fn analyze_mark_collections(path: &Utf8Path, value: &Value, diagnostics: &mut Vec<IoDiagnostic>) {
    let values = match optional_sequence(path, value, "mark_collections") {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for collection in values {
        let Some(_) = collect_item_field(
            diagnostics,
            string_field(path, collection, "key").map(str::to_string),
        ) else {
            continue;
        };
        let Some(_) = collect_item_field(
            diagnostics,
            string_field(path, collection, "name").map(str::to_string),
        ) else {
            continue;
        };
        let Some(_) = collect_item_field(
            diagnostics,
            string_field(path, collection, "color").and_then(|color| {
                parse_color(color)
                    .map(|_| color.to_string())
                    .map_err(|error| {
                        with_yaml_location(
                            error,
                            path,
                            source_range_for_field_value(path, collection, "color"),
                        )
                    })
            }),
        ) else {
            continue;
        };
        let marks = match sequence_values(path, collection, "marks") {
            Ok(marks) => marks,
            Err(error) => {
                push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                continue;
            }
        };
        for mark in marks {
            let result = mark
                .as_str()
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: source_range_for_value(path, mark),
                    message: "marks must be duration strings".to_string(),
                })
                .and_then(|value| {
                    parse_duration_as_time(value).map_err(|error| {
                        with_yaml_location(error, path, source_range_for_value(path, mark))
                    })
                })
                .map(|time| time.as_seconds_f32());
            collect_item_field(diagnostics, result);
        }
    }
}

#[derive(Clone, Copy)]
enum TimelineItemSchema {
    Effect,
    AutomationClip,
}

impl TimelineItemSchema {
    fn field(self) -> &'static str {
        match self {
            Self::Effect => "effects",
            Self::AutomationClip => "automation_clips",
        }
    }

    fn lane(self) -> TimelineLaneField {
        match self {
            Self::Effect => TimelineLaneField::Layer,
            Self::AutomationClip => TimelineLaneField::Lane,
        }
    }

    fn required(self) -> bool {
        matches!(self, Self::Effect)
    }
}

#[derive(Clone, Copy)]
enum TimelineLaneField {
    Layer,
    Lane,
}

impl TimelineLaneField {
    fn field(self) -> &'static str {
        match self {
            Self::Layer => "layer_id",
            Self::Lane => "lane_index",
        }
    }
}

fn analyze_timeline_items(
    path: &Utf8Path,
    sequence: &Value,
    schema: TimelineItemSchema,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let values = match if schema.required() {
        sequence_values(path, sequence, schema.field()).map(Some)
    } else {
        optional_sequence(path, sequence, schema.field())
    } {
        Ok(Some(values)) => values,
        Ok(None) => return,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for value in values {
        push_item_shape_errors(path, value, schema.lane(), diagnostics);
    }
}

fn push_item_shape_errors(
    path: &Utf8Path,
    value: &Value,
    lane: TimelineLaneField,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let checks = [
        u32_field(path, value, "id").map(|_| ()),
        string_field(path, value, "start")
            .and_then(parse_duration_as_time)
            .map(|_| ()),
        string_field(path, value, "duration")
            .and_then(parse_duration)
            .map(|_| ()),
        u32_field(path, value, lane.field()).map(|_| ()),
    ];
    for error in checks.into_iter().filter_map(Result::err) {
        push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
    }
}

fn analyze_graph_items(path: &Utf8Path, sequence: &Value, diagnostics: &mut Vec<IoDiagnostic>) {
    let graph = match required_field(path, sequence, "composition_graph") {
        Ok(graph) => graph,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    let nodes = match sequence_values(path, graph, "nodes") {
        Ok(nodes) => nodes,
        Err(error) => {
            push_load_error(diagnostics, error, IoDiagnosticCode::SequenceField);
            return;
        }
    };
    for node in nodes {
        let id = u32_field(path, node, "id");
        let position = required_field(path, node, "position");
        let parsed = position.and_then(|position| {
            Ok((
                f32_field(path, position, "x")?,
                f32_field(path, position, "y")?,
            ))
        });
        match (id, parsed) {
            (Ok(_), Ok(_)) => {}
            (id, parsed) => {
                if let Err(error) = id {
                    push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                }
                if let Err(error) = parsed {
                    push_load_error(diagnostics, error, IoDiagnosticCode::SequenceItem);
                }
            }
        }
    }
}

fn parse_field<T>(
    diagnostics: &mut Vec<IoDiagnostic>,
    result: Result<T, LoadProjectError>,
    code: IoDiagnosticCode,
) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            push_load_error(diagnostics, error, code);
            None
        }
    }
}

fn collect_item_field<T>(
    diagnostics: &mut Vec<IoDiagnostic>,
    result: Result<T, LoadProjectError>,
) -> Option<T> {
    parse_field(diagnostics, result, IoDiagnosticCode::SequenceItem)
}

fn push_load_error(
    diagnostics: &mut Vec<IoDiagnostic>,
    error: LoadProjectError,
    code: IoDiagnosticCode,
) {
    let mut diagnostic = load_error_diagnostic(error);
    diagnostic.code = code;
    push_diagnostic(diagnostics, diagnostic);
}

fn push_schema_error(
    diagnostics: &mut Vec<IoDiagnostic>,
    path: &Utf8Path,
    range: Option<TextRange>,
    message: impl Into<String>,
    code: IoDiagnosticCode,
) {
    push_diagnostic(diagnostics, schema_diagnostic(path, range, message, code));
}

fn schema_diagnostic(
    path: &Utf8Path,
    range: Option<TextRange>,
    message: impl Into<String>,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    IoDiagnostic {
        path: path.to_path_buf(),
        range,
        severity: IoDiagnosticSeverity::Error,
        code,
        message: message.into(),
        detail: None,
        related: Vec::new(),
    }
}

pub(crate) fn package_parse_diagnostic(
    path: &str,
    error: donder_package::PackageFileParseError,
    code: IoDiagnosticCode,
) -> IoDiagnostic {
    let position = TextPosition {
        line: error.line,
        character: error.column,
    };
    IoDiagnostic {
        path: Utf8PathBuf::from(path),
        range: Some(TextRange {
            start: position.clone(),
            end: TextPosition {
                line: position.line,
                character: position.character.saturating_add(1),
            },
        }),
        severity: IoDiagnosticSeverity::Error,
        code,
        message: error.message,
        detail: error.field_path.map(|path| format!("JSON field: {path}")),
        related: Vec::new(),
    }
}

pub(crate) fn package_validation_diagnostics(
    path: &str,
    issues: Vec<donder_package::PackageValidationIssue>,
    code: IoDiagnosticCode,
) -> Vec<IoDiagnostic> {
    issues
        .into_iter()
        .map(|issue| IoDiagnostic {
            path: Utf8PathBuf::from(path),
            range: None,
            severity: IoDiagnosticSeverity::Error,
            code: code.clone(),
            message: issue.message,
            detail: Some(format!("JSON field: {}", issue.field_path)),
            related: Vec::new(),
        })
        .collect()
}

pub(crate) fn sort_diagnostics(diagnostics: &mut Vec<IoDiagnostic>) {
    diagnostics.sort_by(|left, right| {
        let position = |diagnostic: &IoDiagnostic| {
            diagnostic
                .range
                .as_ref()
                .map(|range| (range.start.line, range.start.character))
                .unwrap_or((u32::MAX, u32::MAX))
        };
        left.path
            .cmp(&right.path)
            .then_with(|| position(left).cmp(&position(right)))
            .then_with(|| left.message.cmp(&right.message))
    });
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| {
        let range = matches!(
            diagnostic.code,
            IoDiagnosticCode::EffectCompile | IoDiagnosticCode::OperatorCompile
        )
        .then_some(None)
        .unwrap_or_else(|| diagnostic.range.clone());
        seen.insert((diagnostic.path.clone(), range, diagnostic.message.clone()))
    });
}
