use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use donder_editor::dto::{
    ObjectKind, WorkspaceEntry, WorkspaceEntryKind, WorkspaceEntryPlainRole, WorkspaceEntryRole,
    WorkspaceOperation,
};
use donder_project_io::{ProjectRecovery, ProjectSession, SourceDocumentFormat};

fn canonical_relative_path(path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.as_str().replace(std::path::MAIN_SEPARATOR, "/"))
}

fn collect_workspace_paths(
    root: &Utf8Path,
    relative: &Utf8Path,
    paths: &mut BTreeMap<Utf8PathBuf, WorkspaceEntryKind>,
) {
    let absolute = root.join(relative);
    let Ok(entries) = fs::read_dir(absolute) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        let path = canonical_relative_path(&relative.join(name));
        let kind = if entry.file_type().is_ok_and(|file_type| file_type.is_dir()) {
            WorkspaceEntryKind::Directory
        } else {
            WorkspaceEntryKind::File
        };
        paths.insert(path.clone(), kind);
        if kind == WorkspaceEntryKind::Directory {
            collect_workspace_paths(root, &path, paths);
        }
    }
}

/// Every file and folder under `root`, with each document and its folders.
fn workspace_paths<'a>(
    root: &Utf8Path,
    documents: impl Iterator<Item = &'a Utf8Path>,
) -> BTreeMap<Utf8PathBuf, WorkspaceEntryKind> {
    let mut paths = BTreeMap::new();
    collect_workspace_paths(root, Utf8Path::new(""), &mut paths);
    for document in documents {
        let document = canonical_relative_path(document);
        let mut current = Utf8PathBuf::new();
        for component in document.components() {
            let camino::Utf8Component::Normal(part) = component else {
                continue;
            };
            current = canonical_relative_path(&current.join(part));
            let kind = if current == document {
                WorkspaceEntryKind::File
            } else {
                WorkspaceEntryKind::Directory
            };
            paths.entry(current.clone()).or_insert(kind);
        }
    }
    paths
}

/// What a document is to the explorer: its format and its first object.
struct DocumentRole {
    format: SourceDocumentFormat,
    first_object: Option<ObjectKind>,
}

fn role(
    path: &Utf8Path,
    kind: WorkspaceEntryKind,
    document: Option<DocumentRole>,
    asset: bool,
) -> WorkspaceEntryRole {
    let plain = WorkspaceEntryRole::Entry;
    if kind == WorkspaceEntryKind::Directory {
        return plain(WorkspaceEntryPlainRole::Directory);
    }
    if path == Utf8Path::new(donder_project_io::PROJECT_ROOT_FILE) {
        return plain(WorkspaceEntryPlainRole::Entrypoint);
    }
    match document {
        Some(DocumentRole {
            first_object: Some(kind),
            ..
        }) => WorkspaceEntryRole::Object(kind),
        Some(DocumentRole {
            format: SourceDocumentFormat::Script,
            ..
        }) => WorkspaceEntryRole::Object(ObjectKind::Effect),
        Some(_) => plain(WorkspaceEntryPlainRole::File),
        None if asset => plain(WorkspaceEntryPlainRole::Asset),
        None => plain(WorkspaceEntryPlainRole::File),
    }
}

fn entry(
    path: Utf8PathBuf,
    kind: WorkspaceEntryKind,
    role: WorkspaceEntryRole,
    operations: Vec<WorkspaceOperation>,
    operation_explanation: Option<String>,
) -> WorkspaceEntry {
    WorkspaceEntry {
        name: path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| path.to_string()),
        parent: path.parent().map(Utf8Path::to_string).unwrap_or_default(),
        path: canonical_relative_path(&path).to_string(),
        kind,
        role,
        operations,
        operation_explanation,
    }
}

const ROOT_FIXED: &str = "project.data.donder must remain at the project root.";

pub(crate) fn workspace_entries(session: &ProjectSession) -> Vec<WorkspaceEntry> {
    let documents = session.source.documents.keys().map(|id| id.path());
    workspace_paths(session.source.project_root(), documents)
        .into_iter()
        .map(|(path, kind)| {
            let document = session
                .source
                .documents
                .get(&donder_project_io::DocumentId::new(path.clone()))
                .map(|document| DocumentRole {
                    format: document.format(),
                    first_object: document.objects().first().map(|object| object.kind().into()),
                });
            let asset = session
                .source
                .referenced_assets
                .iter()
                .any(|asset| asset.relative_path == path);
            let role = role(&path, kind, document, asset);
            let fixed = role == WorkspaceEntryRole::Entry(WorkspaceEntryPlainRole::Entrypoint);
            let structural = session.source.is_structural_workspace_path(&path);
            let mut operations = vec![match kind {
                WorkspaceEntryKind::Directory => WorkspaceOperation::Create,
                WorkspaceEntryKind::File => WorkspaceOperation::Open,
            }];
            if !fixed {
                operations.extend([WorkspaceOperation::Rename, WorkspaceOperation::Move]);
                if !structural {
                    operations.push(WorkspaceOperation::Delete);
                }
            }
            let explanation = if fixed {
                Some(ROOT_FIXED.to_string())
            } else if structural {
                Some(
                    "Imported documents cannot be deleted; rename or move them through the typed path workflow."
                        .to_string(),
                )
            } else {
                None
            };
            entry(path, kind, role, operations, explanation)
        })
        .collect()
}

pub(crate) fn recovery_workspace_entries(recovery: &ProjectRecovery) -> Vec<WorkspaceEntry> {
    let documents = recovery.documents.keys().map(Utf8PathBuf::as_path);
    workspace_paths(&recovery.root, documents)
        .into_iter()
        .map(|(path, kind)| {
            let document = recovery.documents.get(&path).map(|document| DocumentRole {
                format: document.format,
                first_object: document.objects.first().map(|object| object.kind.into()),
            });
            let role = role(&path, kind, document, false);
            let fixed = role == WorkspaceEntryRole::Entry(WorkspaceEntryPlainRole::Entrypoint);
            let operations = match kind {
                WorkspaceEntryKind::File => vec![WorkspaceOperation::Open],
                WorkspaceEntryKind::Directory => Vec::new(),
            };
            let explanation = if fixed {
                ROOT_FIXED
            } else {
                "Project-model operations are disabled until project errors are fixed."
            };
            entry(path, kind, role, operations, Some(explanation.to_string()))
        })
        .collect()
}
