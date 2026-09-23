use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use pubgrub::{
    DefaultStringReporter, OfflineDependencyProvider, PubGrubError, Ranges, Reporter as _, resolve,
};
use semver::{Version, VersionReq};
use uuid::Uuid;

use crate::{
    Dependency, LockedDependency, LockedPackage, Lockfile, PackageError, PackageId,
    PackageManifest, PathLock, ResolutionPin, collect_path_modules,
};

type VersionRange = Ranges<Version>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryRelease {
    pub package: PackageId,
    pub version: Version,
    pub archive_sha256: String,
    pub module_id: Uuid,
    pub dependencies: BTreeMap<String, Dependency>,
    pub yanked: bool,
    pub runtime_compatible: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum SolverPackage {
    Root,
    Registry(PackageId),
    Path(String),
}

impl fmt::Display for SolverPackage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root => formatter.write_str("project"),
            Self::Registry(package) => package.fmt(formatter),
            Self::Path(path) => write!(formatter, "path:{path}"),
        }
    }
}

pub fn resolve_registry(
    root: &camino::Utf8Path,
    manifest: &PackageManifest,
    registry: &str,
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
) -> Result<Lockfile, PackageError> {
    resolve_registry_with_pins(root, manifest, registry, available, &BTreeMap::new())
}

pub fn resolve_registry_with_pins(
    root: &camino::Utf8Path,
    manifest: &PackageManifest,
    registry: &str,
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> Result<Lockfile, PackageError> {
    validate_release_catalog(available)?;
    validate_pins(available, pins)?;
    let path_modules = collect_path_modules(root, manifest)?;
    let mut provider = OfflineDependencyProvider::<SolverPackage, VersionRange>::new();
    let root_version = Version::new(0, 0, 0);
    provider.add_dependencies(
        SolverPackage::Root,
        root_version.clone(),
        solver_dependencies(&manifest.dependencies, None, available, true, pins)?,
    );
    for (path, module) in &path_modules {
        provider.add_dependencies(
            SolverPackage::Path(path.clone()),
            root_version.clone(),
            solver_dependencies(
                &module.manifest.dependencies,
                Some(&module.lock),
                available,
                true,
                pins,
            )?,
        );
    }

    for releases in available.values() {
        for release in releases
            .iter()
            .filter(|release| release_selectable(release, pins))
        {
            provider.add_dependencies(
                SolverPackage::Registry(release.package.clone()),
                release.version.clone(),
                solver_dependencies(&release.dependencies, None, available, false, pins)?,
            );
        }
    }

    let solution = match resolve(&provider, SolverPackage::Root, root_version) {
        Ok(solution) => solution,
        Err(PubGrubError::NoSolution(mut derivation)) => {
            derivation.collapse_no_versions();
            return Err(PackageError::Invalid(format!(
                "dependency resolution failed:\n{}",
                DefaultStringReporter::report(&derivation)
            )));
        }
        Err(error) => {
            return Err(PackageError::Invalid(format!(
                "dependency resolution failed: {error}"
            )));
        }
    };

    let mut packages = BTreeMap::new();
    for (solver_package, version) in solution.iter() {
        let SolverPackage::Registry(package) = solver_package else {
            continue;
        };
        let release = available
            .get(package)
            .and_then(|releases| {
                releases.iter().find(|release| {
                    release.version == *version && release_selectable(release, pins)
                })
            })
            .ok_or_else(|| {
                PackageError::Invalid(format!(
                    "solver selected unavailable release `{package}@{version}`"
                ))
            })?;
        let dependencies = release
            .dependencies
            .iter()
            .map(|(alias, dependency)| match dependency {
                Dependency::Registry { package, .. } => Ok((alias.clone(), package.clone())),
                Dependency::Path { .. } => Err(PackageError::Invalid(format!(
                    "registry release `{package}@{version}` contains a path dependency"
                ))),
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        packages.insert(
            package.clone(),
            LockedPackage {
                version: version.clone(),
                archive_sha256: release.archive_sha256.clone(),
                module_id: release.module_id,
                dependencies,
            },
        );
    }
    let mut lock = Lockfile::from_directory(manifest, root, registry)?;
    lock.packages = packages;
    lock.validate_local(root, manifest)?;
    Ok(lock)
}

fn solver_dependencies(
    dependencies: &BTreeMap<String, Dependency>,
    path_lock: Option<&PathLock>,
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
    allow_path: bool,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> Result<Vec<(SolverPackage, VersionRange)>, PackageError> {
    dependencies
        .iter()
        .map(|(alias, dependency)| match dependency {
            Dependency::Registry { package, version } => {
                requirement_range(package, version, available, pins)
                    .map(|(package, range)| (SolverPackage::Registry(package), range))
            }
            Dependency::Path { path } => {
                if !allow_path {
                    return Err(PackageError::Invalid(
                        "registry releases cannot contain path dependencies".to_string(),
                    ));
                }
                let path = match path_lock {
                    Some(path_lock) => match path_lock.dependencies.get(alias) {
                        Some(LockedDependency::Path { path }) => path.clone(),
                        _ => {
                            return Err(PackageError::Invalid(format!(
                                "path dependency `{alias}` has no locked path edge"
                            )));
                        }
                    },
                    None => path.clone(),
                };
                Ok((
                    SolverPackage::Path(path),
                    VersionRange::singleton(Version::new(0, 0, 0)),
                ))
            }
        })
        .collect()
}

fn requirement_range(
    package: &PackageId,
    requirement: &VersionReq,
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> Result<(PackageId, VersionRange), PackageError> {
    let mut range = VersionRange::empty();
    for release in available.get(package).into_iter().flatten() {
        if release_selectable(release, pins) && requirement.matches(&release.version) {
            range = range.union(&VersionRange::singleton(release.version.clone()));
        }
    }
    Ok((package.clone(), range))
}

fn validate_release_catalog(
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
) -> Result<(), PackageError> {
    let mut module_ids = BTreeMap::<Uuid, PackageId>::new();
    for (package, releases) in available {
        let mut versions = BTreeSet::new();
        let mut package_module_id = None;
        for release in releases {
            if &release.package != package {
                return Err(PackageError::Invalid(format!(
                    "release catalog entry for `{package}` contains `{}`",
                    release.package
                )));
            }
            if !versions.insert(release.version.clone()) {
                return Err(PackageError::Invalid(format!(
                    "release catalog contains duplicate `{package}@{}`",
                    release.version
                )));
            }
            if let Some(expected) = package_module_id
                && expected != release.module_id
            {
                return Err(PackageError::Invalid(format!(
                    "registry package `{package}` changes moduleId between releases"
                )));
            }
            package_module_id = Some(release.module_id);
            if let Some(existing) = module_ids.insert(release.module_id, package.clone())
                && existing != *package
            {
                return Err(PackageError::Invalid(format!(
                    "moduleId `{}` is shared by `{existing}` and `{package}`",
                    release.module_id
                )));
            }
        }
    }
    Ok(())
}

fn validate_pins(
    available: &BTreeMap<PackageId, Vec<RegistryRelease>>,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> Result<(), PackageError> {
    for (package, pin) in pins {
        let present = available.get(package).is_some_and(|releases| {
            releases.iter().any(|release| {
                release.version == pin.version && release.archive_sha256 == pin.archive_sha256
            })
        });
        if !present {
            return Err(PackageError::Invalid(format!(
                "cannot preserve locked `{package}@{}` because the registry did not return that exact release",
                pin.version
            )));
        }
    }
    Ok(())
}

fn release_selectable(
    release: &RegistryRelease,
    pins: &BTreeMap<PackageId, ResolutionPin>,
) -> bool {
    release.runtime_compatible
        && (!release.yanked
            || pins.get(&release.package).is_some_and(|pin| {
                pin.version == release.version && pin.archive_sha256 == release.archive_sha256
            }))
}
