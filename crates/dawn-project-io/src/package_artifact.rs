use crate::{
    CompiledPackage, PackageLoadError, SourceObjectKind, compile_package,
    compile_package_with_cache,
};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

pub fn validate_registry_package_artifact(
    package_root: &Utf8Path,
    package: &dawn_package::PackageId,
    locked: &dawn_package::LockedPackage,
    global_lock: &dawn_package::Lockfile,
    cache: &dawn_package::CacheStore,
) -> Result<(), dawn_package::PackageError> {
    if global_lock.packages.get(package) != Some(locked) {
        return Err(dawn_package::PackageError::Invalid(format!(
            "artifact validation received a lock entry that does not match `{package}`"
        )));
    }

    let manifest = dawn_package::PackageManifest::read(package_root)?;
    let publication = manifest.publication.as_ref().ok_or_else(|| {
        dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` has no publication identity",
            locked.version
        ))
    })?;
    if publication.package != *package
        || publication.version != locked.version
        || manifest.module_id != locked.module_id
    {
        return Err(dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` does not match dawn.lock",
            locked.version
        )));
    }

    let declared_dependencies = manifest
        .dependencies
        .iter()
        .map(|(alias, dependency)| match dependency {
            dawn_package::Dependency::Registry {
                package: dependency,
                version,
            } => {
                let selected = global_lock.packages.get(dependency).ok_or_else(|| {
                    dawn_package::PackageError::Invalid(format!(
                        "cached package `{package}` points to unlocked dependency `{dependency}`"
                    ))
                })?;
                if !version.matches(&selected.version) {
                    return Err(dawn_package::PackageError::Invalid(format!(
                        "locked `{dependency}@{}` does not satisfy `{version}` required by `{package}`",
                        selected.version
                    )));
                }
                Ok((alias.clone(), dependency.clone()))
            }
            dawn_package::Dependency::Path { .. } => {
                Err(dawn_package::PackageError::Invalid(format!(
                    "cached registry package `{package}` contains a path dependency"
                )))
            }
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    if declared_dependencies != locked.dependencies {
        return Err(dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` dependency edges do not match dawn.lock",
            locked.version
        )));
    }

    let mut visiting = vec![package.clone()];
    let mut closure = std::collections::BTreeSet::new();
    for dependency in locked.dependencies.values() {
        collect_registry_package_closure(dependency, global_lock, &mut visiting, &mut closure)?;
    }
    let packages = closure
        .into_iter()
        .map(|dependency| {
            let locked_dependency =
                global_lock
                    .packages
                    .get(&dependency)
                    .cloned()
                    .ok_or_else(|| {
                        dawn_package::PackageError::Invalid(format!(
                            "locked package graph points to missing package `{dependency}`"
                        ))
                    })?;
            Ok((dependency, locked_dependency))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, dawn_package::PackageError>>()?;
    let artifact_lock = dawn_package::Lockfile {
        lock_version: global_lock.lock_version,
        manifest_sha256: dawn_package::manifest_hash(&manifest)?,
        registry: global_lock.registry.clone(),
        packages,
        path_dependencies: std::collections::BTreeMap::new(),
    };

    let compiled = compile_package_with_cache(package_root, manifest, artifact_lock, cache)
        .map_err(|error| {
            dawn_package::PackageError::Invalid(format!(
                "package `{package}@{}` failed compiler validation: {error}",
                locked.version
            ))
        })?;
    let receipt = fs::read(package_root.join("dawn-release.json"))?;
    let receipt = serde_json::from_slice::<dawn_package::ReleaseReceipt>(&receipt)?;
    let compiled_exports = release_export_index(&compiled).map_err(|error| {
        dawn_package::PackageError::Invalid(format!(
            "package `{package}@{}` failed compiler export validation: {error}",
            locked.version
        ))
    })?;
    if receipt.exports != compiled_exports {
        return Err(dawn_package::PackageError::Invalid(format!(
            "package `{package}@{}` release export index does not match compiler output",
            locked.version
        )));
    }
    Ok(())
}

fn collect_registry_package_closure(
    package: &dawn_package::PackageId,
    lockfile: &dawn_package::Lockfile,
    visiting: &mut Vec<dawn_package::PackageId>,
    closure: &mut std::collections::BTreeSet<dawn_package::PackageId>,
) -> Result<(), dawn_package::PackageError> {
    if closure.contains(package) {
        return Ok(());
    }
    if let Some(index) = visiting.iter().position(|entry| entry == package) {
        let mut cycle = visiting[index..]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        cycle.push(package.to_string());
        return Err(dawn_package::PackageError::Invalid(format!(
            "package dependency cycle while validating artifacts: {}",
            cycle.join(" -> ")
        )));
    }
    let locked = lockfile.packages.get(package).ok_or_else(|| {
        dawn_package::PackageError::Invalid(format!(
            "locked package graph points to missing package `{package}`"
        ))
    })?;
    visiting.push(package.clone());
    for dependency in locked.dependencies.values() {
        collect_registry_package_closure(dependency, lockfile, visiting, closure)?;
    }
    let _ = visiting.pop();
    closure.insert(package.clone());
    Ok(())
}

pub fn pack_package(root: &Utf8Path) -> Result<dawn_package::PackedRelease, PackageLoadError> {
    let compiled = compile_package(root)?;
    if compiled
        .graph
        .source
        .source_graph
        .modules()
        .values()
        .any(|module| {
            module
                .manifest
                .dependencies
                .values()
                .any(|dependency| matches!(dependency, dawn_package::Dependency::Path { .. }))
        })
    {
        return Err(dawn_package::PackageError::Invalid(
            "published dependency closure cannot contain path dependencies".to_string(),
        )
        .into());
    }
    let plan = release_archive_plan(&compiled)?;
    dawn_package::pack_directory_with_plan(root, &plan).map_err(Into::into)
}

fn release_archive_plan(
    compiled: &CompiledPackage,
) -> Result<dawn_package::ReleaseArchivePlan, PackageLoadError> {
    let project_module_id = compiled.manifest.module_id;
    let mut pending = std::collections::BTreeSet::new();
    for export in compiled.manifest.exports.values() {
        for document in &export.documents {
            pending.insert(dawn_language::identity::DocumentId::new(
                project_module_id,
                Utf8PathBuf::from(document),
            ));
        }
    }
    if let Some(project) = &compiled.manifest.project {
        pending.insert(dawn_language::identity::DocumentId::new(
            project_module_id,
            Utf8PathBuf::from(&project.entrypoint),
        ));
    }
    let mut reachable = std::collections::BTreeSet::new();
    while let Some(document_id) = pending.pop_first() {
        if document_id.module_id() != project_module_id
            || !reachable.insert(document_id.path().to_string())
        {
            continue;
        }
        let document = compiled
            .graph
            .source
            .documents
            .get(&document_id)
            .ok_or_else(|| {
                dawn_package::PackageError::Invalid(format!(
                    "release root `{}` was not compiled",
                    document_id.path()
                ))
            })?;
        for import in document.imports() {
            for target in import.targets() {
                if target.module_id() == project_module_id
                    && !reachable.contains(target.path().as_str())
                {
                    pending.insert(target.clone());
                }
            }
        }
    }
    reachable.insert(dawn_package::MANIFEST_FILE.to_string());
    reachable.extend(compiled.manifest.assets.keys().cloned());
    for metadata_path in ["README.md", "RELEASE_NOTES.md", "LICENSE"] {
        if compiled
            .graph
            .source
            .project_root()
            .join(metadata_path)
            .is_file()
        {
            reachable.insert(metadata_path.to_string());
        }
    }
    if compiled
        .manifest
        .publication
        .as_ref()
        .is_some_and(|publication| publication.license == "LicenseRef-Custom")
        && !reachable.contains("LICENSE")
    {
        return Err(dawn_package::PackageError::Invalid(
            "LicenseRef-Custom requires a root LICENSE file".to_string(),
        )
        .into());
    }

    Ok(dawn_package::ReleaseArchivePlan {
        files: reachable,
        exports: release_export_index(compiled)?,
    })
}

fn release_export_index(
    compiled: &CompiledPackage,
) -> Result<std::collections::BTreeMap<String, dawn_package::ReleaseExportGroup>, PackageLoadError>
{
    let project_module_id = compiled.manifest.module_id;
    let mut exports = std::collections::BTreeMap::new();
    for (group_name, group) in &compiled.manifest.exports {
        let mut objects = Vec::new();
        let mut object_names = std::collections::BTreeMap::new();
        for document in &group.documents {
            let document_id = dawn_language::identity::DocumentId::new(
                project_module_id,
                Utf8PathBuf::from(document),
            );
            let source = compiled
                .graph
                .source
                .documents
                .get(&document_id)
                .ok_or_else(|| {
                    dawn_package::PackageError::Invalid(format!(
                        "export document `{document}` was not compiled"
                    ))
                })?;
            for object in source.objects() {
                if let Some(previous_document) =
                    object_names.insert(object.id().to_string(), document.clone())
                {
                    return Err(dawn_package::PackageError::Invalid(format!(
                        "export group `{group_name}` exposes object `{}` from both `{previous_document}` and `{document}`",
                        object.id()
                    ))
                    .into());
                }
                objects.push(dawn_package::ReleaseExportObject {
                    document: document.clone(),
                    name: object.id().to_string(),
                    kind: release_object_kind(object.kind())?,
                });
            }
        }
        objects.sort();
        exports.insert(
            group_name.clone(),
            dawn_package::ReleaseExportGroup {
                documents: group.documents.clone(),
                objects,
            },
        );
    }
    Ok(exports)
}

fn release_object_kind(
    kind: &SourceObjectKind,
) -> Result<dawn_package::ExportObjectKind, PackageLoadError> {
    Ok(match kind {
        SourceObjectKind::Project => dawn_package::ExportObjectKind::Project,
        SourceObjectKind::Setup => dawn_package::ExportObjectKind::Setup,
        SourceObjectKind::Controller => dawn_package::ExportObjectKind::Controller,
        SourceObjectKind::Layout => dawn_package::ExportObjectKind::Layout,
        SourceObjectKind::Patch => dawn_package::ExportObjectKind::Patch,
        SourceObjectKind::FixtureDefinition => dawn_package::ExportObjectKind::FixtureDefinition,
        SourceObjectKind::Curve => dawn_package::ExportObjectKind::Curve,
        SourceObjectKind::Gradient => dawn_package::ExportObjectKind::Gradient,
        SourceObjectKind::Sequence => dawn_package::ExportObjectKind::Sequence,
        SourceObjectKind::EffectDefinition => dawn_package::ExportObjectKind::EffectDefinition,
        SourceObjectKind::OperatorDefinition => dawn_package::ExportObjectKind::OperatorDefinition,
        SourceObjectKind::EffectInstance => {
            return Err(dawn_package::PackageError::Invalid(
                "effect instances cannot be package exports".to_string(),
            )
            .into());
        }
    })
}
