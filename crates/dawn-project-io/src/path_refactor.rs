use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::identity::DocumentId;
use dawn_package::{
    Dependency, Lockfile, PackageManifest, ResolvedModule, ResolvedModuleOrigin,
    ResolvedSourceGraph,
};
use tempfile::Builder;
use uuid::Uuid;

use crate::serialization::document_text;
use crate::source::ProjectSession;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PathChangeSourceKind {
    File,
    Directory,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PathChangeOwnership {
    Project,
    PathDependency {
        module_id: Uuid,
        module_root: String,
    },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PathChangeImpact {
    pub documents: Vec<String>,
    pub imports: Vec<String>,
    pub manifests: Vec<String>,
    pub assets: Vec<String>,
    pub modules: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PathChangePlan {
    pub source: Utf8PathBuf,
    pub destination: Utf8PathBuf,
    pub source_kind: PathChangeSourceKind,
    pub ownership: PathChangeOwnership,
    pub structural: bool,
    pub impact: PathChangeImpact,
    document_remaps: BTreeMap<DocumentId, DocumentId>,
    module_root_remaps: BTreeMap<Uuid, Utf8PathBuf>,
}

impl PathChangePlan {
    pub fn remap_identity(
        &self,
        identity: &dawn_language::identity::SourceIdentity,
    ) -> dawn_language::identity::SourceIdentity {
        dawn_language::source_remap::remap_identity(identity, &self.document_remaps)
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

    let mutable_modules = mutable_modules(session)?;
    for (module_id, (module_root, _)) in &mutable_modules {
        let relative_root = module_root
            .strip_prefix(project_root)
            .map_err(|_| "A local module is outside the project root.".to_string())?;
        if source == relative_root.join(dawn_package::MANIFEST_FILE)
            || source == relative_root.join(dawn_package::LOCK_FILE)
        {
            return Err(format!(
                "{} and {} remain fixed at their module root.",
                dawn_package::MANIFEST_FILE,
                dawn_package::LOCK_FILE
            ));
        }
        if *module_id == session.source.project_module_id() && source == relative_root {
            return Err("The project root cannot be moved or renamed.".to_string());
        }
    }

    let module_root_remaps = mutable_modules
        .iter()
        .map(|(module_id, (root, _))| {
            let relative = root
                .strip_prefix(project_root)
                .map_err(|_| "A local module is outside the project root.".to_string())?;
            let relative = logical_path(relative);
            let next = replace_prefix(&relative, &source, &destination).unwrap_or(relative);
            Ok((*module_id, next))
        })
        .collect::<Result<BTreeMap<_, _>, String>>()?;

    let mut document_remaps = BTreeMap::new();
    let mut impacted_documents = BTreeSet::new();
    for document in session.source.documents.keys() {
        let Some((old_module_root, _)) = mutable_modules.get(&document.module_id()) else {
            continue;
        };
        let old_absolute = old_module_root.join(document.path());
        let old_project_relative = logical_path(
            old_absolute
                .strip_prefix(project_root)
                .map_err(|_| "A local document is outside the project root.".to_string())?,
        );
        let Some(next_project_relative) =
            replace_prefix(&old_project_relative, &source, &destination)
        else {
            continue;
        };
        let next_module_root = module_root_remaps
            .get(&document.module_id())
            .ok_or_else(|| "Document module is unavailable.".to_string())?;
        let next_document_path = logical_path(
            next_project_relative
                .strip_prefix(next_module_root)
                .map_err(|_| {
                    "Documents cannot be moved across package module boundaries.".to_string()
                })?,
        );
        if next_document_path.as_str().is_empty() {
            return Err("A module root cannot replace one of its documents.".to_string());
        }
        let next = DocumentId::new(document.module_id(), next_document_path);
        impacted_documents.insert(display_document(session, document));
        document_remaps.insert(document.clone(), next);
    }

    let moved_modules = module_root_remaps
        .iter()
        .filter_map(|(module_id, next)| {
            let (old, _) = mutable_modules.get(module_id)?;
            let old = old.strip_prefix(project_root).ok()?;
            (old != next).then(|| format!("{} -> {}", old, next))
        })
        .collect::<Vec<_>>();

    let mut impacted_assets = BTreeSet::new();
    for asset in &session.source.referenced_assets {
        let Some((module_root, _)) = mutable_modules.get(&asset.module_id) else {
            continue;
        };
        let absolute = module_root.join(&asset.relative_path);
        let relative = absolute
            .strip_prefix(project_root)
            .map_err(|_| "A local asset is outside the project root.".to_string())?;
        if replace_prefix(relative, &source, &destination).is_some() {
            impacted_assets.insert(relative.to_string());
        }
    }

    let mut impacted_imports = BTreeSet::new();
    for (document_id, document) in &session.source.documents {
        if document.imports().iter().any(|edge| {
            edge.targets()
                .iter()
                .any(|target| document_remaps.contains_key(target))
        }) {
            impacted_imports.insert(display_document(session, document_id));
        }
    }

    let impacted_manifests = impacted_manifest_paths(
        session,
        &mutable_modules,
        &module_root_remaps,
        &source,
        &destination,
    );
    let structural = !document_remaps.is_empty()
        || !moved_modules.is_empty()
        || !impacted_assets.is_empty()
        || !impacted_manifests.is_empty();

    let ownership = source_owner(
        session,
        &mutable_modules,
        &source_absolute,
        &module_root_remaps,
    )?;

    Ok(PathChangePlan {
        source,
        destination,
        source_kind,
        ownership,
        structural,
        impact: PathChangeImpact {
            documents: impacted_documents.into_iter().collect(),
            imports: impacted_imports.into_iter().collect(),
            manifests: impacted_manifests.into_iter().collect(),
            assets: impacted_assets.into_iter().collect(),
            modules: moved_modules,
        },
        document_remaps,
        module_root_remaps,
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
        .prefix(".dawn-path-refactor-")
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
        refresh_locks_and_graph(&mut candidate, &mut backups, &mut written)?;
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
    dawn_language::source_remap::remap_document_paths(
        &mut candidate.project,
        &plan.document_remaps,
    );
    if let Some(entrypoint) = candidate.source.entrypoint.as_mut()
        && let Some(next) = plan.document_remaps.get(entrypoint)
    {
        *entrypoint = next.clone();
    }

    crate::source_copy::remap_documents(&mut candidate.source, &plan.document_remaps)?;

    let old_assets = candidate.source.referenced_assets.clone();
    for asset in &mut candidate.source.referenced_assets {
        asset.referenced_by = asset
            .referenced_by
            .iter()
            .map(|document| {
                plan.document_remaps
                    .get(document)
                    .cloned()
                    .unwrap_or_else(|| document.clone())
            })
            .collect();
    }

    let project_root = candidate.source.project_root().to_path_buf();
    let old_modules = candidate.source.source_graph.modules().clone();
    for asset in &mut candidate.source.referenced_assets {
        let Some(module) = old_modules.get(&asset.module_id) else {
            continue;
        };
        if matches!(
            module.origin,
            ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            continue;
        }
        let old_project_relative = module
            .root
            .join(&asset.relative_path)
            .strip_prefix(&project_root)
            .map_err(|_| "A local asset is outside the project root.".to_string())?
            .to_path_buf();
        let next_project_relative =
            replace_prefix(&old_project_relative, &plan.source, &plan.destination)
                .unwrap_or(old_project_relative);
        let next_module_root = plan
            .module_root_remaps
            .get(&asset.module_id)
            .ok_or_else(|| "Asset module is unavailable.".to_string())?;
        asset.relative_path = logical_path(
            next_project_relative
                .strip_prefix(next_module_root)
                .map_err(|_| {
                    "Assets cannot be moved across package module boundaries.".to_string()
                })?,
        );
    }

    let mut modules = BTreeMap::new();
    for (module_id, mut module) in old_modules {
        let old_root_relative = logical_path(
            module
                .root
                .strip_prefix(&project_root)
                .map_err(|_| "A local module is outside the project root.".to_string())?,
        );
        if let Some(next_root) = plan.module_root_remaps.get(&module_id) {
            module.root = project_root.join(next_root);
            if let ResolvedModuleOrigin::PathDependency { declared_path, .. } = &mut module.origin {
                *declared_path = next_root.to_string();
            }
            remap_manifest_paths_for_move(
                &mut module.manifest,
                &old_root_relative,
                next_root,
                &plan.source,
                &plan.destination,
            )?;
        }
        modules.insert(module_id, module);
    }
    rewrite_manifests(
        &mut modules,
        &plan.document_remaps,
        &old_assets,
        &candidate.source.referenced_assets,
    )?;
    candidate.source.source_graph = ResolvedSourceGraph::from_modules_with_staged_roots(
        candidate.source.project_module_id(),
        modules,
    )
    .map_err(|error| error.to_string())?;
    for asset in &mut candidate.source.referenced_assets {
        let module = candidate
            .source
            .source_graph
            .module(asset.module_id)
            .map_err(|error| error.to_string())?;
        asset.absolute_path = module.root.join(&asset.relative_path);
    }
    Ok(())
}

fn rewrite_manifests(
    modules: &mut BTreeMap<Uuid, ResolvedModule>,
    document_remaps: &BTreeMap<DocumentId, DocumentId>,
    old_assets: &[crate::ReferencedAsset],
    new_assets: &[crate::ReferencedAsset],
) -> Result<(), String> {
    let roots = modules
        .iter()
        .map(|(id, module)| (*id, module.root.clone()))
        .collect::<BTreeMap<_, _>>();
    for module in modules.values_mut() {
        for (from, to) in document_remaps {
            if from.module_id() == module.manifest.module_id {
                remap_manifest_path(&mut module.manifest, from.path(), to.path());
            }
        }
        for (from, to) in old_assets.iter().zip(new_assets) {
            if from.module_id == module.manifest.module_id && from.relative_path != to.relative_path
            {
                remap_manifest_asset(&mut module.manifest, &from.relative_path, &to.relative_path);
            }
        }
        for (alias, dependency) in &mut module.manifest.dependencies {
            let Dependency::Path { path } = dependency else {
                continue;
            };
            let target_id = module.dependencies.get(alias).ok_or_else(|| {
                format!("Path dependency `{alias}` has no resolved target module.")
            })?;
            let target_root = roots
                .get(target_id)
                .ok_or_else(|| format!("Path dependency `{alias}` target is unavailable."))?;
            let relative = pathdiff::diff_utf8_paths(target_root, &module.root)
                .ok_or_else(|| format!("Cannot express path dependency `{alias}` relatively."))?;
            *path = logical_path(&relative).to_string();
        }
    }
    Ok(())
}

fn remap_manifest_paths_for_move(
    manifest: &mut PackageManifest,
    old_module_root: &Utf8Path,
    new_module_root: &Utf8Path,
    source: &Utf8Path,
    destination: &Utf8Path,
) -> Result<(), String> {
    let remap = |path: &str| -> Result<String, String> {
        let old_project_path = logical_path(&old_module_root.join(path));
        let next_project_path =
            replace_prefix(&old_project_path, source, destination).unwrap_or(old_project_path);
        Ok(logical_path(
            next_project_path
                .strip_prefix(new_module_root)
                .map_err(|_| "Manifest path moved across a package module boundary.".to_string())?,
        )
        .to_string())
    };
    if let Some(project) = &mut manifest.project {
        project.entrypoint = remap(&project.entrypoint)?;
    }
    for export in manifest.exports.values_mut() {
        for document in &mut export.documents {
            *document = remap(document)?;
        }
    }
    manifest.assets = std::mem::take(&mut manifest.assets)
        .into_iter()
        .map(|(path, declaration)| remap(&path).map(|path| (path, declaration)))
        .collect::<Result<_, _>>()?;
    Ok(())
}

fn prepare_writes(candidate: &ProjectSession) -> Result<BTreeMap<Utf8PathBuf, Vec<u8>>, String> {
    let mut writes = BTreeMap::new();
    for (document_id, document) in &candidate.source.documents {
        let module = candidate
            .source
            .source_graph
            .module(document_id.module_id())
            .map_err(|error| error.to_string())?;
        if matches!(
            module.origin,
            ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            continue;
        }
        let text =
            document_text(candidate, document_id, document).map_err(|error| error.to_string())?;
        writes.insert(module.root.join(document_id.path()), text.into_bytes());
    }
    for module in candidate.source.source_graph.modules().values() {
        if matches!(
            module.origin,
            ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            continue;
        }
        writes.insert(
            module.root.join(dawn_package::MANIFEST_FILE),
            dawn_package::canonical_json(&module.manifest).map_err(|error| error.to_string())?,
        );
    }
    Ok(writes)
}

fn remap_manifest_path(manifest: &mut PackageManifest, from: &Utf8Path, to: &Utf8Path) {
    let from = from.as_str().replace('\\', "/");
    let to = to.as_str().replace('\\', "/");
    if let Some(project) = &mut manifest.project
        && project.entrypoint == from
    {
        project.entrypoint = to.clone();
    }
    for export in manifest.exports.values_mut() {
        for document in &mut export.documents {
            if document == &from {
                *document = to.clone();
            }
        }
    }
}

fn remap_manifest_asset(manifest: &mut PackageManifest, from: &Utf8Path, to: &Utf8Path) {
    let from = from.as_str().replace('\\', "/");
    let to = to.as_str().replace('\\', "/");
    if let Some(declaration) = manifest.assets.remove(&from) {
        manifest.assets.insert(to, declaration);
    }
}

fn refresh_locks_and_graph(
    candidate: &mut ProjectSession,
    backups: &mut BTreeMap<Utf8PathBuf, Option<Vec<u8>>>,
    written: &mut BTreeSet<Utf8PathBuf>,
) -> Result<(), String> {
    let project_module_id = candidate.source.project_module_id();
    let old_modules = candidate.source.source_graph.modules().clone();
    let mut modules = old_modules.clone();
    let mut refreshed_root_lock = None;
    for (module_id, module) in &old_modules {
        if matches!(
            module.origin,
            ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            continue;
        }
        let lock_path = module.root.join(dawn_package::LOCK_FILE);
        if !lock_path.is_file() && *module_id != project_module_id {
            continue;
        }
        let previous = Lockfile::read(&module.root).map_err(|error| error.to_string())?;
        let mut refreshed =
            Lockfile::from_directory(&module.manifest, &module.root, previous.registry.clone())
                .map_err(|error| error.to_string())?;
        refreshed.packages = previous.packages;
        refreshed
            .validate_local(&module.root, &module.manifest)
            .map_err(|error| error.to_string())?;
        backup_path(backups, &lock_path)?;
        write_bytes(
            &lock_path,
            &dawn_package::canonical_json(&refreshed).map_err(|error| error.to_string())?,
        )?;
        written.insert(lock_path);
        if *module_id == project_module_id {
            refreshed_root_lock = Some(refreshed);
        }
    }

    if let Some(lock) = refreshed_root_lock {
        for module in modules.values_mut() {
            if let ResolvedModuleOrigin::PathDependency {
                declared_path,
                content_sha256,
            } = &mut module.origin
            {
                let path_lock = lock.path_dependencies.get(declared_path).ok_or_else(|| {
                    format!("Refreshed lock is missing local module `{declared_path}`.")
                })?;
                *content_sha256 = path_lock.content_sha256.clone();
            }
        }
    }
    candidate.source.source_graph = ResolvedSourceGraph::from_modules(project_module_id, modules)
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn validate_candidate(candidate: &ProjectSession) -> Result<(), String> {
    let project = candidate.source.source_graph.project_module();
    let lock = Lockfile::read(&project.root).map_err(|error| error.to_string())?;
    lock.validate_local(&project.root, &project.manifest)
        .map_err(|error| error.to_string())?;
    for (document_id, document) in &candidate.source.documents {
        let module = candidate
            .source
            .source_graph
            .module(document_id.module_id())
            .map_err(|error| error.to_string())?;
        if !matches!(
            module.origin,
            ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            let _ = document_text(candidate, document_id, document)
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

fn mutable_modules(
    session: &ProjectSession,
) -> Result<BTreeMap<Uuid, (Utf8PathBuf, PackageManifest)>, String> {
    Ok(session
        .source
        .source_graph
        .modules()
        .iter()
        .filter_map(|(id, module)| {
            (!matches!(
                module.origin,
                ResolvedModuleOrigin::RegistryDependency { .. }
            ))
            .then_some((*id, (module.root.clone(), module.manifest.clone())))
        })
        .collect())
}

fn source_owner(
    session: &ProjectSession,
    modules: &BTreeMap<Uuid, (Utf8PathBuf, PackageManifest)>,
    source: &Utf8Path,
    remaps: &BTreeMap<Uuid, Utf8PathBuf>,
) -> Result<PathChangeOwnership, String> {
    let mut owners = modules
        .iter()
        .filter(|(_, (root, _))| source.starts_with(root))
        .collect::<Vec<_>>();
    owners.sort_by_key(|(_, (root, _))| root.components().count());
    let Some((module_id, (module_root, _))) = owners.last() else {
        return Err("The source is not owned by the project or a local dependency.".to_string());
    };
    if **module_id == session.source.project_module_id() {
        Ok(PathChangeOwnership::Project)
    } else {
        Ok(PathChangeOwnership::PathDependency {
            module_id: **module_id,
            module_root: remaps
                .get(module_id)
                .cloned()
                .unwrap_or_else(|| module_root.to_path_buf())
                .to_string(),
        })
    }
}

fn impacted_manifest_paths(
    session: &ProjectSession,
    modules: &BTreeMap<Uuid, (Utf8PathBuf, PackageManifest)>,
    remaps: &BTreeMap<Uuid, Utf8PathBuf>,
    source: &Utf8Path,
    destination: &Utf8Path,
) -> BTreeSet<String> {
    let project_root = session.source.project_root();
    modules
        .iter()
        .filter_map(|(module_id, (root, manifest))| {
            let relative_root = root.strip_prefix(project_root).ok()?;
            let path_changed = remaps
                .get(module_id)
                .is_some_and(|next| next != relative_root);
            let owned_path_changed = manifest
                .project
                .iter()
                .map(|project| project.entrypoint.as_str())
                .chain(
                    manifest
                        .exports
                        .values()
                        .flat_map(|group| group.documents.iter().map(String::as_str)),
                )
                .chain(manifest.assets.keys().map(String::as_str))
                .any(|path| {
                    replace_prefix(&relative_root.join(path), source, destination).is_some()
                });
            (path_changed || owned_path_changed)
                .then(|| relative_root.join(dawn_package::MANIFEST_FILE).to_string())
        })
        .collect()
}

fn display_document(session: &ProjectSession, document: &DocumentId) -> String {
    session
        .source
        .module(document.module_id())
        .map(|module| module.root.join(document.path()).to_string())
        .unwrap_or_else(|| document.path().to_string())
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
    dawn_package::atomic_write(path, bytes)
        .map_err(|error| format!("Failed to write `{path}`: {error}"))
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
                dawn_package::atomic_write(path, bytes).map_err(io::Error::other)?;
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
