use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use semver::Version;
use tempfile::Builder;
use uuid::Uuid;

use crate::{
    CacheStore, DawnDirectories, Dependency, LockedDependency, LockedPackage, Lockfile,
    PackageError, PackageId, PackageManifest, RegistryClient, RegistryConfig, ResolutionPin,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CandidateResolution {
    Sync,
    UpdateAll,
    UpdateAlias(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PackageVersionChangeKind {
    Added,
    Removed,
    Updated,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageVersionChange {
    pub package: PackageId,
    pub previous_version: Option<Version>,
    pub candidate_version: Option<Version>,
    pub kind: PackageVersionChangeKind,
}

#[derive(Clone, Debug)]
pub struct PreparedPackageCandidate {
    pub manifest: PackageManifest,
    pub lock: Lockfile,
    pub cache: CacheStore,
    pub previous_lock: Option<Lockfile>,
    pub changes: Vec<PackageVersionChange>,
    pub deprecated_packages: BTreeSet<PackageId>,
}

#[derive(Clone, Debug)]
pub struct ForkedDependency<T> {
    pub value: T,
    pub package: PackageId,
    pub version: Version,
    pub destination: Utf8PathBuf,
}

pub struct PackageService;

impl PackageService {
    pub fn prepare(
        root: &Utf8Path,
        resolution: CandidateResolution,
        validate_artifact: impl FnMut(
            &Utf8Path,
            &PackageId,
            &LockedPackage,
            &Lockfile,
            &CacheStore,
        ) -> Result<(), PackageError>,
    ) -> Result<PreparedPackageCandidate, PackageError> {
        let manifest = PackageManifest::read(root)?;
        let registry = RegistryConfig::read()?;
        let cache = DawnDirectories::discover()?.package_cache();
        let previous_lock = read_optional_lock(root)?;
        let previous_is_current = previous_lock.as_ref().is_some_and(|lock| {
            same_registry(&lock.registry, &registry.registry)
                && lock.validate_local(root, &manifest).is_ok()
        });

        let (lock, mut deprecated_packages, metadata_loaded) = match &resolution {
            CandidateResolution::Sync if previous_is_current => {
                let lock = previous_lock.clone().ok_or_else(|| {
                    PackageError::Invalid(
                        "current lockfile disappeared during candidate preparation".to_string(),
                    )
                })?;
                (lock, BTreeSet::new(), false)
            }
            CandidateResolution::Sync | CandidateResolution::UpdateAll => {
                let (lock, deprecated) = resolve(root, &manifest, &registry, &BTreeMap::new())?;
                (lock, deprecated, true)
            }
            CandidateResolution::UpdateAlias(alias) => {
                let current = previous_lock.as_ref().ok_or_else(|| {
                    PackageError::Invalid(
                        "a current dawn.lock is required for a scoped update; run Sync first"
                            .to_string(),
                    )
                })?;
                if !previous_is_current {
                    return Err(PackageError::Invalid(
                        "dawn.lock is stale; run Sync before a scoped update".to_string(),
                    ));
                }
                let target = update_target(&manifest, alias)?;
                let pins = preservation_pins(&manifest, current, &target)?;
                let (lock, deprecated) = resolve(root, &manifest, &registry, &pins)?;
                (lock, deprecated, true)
            }
        };

        reject_module_identity_changes(previous_lock.as_ref(), &lock)?;
        if !lock.packages.is_empty() {
            let client = RegistryClient::discover(&registry)?;
            client.ensure_locked_artifacts(&lock, &cache, validate_artifact)?;
            if !metadata_loaded {
                deprecated_packages = client.deprecated_locked_packages(&lock)?;
            }
        }
        let changes = package_changes(previous_lock.as_ref(), &lock);
        Ok(PreparedPackageCandidate {
            manifest,
            lock,
            cache,
            previous_lock,
            changes,
            deprecated_packages,
        })
    }

    pub fn fork_dependency<T>(
        root: &Utf8Path,
        dependency_alias: &str,
        mut validate_artifact: impl FnMut(
            &Utf8Path,
            &PackageId,
            &LockedPackage,
            &Lockfile,
            &CacheStore,
        ) -> Result<(), PackageError>,
        validate_candidate: impl FnOnce(&PreparedPackageCandidate) -> Result<T, String>,
    ) -> Result<ForkedDependency<T>, PackageError> {
        let synchronized = Self::prepare(root, CandidateResolution::Sync, &mut validate_artifact)?;
        let original_manifest = synchronized.manifest;
        let dependency = original_manifest
            .dependencies
            .get(dependency_alias)
            .ok_or_else(|| {
                PackageError::Invalid(format!(
                    "dependency alias `{dependency_alias}` is not declared"
                ))
            })?;
        let Dependency::Registry { package, .. } = dependency else {
            return Err(PackageError::Invalid(format!(
                "dependency `{dependency_alias}` is a path dependency; only registry dependencies can be forked"
            )));
        };
        let package = package.clone();
        let package_name = package
            .as_str()
            .split_once('/')
            .map(|(_, name)| name)
            .ok_or_else(|| {
                PackageError::Invalid(format!("package `{package}` has no package-name component"))
            })?;
        let locked = synchronized.lock.packages.get(&package).ok_or_else(|| {
            PackageError::Invalid(format!("dependency `{package}` is not locked"))
        })?;
        let version = locked.version.clone();
        let source = synchronized.cache.package_root(&locked.archive_sha256)?;
        let destination_relative = Utf8PathBuf::from("modules").join(package_name);
        let destination = root.join(&destination_relative);
        if destination.exists() {
            return Err(PackageError::Invalid(format!(
                "fork destination already exists: `{destination}`"
            )));
        }

        let fork_manifest = PackageManifest::read(&source)?;
        let destination_manifest_path = destination_relative.as_str().replace('\\', "/");
        let (candidate_manifest, fork_manifest) = prepare_path_fork_manifests(
            &original_manifest,
            dependency_alias,
            fork_manifest,
            &destination_manifest_path,
        )?;

        let modules_directory = root.join("modules");
        fs::create_dir_all(&modules_directory)?;
        let temporary = Builder::new()
            .prefix(".dawn-fork-")
            .tempdir_in(&modules_directory)?;
        let temporary_root =
            Utf8PathBuf::from_path_buf(temporary.path().to_path_buf()).map_err(|_| {
                PackageError::Invalid("temporary directory path is not UTF-8".to_string())
            })?;
        let staged = temporary_root.join("package");
        copy_package_tree(&source, &staged)?;
        remove_if_file(&staged.join("dawn-release.json"))?;
        remove_if_file(&staged.join(crate::LOCK_FILE))?;
        fork_manifest.write(&staged)?;

        fs::rename(&staged, &destination)?;
        if let Err(error) = candidate_manifest.write(root) {
            return rollback_fork(root, &destination, &original_manifest, error);
        }

        let candidate =
            match Self::prepare(root, CandidateResolution::UpdateAll, &mut validate_artifact) {
                Ok(candidate) => candidate,
                Err(error) => {
                    return rollback_fork(root, &destination, &original_manifest, error);
                }
            };
        let value = match validate_candidate(&candidate) {
            Ok(value) => value,
            Err(error) => {
                return rollback_fork(
                    root,
                    &destination,
                    &original_manifest,
                    PackageError::Invalid(error),
                );
            }
        };
        if let Err(error) = candidate.lock.write(root) {
            return rollback_fork(root, &destination, &original_manifest, error);
        }
        Ok(ForkedDependency {
            value,
            package,
            version,
            destination: destination_relative,
        })
    }
}

fn resolve(
    root: &Utf8Path,
    manifest: &PackageManifest,
    registry: &RegistryConfig,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> Result<(Lockfile, BTreeSet<PackageId>), PackageError> {
    let local = Lockfile::from_directory(manifest, root, registry.registry.trim_end_matches('/'))?;
    let has_registry = manifest
        .dependencies
        .values()
        .any(|dependency| matches!(dependency, Dependency::Registry { .. }))
        || local.path_dependencies.values().any(|dependency| {
            dependency
                .dependencies
                .values()
                .any(|dependency| matches!(dependency, LockedDependency::Registry { .. }))
        });
    if !has_registry {
        return Ok((local, BTreeSet::new()));
    }
    let resolution = RegistryClient::discover(registry)?.resolve_with_pins(root, manifest, pins)?;
    Ok((resolution.lock, resolution.deprecated_packages))
}

fn read_optional_lock(root: &Utf8Path) -> Result<Option<Lockfile>, PackageError> {
    match Lockfile::read(root) {
        Ok(lock) => Ok(Some(lock)),
        Err(PackageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn update_target(manifest: &PackageManifest, alias: &str) -> Result<PackageId, PackageError> {
    let dependency = manifest.dependencies.get(alias).ok_or_else(|| {
        PackageError::Invalid(format!("dependency alias `{alias}` is not declared"))
    })?;
    match dependency {
        Dependency::Registry { package, .. } => Ok(package.clone()),
        Dependency::Path { .. } => Err(PackageError::Invalid(format!(
            "dependency `{alias}` is a path dependency and has no registry version to update"
        ))),
    }
}

fn preservation_pins(
    manifest: &PackageManifest,
    lock: &Lockfile,
    target: &PackageId,
) -> Result<BTreeMap<PackageId, ResolutionPin>, PackageError> {
    let mut preserved = BTreeSet::new();
    let mut visited_paths = BTreeSet::new();
    for dependency in manifest.dependencies.values() {
        match dependency {
            Dependency::Registry { package, .. } => {
                if package != target {
                    collect_locked_package_closure(package, target, lock, &mut preserved)?;
                }
            }
            Dependency::Path { path } => {
                collect_path_package_roots(path, target, lock, &mut visited_paths, &mut preserved)?
            }
        }
    }
    preserved
        .into_iter()
        .map(|package| {
            let locked = lock.packages.get(&package).ok_or_else(|| {
                PackageError::Invalid(format!(
                    "package `{package}` selected for preservation is not locked"
                ))
            })?;
            Ok((
                package,
                ResolutionPin {
                    version: locked.version.clone(),
                    archive_sha256: locked.archive_sha256.clone(),
                },
            ))
        })
        .collect()
}

fn collect_path_package_roots(
    path: &str,
    target: &PackageId,
    lock: &Lockfile,
    visited_paths: &mut BTreeSet<String>,
    preserved: &mut BTreeSet<PackageId>,
) -> Result<(), PackageError> {
    if !visited_paths.insert(path.to_string()) {
        return Ok(());
    }
    let path_lock = lock
        .path_dependencies
        .get(path)
        .ok_or_else(|| PackageError::Invalid(format!("path dependency `{path}` is not locked")))?;
    for dependency in path_lock.dependencies.values() {
        match dependency {
            LockedDependency::Registry { package } => {
                if package != target {
                    collect_locked_package_closure(package, target, lock, preserved)?;
                }
            }
            LockedDependency::Path { path } => {
                collect_path_package_roots(path, target, lock, visited_paths, preserved)?
            }
        }
    }
    Ok(())
}

fn collect_locked_package_closure(
    package: &PackageId,
    target: &PackageId,
    lock: &Lockfile,
    preserved: &mut BTreeSet<PackageId>,
) -> Result<(), PackageError> {
    if package == target || !preserved.insert(package.clone()) {
        return Ok(());
    }
    let locked = lock
        .packages
        .get(package)
        .ok_or_else(|| PackageError::Invalid(format!("package `{package}` is not locked")))?;
    for dependency in locked.dependencies.values() {
        collect_locked_package_closure(dependency, target, lock, preserved)?;
    }
    Ok(())
}

fn reject_module_identity_changes(
    previous: Option<&Lockfile>,
    candidate: &Lockfile,
) -> Result<(), PackageError> {
    let Some(previous) = previous else {
        return Ok(());
    };
    for (package, candidate_package) in &candidate.packages {
        let Some(previous_package) = previous.packages.get(package) else {
            continue;
        };
        if previous_package.module_id != candidate_package.module_id {
            return Err(PackageError::Invalid(format!(
                "candidate `{package}@{}` changes moduleId from `{}` to `{}`",
                candidate_package.version, previous_package.module_id, candidate_package.module_id
            )));
        }
    }
    Ok(())
}

fn package_changes(previous: Option<&Lockfile>, candidate: &Lockfile) -> Vec<PackageVersionChange> {
    let empty = BTreeMap::new();
    let previous = previous.map_or(&empty, |lock| &lock.packages);
    let packages = previous
        .keys()
        .chain(candidate.packages.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    packages
        .into_iter()
        .filter_map(|package| {
            let before = previous.get(&package);
            let after = candidate.packages.get(&package);
            if before.map(|item| (&item.version, &item.archive_sha256))
                == after.map(|item| (&item.version, &item.archive_sha256))
            {
                return None;
            }
            let kind = match (before, after) {
                (None, Some(_)) => PackageVersionChangeKind::Added,
                (Some(_), None) => PackageVersionChangeKind::Removed,
                (Some(_), Some(_)) => PackageVersionChangeKind::Updated,
                (None, None) => return None,
            };
            Some(PackageVersionChange {
                package,
                previous_version: before.map(|item| item.version.clone()),
                candidate_version: after.map(|item| item.version.clone()),
                kind,
            })
        })
        .collect()
}

fn same_registry(left: &str, right: &str) -> bool {
    left.trim_end_matches('/') == right.trim_end_matches('/')
}

fn prepare_path_fork_manifests(
    original_manifest: &PackageManifest,
    dependency_alias: &str,
    mut fork_manifest: PackageManifest,
    destination_manifest_path: &str,
) -> Result<(PackageManifest, PackageManifest), PackageError> {
    let mut candidate_manifest = original_manifest.clone();
    let previous = candidate_manifest.dependencies.insert(
        dependency_alias.to_string(),
        Dependency::Path {
            path: destination_manifest_path.to_string(),
        },
    );
    if previous.is_none() {
        return Err(PackageError::Invalid(format!(
            "dependency alias `{dependency_alias}` disappeared during fork preparation"
        )));
    }
    fork_manifest.module_id = Uuid::new_v4();
    fork_manifest.publication = None;
    Ok((candidate_manifest, fork_manifest))
}

pub fn copy_package_tree(source: &Utf8Path, destination: &Utf8Path) -> Result<(), PackageError> {
    if !source.is_dir() {
        return Err(PackageError::Invalid(format!(
            "package source does not exist: `{source}`"
        )));
    }
    fs::create_dir_all(destination)?;
    let mut entries = fs::read_dir(source)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let source_path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|_| PackageError::Invalid("package contains a non-UTF-8 path".to_string()))?;
        let destination_path = destination.join(entry.file_name().to_string_lossy().as_ref());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(PackageError::Invalid(format!(
                "package contains a symbolic link: `{source_path}`"
            )));
        }
        if file_type.is_dir() {
            copy_package_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            let _ = fs::copy(&source_path, &destination_path)?;
        } else {
            return Err(PackageError::Invalid(format!(
                "package contains an unsupported filesystem entry: `{source_path}`"
            )));
        }
    }
    Ok(())
}

fn remove_if_file(path: &Utf8Path) -> Result<(), PackageError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn rollback_fork<T>(
    root: &Utf8Path,
    destination: &Utf8Path,
    original_manifest: &PackageManifest,
    operation_error: PackageError,
) -> Result<T, PackageError> {
    let mut rollback_failures = Vec::new();
    if let Err(error) = original_manifest.write(root) {
        rollback_failures.push(format!("restoring dawn-package.json failed: {error}"));
    }
    if let Err(error) = remove_fork_directory(root, destination) {
        rollback_failures.push(format!("removing the staged fork failed: {error}"));
    }
    if rollback_failures.is_empty() {
        Err(operation_error)
    } else {
        Err(PackageError::Invalid(format!(
            "fork failed ({operation_error}); rollback also failed: {}",
            rollback_failures.join("; ")
        )))
    }
}

fn remove_fork_directory(root: &Utf8Path, target: &Utf8Path) -> Result<(), PackageError> {
    let canonical_root = root.canonicalize_utf8().map_err(PackageError::Io)?;
    let canonical_target = target.canonicalize_utf8().map_err(PackageError::Io)?;
    let modules = canonical_root.join("modules");
    if canonical_target == canonical_root
        || !canonical_target.starts_with(&modules)
        || canonical_target.parent() != Some(modules.as_path())
    {
        return Err(PackageError::Invalid(format!(
            "refusing to remove fork outside the project modules directory: `{target}`"
        )));
    }
    fs::remove_dir_all(canonical_target)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AssetDeclaration, AssetKind, ExportGroup, PackageManifest, ProjectManifest, Publication,
    };
    use semver::{Version, VersionReq};

    fn manifest(module_id: Uuid) -> PackageManifest {
        PackageManifest {
            manifest_version: crate::MANIFEST_VERSION,
            module_id,
            language_version: "0.1".to_string(),
            project: Some(ProjectManifest {
                entrypoint: "project.dawn".to_string(),
            }),
            publication: None,
            exports: BTreeMap::from([(
                "project".to_string(),
                ExportGroup {
                    documents: vec!["project.dawn".to_string()],
                },
            )]),
            dependencies: BTreeMap::new(),
            assets: BTreeMap::new(),
        }
    }

    #[test]
    fn fork_becomes_a_reidentified_path_dependency() {
        let foundation = Dependency::Registry {
            package: PackageId::new("test/foundation").expect("package"),
            version: VersionReq::parse("^1.0").expect("requirement"),
        };
        let original_fork_module_id = Uuid::new_v4();
        let mut fork = manifest(original_fork_module_id);
        fork.project = None;
        fork.publication = Some(Publication {
            package: PackageId::new("test/library").expect("package"),
            version: Version::parse("1.0.0").expect("version"),
            display_name: "Library".to_string(),
            summary: "Fork fixture".to_string(),
            license: "MIT".to_string(),
            tags: Vec::new(),
        });
        fork.exports = BTreeMap::from([(
            "content".to_string(),
            ExportGroup {
                documents: vec!["main.sequence.dawn".to_string()],
            },
        )]);
        fork.dependencies = BTreeMap::from([("foundation".to_string(), foundation.clone())]);
        fork.assets = BTreeMap::from([(
            "audio/click.wav".to_string(),
            AssetDeclaration {
                kind: AssetKind::Audio,
            },
        )]);

        let library = Dependency::Registry {
            package: PackageId::new("test/library").expect("package"),
            version: VersionReq::parse("^1.0").expect("requirement"),
        };
        let mut project = manifest(Uuid::new_v4());
        project.dependencies.insert("library".to_string(), library);
        project.dependencies.insert(
            "other".to_string(),
            Dependency::Registry {
                package: PackageId::new("test/other").expect("package"),
                version: VersionReq::parse("^1.0").expect("requirement"),
            },
        );
        let original_project_assets = project.assets.clone();
        let (candidate, forked) =
            prepare_path_fork_manifests(&project, "library", fork.clone(), "modules/library")
                .expect("path fork");

        assert_eq!(
            candidate.dependencies.get("library"),
            Some(&Dependency::Path {
                path: "modules/library".to_string()
            })
        );
        assert_eq!(
            candidate.dependencies.get("other"),
            project.dependencies.get("other")
        );
        assert_eq!(candidate.assets, original_project_assets);
        assert_ne!(forked.module_id, original_fork_module_id);
        assert_eq!(forked.publication, None);
        assert_eq!(forked.dependencies, fork.dependencies);
        assert_eq!(forked.assets, fork.assets);
    }
}
