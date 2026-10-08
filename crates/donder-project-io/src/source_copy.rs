use crate::{ExportReport, ProjectSession, SourceProject};
use camino::Utf8Path;
use donder_language::ImportSource;
use donder_model::DocumentId;
use std::collections::BTreeMap;

/// Rewrite document identities and imports using their resolved targets.
/// Local imports follow the renamed document paths.
pub(crate) fn remap_documents(
    source: &mut SourceProject,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) -> Result<(), String> {
    let mut documents = indexmap::IndexMap::new();
    for (old_id, mut document) in std::mem::take(&mut source.documents) {
        let next_id = remaps
            .get(&old_id)
            .cloned()
            .unwrap_or_else(|| old_id.clone());
        for import in &mut document.imports {
            for target in &mut import.targets {
                if let Some(next) = remaps.get(target) {
                    *target = next.clone();
                }
            }
            if import
                .targets
                .iter()
                .all(|target| target.module_id() == next_id.module_id())
            {
                import.declaration.source = ImportSource::LocalDocuments {
                    documents: import
                        .targets
                        .iter()
                        .map(|target| target.path().to_path_buf())
                        .collect(),
                };
            }
        }
        if documents.insert(next_id, document).is_some() {
            return Err("Copied document paths collide.".into());
        }
    }
    source.documents = documents;
    Ok(())
}

/// Copy the loaded project and its referenced local assets into a new folder.
/// All writes are staged before the destination becomes visible.
pub fn export_editable_project(
    session: &ProjectSession,
    destination: &Utf8Path,
) -> Result<ExportReport, String> {
    if destination.exists() {
        return Err("Choose a new folder for the project copy.".into());
    }
    let parent = destination
        .parent()
        .ok_or("The copy needs a parent directory.")?;
    if !parent.is_dir() {
        return Err("The copy's parent directory does not exist.".into());
    }
    let temporary = tempfile::Builder::new()
        .prefix(".donder-copy-")
        .tempdir_in(parent)
        .map_err(|error| error.to_string())?;
    let staged = Utf8Path::from_path(temporary.path())
        .ok_or("Copy path is not UTF-8.")?
        .join("project");
    std::fs::create_dir(&staged).map_err(|error| error.to_string())?;
    let report = crate::export_project(session, &staged).map_err(|error| error.to_string())?;
    crate::load_project(&staged)
        .map_err(|error| format!("Project copy could not be loaded: {error}"))?;
    std::fs::rename(&staged, destination)
        .map_err(|error| format!("Could not finish project copy: {error}"))?;
    Ok(report)
}
