use std::collections::BTreeMap;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};

use crate::dto::{WorkspaceEntry, WorkspaceEntryKind, WorkspaceEntryRole, WorkspaceOperation};
use donder_project_io::{ProjectRecovery, ProjectSession};

#[derive(Clone, Copy)]
pub(crate) enum FsEntryKind {
    File,
    Directory,
}

pub(crate) fn canonical_relative_path(path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.as_str().replace(std::path::MAIN_SEPARATOR, "/"))
}

pub(crate) fn collect_workspace_paths(
    root: &Utf8Path,
    relative: &Utf8Path,
    paths: &mut BTreeMap<Utf8PathBuf, FsEntryKind>,
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
            FsEntryKind::Directory
        } else {
            FsEntryKind::File
        };
        paths.insert(path.clone(), kind);
        if matches!(kind, FsEntryKind::Directory) {
            collect_workspace_paths(root, &path, paths);
        }
    }
}

pub(crate) fn insert_path_with_parents(
    paths: &mut BTreeMap<Utf8PathBuf, FsEntryKind>,
    path: &Utf8Path,
) {
    let mut current = Utf8PathBuf::new();
    let path = canonical_relative_path(path);
    for component in path.components() {
        let camino::Utf8Component::Normal(part) = component else {
            continue;
        };
        current.push(part);
        current = canonical_relative_path(&current);
        let kind = if current == path {
            FsEntryKind::File
        } else {
            FsEntryKind::Directory
        };
        paths.entry(current.clone()).or_insert(kind);
    }
}

pub(crate) fn workspace_entries(session: &ProjectSession) -> Vec<WorkspaceEntry> {
    let mut paths = BTreeMap::new();
    collect_workspace_paths(session.source.project_root(), Utf8Path::new(""), &mut paths);
    for document in session.source.documents.keys() {
        if session.source.is_project_owned(document) {
            insert_path_with_parents(&mut paths, document.path());
        }
    }
    paths
        .into_iter()
        .map(|(path, kind)| workspace_entry(session, path, kind))
        .collect()
}

pub(crate) fn recovery_workspace_entries(recovery: &ProjectRecovery) -> Vec<WorkspaceEntry> {
    let mut paths = BTreeMap::new();
    collect_workspace_paths(&recovery.root, Utf8Path::new(""), &mut paths);
    for path in recovery.documents.keys() {
        insert_path_with_parents(&mut paths, path);
    }
    paths
        .into_iter()
        .map(|(path, kind)| {
            let name = path
                .file_name()
                .map(ToString::to_string)
                .unwrap_or_else(|| path.to_string());
            let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
            let role = recovery_workspace_role(recovery, &path, kind);
            let fixed = matches!(role, WorkspaceEntryRole::Configuration);
            WorkspaceEntry {
                path: canonical_relative_path(&path).to_string(),
                kind: match kind {
                    FsEntryKind::Directory => WorkspaceEntryKind::Directory,
                    FsEntryKind::File => WorkspaceEntryKind::File,
                },
                name,
                parent,
                role,
                operations: match kind {
                    FsEntryKind::File => vec![WorkspaceOperation::Open],
                    FsEntryKind::Directory => Vec::new(),
                },
                operation_explanation: Some(if fixed {
                    "Project configuration remains at the project root.".to_string()
                } else {
                    "Project-model operations are disabled until project errors are fixed."
                        .to_string()
                }),
            }
        })
        .collect()
}

fn recovery_workspace_role(
    recovery: &ProjectRecovery,
    path: &Utf8Path,
    kind: FsEntryKind,
) -> WorkspaceEntryRole {
    if matches!(kind, FsEntryKind::Directory) {
        return WorkspaceEntryRole::Directory;
    }
    if path == Utf8Path::new(donder_project_io::PROJECT_CONFIG_FILE) {
        return WorkspaceEntryRole::Configuration;
    }
    let Some(document) = recovery.documents.get(path) else {
        return WorkspaceEntryRole::File;
    };
    match document.kind {
        donder_project_io::RecoveryDocumentKind::Effect => WorkspaceEntryRole::Effect,
        donder_project_io::RecoveryDocumentKind::Operator => WorkspaceEntryRole::Operator,
        donder_project_io::RecoveryDocumentKind::Other => WorkspaceEntryRole::File,
        donder_project_io::RecoveryDocumentKind::Donder => document
            .objects
            .iter()
            .map(|object| crate::dto::workspace_role_for_source_object(&object.kind))
            .next()
            .unwrap_or(WorkspaceEntryRole::File),
    }
}

fn workspace_entry(
    session: &ProjectSession,
    path: Utf8PathBuf,
    kind: FsEntryKind,
) -> WorkspaceEntry {
    let name = path
        .file_name()
        .map(ToString::to_string)
        .unwrap_or_else(|| path.to_string());
    let parent = path.parent().map(Utf8Path::to_string).unwrap_or_default();
    let role = workspace_role(session, &path, kind);
    let fixed = matches!(role, WorkspaceEntryRole::Configuration);
    let structural = session.source.is_structural_workspace_path(&path);
    let operations = match kind {
        FsEntryKind::Directory => {
            let mut operations = vec![WorkspaceOperation::Create];
            if !fixed {
                operations.extend([WorkspaceOperation::Rename, WorkspaceOperation::Move]);
                if !structural {
                    operations.push(WorkspaceOperation::Delete);
                }
            }
            operations
        }
        FsEntryKind::File => {
            let mut operations = vec![WorkspaceOperation::Open];
            if !fixed {
                operations.extend([WorkspaceOperation::Rename, WorkspaceOperation::Move]);
                if !structural {
                    operations.push(WorkspaceOperation::Delete);
                }
            }
            operations
        }
    };
    WorkspaceEntry {
        path: canonical_relative_path(&path).to_string(),
        kind: match kind {
            FsEntryKind::Directory => WorkspaceEntryKind::Directory,
            FsEntryKind::File => WorkspaceEntryKind::File,
        },
        name,
        parent,
        role,
        operations,
        operation_explanation: if fixed {
            Some("Project configuration remains at the project root.".to_string())
        } else if structural {
            Some(
                "Imported documents cannot be deleted; rename or move them through the typed path workflow."
                    .to_string(),
            )
        } else {
            None
        },
    }
}

fn workspace_role(
    session: &ProjectSession,
    relative: &Utf8Path,
    kind: FsEntryKind,
) -> WorkspaceEntryRole {
    if matches!(kind, FsEntryKind::Directory) {
        return WorkspaceEntryRole::Directory;
    }
    let module_id = session.source.project_module_id();
    if relative == Utf8Path::new(donder_project_io::PROJECT_CONFIG_FILE) {
        return WorkspaceEntryRole::Configuration;
    }
    let document_id = donder_language::identity::DocumentId::new(module_id, relative.to_path_buf());
    if session.source.entrypoint.as_ref() == Some(&document_id) {
        return WorkspaceEntryRole::Entrypoint;
    }
    if let Some(document) = session.source.documents.get(&document_id) {
        return match document.kind() {
            donder_project_io::SourceDocumentKind::Effect { .. } => WorkspaceEntryRole::Effect,
            donder_project_io::SourceDocumentKind::Operator { .. } => WorkspaceEntryRole::Operator,
            donder_project_io::SourceDocumentKind::Donder { .. } => document
                .objects()
                .iter()
                .map(|object| crate::dto::workspace_role_for_source_object(object.kind()))
                .next()
                .unwrap_or(WorkspaceEntryRole::File),
        };
    }
    if session
        .source
        .referenced_assets
        .iter()
        .any(|asset| asset.module_id == module_id && asset.relative_path == relative)
    {
        WorkspaceEntryRole::Asset
    } else {
        WorkspaceEntryRole::File
    }
}
