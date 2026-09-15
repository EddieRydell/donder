#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod auth;
mod project_ops;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use clap::{Parser, Subcommand};
use dawn_package::{
    AssetDeclaration, AssetKind, CacheStore, CandidateResolution, DawnDirectories, Dependency,
    ExportGroup, LockedDependency, Lockfile, PackageError, PackageId, PackageManifest,
    PackageService, PackedRelease, ProjectManifest, RegistryConfig,
};
use semver::VersionReq;
use uuid::Uuid;

#[derive(Debug, Parser)]
#[command(name = "dawn", version, about = "Dawn projects and packages")]
pub struct Cli {
    #[arg(short, long, default_value = ".")]
    path: Utf8PathBuf,
    #[command(subcommand)]
    command: Command,
}

impl Cli {
    pub fn parse_args() -> Self {
        Self::parse()
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Check,
    Add {
        alias: String,
        package: String,
    },
    Remove {
        alias: String,
    },
    Sync,
    Update {
        alias: Option<String>,
    },
    Tree,
    Pack,
    Publish {
        #[arg(long)]
        dry_run: bool,
    },
    Login,
    Logout,
    Whoami,
    Fork {
        dependency: String,
    },
    New {
        #[arg(long)]
        from: String,
    },
}

#[derive(Debug)]
pub enum CliError {
    Package(PackageError),
    Project(dawn_project_io::PackageLoadError),
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => error.fmt(formatter),
            Self::Project(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CliError {}

impl From<PackageError> for CliError {
    fn from(error: PackageError) -> Self {
        Self::Package(error)
    }
}

impl From<dawn_project_io::PackageLoadError> for CliError {
    fn from(error: dawn_project_io::PackageLoadError) -> Self {
        Self::Project(error)
    }
}

pub fn run(cli: Cli) -> Result<(), CliError> {
    match cli.command {
        Command::Init => init(&cli.path),
        Command::Check => check(&cli.path),
        Command::Add { alias, package } => add(&cli.path, &alias, &package),
        Command::Remove { alias } => remove(&cli.path, &alias),
        Command::Sync => sync(&cli.path, SyncMode::Preserve),
        Command::Update { alias } => update(&cli.path, alias.as_deref()),
        Command::Tree => tree(&cli.path),
        Command::Pack => pack(&cli.path),
        Command::Publish { dry_run } => publish(&cli.path, dry_run),
        Command::Login => auth::login(),
        Command::Logout => auth::logout(),
        Command::Whoami => auth::whoami(),
        Command::Fork { dependency } => project_ops::fork(&cli.path, &dependency),
        Command::New { from } => project_ops::new_project(&cli.path, &from),
    }
}

fn init(root: &Utf8Path) -> Result<(), CliError> {
    let registry = RegistryConfig::read()?;
    init_with_registry(root, &registry.registry)
}

fn init_with_registry(root: &Utf8Path, registry: &str) -> Result<(), CliError> {
    fs::create_dir_all(root)?;
    if root.join(dawn_package::MANIFEST_FILE).exists() {
        return Err(PackageError::Invalid(format!(
            "{} already exists",
            root.join(dawn_package::MANIFEST_FILE)
        ))
        .into());
    }
    let documents = dawn_documents(root)?;
    if documents.is_empty() {
        return Err(PackageError::Invalid(
            "dawn init requires at least one existing .dawn document".to_string(),
        )
        .into());
    }
    let project = documents
        .iter()
        .find(|path| path.as_str() == "project.dawn")
        .map(|path| ProjectManifest {
            entrypoint: path.to_string(),
        });
    let export_name = if project.is_some() {
        "project"
    } else {
        "default"
    };
    let export_documents = match &project {
        Some(project) => vec![project.entrypoint.clone()],
        None => documents.iter().map(ToString::to_string).collect(),
    };
    let manifest = PackageManifest {
        manifest_version: dawn_package::MANIFEST_VERSION,
        module_id: Uuid::new_v4(),
        language_version: dawn_package::LANGUAGE_VERSION.to_string(),
        project,
        publication: None,
        exports: BTreeMap::from([(
            export_name.to_string(),
            ExportGroup {
                documents: export_documents,
            },
        )]),
        dependencies: BTreeMap::new(),
        assets: audio_assets(root)?,
    };
    manifest.write(root)?;
    Lockfile::from_directory(&manifest, root, registry)?.write(root)?;
    println!("initialized {}", root.join(dawn_package::MANIFEST_FILE));
    Ok(())
}

fn check(root: &Utf8Path) -> Result<(), CliError> {
    let compiled = dawn_project_io::compile_package(root)?;
    let kind = if compiled.graph.project.is_some() {
        "project"
    } else {
        "library"
    };
    println!(
        "{kind} package is valid ({} compiled documents)",
        compiled.graph.source.documents.len()
    );
    Ok(())
}

fn add(root: &Utf8Path, alias: &str, package_spec: &str) -> Result<(), CliError> {
    let (package, version) = parse_package_spec(package_spec)?;
    let original_manifest = PackageManifest::read(root)?;
    let mut manifest = original_manifest.clone();
    if manifest.dependencies.contains_key(alias) {
        return Err(PackageError::Invalid(format!(
            "dependency alias `{alias}` is already declared"
        ))
        .into());
    }
    manifest
        .dependencies
        .insert(alias.to_string(), Dependency::Registry { package, version });
    commit_manifest_and_sync(root, &original_manifest, &manifest)
}

fn remove(root: &Utf8Path, alias: &str) -> Result<(), CliError> {
    let original_manifest = PackageManifest::read(root)?;
    let mut manifest = original_manifest.clone();
    if manifest.dependencies.remove(alias).is_none() {
        return Err(
            PackageError::Invalid(format!("dependency alias `{alias}` is not declared")).into(),
        );
    }
    commit_manifest_and_sync(root, &original_manifest, &manifest)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SyncMode {
    Preserve,
    Resolve,
}

pub(crate) fn sync(root: &Utf8Path, mode: SyncMode) -> Result<(), CliError> {
    let resolution = match mode {
        SyncMode::Preserve => CandidateResolution::Sync,
        SyncMode::Resolve => CandidateResolution::UpdateAll,
    };
    accept_package_candidate(root, resolution)
}

fn accept_package_candidate(
    root: &Utf8Path,
    resolution: CandidateResolution,
) -> Result<(), CliError> {
    let candidate = PackageService::prepare(
        root,
        resolution,
        dawn_project_io::validate_registry_package_artifact,
    )?;
    for package in &candidate.deprecated_packages {
        eprintln!("warning: package `{package}` is deprecated");
    }
    let _ = dawn_project_io::compile_package_with_cache(
        root,
        candidate.manifest,
        candidate.lock.clone(),
        &candidate.cache,
    )?;
    candidate.lock.write(root)?;
    println!("synchronized {}", root.join(dawn_package::LOCK_FILE));
    Ok(())
}

fn update(root: &Utf8Path, alias: Option<&str>) -> Result<(), CliError> {
    let resolution = alias.map_or(CandidateResolution::UpdateAll, |alias| {
        CandidateResolution::UpdateAlias(alias.to_string())
    });
    accept_package_candidate(root, resolution)
}

fn commit_manifest_and_sync(
    root: &Utf8Path,
    original: &PackageManifest,
    candidate: &PackageManifest,
) -> Result<(), CliError> {
    candidate.write(root)?;
    if let Err(operation_error) = sync(root, SyncMode::Resolve) {
        original.write(root).map_err(|rollback_error| {
            PackageError::Invalid(format!(
                "dependency change failed ({operation_error}) and restoring dawn-package.json also failed: {rollback_error}"
            ))
        })?;
        return Err(operation_error);
    }
    Ok(())
}

fn tree(root: &Utf8Path) -> Result<(), CliError> {
    let manifest = PackageManifest::read(root)?;
    let lock = Lockfile::read(root)?;
    lock.validate_local(root, &manifest)?;
    println!("project ({})", manifest.module_id);
    let mut visited = BTreeSet::new();
    for (alias, dependency) in &manifest.dependencies {
        print_dependency(alias, dependency, &lock, 1, &mut visited)?;
    }
    Ok(())
}

fn print_dependency(
    alias: &str,
    dependency: &Dependency,
    lock: &Lockfile,
    depth: usize,
    visited: &mut BTreeSet<String>,
) -> Result<(), CliError> {
    let indent = "  ".repeat(depth);
    match dependency {
        Dependency::Registry { package, .. } => {
            let locked = lock.packages.get(package).ok_or_else(|| {
                PackageError::Invalid(format!("dependency `{package}` is not locked"))
            })?;
            println!(
                "{indent}{alias}: {package}@{} [{}]",
                locked.version, locked.module_id
            );
            let key = format!("registry:{package}");
            if visited.insert(key) {
                for (child_alias, child) in &locked.dependencies {
                    print_dependency(
                        child_alias,
                        &Dependency::Registry {
                            package: child.clone(),
                            version: VersionReq::STAR,
                        },
                        lock,
                        depth + 1,
                        visited,
                    )?;
                }
            }
        }
        Dependency::Path { path } => {
            let locked = lock.path_dependencies.get(path).ok_or_else(|| {
                PackageError::Invalid(format!("path dependency `{path}` is not locked"))
            })?;
            println!(
                "{indent}{alias}: path:{} [{}]",
                locked.path, locked.module_id
            );
            let key = format!("path:{}", locked.path);
            if visited.insert(key) {
                for (child_alias, child) in &locked.dependencies {
                    match child {
                        LockedDependency::Registry { package } => {
                            print_dependency(
                                child_alias,
                                &Dependency::Registry {
                                    package: package.clone(),
                                    version: VersionReq::STAR,
                                },
                                lock,
                                depth + 1,
                                visited,
                            )?;
                        }
                        LockedDependency::Path { path } => {
                            print_dependency(
                                child_alias,
                                &Dependency::Path { path: path.clone() },
                                lock,
                                depth + 1,
                                visited,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn pack(root: &Utf8Path) -> Result<(), CliError> {
    let release = dawn_project_io::pack_package(root)?;
    let path = write_release_artifact(root, &release)?;
    print_release(&release);
    println!("archive: {path}");
    Ok(())
}

fn publish(root: &Utf8Path, dry_run: bool) -> Result<(), CliError> {
    let release = dawn_project_io::pack_package(root)?;
    if dry_run {
        print_release(&release);
        return Ok(());
    }
    let manifest = PackageManifest::read(root)?;
    let publication = manifest.publication.ok_or_else(|| {
        PackageError::Invalid("publication metadata is required to publish".to_string())
    })?;
    let (client, access_token) = auth::authenticated_client()?;
    let filename = release_filename(&publication.package, &publication.version)?;
    let finalized = client.publish(&access_token, &filename, &release.archive)?;
    if finalized.package != publication.package
        || finalized.version != publication.version
        || finalized.archive_sha256 != release.archive_sha256
    {
        return Err(PackageError::Invalid(
            "registry publication response does not match the packed release".to_string(),
        )
        .into());
    }
    println!(
        "published {}@{} ({})",
        finalized.package, finalized.version, finalized.archive_sha256
    );
    Ok(())
}

fn write_release_artifact(
    root: &Utf8Path,
    release: &PackedRelease,
) -> Result<Utf8PathBuf, CliError> {
    let publication = &release.receipt;
    let directory = root.join("target").join("dawn");
    fs::create_dir_all(&directory)?;
    let package_name = package_name_component(&publication.package)?;
    let hash_prefix = release.archive_sha256.get(..12).ok_or_else(|| {
        PackageError::Invalid("packed release returned an invalid archive hash".to_string())
    })?;
    let filename = format!(
        "{}-{}-{}.zip",
        package_name, publication.version, hash_prefix
    );
    let path = directory.join(filename);
    if path.is_file() {
        let existing = fs::read(&path)?;
        if dawn_package::sha256_hex(&existing) != release.archive_sha256 {
            return Err(PackageError::Invalid(format!(
                "existing release artifact is corrupt: `{path}`"
            ))
            .into());
        }
        return Ok(path);
    }
    dawn_package::atomic_write(&path, &release.archive)?;
    Ok(path)
}

fn release_filename(package: &PackageId, version: &semver::Version) -> Result<String, CliError> {
    let name = package_name_component(package)?;
    Ok(format!("{name}-{version}.zip"))
}

fn package_name_component(package: &PackageId) -> Result<&str, CliError> {
    package
        .as_str()
        .split_once('/')
        .map(|(_, name)| name)
        .ok_or_else(|| {
            PackageError::Invalid(format!("package `{package}` has no package-name component"))
                .into()
        })
}

fn print_release(release: &PackedRelease) {
    println!("archive sha256: {}", release.archive_sha256);
    println!("archive bytes: {}", release.archive.len());
    println!("files: {}", release.receipt.files.len());
}

pub(crate) fn parse_package_spec(value: &str) -> Result<(PackageId, VersionReq), CliError> {
    let (package, requirement) = value.rsplit_once('@').ok_or_else(|| {
        PackageError::Invalid("package must use owner/name@requirement syntax".to_string())
    })?;
    let package = PackageId::new(package)?;
    let requirement =
        VersionReq::parse(requirement).map_err(|error| PackageError::Invalid(error.to_string()))?;
    Ok((package, requirement))
}

fn dawn_documents(root: &Utf8Path) -> Result<Vec<Utf8PathBuf>, CliError> {
    let mut files = Vec::new();
    collect_matching_files(root, root, &mut files, |path| path.ends_with(".dawn"))?;
    Ok(files)
}

fn audio_assets(root: &Utf8Path) -> Result<BTreeMap<String, AssetDeclaration>, CliError> {
    let mut files = Vec::new();
    collect_matching_files(root, root, &mut files, |path| {
        matches!(
            Utf8Path::new(path)
                .extension()
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str(),
            "mp3" | "wav" | "ogg" | "flac"
        )
    })?;
    Ok(files
        .into_iter()
        .map(|path| {
            (
                path.to_string(),
                AssetDeclaration {
                    kind: AssetKind::Audio,
                },
            )
        })
        .collect())
}

fn collect_matching_files(
    root: &Utf8Path,
    directory: &Utf8Path,
    result: &mut Vec<Utf8PathBuf>,
    matches: impl Fn(&str) -> bool + Copy,
) -> Result<(), CliError> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|_| PackageError::Invalid("package path is not UTF-8".to_string()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PackageError::Invalid("package path escaped its root".to_string()))?;
        if relative.starts_with("target") || relative.starts_with(".git") {
            continue;
        }
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            return Err(PackageError::Invalid(format!(
                "symbolic links are not allowed: `{relative}`"
            ))
            .into());
        }
        if file_type.is_dir() {
            collect_matching_files(root, &path, result, matches)?;
        } else if file_type.is_file() {
            let relative = Utf8PathBuf::from(relative.as_str().replace('\\', "/"));
            if matches(relative.as_str()) {
                result.push(relative);
            }
        }
    }
    result.sort();
    Ok(())
}

impl From<std::io::Error> for CliError {
    fn from(error: std::io::Error) -> Self {
        Self::Package(PackageError::Io(error))
    }
}

pub(crate) fn cache() -> Result<CacheStore, CliError> {
    Ok(DawnDirectories::discover()?.package_cache())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn clap_exposes_the_complete_package_workflow() {
        let commands = [
            vec!["dawn", "init"],
            vec!["dawn", "check"],
            vec!["dawn", "add", "library", "alice/library@^1.0"],
            vec!["dawn", "remove", "library"],
            vec!["dawn", "sync"],
            vec!["dawn", "update"],
            vec!["dawn", "update", "library"],
            vec!["dawn", "tree"],
            vec!["dawn", "pack"],
            vec!["dawn", "publish"],
            vec!["dawn", "publish", "--dry-run"],
            vec!["dawn", "login"],
            vec!["dawn", "logout"],
            vec!["dawn", "whoami"],
            vec!["dawn", "fork", "library"],
            vec!["dawn", "new", "--from", "alice/template@^1.0"],
        ];

        for command in commands {
            Cli::try_parse_from(command).expect("CLI command");
        }
    }

    #[test]
    fn init_creates_a_manifest_and_deterministic_empty_lock() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        fs::write(root.join("project.dawn"), "project: {}\n").expect("document");

        init_with_registry(root, "https://registry.dawn.dev").expect("init");

        let manifest = PackageManifest::read(root).expect("manifest");
        let lock = Lockfile::read(root).expect("lock");
        assert_eq!(
            manifest
                .project
                .as_ref()
                .map(|project| project.entrypoint.as_str()),
            Some("project.dawn")
        );
        assert_eq!(manifest.publication, None);
        assert!(manifest.dependencies.is_empty());
        assert!(lock.packages.is_empty());
        assert_eq!(lock.registry, "https://registry.dawn.dev");
        lock.validate_local(root, &manifest).expect("current lock");
    }

    #[test]
    fn package_specs_require_identity_and_requirement() {
        assert!(parse_package_spec("alice/library@^1.2").is_ok());
        assert!(parse_package_spec("alice/library").is_err());
        assert!(parse_package_spec("Alice/library@^1.2").is_err());
        assert!(parse_package_spec("alice/library@not-semver").is_err());
    }
}
