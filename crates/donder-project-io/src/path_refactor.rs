use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::DocumentId;
use tempfile::Builder;

use crate::serialization::document_text;
use crate::source::ProjectSession;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathChangeSourceKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathChangeImpact {
    pub documents: Vec<String>,
    pub imports: Vec<String>,
    pub assets: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathChangePlan {
    pub source: Utf8PathBuf,
    pub destination: Utf8PathBuf,
    pub source_kind: PathChangeSourceKind,
    pub structural: bool,
    pub impact: PathChangeImpact,
    document_remaps: BTreeMap<DocumentId, DocumentId>,
}

impl PathChangePlan {
    pub fn remap_object_identity(
        &self,
        identity: &donder_language::identity::ObjectIdentity,
    ) -> donder_language::identity::ObjectIdentity {
        donder_language::source_remap::remap_object_identity(identity, &self.document_remaps)
    }

    pub fn remap_identity(
        &self,
        identity: &donder_language::identity::SourceIdentity,
    ) -> donder_language::identity::SourceIdentity {
        donder_language::source_remap::remap_identity(identity, &self.document_remaps)
    }
}

pub fn plan_path_change(
    session: &ProjectSession,
    source: &Utf8Path,
    destination: &Utf8Path,
) -> Result<PathChangePlan, String> {
    let project_root = session.source.project_root();
    let source = normalize_relative(source)?;
    let destination = normalize_relative(destination)?;
    if source.as_str().is_empty() || destination.as_str().is_empty() {
        return Err("The project root cannot be moved or renamed.".to_string());
    }
    if destination == source || destination.starts_with(&source) {
        return Err("A path cannot be moved into itself or one of its descendants.".to_string());
    }

    let source_absolute = checked_existing_path(project_root, &source)?;
    let destination_absolute = checked_destination(project_root, &destination)?;
    if destination_absolute.exists() {
        return Err(format!("Destination already exists: {destination}"));
    }
    let metadata = fs::metadata(&source_absolute).map_err(|error| error.to_string())?;
    let source_kind = if metadata.is_dir() {
        PathChangeSourceKind::Directory
    } else if metadata.is_file() {
        PathChangeSourceKind::File
    } else {
        return Err("Only regular files and directories can be moved.".to_string());
    };

    if source.as_str() == crate::PROJECT_ROOT_FILE {
        return Err("project.donder must remain at the project root.".into());
    }
    let document_remaps = session
        .source
        .documents
        .keys()
        .filter_map(|document| {
            replace_prefix(document.path(), &source, &destination).map(|path| {
                (
                    document.clone(),
                    DocumentId::new(document.module_id(), path),
                )
            })
        })
        .collect::<BTreeMap<_, _>>();
    let assets = session
        .source
        .referenced_assets
        .iter()
        .filter(|asset| replace_prefix(&asset.relative_path, &source, &destination).is_some())
        .map(|asset| asset.relative_path.to_string())
        .collect::<Vec<_>>();
    let imports = session
        .source
        .documents
        .iter()
        .filter(|(_, document)| {
            document.imports().iter().any(|edge| {
                edge.targets()
                    .iter()
                    .any(|target| document_remaps.contains_key(target))
            })
        })
        .map(|(id, _)| id.path().to_string())
        .collect();
    let structural = !document_remaps.is_empty() || !assets.is_empty();
    Ok(PathChangePlan {
        source,
        destination,
        source_kind,
        structural,
        impact: PathChangeImpact {
            documents: document_remaps
                .keys()
                .map(|id| id.path().to_string())
                .collect(),
            imports,
            assets,
        },
        document_remaps,
    })
}

pub fn apply_path_change(
    session: &ProjectSession,
    plan: &PathChangePlan,
) -> Result<ProjectSession, String> {
    let fresh = plan_path_change(session, &plan.source, &plan.destination)?;
    if &fresh != plan {
        return Err("The path-change plan is stale; plan the operation again.".to_string());
    }
    let project_root = session.source.project_root().to_path_buf();
    let source_absolute = project_root.join(&plan.source);
    let destination_absolute = project_root.join(&plan.destination);
    if !plan.structural {
        if let Some(parent) = destination_absolute.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::rename(&source_absolute, &destination_absolute)
            .map_err(|error| format!("Failed to apply path change: {error}"))?;
        return Ok(session.clone());
    }

    let mut candidate = session.clone();
    remap_candidate(&mut candidate, plan)?;
    crate::serialization::validate_source_inventory(&candidate)
        .map_err(|error| error.to_string())?;
    let prepared = prepare_writes(&candidate)?;

    let temporary = Builder::new()
        .prefix(".donder-path-refactor-")
        .tempdir_in(&project_root)
        .map_err(|error| format!("Failed to stage path change: {error}"))?;
    let staged = Utf8Path::from_path(temporary.path())
        .ok_or_else(|| "Temporary path is not valid UTF-8.".to_string())?
        .join("payload");

    fs::rename(&source_absolute, &staged)
        .map_err(|error| format!("Failed to stage `{}`: {error}", plan.source))?;
    let move_result = (|| {
        if let Some(parent) = destination_absolute.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&staged, &destination_absolute)
    })();
    if let Err(error) = move_result {
        let message = format!(
            "Failed to move `{}` to `{}`: {error}",
            plan.source, plan.destination
        );
        if let Err(restore_error) = fs::rename(&staged, &source_absolute) {
            let retained = temporary.keep();
            return Err(format!(
                "{message}; restoring the source also failed: {restore_error}; original source retained at {}",
                retained.join("payload").display()
            ));
        }
        return Err(message);
    }

    let mut backups = BTreeMap::new();
    let mut written = BTreeSet::new();
    let result: Result<(), String> = (|| {
        for (path, bytes) in prepared {
            backup_path(&mut backups, &path)?;
            write_bytes(&path, &bytes)?;
            written.insert(path);
        }
        validate_candidate(&candidate)?;
        Ok(())
    })();

    if let Err(error) = result {
        let write_rollback = rollback_writes(&backups, &written);
        let move_rollback = fs::rename(&destination_absolute, &source_absolute);
        let rollback_error = write_rollback.err().or_else(|| move_rollback.err());
        return Err(rollback_error.map_or(error.clone(), |rollback_error| {
            format!("{error}; rollback also failed: {rollback_error}")
        }));
    }
    Ok(candidate)
}

fn remap_candidate(candidate: &mut ProjectSession, plan: &PathChangePlan) -> Result<(), String> {
    donder_language::source_remap::remap_document_paths(
        &mut candidate.project,
        &plan.document_remaps,
    );
    crate::source_copy::remap_documents(&mut candidate.source, &plan.document_remaps)?;

    let root = candidate.source.project_root().to_owned();
    for asset in &mut candidate.source.referenced_assets {
        asset.referenced_by = asset
            .referenced_by
            .iter()
            .map(|id| {
                plan.document_remaps
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| id.clone())
            })
            .collect();
        if let Some(next) = replace_prefix(&asset.relative_path, &plan.source, &plan.destination) {
            asset.relative_path = next;
        }
        asset.absolute_path = root.join(&asset.relative_path);
    }
    Ok(())
}

fn prepare_writes(candidate: &ProjectSession) -> Result<BTreeMap<Utf8PathBuf, Vec<u8>>, String> {
    let mut writes = BTreeMap::new();
    for (id, document) in &candidate.source.documents {
        let text = document_text(candidate, id, document).map_err(|error| error.to_string())?;
        writes.insert(
            candidate.source.project_root().join(id.path()),
            text.into_bytes(),
        );
    }
    Ok(writes)
}

fn validate_candidate(candidate: &ProjectSession) -> Result<(), String> {
    candidate.source.workspace.metadata.validate()?;
    crate::serialization::validate_source_inventory(candidate).map_err(|error| error.to_string())
}

fn normalize_relative(path: &Utf8Path) -> Result<Utf8PathBuf, String> {
    if path.is_absolute() || path.as_str().contains('\\') {
        return Err("Workspace paths must be project-relative and use `/` separators.".to_string());
    }
    let mut normalized = Utf8PathBuf::new();
    for component in path.components() {
        match component {
            camino::Utf8Component::Normal(part) => normalized.push(part),
            camino::Utf8Component::CurDir => {}
            camino::Utf8Component::ParentDir
            | camino::Utf8Component::RootDir
            | camino::Utf8Component::Prefix(_) => {
                return Err("Workspace paths cannot escape the project root.".to_string());
            }
        }
    }
    Ok(Utf8PathBuf::from(normalized.as_str().replace('\\', "/")))
}

fn checked_existing_path(root: &Utf8Path, relative: &Utf8Path) -> Result<Utf8PathBuf, String> {
    let canonical_root = root
        .canonicalize_utf8()
        .map_err(|error| error.to_string())?;
    let canonical = root
        .join(relative)
        .canonicalize_utf8()
        .map_err(|error| error.to_string())?;
    if !canonical.starts_with(&canonical_root) {
        return Err("Source path escapes the project root.".to_string());
    }
    Ok(canonical)
}

fn checked_destination(root: &Utf8Path, relative: &Utf8Path) -> Result<Utf8PathBuf, String> {
    let canonical_root = root
        .canonicalize_utf8()
        .map_err(|error| error.to_string())?;
    let destination = root.join(relative);
    let parent = destination
        .parent()
        .ok_or_else(|| "Destination has no parent directory.".to_string())?;
    let canonical_parent = parent
        .canonicalize_utf8()
        .map_err(|error| error.to_string())?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("Destination path escapes the project root.".to_string());
    }
    Ok(canonical_parent.join(
        destination
            .file_name()
            .ok_or_else(|| "Destination has no file name.".to_string())?,
    ))
}

fn replace_prefix(path: &Utf8Path, from: &Utf8Path, to: &Utf8Path) -> Option<Utf8PathBuf> {
    let suffix = path.strip_prefix(from).ok()?;
    let replaced = if suffix.as_str().is_empty() {
        to.to_path_buf()
    } else {
        to.join(suffix)
    };
    Some(logical_path(&replaced))
}

fn logical_path(path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(path.as_str().replace('\\', "/"))
}

fn backup_path(
    backups: &mut BTreeMap<Utf8PathBuf, Option<Vec<u8>>>,
    path: &Utf8Path,
) -> Result<(), String> {
    if backups.contains_key(path) {
        return Ok(());
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("Failed to back up `{path}`: {error}")),
    };
    backups.insert(path.to_path_buf(), bytes);
    Ok(())
}

fn write_bytes(path: &Utf8Path, bytes: &[u8]) -> Result<(), String> {
    crate::atomic_write(path, bytes).map_err(|error| format!("Failed to write `{path}`: {error}"))
}

fn rollback_writes(
    backups: &BTreeMap<Utf8PathBuf, Option<Vec<u8>>>,
    written: &BTreeSet<Utf8PathBuf>,
) -> io::Result<()> {
    for (path, bytes) in backups.iter().rev() {
        if !written.contains(path) {
            continue;
        }
        match bytes {
            Some(bytes) => {
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                crate::atomic_write(path, bytes).map_err(io::Error::other)?;
            }
            None => match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            },
        }
    }
    Ok(())
}
