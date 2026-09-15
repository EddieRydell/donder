use std::collections::BTreeMap;
use std::fs;
use std::io;

use camino::{Utf8Path, Utf8PathBuf};
use dawn_package::{
    Dependency, ExportGroup, PackageError, PackageManifest, RegistryClient, RegistryConfig,
};
use tempfile::{Builder, TempDir};
use uuid::Uuid;

use crate::{CliError, SyncMode, cache, parse_package_spec, sync};

const RELEASE_RECEIPT_FILE: &str = "dawn-release.json";

pub(crate) fn fork(root: &Utf8Path, dependency_alias: &str) -> Result<(), CliError> {
    let forked = dawn_package::PackageService::fork_dependency(
        root,
        dependency_alias,
        dawn_project_io::validate_registry_package_artifact,
        |candidate| {
            dawn_project_io::compile_package_with_cache(
                root,
                candidate.manifest.clone(),
                candidate.lock.clone(),
                &candidate.cache,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        },
    )?;
    println!(
        "forked {package}@{} into {}",
        forked.version,
        forked.destination,
        package = forked.package
    );
    Ok(())
}

pub(crate) fn new_project(destination: &Utf8Path, package_spec: &str) -> Result<(), CliError> {
    let (package, requirement) = parse_package_spec(package_spec)?;
    let destination_was_empty = validate_new_destination(destination)?;
    let parent = usable_parent(destination);
    fs::create_dir_all(parent)?;
    let temporary = utf8_tempdir(Builder::new().prefix(".dawn-new-").tempdir_in(parent)?)?;
    let resolver_root = temporary.path().join("resolver");
    let staged = temporary.path().join("project");
    fs::create_dir_all(&resolver_root)?;
    fs::write(
        resolver_root.join("resolver-root.dawn"),
        b"# Internal template resolver root.\n",
    )?;

    let resolver_manifest = PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: Uuid::new_v4(),
        language_version: "0.1".to_string(),
        project: None,
        publication: None,
        exports: BTreeMap::from([(
            "resolver".to_string(),
            ExportGroup {
                documents: vec!["resolver-root.dawn".to_string()],
            },
        )]),
        dependencies: BTreeMap::from([(
            "template".to_string(),
            Dependency::Registry {
                package: package.clone(),
                version: requirement,
            },
        )]),
        assets: BTreeMap::new(),
    };
    resolver_manifest.write(&resolver_root)?;

    let registry = RegistryConfig::read()?;
    let client = RegistryClient::discover(&registry)?;
    let lock = client.resolve_lock(&resolver_root, &resolver_manifest)?;
    let cache = cache()?;
    client.ensure_locked_artifacts(
        &lock,
        &cache,
        dawn_project_io::validate_registry_package_artifact,
    )?;
    let locked = lock.packages.get(&package).ok_or_else(|| {
        PackageError::Invalid(format!(
            "template `{package}` was not selected by resolution"
        ))
    })?;
    let source = cache.package_root(&locked.archive_sha256)?;
    prepare_template_copy(&source, &staged, &package, &locked.version)?;
    sync(&staged, SyncMode::Resolve)?;

    if destination_was_empty {
        fs::remove_dir(destination)?;
    }
    fs::rename(&staged, destination)?;
    println!("created {} from {package}@{}", destination, locked.version);
    Ok(())
}

fn prepare_template_copy(
    source: &Utf8Path,
    staged: &Utf8Path,
    package: &dawn_package::PackageId,
    version: &semver::Version,
) -> Result<(), CliError> {
    dawn_package::copy_package_tree(source, staged)?;
    remove_generated_package_files(staged)?;

    let mut manifest = PackageManifest::read(staged)?;
    if manifest.project.is_none() {
        return Err(PackageError::Invalid(format!(
            "template `{package}@{version}` does not contain a project entrypoint"
        ))
        .into());
    }
    manifest.module_id = Uuid::new_v4();
    manifest.publication = None;
    manifest.write(staged)?;
    Ok(())
}

fn validate_new_destination(destination: &Utf8Path) -> Result<bool, CliError> {
    match fs::metadata(destination) {
        Ok(metadata) if !metadata.is_dir() => Err(PackageError::Invalid(format!(
            "project destination is not a directory: `{destination}`"
        ))
        .into()),
        Ok(_) => {
            if fs::read_dir(destination)?.next().transpose()?.is_some() {
                return Err(PackageError::Invalid(format!(
                    "project destination is not empty: `{destination}`"
                ))
                .into());
            }
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn usable_parent(path: &Utf8Path) -> &Utf8Path {
    path.parent()
        .filter(|parent| !parent.as_str().is_empty())
        .unwrap_or_else(|| Utf8Path::new("."))
}

fn remove_generated_package_files(root: &Utf8Path) -> Result<(), CliError> {
    remove_if_file(&root.join(RELEASE_RECEIPT_FILE))?;
    remove_if_file(&root.join(dawn_package::LOCK_FILE))
}

fn remove_if_file(path: &Utf8Path) -> Result<(), CliError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

struct Utf8TempDir {
    _inner: TempDir,
    path: Utf8PathBuf,
}

impl Utf8TempDir {
    fn path(&self) -> &Utf8Path {
        &self.path
    }
}

fn utf8_tempdir(inner: TempDir) -> Result<Utf8TempDir, CliError> {
    let path = Utf8PathBuf::from_path_buf(inner.path().to_path_buf())
        .map_err(|_| PackageError::Invalid("temporary directory path is not UTF-8".to_string()))?;
    Ok(Utf8TempDir {
        _inner: inner,
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dawn_package::{ProjectManifest, Publication};
    use semver::Version;
    use tempfile::tempdir;

    #[test]
    fn template_copy_is_reidentified_and_clears_publication() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let source = root.join("source");
        let staged = root.join("staged");
        fs::create_dir_all(&source).expect("source");
        fs::write(source.join("project.dawn"), "project: {}\n").expect("project");
        fs::write(source.join(RELEASE_RECEIPT_FILE), "{}").expect("receipt");
        fs::write(source.join(dawn_package::LOCK_FILE), "{}").expect("lock");
        let original_module_id = Uuid::new_v4();
        PackageManifest {
            manifest_version: dawn_package::MANIFEST_VERSION,
            module_id: original_module_id,
            language_version: "0.1".to_string(),
            project: Some(ProjectManifest {
                entrypoint: "project.dawn".to_string(),
            }),
            publication: Some(Publication {
                package: dawn_package::PackageId::new("test/template").expect("package"),
                version: Version::parse("1.0.0").expect("version"),
                display_name: "Template".to_string(),
                summary: "Template fixture".to_string(),
                license: "MIT".to_string(),
                tags: Vec::new(),
            }),
            exports: BTreeMap::from([(
                "project".to_string(),
                ExportGroup {
                    documents: vec!["project.dawn".to_string()],
                },
            )]),
            dependencies: BTreeMap::new(),
            assets: BTreeMap::new(),
        }
        .write(&source)
        .expect("manifest");

        prepare_template_copy(
            &source,
            &staged,
            &dawn_package::PackageId::new("test/template").expect("package"),
            &Version::parse("1.0.0").expect("version"),
        )
        .expect("prepare template");

        let manifest = PackageManifest::read(&staged).expect("copied manifest");
        assert_ne!(manifest.module_id, original_module_id);
        assert_eq!(manifest.publication, None);
        assert!(manifest.project.is_some());
        assert!(!staged.join(RELEASE_RECEIPT_FILE).exists());
        assert!(!staged.join(dawn_package::LOCK_FILE).exists());
    }
}
