use donder_language::compiler::{DeclarationKind, partial_declaration_spans};
use donder_model::ObjectKind;
use donder_runtime_types::Identifier;
use std::collections::HashSet;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;

use crate::diagnostics::{byte_range, data_diagnostic, push_diagnostic, script_diagnostics};
use crate::project_loading::SourceTexts;
use crate::{
    IoDiagnostic, IoDiagnosticCode, SourceDocumentFormat, TextRange, source_document_format,
};

/// What a project's documents declare, read even when the project does not
/// load, so the workspace can still list and open them.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRecovery {
    pub root: Utf8PathBuf,
    /// Every readable document.
    pub documents: IndexMap<Utf8PathBuf, RecoveryDocument>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryDocument {
    pub format: SourceDocumentFormat,
    pub objects: Vec<RecoveryObject>,
    /// Where the first declaration is named.
    pub(crate) first_name: Option<TextRange>,
}

/// An object a document declares, by its name.
#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryObject {
    pub kind: ObjectKind,
    pub(crate) name: Identifier,
}

/// Read every document at `paths`, reporting its syntax errors, and list what
/// each declares. Scripts the load compiled already reported their errors.
pub(crate) fn analyze_project_documents(
    root: &Utf8Path,
    paths: impl IntoIterator<Item = Utf8PathBuf>,
    texts: &mut SourceTexts,
    checked_scripts: &indexmap::IndexSet<Utf8PathBuf>,
    active_documents: Option<&indexmap::IndexSet<Utf8PathBuf>>,
    cache: &mut crate::ProjectCheckCache,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> ProjectRecovery {
    let mut documents = IndexMap::new();
    for path in paths {
        let mut inactive_diagnostics = Vec::new();
        let diagnostics = if active_documents.is_none_or(|active| active.contains(&path)) {
            &mut *diagnostics
        } else {
            &mut inactive_diagnostics
        };
        let format = source_document_format(&path);
        if format == SourceDocumentFormat::Other {
            documents.insert(
                path,
                RecoveryDocument {
                    format,
                    objects: Vec::new(),
                    first_name: None,
                },
            );
            continue;
        }
        let text = match texts.text(&path) {
            Ok(text) => text,
            Err(error) => {
                push_diagnostic(
                    diagnostics,
                    IoDiagnostic::error(
                        path.clone(),
                        None,
                        IoDiagnosticCode::IoRead,
                        error.to_string(),
                    ),
                );
                continue;
            }
        };
        let document = if format == SourceDocumentFormat::Script {
            if !checked_scripts.contains(&path) {
                for diagnostic in script_diagnostics(&path, text) {
                    push_diagnostic(diagnostics, diagnostic);
                }
            }
            let declarations = partial_declaration_spans(text);
            RecoveryDocument {
                format,
                first_name: declarations.first().map(|declaration| {
                    byte_range(text, declaration.name_span.start, declaration.name_span.end)
                }),
                objects: declarations
                    .into_iter()
                    .map(|declaration| RecoveryObject {
                        kind: match declaration.kind {
                            DeclarationKind::Effect => ObjectKind::EffectDefinition,
                            DeclarationKind::Operator => ObjectKind::OperatorDefinition,
                        },
                        name: declaration.name,
                    })
                    .collect(),
            }
        } else {
            analyze_data_text(&path, text, cache, diagnostics)
        };
        documents.insert(path, document);
    }
    ProjectRecovery {
        root: root.to_path_buf(),
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

fn analyze_data_text(
    path: &Utf8Path,
    text: &str,
    cache: &mut crate::ProjectCheckCache,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let read = cache.read(path, text);
    for diagnostic in &read.diagnostics {
        push_diagnostic(diagnostics, data_diagnostic(path, text, diagnostic.clone()));
    }
    let document = &read.document;
    RecoveryDocument {
        format: SourceDocumentFormat::Data,
        first_name: document
            .declarations
            .first()
            .map(|(name, _, _)| byte_range(text, name.span.start, name.span.end)),
        objects: document
            .declarations
            .iter()
            .map(|(name, _, declaration)| RecoveryObject {
                kind: declaration.kind(),
                name: name.value.clone(),
            })
            .collect(),
    }
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
        seen.insert((
            diagnostic.path.clone(),
            diagnostic.range.clone(),
            diagnostic.message.clone(),
        ))
    });
}
