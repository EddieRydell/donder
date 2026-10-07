use std::collections::HashSet;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexMap;

use crate::diagnostics::{data_diagnostic, push_diagnostic, script_diagnostics};
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, SourceDocumentFormat, SourceObjectKind,
    source_document_format,
};

/// What a project's documents declare, read even when the project does not
/// load, so the workspace can still list and open them.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRecovery {
    pub root: Utf8PathBuf,
    pub documents: IndexMap<Utf8PathBuf, RecoveryDocument>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryDocument {
    pub kind: RecoveryDocumentKind,
    pub objects: Vec<RecoveryObject>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryDocumentKind {
    Data,
    Script,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RecoveryObject {
    pub key: String,
    pub kind: SourceObjectKind,
}

pub(crate) fn analyze_project_documents(
    root: &Utf8Path,
    overrides: &crate::SourceOverrides,
    checked_scripts: &indexmap::IndexSet<Utf8PathBuf>,
    active_documents: Option<&indexmap::IndexSet<Utf8PathBuf>>,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> ProjectRecovery {
    let mut documents = IndexMap::new();
    let paths: std::collections::BTreeSet<_> = project_file_inventory(root)
        .into_iter()
        .chain(overrides.keys().cloned())
        .collect();
    for path in paths {
        let mut inactive_diagnostics = Vec::new();
        let diagnostics = if active_documents.is_none_or(|active| active.contains(&path)) {
            &mut *diagnostics
        } else {
            &mut inactive_diagnostics
        };
        let format = source_document_format(&path);
        let document = match format {
            SourceDocumentFormat::Other => RecoveryDocument {
                kind: RecoveryDocumentKind::Other,
                objects: Vec::new(),
            },
            SourceDocumentFormat::Script if checked_scripts.contains(&path) => RecoveryDocument {
                kind: RecoveryDocumentKind::Script,
                objects: Vec::new(),
            },
            SourceDocumentFormat::Script | SourceDocumentFormat::Data => {
                let text = match overrides.get(&path).map_or_else(
                    || fs::read_to_string(root.join(&path)),
                    |text| Ok(text.clone()),
                ) {
                    Ok(text) => text,
                    Err(error) => {
                        push_diagnostic(
                            diagnostics,
                            IoDiagnostic {
                                path: path.clone(),
                                range: None,
                                severity: IoDiagnosticSeverity::Error,
                                code: IoDiagnosticCode::IoRead,
                                message: error.to_string(),
                                detail: None,
                                fix: None,
                                related: Vec::new(),
                            },
                        );
                        String::new()
                    }
                };
                if format == SourceDocumentFormat::Script {
                    for diagnostic in script_diagnostics(&path, &text) {
                        push_diagnostic(diagnostics, diagnostic);
                    }
                    RecoveryDocument {
                        kind: RecoveryDocumentKind::Script,
                        objects: Vec::new(),
                    }
                } else {
                    analyze_data_text(&path, &text, diagnostics)
                }
            }
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
    diagnostics: &mut Vec<IoDiagnostic>,
) -> RecoveryDocument {
    let (document, local) = crate::document::read(text);
    for diagnostic in local {
        push_diagnostic(diagnostics, data_diagnostic(path, text, diagnostic));
    }
    RecoveryDocument {
        kind: RecoveryDocumentKind::Data,
        objects: document
            .declarations
            .iter()
            .map(|(name, _, declaration)| RecoveryObject {
                key: name.value.as_str().to_string(),
                kind: declaration.kind(),
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
