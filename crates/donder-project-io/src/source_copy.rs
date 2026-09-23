use crate::{ExportReport, ProjectSession, SourceDocumentKind, SourceProject};
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::DocumentId;
use donder_language::imports::ImportSource;
use std::collections::BTreeMap;

/// Rewrite document identities and imports using their resolved targets.
/// Module relocation converts dependency imports into local document imports.
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
        if let SourceDocumentKind::Effect { source: text } = &mut document.kind {
            let imports = donder_language::dsl::effect_source_imports(text)
                .map_err(|errors| format!("Cannot remap effect imports: {errors:?}"))?;
            for import in imports.into_iter().rev() {
                let rewritten = document
                    .imports
                    .iter()
                    .find(|edge| edge.declaration.alias == import.declaration.alias)
                    .ok_or("Effect import is missing from source metadata.")?;
                if rewritten.declaration.source == import.declaration.source {
                    continue;
                }
                let ImportSource::LocalDocuments { documents: paths } =
                    &rewritten.declaration.source
                else {
                    return Err(
                        "An effect import cannot be rewritten to another dependency.".into(),
                    );
                };
                match import.declaration.source {
                    ImportSource::LocalDocuments { .. } => {
                        if paths.len() != import.source_spans.len() {
                            return Err("Effect import target count changed.".into());
                        }
                        for (path, span) in paths.iter().zip(import.source_spans).rev() {
                            let quoted = serde_json::to_string(path.as_str())
                                .map_err(|error| error.to_string())?;
                            text.replace_range(span.start..span.end, &quoted);
                        }
                    }
                    ImportSource::DependencyExport { .. } => {
                        let first = import
                            .source_spans
                            .first()
                            .ok_or("Dependency import has no source span.")?;
                        let last = import
                            .source_spans
                            .last()
                            .ok_or("Dependency import has no source span.")?;
                        let local = serde_json::to_string(
                            &paths.iter().map(|path| path.as_str()).collect::<Vec<_>>(),
                        )
                        .map_err(|error| error.to_string())?;
                        text.replace_range(first.start..last.end, &local);
                    }
                }
            }
        }
        if documents.insert(next_id, document).is_some() {
            return Err("Copied document paths collide.".into());
        }
    }
    source.documents = documents;
    Ok(())
}

/// Export the loaded show and its complete imported source graph as one editable
/// project. All writes are staged; the destination must not already exist.
pub fn export_editable_project(
    session: &ProjectSession,
    destination: &Utf8Path,
) -> Result<ExportReport, String> {
    if destination.exists() {
        return Err("Choose a new folder for the editable project copy.".into());
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
    let mut copied = session.clone();
    let project_module = session.source.project_module_id();
    let remaps = session
        .source
        .documents
        .keys()
        .filter(|id| id.module_id() != project_module)
        .map(|id| {
            (
                id.clone(),
                DocumentId::new(project_module, dependency_path(id.module_id(), id.path())),
            )
        })
        .collect::<BTreeMap<_, _>>();
    donder_language::source_remap::remap_document_paths(&mut copied.project, &remaps);
    remap_documents(&mut copied.source, &remaps)?;
    let mut manifest = session
        .source
        .source_graph
        .project_module()
        .manifest
        .clone();
    manifest.dependencies.clear();
    manifest.assets.clear();
    let entrypoint = copied
        .source
        .entrypoint
        .as_ref()
        .ok_or("Project entrypoint is missing.")?;
    manifest.project = Some(donder_package::ProjectManifest {
        entrypoint: entrypoint.path().to_string(),
    });
    manifest.exports = BTreeMap::from([(
        "project".into(),
        donder_package::ExportGroup {
            documents: vec![entrypoint.path().to_string()],
        },
    )]);
    manifest.publication = None;
    let mut assets = Vec::new();
    for asset in &mut copied.source.referenced_assets {
        let original = asset.absolute_path.clone();
        let module = session
            .source
            .source_graph
            .module(asset.module_id)
            .map_err(|error| error.to_string())?;
        let declaration = module
            .manifest
            .assets
            .get(asset.relative_path.as_str())
            .ok_or("Audio asset declaration is missing.")?
            .clone();
        if asset.module_id != project_module {
            asset.relative_path = dependency_path(asset.module_id, &asset.relative_path);
        }
        asset.module_id = project_module;
        asset.referenced_by = asset
            .referenced_by
            .iter()
            .map(|id| remaps.get(id).cloned().unwrap_or_else(|| id.clone()))
            .collect();
        asset.absolute_path = staged.join(&asset.relative_path);
        if manifest
            .assets
            .insert(asset.relative_path.to_string(), declaration)
            .is_some()
        {
            return Err("Copied audio asset paths collide.".into());
        }
        assets.push((original, asset.relative_path.clone()));
    }
    copied.source.source_graph =
        donder_package::ResolvedSourceGraph::project(&staged, manifest.clone())
            .map_err(|error| error.to_string())?;
    let mut written_files = crate::serialization::write_source_documents(&copied, &staged)
        .map_err(|error| error.to_string())?;
    let mut copied_assets = Vec::new();
    for (original, path) in assets {
        let target = staged.join(&path);
        if target.exists() {
            return Err("A copied audio asset conflicts with a source document.".into());
        }
        std::fs::create_dir_all(
            target
                .parent()
                .ok_or("Audio asset has no parent directory.")?,
        )
        .map_err(|error| error.to_string())?;
        std::fs::copy(&original, target)
            .map_err(|error| format!("Could not copy audio `{original}`: {error}"))?;
        copied_assets.push(path);
    }
    manifest.write(&staged).map_err(|error| error.to_string())?;
    let registry = donder_package::Lockfile::read(session.source.project_root())
        .map_err(|error| error.to_string())?
        .registry;
    donder_package::Lockfile::new(&manifest, &registry)
        .map_err(|error| error.to_string())?
        .write(&staged)
        .map_err(|error| error.to_string())?;
    crate::load_package(&staged)
        .map_err(|error| format!("Editable copy could not be loaded: {error:?}"))?;
    std::fs::rename(&staged, destination)
        .map_err(|error| format!("Could not finish editable copy: {error}"))?;
    written_files.extend([
        donder_package::MANIFEST_FILE.into(),
        donder_package::LOCK_FILE.into(),
    ]);
    Ok(ExportReport {
        written_files,
        copied_assets,
    })
}

fn dependency_path(module: uuid::Uuid, path: &Utf8Path) -> Utf8PathBuf {
    Utf8PathBuf::from(format!(
        "dependencies/{module}/{}",
        path.as_str().replace('\\', "/")
    ))
}
