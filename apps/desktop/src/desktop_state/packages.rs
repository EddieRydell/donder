use std::io;

use camino::Utf8Path;
use donder_package::{
    CacheStatus, CandidateResolution, Dependency, Lockfile, PackageManifest, PackageService,
    PreparedPackageCandidate, RegistryConfig, ResolvedModuleOrigin,
};
use donder_project_io::{
    PackageCompatibilityReport, ProjectSession, analyze_package_candidate, load_package_with_cache,
    validate_registry_package_artifact,
};

use super::{DesktopState, lock_unpoisoned};
use crate::dto::{
    AppSnapshot, PackageCacheState, PackageCompatibilityWarning, PackageDependencySource,
    PackageDependencyStatus, PackageModuleStatus, PackageReadiness, PackageStatus,
};

impl DesktopState {
    pub fn sync_packages(&self) -> AppSnapshot {
        self.accept_package_candidate(CandidateResolution::Sync)
    }

    pub fn update_packages(&self, alias: Option<String>) -> AppSnapshot {
        let resolution = alias.map_or(
            CandidateResolution::UpdateAll,
            CandidateResolution::UpdateAlias,
        );
        self.accept_package_candidate(resolution)
    }

    pub fn check_package_updates(&self) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(root) = self.snapshot().project_root else {
            return self.update_snapshot(|snapshot| {
                snapshot.package.message = Some("Open a package project first".to_string());
            });
        };
        let root = Utf8Path::new(&root);
        match prepare_loaded_candidate(root, CandidateResolution::UpdateAll) {
            Ok((candidate, candidate_session)) => {
                let report = self
                    .project_session()
                    .as_deref()
                    .map_or_else(PackageCompatibilityReport::default, |current| {
                        analyze_package_candidate(current, &candidate_session)
                    });
                let mut status = package_status(root, self.project_session().as_deref());
                decorate_candidate_status(&mut status, &candidate, &report);
                self.update_snapshot(|snapshot| {
                    snapshot.package = status;
                    snapshot.status = if candidate.changes.is_empty() {
                        "All packages are current".to_string()
                    } else {
                        format!("{} package update(s) available", candidate.changes.len())
                    };
                })
            }
            Err(error) => self.package_operation_error("Package update check failed", error),
        }
    }

    pub fn remove_package_dependency(&self, alias: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(root) = self.package_mutation_root() else {
            return self.snapshot();
        };
        let _filesystem = lock_unpoisoned(&self.filesystem);
        let root = Utf8Path::new(&root);
        let original = match PackageManifest::read(root) {
            Ok(manifest) => manifest,
            Err(error) => {
                return self.package_operation_error("Dependency removal failed", error);
            }
        };
        let mut manifest = original.clone();
        if manifest.dependencies.remove(alias).is_none() {
            return self.package_operation_error(
                "Dependency removal failed",
                format!("dependency alias `{alias}` is not declared"),
            );
        }
        if let Err(error) = manifest.write(root) {
            return self.package_operation_error("Dependency removal failed", error);
        }
        let prepared = prepare_loaded_candidate(root, CandidateResolution::UpdateAll);
        let (candidate, _session) = match prepared {
            Ok(candidate) => candidate,
            Err(error) => {
                return self.rollback_manifest_operation(
                    root,
                    &original,
                    "Dependency removal failed",
                    error,
                );
            }
        };
        if let Err(error) = candidate.lock.write(root) {
            return self.rollback_manifest_operation(
                root,
                &original,
                "Dependency removal failed",
                error.to_string(),
            );
        }
        self.accept_loaded_package(root, format!("Removed dependency `{alias}`"))
    }

    pub fn fork_package_dependency(&self, alias: &str) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let Some(root) = self.package_mutation_root() else {
            return self.snapshot();
        };
        let _filesystem = lock_unpoisoned(&self.filesystem);
        let root = Utf8Path::new(&root);
        let forked = PackageService::fork_dependency(
            root,
            alias,
            validate_registry_package_artifact,
            |candidate| {
                load_package_with_cache(
                    root,
                    candidate.manifest.clone(),
                    candidate.lock.clone(),
                    &candidate.cache,
                )
                .map(|loaded| loaded.session)
                .map_err(|error| error.to_string())
            },
        );
        match forked {
            Ok(forked) => self.accept_loaded_package(
                root,
                format!(
                    "Forked {}@{} into {}",
                    forked.package, forked.version, forked.destination
                ),
            ),
            Err(error) => self.package_operation_error("Package fork failed", error),
        }
    }

    pub fn open_package_page(&self, alias: &str) -> AppSnapshot {
        let snapshot = self.snapshot();
        let Some(url) = snapshot
            .package
            .dependencies
            .iter()
            .find(|dependency| dependency.alias == alias)
            .and_then(|dependency| dependency.website_url.as_deref())
        else {
            return self.package_operation_error(
                "Package page was not opened",
                format!("dependency `{alias}` has no registry discovery page"),
            );
        };
        match webbrowser::open(url) {
            Ok(()) => snapshot,
            Err(error) => self.package_operation_error("Package page was not opened", error),
        }
    }

    fn accept_package_candidate(&self, resolution: CandidateResolution) -> AppSnapshot {
        let _authoring = self.settled_authoring();
        let snapshot = self.snapshot();
        let Some(root) = snapshot.project_root else {
            return self.update_snapshot(|snapshot| {
                snapshot.package.message = Some("Open a package project first".to_string());
            });
        };
        let dirty = lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .find(|document| document.buffer.dirty)
            .map(|document| document.buffer.path.clone());
        if let Some(dirty) = dirty {
            return self.update_snapshot(|snapshot| {
                snapshot.package.message =
                    Some(format!("Save `{}` before changing package versions", dirty));
                snapshot.status = "Package operation blocked by unsaved edits".to_string();
            });
        }
        let _filesystem = lock_unpoisoned(&self.filesystem);

        let root = Utf8Path::new(&root);
        let (candidate, candidate_session) =
            match prepare_loaded_candidate(root, resolution.clone()) {
                Ok(candidate) => candidate,
                Err(error) => {
                    return self.package_operation_error("Package synchronization failed", error);
                }
            };
        let report = self
            .project_session()
            .as_deref()
            .map_or_else(PackageCompatibilityReport::default, |current| {
                analyze_package_candidate(current, &candidate_session)
            });
        if report.has_breaking_changes() {
            let mut status = package_status(root, self.project_session().as_deref());
            decorate_candidate_status(&mut status, &candidate, &report);
            status.message = Some(
                "Candidate package changes remove or alter public project resources and cannot be accepted automatically"
                    .to_string()
            );
            return self.update_snapshot(|snapshot| {
                snapshot.package = status;
                snapshot.status =
                    "Package candidate has breaking compatibility changes".to_string();
            });
        }
        if let Err(error) = candidate.lock.write(root) {
            return self
                .package_operation_error("Package lockfile was not accepted", error.to_string());
        }

        lock_unpoisoned(&self.gui_history).clear();
        self.load_working_copy(root);
        let mut status = package_status(root, self.project_session().as_deref());
        decorate_deprecation_status(&mut status, &candidate);
        if matches!(
            resolution,
            CandidateResolution::UpdateAll | CandidateResolution::UpdateAlias(_)
        ) {
            status.update_checked = true;
            for dependency in &mut status.dependencies {
                if matches!(dependency.source, PackageDependencySource::Registry) {
                    dependency.update_available = Some(false);
                }
            }
        }
        let change_count = candidate.changes.len();
        self.update_snapshot(|snapshot| {
            snapshot.package = status;
            snapshot.status = if change_count == 0 {
                "Package lock and cache synchronized".to_string()
            } else {
                format!("Accepted {change_count} package change(s)")
            };
        })
    }

    fn package_operation_error(&self, status: &str, error: impl ToString) -> AppSnapshot {
        self.update_snapshot(|snapshot| {
            snapshot.package.message = Some(error.to_string());
            snapshot.status = status.to_string();
        })
    }

    fn package_mutation_root(&self) -> Option<String> {
        let snapshot = self.snapshot();
        let Some(root) = snapshot.project_root else {
            self.update_snapshot(|snapshot| {
                snapshot.package.message = Some("Open a package project first".to_string());
            });
            return None;
        };
        let dirty = lock_unpoisoned(&self.workspace)
            .documents
            .values()
            .find(|document| document.buffer.dirty)
            .map(|document| document.buffer.path.clone());
        if let Some(dirty) = dirty {
            self.update_snapshot(|snapshot| {
                snapshot.package.message =
                    Some(format!("Save `{}` before changing dependencies", dirty));
                snapshot.status = "Package operation blocked by unsaved edits".to_string();
            });
            return None;
        }
        Some(root)
    }

    fn rollback_manifest_operation(
        &self,
        root: &Utf8Path,
        original: &PackageManifest,
        status: &str,
        operation_error: String,
    ) -> AppSnapshot {
        match original.write(root) {
            Ok(()) => self.package_operation_error(status, operation_error),
            Err(rollback_error) => self.package_operation_error(
                status,
                format!(
                    "{operation_error}; restoring donder-package.json also failed: {rollback_error}"
                ),
            ),
        }
    }

    fn accept_loaded_package(&self, root: &Utf8Path, status: String) -> AppSnapshot {
        lock_unpoisoned(&self.gui_history).clear();
        self.load_working_copy(root);
        let package = package_status(root, self.project_session().as_deref());
        self.update_snapshot(|snapshot| {
            snapshot.package = package;
            snapshot.status = status;
        })
    }
}

fn prepare_loaded_candidate(
    root: &Utf8Path,
    resolution: CandidateResolution,
) -> Result<(PreparedPackageCandidate, ProjectSession), String> {
    let candidate = PackageService::prepare(root, resolution, validate_registry_package_artifact)
        .map_err(|error| error.to_string())?;
    let loaded = load_package_with_cache(
        root,
        candidate.manifest.clone(),
        candidate.lock.clone(),
        &candidate.cache,
    )
    .map_err(|error| error.to_string())?;
    Ok((candidate, loaded.session))
}

pub(crate) fn package_status(root: &Utf8Path, session: Option<&ProjectSession>) -> PackageStatus {
    package_status_with_sources(root, session, &donder_project_io::SourceOverrides::new())
}

pub(super) fn package_status_with_sources(
    root: &Utf8Path,
    session: Option<&ProjectSession>,
    sources: &donder_project_io::SourceOverrides,
) -> PackageStatus {
    let manifest = sources
        .get(Utf8Path::new(donder_package::MANIFEST_FILE))
        .map_or_else(
            || PackageManifest::read(root).map_err(|error| error.to_string()),
            |text| {
                PackageManifest::parse_for_analysis(text.as_bytes())
                    .map_err(|error| error.to_string())
            },
        );
    let manifest = match manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            return invalid_status(
                root,
                root.join(donder_package::LOCK_FILE).is_file(),
                error.to_string(),
            );
        }
    };
    let lock = sources
        .get(Utf8Path::new(donder_package::LOCK_FILE))
        .map_or_else(
            || read_optional_lock(root),
            |text| {
                Lockfile::parse_for_analysis(text.as_bytes())
                    .map(Some)
                    .map_err(|error| error.to_string())
            },
        );
    let lock = match lock {
        Ok(lock) => lock,
        Err(error) => {
            return PackageStatus {
                readiness: PackageReadiness::Invalid,
                root: Some(root.to_string()),
                manifest_valid: true,
                lock_present: true,
                lock_current: false,
                registry: None,
                update_checked: false,
                dependencies: Vec::new(),
                modules: dependency_modules(session),
                warnings: Vec::new(),
                message: Some(error),
            };
        }
    };
    let cache = donder_package::DonderDirectories::discover()
        .map(|directories| directories.package_cache());
    let dependencies = manifest
        .dependencies
        .iter()
        .map(|(alias, dependency)| match dependency {
            Dependency::Registry { package, version } => {
                let locked = lock.as_ref().and_then(|lock| lock.packages.get(package));
                let cache_state = match (&cache, locked) {
                    (Ok(cache), Some(locked)) => match cache.status(&locked.archive_sha256) {
                        Ok(CacheStatus::Ready) => PackageCacheState::Ready,
                        Ok(CacheStatus::Missing) => PackageCacheState::Missing,
                        Err(_) => PackageCacheState::Error,
                    },
                    (Err(_), Some(_)) => PackageCacheState::Error,
                    (_, None) => PackageCacheState::Unknown,
                };
                PackageDependencyStatus {
                    alias: alias.clone(),
                    source: PackageDependencySource::Registry,
                    requirement: version.to_string(),
                    package: Some(package.to_string()),
                    locked_version: locked.map(|locked| locked.version.to_string()),
                    module_id: locked.map(|locked| locked.module_id.to_string()),
                    cache: cache_state,
                    update_available: None,
                    website_url: lock
                        .as_ref()
                        .and_then(|lock| package_website_url(&lock.registry, package)),
                    warnings: Vec::new(),
                }
            }
            Dependency::Path { path } => {
                let locked = lock
                    .as_ref()
                    .and_then(|lock| lock.path_dependencies.get(path));
                PackageDependencyStatus {
                    alias: alias.clone(),
                    source: PackageDependencySource::Path,
                    requirement: path.clone(),
                    package: None,
                    locked_version: None,
                    module_id: locked.map(|locked| locked.module_id.to_string()),
                    cache: PackageCacheState::Local,
                    update_available: None,
                    website_url: None,
                    warnings: Vec::new(),
                }
            }
        })
        .collect();
    let lock_current = lock
        .as_ref()
        .is_some_and(|lock| lock.validate_local(root, &manifest).is_ok());
    let readiness = if lock.is_none() || !lock_current {
        PackageReadiness::NeedsSync
    } else {
        PackageReadiness::Ready
    };
    PackageStatus {
        readiness,
        root: Some(root.to_string()),
        manifest_valid: true,
        lock_present: lock.is_some(),
        lock_current,
        registry: lock.as_ref().map(|lock| lock.registry.clone()),
        update_checked: false,
        dependencies,
        modules: dependency_modules(session),
        warnings: Vec::new(),
        message: cache.err().map(|error| error.to_string()),
    }
}

fn package_website_url(registry: &str, package: &donder_package::PackageId) -> Option<String> {
    let registry = RegistryConfig {
        registry: registry.to_string(),
    }
    .parsed_registry()
    .ok()?;
    Some(format!(
        "{}/registry/{package}",
        registry.as_str().trim_end_matches('/')
    ))
}

fn decorate_candidate_status(
    status: &mut PackageStatus,
    candidate: &PreparedPackageCandidate,
    report: &PackageCompatibilityReport,
) {
    status.update_checked = true;
    for dependency in &mut status.dependencies {
        let Some(package) = dependency.package.as_deref() else {
            continue;
        };
        let current = dependency.locked_version.as_deref();
        let candidate_version = candidate
            .lock
            .packages
            .iter()
            .find(|(identity, _)| identity.as_str() == package)
            .map(|(_, locked)| locked.version.to_string());
        dependency.update_available = Some(current != candidate_version.as_deref());
        dependency.warnings = report
            .issues
            .iter()
            .filter(|issue| issue.package == package)
            .map(|issue| issue.message.clone())
            .collect();
    }
    status.warnings = report
        .issues
        .iter()
        .map(|issue| PackageCompatibilityWarning {
            package: issue.package.clone(),
            message: issue.message.clone(),
            breaking: issue.breaking,
        })
        .collect();
    decorate_deprecation_status(status, candidate);
}

pub(crate) fn decorate_deprecation_status(
    status: &mut PackageStatus,
    candidate: &PreparedPackageCandidate,
) {
    for dependency in &mut status.dependencies {
        let Some(package) = dependency.package.as_deref() else {
            continue;
        };
        let package_id = candidate
            .deprecated_packages
            .iter()
            .find(|candidate| candidate.as_str() == package);
        if package_id.is_some() {
            dependency
                .warnings
                .push("This package is deprecated but remains installable.".to_string());
        }
    }
    for package in &candidate.deprecated_packages {
        status.warnings.push(PackageCompatibilityWarning {
            package: package.to_string(),
            message: "Package is deprecated but remains installable.".to_string(),
            breaking: false,
        });
    }
    if !status.warnings.is_empty() {
        status.readiness = PackageReadiness::Warning;
    }
}

fn dependency_modules(session: Option<&ProjectSession>) -> Vec<PackageModuleStatus> {
    let Some(session) = session else {
        return Vec::new();
    };
    let mut modules = session
        .source
        .source_graph
        .modules()
        .iter()
        .filter_map(|(module_id, module)| {
            let (identity, version) = match &module.origin {
                ResolvedModuleOrigin::Project => return None,
                ResolvedModuleOrigin::PathDependency { declared_path, .. } => {
                    (format!("path:{declared_path}"), None)
                }
                ResolvedModuleOrigin::RegistryDependency {
                    package, version, ..
                } => (package.to_string(), Some(version.to_string())),
            };
            let documents = session
                .source
                .documents
                .keys()
                .filter(|document| document.module_id() == *module_id)
                .map(|document| document.path().to_string())
                .collect();
            Some(PackageModuleStatus {
                identity,
                module_id: module_id.to_string(),
                version,
                documents,
            })
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| left.identity.cmp(&right.identity));
    modules
}

fn read_optional_lock(root: &Utf8Path) -> Result<Option<Lockfile>, String> {
    match Lockfile::read(root) {
        Ok(lock) => Ok(Some(lock)),
        Err(donder_package::PackageError::Io(error)) if error.kind() == io::ErrorKind::NotFound => {
            Ok(None)
        }
        Err(error) => Err(error.to_string()),
    }
}

fn invalid_status(root: &Utf8Path, lock_present: bool, message: String) -> PackageStatus {
    PackageStatus {
        readiness: PackageReadiness::Invalid,
        root: Some(root.to_string()),
        manifest_valid: false,
        lock_present,
        lock_current: false,
        registry: None,
        update_checked: false,
        dependencies: Vec::new(),
        modules: Vec::new(),
        warnings: Vec::new(),
        message: Some(message),
    }
}
