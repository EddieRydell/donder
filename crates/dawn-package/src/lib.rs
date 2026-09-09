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

mod validation;
use camino::{Utf8Path, Utf8PathBuf};
use semver::{Op, Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::{self, Cursor, Read, Write};
use tempfile::NamedTempFile;
use uuid::Uuid;
use validation::{require_dawn_document, valid_alias, validate_relative_path};
pub use validation::{validate_module_relative_dawn_path, validate_package_reference_name};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

mod cache;
mod config;
mod registry_client;
mod registry_protocol;
mod resolver;
mod service;
pub use cache::{CacheStatus, CacheStore};
pub use config::{DEFAULT_REGISTRY_URL, DawnDirectories, RegistryConfig};
pub use registry_client::{RegistryClient, RegistryResolution};
pub use registry_protocol::*;
pub use resolver::{RegistryRelease, resolve_registry, resolve_registry_with_pins};
pub use service::{
    CandidateResolution, ForkedDependency, PackageService, PackageVersionChange,
    PackageVersionChangeKind, PreparedPackageCandidate, copy_package_tree,
};

pub const MANIFEST_FILE: &str = "dawn-package.json";
pub const LOCK_FILE: &str = "dawn.lock";
pub const MANIFEST_VERSION: u8 = 2;
pub const LOCK_VERSION: u8 = 1;
pub const RELEASE_RECEIPT_VERSION: u8 = 1;
pub const LANGUAGE_VERSION: &str = "0.1";
pub const MAX_ARCHIVE_BYTES: usize = 25 * 1024 * 1024;
pub const MAX_EXPANDED_BYTES: u64 = 50 * 1024 * 1024;
pub const MAX_FILES: usize = 500;
pub const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
pub const MAX_TEXT_BYTES: u64 = 1024 * 1024;
pub const MAX_PREVIEW_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug)]
pub enum PackageError {
    Io(io::Error),
    Json(serde_json::Error),
    Invalid(String),
    Archive(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Archive(message) => write!(formatter, "archive error: {message}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<io::Error> for PackageError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for PackageError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<zip::result::ZipError> for PackageError {
    fn from(error: zip::result::ZipError) -> Self {
        Self::Archive(error.to_string())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageFileParseError {
    pub field_path: Option<String>,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl fmt::Display for PackageFileParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.field_path {
            write!(formatter, "{path}: {}", self.message)
        } else {
            formatter.write_str(&self.message)
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackageValidationIssue {
    pub field_path: String,
    pub message: String,
}

fn validation_result(issues: Vec<PackageValidationIssue>) -> Result<(), PackageError> {
    match issues.into_iter().next() {
        Some(issue) => Err(PackageError::Invalid(issue.message)),
        None => Ok(()),
    }
}

fn push_package_validation_issue(
    issues: &mut Vec<PackageValidationIssue>,
    field_path: impl Into<String>,
    message: impl Into<String>,
) {
    issues.push(PackageValidationIssue {
        field_path: field_path.into(),
        message: message.into(),
    });
}

fn collect_path_issue(
    issues: &mut Vec<PackageValidationIssue>,
    field_path: &str,
    value: &str,
    validate: impl FnOnce(&str) -> Result<(), PackageError>,
) {
    if let Err(error) = validate(value) {
        push_package_validation_issue(issues, field_path, error.to_string());
    }
}

fn parse_json_with_path<T>(bytes: &[u8]) -> Result<T, PackageFileParseError>
where
    T: for<'de> Deserialize<'de>,
{
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    serde_path_to_error::deserialize(&mut deserializer).map_err(|error| {
        let inner = error.inner();
        let path = error.path().to_string();
        PackageFileParseError {
            field_path: (!path.is_empty() && path != ".").then_some(path),
            line: inner.line().saturating_sub(1) as u32,
            column: inner.column().saturating_sub(1) as u32,
            message: inner.to_string(),
        }
    })
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize)]
#[serde(transparent)]
pub struct PackageId(String);

impl PackageId {
    pub fn new(value: impl Into<String>) -> Result<Self, PackageError> {
        let value = value.into();
        let mut parts = value.split('/');
        let owner = parts.next().unwrap_or_default();
        let name = parts.next().unwrap_or_default();
        if parts.next().is_some() || !valid_segment(owner, 39) || !valid_segment(name, 64) {
            return Err(PackageError::Invalid(format!(
                "invalid package identity `{value}`"
            )));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for PackageId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(serde::de::Error::custom)
    }
}

fn valid_segment(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && value
            .as_bytes()
            .last()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PackageManifest {
    pub manifest_version: u8,
    pub module_id: Uuid,
    pub language_version: String,
    pub requires_dawn: VersionReq,
    #[serde(default)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectManifest>,
    #[serde(deserialize_with = "deserialize_required_option")]
    pub publication: Option<Publication>,
    pub exports: BTreeMap<String, ExportGroup>,
    pub dependencies: BTreeMap<String, Dependency>,
    pub assets: BTreeMap<String, AssetDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub entrypoint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Publication {
    pub package: PackageId,
    pub version: Version,
    pub display_name: String,
    pub summary: String,
    pub license: String,
    pub tags: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportGroup {
    pub documents: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssetDeclaration {
    pub kind: AssetKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetKind {
    Audio,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum Dependency {
    Registry {
        package: PackageId,
        version: VersionReq,
    },
    Path {
        path: String,
    },
}

fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

impl PackageManifest {
    pub fn read_for_analysis(root: &Utf8Path) -> Result<Self, PackageFileParseError> {
        let path = root.join(MANIFEST_FILE);
        let bytes = fs::read(&path).map_err(|error| PackageFileParseError {
            field_path: None,
            line: 0,
            column: 0,
            message: error.to_string(),
        })?;
        Self::parse_for_analysis(&bytes)
    }

    pub fn parse_for_analysis(bytes: &[u8]) -> Result<Self, PackageFileParseError> {
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PackageFileParseError {
                field_path: None,
                line: 0,
                column: 0,
                message: format!("manifest exceeds {MAX_MANIFEST_BYTES} bytes"),
            });
        }
        parse_json_with_path(bytes)
    }

    pub fn validation_issues(&self, root: &Utf8Path) -> Vec<PackageValidationIssue> {
        let mut issues = self.contract_issues();
        if let Some(project) = &self.project {
            collect_path_issue(
                &mut issues,
                "project.entrypoint",
                &project.entrypoint,
                |value| require_file(root, value, "project.entrypoint"),
            );
        }
        for (group, export) in &self.exports {
            for (index, document) in export.documents.iter().enumerate() {
                let field_path = format!("exports.{group}.documents[{index}]");
                collect_path_issue(&mut issues, &field_path, document, |value| {
                    require_file(root, value, "export document")
                });
            }
        }
        for (alias, dependency) in &self.dependencies {
            if let Dependency::Path { path } = dependency {
                let field_path = format!("dependencies.{alias}");
                collect_path_issue(&mut issues, &field_path, path, |value| {
                    require_directory(root, value, "path dependency")
                });
            }
        }
        for path in self.assets.keys() {
            let field_path = format!("assets.{path}");
            collect_path_issue(&mut issues, &field_path, path, |value| {
                require_file(root, value, "asset")
            });
        }
        issues
    }

    fn contract_issues(&self) -> Vec<PackageValidationIssue> {
        let mut issues = Vec::new();

        if self.manifest_version != MANIFEST_VERSION {
            push_package_validation_issue(
                &mut issues,
                "manifestVersion",
                format!("manifestVersion must be {MANIFEST_VERSION}"),
            );
        }
        if self.module_id.get_version_num() != 4 {
            push_package_validation_issue(
                &mut issues,
                "moduleId",
                "moduleId must be a version 4 UUID",
            );
        }
        if !valid_language_version(&self.language_version) {
            push_package_validation_issue(
                &mut issues,
                "languageVersion",
                "languageVersion must be an exact major.minor language version",
            );
        }
        if !is_bounded_version_requirement(&self.requires_dawn) {
            push_package_validation_issue(
                &mut issues,
                "requiresDawn",
                "requiresDawn must have lower and upper semantic-version bounds",
            );
        }
        if self.exports.is_empty() {
            push_package_validation_issue(
                &mut issues,
                "exports",
                "at least one export group is required",
            );
        }
        if let Some(project) = &self.project {
            collect_path_issue(
                &mut issues,
                "project.entrypoint",
                &project.entrypoint,
                |value| validate_relative_path(value, "project.entrypoint"),
            );
            collect_path_issue(
                &mut issues,
                "project.entrypoint",
                &project.entrypoint,
                |value| require_dawn_document(value, "project.entrypoint"),
            );
        }
        for (group, export) in &self.exports {
            let group_path = format!("exports.{group}");
            if !valid_alias(group) {
                push_package_validation_issue(
                    &mut issues,
                    &group_path,
                    format!("invalid export group `{group}`"),
                );
            }
            if export.documents.is_empty() {
                push_package_validation_issue(
                    &mut issues,
                    &group_path,
                    format!("export group `{group}` is empty"),
                );
            }
            let mut documents = BTreeSet::new();
            for (index, document) in export.documents.iter().enumerate() {
                let field_path = format!("{group_path}.documents[{index}]");
                collect_path_issue(
                    &mut issues,
                    &field_path,
                    document,
                    validate_module_relative_dawn_path,
                );
                if !documents.insert(document) {
                    push_package_validation_issue(
                        &mut issues,
                        &field_path,
                        format!("export group `{group}` lists `{document}` more than once"),
                    );
                }
            }
        }
        for (alias, dependency) in &self.dependencies {
            let field_path = format!("dependencies.{alias}");
            if !valid_alias(alias) {
                push_package_validation_issue(
                    &mut issues,
                    &field_path,
                    format!("invalid dependency alias `{alias}`"),
                );
            }
            if let Dependency::Path { path } = dependency {
                collect_path_issue(&mut issues, &field_path, path, |value| {
                    validate_relative_path(value, "path dependency")
                });
            }
        }
        for (path, asset) in &self.assets {
            let field_path = format!("assets.{path}");
            collect_path_issue(&mut issues, &field_path, path, |value| {
                validate_relative_path(value, "asset")
            });
            if asset.kind != AssetKind::Audio {
                push_package_validation_issue(
                    &mut issues,
                    &field_path,
                    format!("unsupported asset kind for `{path}`"),
                );
            }
            let extension = Utf8Path::new(path)
                .extension()
                .unwrap_or_default()
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "mp3" | "wav" | "ogg" | "flac") {
                push_package_validation_issue(
                    &mut issues,
                    &field_path,
                    format!("audio asset `{path}` must be MP3, WAV, OGG, or FLAC"),
                );
            }
        }
        if let Some(publication) = &self.publication {
            if publication.display_name.trim().is_empty() || publication.display_name.len() > 80 {
                push_package_validation_issue(
                    &mut issues,
                    "publication.displayName",
                    "publication displayName must contain 1 to 80 characters",
                );
            }
            if publication.summary.trim().is_empty() || publication.summary.len() > 240 {
                push_package_validation_issue(
                    &mut issues,
                    "publication.summary",
                    "publication summary must contain 1 to 240 characters",
                );
            }
            if let Err(error) = spdx::Expression::parse(&publication.license) {
                push_package_validation_issue(
                    &mut issues,
                    "publication.license",
                    format!("publication license must be a valid SPDX expression: {error}"),
                );
            }
            if publication.tags.len() > 10 {
                push_package_validation_issue(
                    &mut issues,
                    "publication.tags",
                    "publication may contain at most 10 tags",
                );
            }
            for (index, tag) in publication.tags.iter().enumerate() {
                if !valid_tag(tag) {
                    push_package_validation_issue(
                        &mut issues,
                        format!("publication.tags[{index}]"),
                        format!("invalid publication tag `{tag}`"),
                    );
                }
            }
            if publication.tags.iter().collect::<BTreeSet<_>>().len() != publication.tags.len() {
                push_package_validation_issue(
                    &mut issues,
                    "publication.tags",
                    "publication tags must be unique",
                );
            }
        }
        issues
    }

    pub fn read(root: &Utf8Path) -> Result<Self, PackageError> {
        let path = root.join(MANIFEST_FILE);
        let bytes = fs::read(&path)?;
        if bytes.len() as u64 > MAX_MANIFEST_BYTES {
            return Err(PackageError::Invalid(format!(
                "manifest exceeds {} bytes",
                MAX_MANIFEST_BYTES
            )));
        }
        let manifest = serde_json::from_slice::<Self>(&bytes)?;
        manifest.validate(root)?;
        Ok(manifest)
    }

    pub fn write(&self, root: &Utf8Path) -> Result<(), PackageError> {
        self.validate(root)?;
        let bytes = canonical_json(self)?;
        atomic_write(&root.join(MANIFEST_FILE), &bytes)
    }

    pub fn validate(&self, root: &Utf8Path) -> Result<(), PackageError> {
        validation_result(self.validation_issues(root))
    }

    pub fn validate_contract(&self) -> Result<(), PackageError> {
        validation_result(self.contract_issues())
    }

    pub fn validate_runtime_compatibility(&self) -> Result<(), PackageError> {
        if self.language_version != LANGUAGE_VERSION {
            return Err(PackageError::Invalid(format!(
                "package languageVersion `{}` is incompatible with Dawn languageVersion `{LANGUAGE_VERSION}`",
                self.language_version
            )));
        }
        let dawn_version = current_dawn_version()?;
        if !self.requires_dawn.matches(&dawn_version) {
            return Err(PackageError::Invalid(format!(
                "package requires Dawn `{}`, but this is Dawn `{dawn_version}`",
                self.requires_dawn
            )));
        }
        Ok(())
    }

    pub fn project_entrypoint(&self, root: &Utf8Path) -> Result<Utf8PathBuf, PackageError> {
        let entrypoint = self
            .project
            .as_ref()
            .ok_or_else(|| PackageError::Invalid("manifest has no project entrypoint".to_string()))?
            .entrypoint
            .clone();
        require_file(root, &entrypoint, "project.entrypoint")?;
        Ok(Utf8PathBuf::from(entrypoint))
    }
}

pub fn current_dawn_version() -> Result<Version, PackageError> {
    Version::parse(env!("CARGO_PKG_VERSION")).map_err(|error| {
        PackageError::Invalid(format!(
            "Dawn build has an invalid semantic version: {error}"
        ))
    })
}

pub fn is_bounded_version_requirement(requirement: &VersionReq) -> bool {
    let has_lower_bound = requirement.comparators.iter().any(|comparator| {
        matches!(
            comparator.op,
            Op::Exact | Op::Greater | Op::GreaterEq | Op::Tilde | Op::Caret | Op::Wildcard
        )
    });
    let has_upper_bound = requirement.comparators.iter().any(|comparator| {
        matches!(
            comparator.op,
            Op::Exact | Op::Less | Op::LessEq | Op::Tilde | Op::Caret | Op::Wildcard
        )
    });
    has_lower_bound && has_upper_bound
}

fn valid_language_version(value: &str) -> bool {
    let mut components = value.split('.');
    let major = components.next().unwrap_or_default();
    let minor = components.next().unwrap_or_default();
    components.next().is_none()
        && !major.is_empty()
        && !minor.is_empty()
        && major.bytes().all(|byte| byte.is_ascii_digit())
        && minor.bytes().all(|byte| byte.is_ascii_digit())
        && (major == "0" || !major.starts_with('0'))
        && (minor == "0" || !minor.starts_with('0'))
}

fn valid_object_name(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || byte == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

fn valid_tag(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && value
            .as_bytes()
            .first()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
}

fn require_file(root: &Utf8Path, path: &str, label: &str) -> Result<(), PackageError> {
    let absolute = root.join(path);
    if !absolute.is_file() {
        return Err(PackageError::Invalid(format!(
            "{label} `{path}` does not exist"
        )));
    }
    Ok(())
}

fn require_directory(root: &Utf8Path, path: &str, label: &str) -> Result<(), PackageError> {
    if !root.join(path).is_dir() {
        return Err(PackageError::Invalid(format!(
            "{label} `{path}` does not exist"
        )));
    }
    Ok(())
}

pub fn canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, PackageError> {
    Ok(serde_json::to_vec(value)?)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn manifest_hash(manifest: &PackageManifest) -> Result<String, PackageError> {
    Ok(sha256_hex(&canonical_json(manifest)?))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct Lockfile {
    pub lock_version: u8,
    pub manifest_sha256: String,
    pub registry: String,
    pub packages: BTreeMap<PackageId, LockedPackage>,
    pub path_dependencies: BTreeMap<String, PathLock>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct LockedPackage {
    pub version: Version,
    pub archive_sha256: String,
    pub module_id: Uuid,
    pub dependencies: BTreeMap<String, PackageId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolutionPin {
    pub version: Version,
    pub archive_sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PathLock {
    pub path: String,
    pub content_sha256: String,
    pub module_id: Uuid,
    pub dependencies: BTreeMap<String, LockedDependency>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
pub enum LockedDependency {
    Registry { package: PackageId },
    Path { path: String },
}

impl Lockfile {
    pub fn read_for_analysis(root: &Utf8Path) -> Result<Self, PackageFileParseError> {
        let bytes = fs::read(root.join(LOCK_FILE)).map_err(|error| PackageFileParseError {
            field_path: None,
            line: 0,
            column: 0,
            message: error.to_string(),
        })?;
        Self::parse_for_analysis(&bytes)
    }

    pub fn parse_for_analysis(bytes: &[u8]) -> Result<Self, PackageFileParseError> {
        parse_json_with_path(bytes)
    }

    pub fn validation_issues(&self, manifest: &PackageManifest) -> Vec<PackageValidationIssue> {
        let mut issues = Vec::new();
        if self.lock_version != LOCK_VERSION {
            push_package_validation_issue(
                &mut issues,
                "lockVersion",
                "unsupported lockfile version",
            );
        }
        match manifest_hash(manifest) {
            Ok(expected) if self.manifest_sha256 != expected => {
                push_package_validation_issue(
                    &mut issues,
                    "manifestSha256",
                    "dawn.lock does not match dawn-package.json",
                );
            }
            Err(error) => {
                push_package_validation_issue(&mut issues, "manifestSha256", error.to_string());
            }
            Ok(_) => {}
        }
        if self.registry.trim().is_empty() {
            push_package_validation_issue(&mut issues, "registry", "dawn.lock registry is empty");
        }

        let mut module_ids = BTreeMap::from([(manifest.module_id, "project".to_string())]);
        for (package, locked) in &self.packages {
            let package_path = format!("packages.{package}");
            if let Err(error) = validate_sha256(&locked.archive_sha256, "locked archive hash") {
                push_package_validation_issue(
                    &mut issues,
                    format!("{package_path}.archiveSha256"),
                    error.to_string(),
                );
            }
            if locked.module_id.get_version_num() != 4 {
                push_package_validation_issue(
                    &mut issues,
                    format!("{package_path}.moduleId"),
                    format!("locked package `{package}` has an invalid moduleId"),
                );
            }
            if let Some(existing) = module_ids.insert(locked.module_id, package.to_string()) {
                push_package_validation_issue(
                    &mut issues,
                    format!("{package_path}.moduleId"),
                    format!(
                        "moduleId `{}` is shared by `{existing}` and `{package}`",
                        locked.module_id
                    ),
                );
            }
            for (alias, dependency) in &locked.dependencies {
                if !self.packages.contains_key(dependency) {
                    push_package_validation_issue(
                        &mut issues,
                        format!("{package_path}.dependencies.{alias}"),
                        format!(
                            "locked package `{package}` points to unlocked dependency `{dependency}`"
                        ),
                    );
                }
            }
        }
        for (path, locked) in &self.path_dependencies {
            let dependency_path = format!("pathDependencies.{path}");
            if let Err(error) = validate_relative_path(path, "locked path dependency") {
                push_package_validation_issue(&mut issues, &dependency_path, error.to_string());
            }
            if locked.path != *path {
                push_package_validation_issue(
                    &mut issues,
                    format!("{dependency_path}.path"),
                    format!(
                        "path dependency key `{path}` does not match `{}`",
                        locked.path
                    ),
                );
            }
            if let Err(error) =
                validate_sha256(&locked.content_sha256, "path dependency content hash")
            {
                push_package_validation_issue(
                    &mut issues,
                    format!("{dependency_path}.contentSha256"),
                    error.to_string(),
                );
            }
            if locked.module_id.get_version_num() != 4 {
                push_package_validation_issue(
                    &mut issues,
                    format!("{dependency_path}.moduleId"),
                    format!("path dependency `{path}` has an invalid moduleId"),
                );
            }
            if let Some(existing) = module_ids.insert(locked.module_id, format!("path:{path}")) {
                push_package_validation_issue(
                    &mut issues,
                    format!("{dependency_path}.moduleId"),
                    format!(
                        "moduleId `{}` is shared by `{existing}` and `path:{path}`",
                        locked.module_id
                    ),
                );
            }
            for (alias, dependency) in &locked.dependencies {
                let field_path = format!("{dependency_path}.dependencies.{alias}");
                match dependency {
                    LockedDependency::Registry { package }
                        if !self.packages.contains_key(package) =>
                    {
                        push_package_validation_issue(
                            &mut issues,
                            field_path,
                            format!(
                                "path dependency `{path}` points to unlocked package `{package}`"
                            ),
                        );
                    }
                    LockedDependency::Path {
                        path: dependency_path,
                    } if !self.path_dependencies.contains_key(dependency_path) => {
                        push_package_validation_issue(
                            &mut issues,
                            field_path,
                            format!(
                                "path dependency `{path}` points to unlocked path `{dependency_path}`"
                            ),
                        );
                    }
                    _ => {}
                }
            }
        }
        for (alias, dependency) in &manifest.dependencies {
            match dependency {
                Dependency::Registry { package, .. } if !self.packages.contains_key(package) => {
                    push_package_validation_issue(
                        &mut issues,
                        format!("packages.{package}"),
                        format!(
                            "registry dependency `{alias}` points to unlocked package `{package}`"
                        ),
                    );
                }
                Dependency::Path { path } if !self.path_dependencies.contains_key(path) => {
                    push_package_validation_issue(
                        &mut issues,
                        format!("pathDependencies.{path}"),
                        format!("path dependency `{alias}` at `{path}` is not locked"),
                    );
                }
                _ => {}
            }
        }
        if let Err(error) = validate_lock_graph(manifest, self) {
            let message = error.to_string();
            if !issues.iter().any(|issue| issue.message == message) {
                push_package_validation_issue(&mut issues, "packages", message);
            }
        }
        issues
    }

    pub fn new(
        manifest: &PackageManifest,
        registry: impl Into<String>,
    ) -> Result<Self, PackageError> {
        Ok(Self {
            lock_version: LOCK_VERSION,
            manifest_sha256: manifest_hash(manifest)?,
            registry: registry.into(),
            packages: BTreeMap::new(),
            path_dependencies: BTreeMap::new(),
        })
    }

    pub fn from_directory(
        manifest: &PackageManifest,
        root: &Utf8Path,
        registry: impl Into<String>,
    ) -> Result<Self, PackageError> {
        let mut lock = Self::new(manifest, registry)?;
        lock.path_dependencies = collect_path_modules(root, manifest)?
            .into_iter()
            .map(|(path, module)| (path, module.lock))
            .collect();
        Ok(lock)
    }

    pub fn read(root: &Utf8Path) -> Result<Self, PackageError> {
        Ok(serde_json::from_slice(&fs::read(root.join(LOCK_FILE))?)?)
    }

    pub fn write(&self, root: &Utf8Path) -> Result<(), PackageError> {
        let bytes = canonical_json(self)?;
        atomic_write(&root.join(LOCK_FILE), &bytes)
    }

    pub fn validate_manifest(&self, manifest: &PackageManifest) -> Result<(), PackageError> {
        validation_result(self.validation_issues(manifest))
    }

    pub fn validate_local(
        &self,
        root: &Utf8Path,
        manifest: &PackageManifest,
    ) -> Result<(), PackageError> {
        self.validate_manifest(manifest)?;
        let current = collect_path_modules(root, manifest)?
            .into_iter()
            .map(|(path, module)| (path, module.lock))
            .collect::<BTreeMap<_, _>>();
        if current != self.path_dependencies {
            return Err(PackageError::Invalid(
                "dawn.lock does not match path dependency content".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PathModuleRecord {
    pub manifest: PackageManifest,
    pub lock: PathLock,
}

pub(crate) fn collect_path_modules(
    root: &Utf8Path,
    manifest: &PackageManifest,
) -> Result<BTreeMap<String, PathModuleRecord>, PackageError> {
    let canonical_root = root.canonicalize_utf8().map_err(PackageError::Io)?;
    let mut modules = BTreeMap::new();
    let mut visiting = Vec::new();
    for dependency in manifest.dependencies.values() {
        if let Dependency::Path { path } = dependency {
            collect_path_module(&canonical_root, path, &mut modules, &mut visiting)?;
        }
    }
    Ok(modules)
}

fn collect_path_module(
    project_root: &Utf8Path,
    relative_path: &str,
    modules: &mut BTreeMap<String, PathModuleRecord>,
    visiting: &mut Vec<String>,
) -> Result<(), PackageError> {
    if modules.contains_key(relative_path) {
        return Ok(());
    }
    if let Some(index) = visiting.iter().position(|path| path == relative_path) {
        let mut cycle = visiting[index..].to_vec();
        cycle.push(relative_path.to_string());
        return Err(PackageError::Invalid(format!(
            "path dependency cycle: {}",
            cycle.join(" -> ")
        )));
    }
    visiting.push(relative_path.to_string());
    let module_root = project_root.join(relative_path);
    let canonical_module_root = module_root.canonicalize_utf8().map_err(PackageError::Io)?;
    if !canonical_module_root.starts_with(project_root) {
        return Err(PackageError::Invalid(format!(
            "path dependency `{relative_path}` escapes the project root"
        )));
    }
    let module_manifest = PackageManifest::read(&canonical_module_root)?;
    let mut dependencies = BTreeMap::new();
    for (alias, dependency) in &module_manifest.dependencies {
        let locked = match dependency {
            Dependency::Registry { package, .. } => LockedDependency::Registry {
                package: package.clone(),
            },
            Dependency::Path { path } => {
                let dependency_root = canonical_module_root.join(path);
                let canonical_dependency_root = dependency_root
                    .canonicalize_utf8()
                    .map_err(PackageError::Io)?;
                let dependency_path = canonical_dependency_root
                    .strip_prefix(project_root)
                    .map_err(|_| {
                        PackageError::Invalid(format!(
                            "path dependency `{path}` from `{relative_path}` escapes the project root"
                        ))
                    })?
                    .as_str()
                    .replace('\\', "/");
                validate_relative_path(&dependency_path, "path dependency")?;
                collect_path_module(project_root, &dependency_path, modules, visiting)?;
                LockedDependency::Path {
                    path: dependency_path,
                }
            }
        };
        dependencies.insert(alias.clone(), locked);
    }
    let lock = PathLock {
        path: relative_path.to_string(),
        content_sha256: directory_content_hash(&canonical_module_root, &module_manifest)?,
        module_id: module_manifest.module_id,
        dependencies,
    };
    modules.insert(
        relative_path.to_string(),
        PathModuleRecord {
            manifest: module_manifest,
            lock,
        },
    );
    let _ = visiting.pop();
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<(), PackageError> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(PackageError::Invalid(format!(
            "{label} must be a lowercase SHA-256 digest"
        )));
    }
    Ok(())
}

fn validate_lock_graph(
    manifest: &PackageManifest,
    lockfile: &Lockfile,
) -> Result<(), PackageError> {
    let mut graph = BTreeMap::new();
    graph.insert(
        "project".to_string(),
        manifest
            .dependencies
            .values()
            .map(lock_node_for_manifest_dependency)
            .collect::<Vec<_>>(),
    );
    for (package, locked) in &lockfile.packages {
        graph.insert(
            format!("registry:{package}"),
            locked
                .dependencies
                .values()
                .map(|dependency| format!("registry:{dependency}"))
                .collect(),
        );
    }
    for (path, locked) in &lockfile.path_dependencies {
        graph.insert(
            format!("path:{path}"),
            locked
                .dependencies
                .values()
                .map(lock_node_for_locked_dependency)
                .collect(),
        );
    }
    let mut visited = BTreeSet::new();
    let mut visiting = Vec::new();
    visit_lock_node("project", &graph, &mut visiting, &mut visited)?;
    if visited.len() != graph.len() {
        let unreachable = graph
            .keys()
            .filter(|node| !visited.contains(*node))
            .cloned()
            .collect::<Vec<_>>();
        return Err(PackageError::Invalid(format!(
            "dawn.lock contains unreachable modules: {}",
            unreachable.join(", ")
        )));
    }
    Ok(())
}

fn lock_node_for_manifest_dependency(dependency: &Dependency) -> String {
    match dependency {
        Dependency::Registry { package, .. } => format!("registry:{package}"),
        Dependency::Path { path } => format!("path:{path}"),
    }
}

fn lock_node_for_locked_dependency(dependency: &LockedDependency) -> String {
    match dependency {
        LockedDependency::Registry { package } => format!("registry:{package}"),
        LockedDependency::Path { path } => format!("path:{path}"),
    }
}

fn visit_lock_node(
    node: &str,
    graph: &BTreeMap<String, Vec<String>>,
    visiting: &mut Vec<String>,
    visited: &mut BTreeSet<String>,
) -> Result<(), PackageError> {
    if visited.contains(node) {
        return Ok(());
    }
    if let Some(index) = visiting.iter().position(|entry| entry == node) {
        let mut cycle = visiting[index..].to_vec();
        cycle.push(node.to_string());
        return Err(PackageError::Invalid(format!(
            "package dependency cycle: {}",
            cycle.join(" -> ")
        )));
    }
    let dependencies = graph.get(node).ok_or_else(|| {
        PackageError::Invalid(format!("dawn.lock points to missing module `{node}`"))
    })?;
    visiting.push(node.to_string());
    for dependency in dependencies {
        visit_lock_node(dependency, graph, visiting, visited)?;
    }
    let _ = visiting.pop();
    visited.insert(node.to_string());
    Ok(())
}

/// A package-resolved source module. Physical roots are deliberately kept
/// outside `DocumentId`; moving an immutable cache entry never changes domain
/// identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedModule {
    pub manifest: PackageManifest,
    pub root: Utf8PathBuf,
    pub origin: ResolvedModuleOrigin,
    pub dependencies: BTreeMap<String, Uuid>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolvedModuleOrigin {
    Project,
    PathDependency {
        declared_path: String,
        content_sha256: String,
    },
    RegistryDependency {
        package: PackageId,
        version: Version,
        archive_sha256: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedSourceGraph {
    project_module_id: Uuid,
    modules: BTreeMap<Uuid, ResolvedModule>,
}

impl ResolvedSourceGraph {
    pub fn project(root: &Utf8Path, manifest: PackageManifest) -> Result<Self, PackageError> {
        let root = root.canonicalize_utf8().map_err(PackageError::Io)?;
        let project_module_id = manifest.module_id;
        let module = ResolvedModule {
            manifest,
            root,
            origin: ResolvedModuleOrigin::Project,
            dependencies: BTreeMap::new(),
        };
        Ok(Self {
            project_module_id,
            modules: BTreeMap::from([(project_module_id, module)]),
        })
    }

    pub fn from_modules(
        project_module_id: Uuid,
        modules: BTreeMap<Uuid, ResolvedModule>,
    ) -> Result<Self, PackageError> {
        let graph = Self {
            project_module_id,
            modules,
        };
        graph.validate()?;
        Ok(graph)
    }

    /// Builds a graph whose local module roots are the validated destinations
    /// of an in-progress atomic filesystem transaction.
    ///
    /// All identity, ownership, manifest, dependency-edge, and reachability
    /// invariants are checked. Physical-root existence is deferred until the
    /// staged move commits.
    pub fn from_modules_with_staged_roots(
        project_module_id: Uuid,
        modules: BTreeMap<Uuid, ResolvedModule>,
    ) -> Result<Self, PackageError> {
        let graph = Self {
            project_module_id,
            modules,
        };
        graph.validate_inner(false)?;
        Ok(graph)
    }

    pub fn from_lock(
        root: &Utf8Path,
        manifest: PackageManifest,
        lockfile: &Lockfile,
        cache: &CacheStore,
    ) -> Result<Self, PackageError> {
        lockfile.validate_local(root, &manifest)?;
        let project_root = root.canonicalize_utf8().map_err(PackageError::Io)?;
        let path_modules = collect_path_modules(&project_root, &manifest)?;
        let mut package_module_ids = BTreeMap::new();
        let mut path_module_ids = BTreeMap::new();
        for (package, locked) in &lockfile.packages {
            package_module_ids.insert(package.clone(), locked.module_id);
        }
        for (path, locked) in &lockfile.path_dependencies {
            path_module_ids.insert(path.clone(), locked.module_id);
        }

        let mut modules = BTreeMap::new();
        let project_dependencies = resolved_manifest_dependencies(
            &manifest.dependencies,
            None,
            &package_module_ids,
            &path_module_ids,
        )?;
        insert_resolved_module(
            &mut modules,
            ResolvedModule {
                manifest: manifest.clone(),
                root: project_root.clone(),
                origin: ResolvedModuleOrigin::Project,
                dependencies: project_dependencies,
            },
        )?;

        for (path, record) in path_modules {
            let locked = lockfile.path_dependencies.get(&path).ok_or_else(|| {
                PackageError::Invalid(format!("path dependency `{path}` is not locked"))
            })?;
            let dependencies = resolved_manifest_dependencies(
                &record.manifest.dependencies,
                Some(&locked.dependencies),
                &package_module_ids,
                &path_module_ids,
            )?;
            insert_resolved_module(
                &mut modules,
                ResolvedModule {
                    manifest: record.manifest,
                    root: project_root.join(&path),
                    origin: ResolvedModuleOrigin::PathDependency {
                        declared_path: path,
                        content_sha256: locked.content_sha256.clone(),
                    },
                    dependencies,
                },
            )?;
        }

        for (package, locked) in &lockfile.packages {
            let package_root = cache
                .package_root(&locked.archive_sha256)?
                .canonicalize_utf8()
                .map_err(PackageError::Io)?;
            let package_manifest = PackageManifest::read(&package_root)?;
            let publication = package_manifest.publication.as_ref().ok_or_else(|| {
                PackageError::Invalid(format!(
                    "cached package `{package}@{}` has no publication identity",
                    locked.version
                ))
            })?;
            if publication.package != *package
                || publication.version != locked.version
                || package_manifest.module_id != locked.module_id
            {
                return Err(PackageError::Invalid(format!(
                    "cached package `{package}@{}` does not match dawn.lock",
                    locked.version
                )));
            }
            let declared_dependencies = package_manifest
                .dependencies
                .iter()
                .map(|(alias, dependency)| match dependency {
                    Dependency::Registry { package, version } => {
                        let selected = lockfile.packages.get(package).ok_or_else(|| {
                            PackageError::Invalid(format!(
                                "cached package `{}` points to unlocked dependency `{package}`",
                                publication.package
                            ))
                        })?;
                        if !version.matches(&selected.version) {
                            return Err(PackageError::Invalid(format!(
                                "locked `{package}@{}` does not satisfy `{version}` required by `{}`",
                                selected.version, publication.package
                            )));
                        }
                        Ok((alias.clone(), package.clone()))
                    }
                    Dependency::Path { .. } => Err(PackageError::Invalid(format!(
                        "cached registry package `{}` contains a path dependency",
                        publication.package
                    ))),
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            if declared_dependencies != locked.dependencies {
                return Err(PackageError::Invalid(format!(
                    "cached package `{package}@{}` dependency edges do not match dawn.lock",
                    locked.version
                )));
            }
            let dependencies = locked
                .dependencies
                .iter()
                .map(|(alias, package)| {
                    package_module_ids
                        .get(package)
                        .copied()
                        .map(|module_id| (alias.clone(), module_id))
                        .ok_or_else(|| {
                            PackageError::Invalid(format!(
                                "locked dependency `{package}` has no moduleId"
                            ))
                        })
                })
                .collect::<Result<BTreeMap<_, _>, _>>()?;
            insert_resolved_module(
                &mut modules,
                ResolvedModule {
                    manifest: package_manifest,
                    root: package_root,
                    origin: ResolvedModuleOrigin::RegistryDependency {
                        package: package.clone(),
                        version: locked.version.clone(),
                        archive_sha256: locked.archive_sha256.clone(),
                    },
                    dependencies,
                },
            )?;
        }
        Self::from_modules(manifest.module_id, modules)
    }

    pub fn project_module_id(&self) -> Uuid {
        self.project_module_id
    }

    pub fn project_module(&self) -> &ResolvedModule {
        &self.modules[&self.project_module_id]
    }

    pub fn modules(&self) -> &BTreeMap<Uuid, ResolvedModule> {
        &self.modules
    }

    pub fn module(&self, module_id: Uuid) -> Result<&ResolvedModule, PackageError> {
        self.modules.get(&module_id).ok_or_else(|| {
            PackageError::Invalid(format!("resolved source graph has no module `{module_id}`"))
        })
    }

    pub fn dependency(
        &self,
        module_id: Uuid,
        alias: &str,
    ) -> Result<&ResolvedModule, PackageError> {
        let module = self.module(module_id)?;
        let target = module.dependencies.get(alias).ok_or_else(|| {
            PackageError::Invalid(format!(
                "module `{module_id}` has no dependency alias `{alias}`"
            ))
        })?;
        self.module(*target)
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        self.validate_inner(true)
    }

    fn validate_inner(&self, require_physical_roots: bool) -> Result<(), PackageError> {
        let project = self.module(self.project_module_id)?;
        if project.origin != ResolvedModuleOrigin::Project {
            return Err(PackageError::Invalid(
                "project module must have project ownership".to_string(),
            ));
        }
        let project_count = self
            .modules
            .values()
            .filter(|module| module.origin == ResolvedModuleOrigin::Project)
            .count();
        if project_count != 1 {
            return Err(PackageError::Invalid(
                "resolved source graph must contain exactly one project module".to_string(),
            ));
        }
        for (module_id, module) in &self.modules {
            module.manifest.validate_runtime_compatibility()?;
            if module.manifest.module_id != *module_id {
                return Err(PackageError::Invalid(format!(
                    "resolved module key `{module_id}` does not match its manifest moduleId"
                )));
            }
            if !module.root.is_absolute() || (require_physical_roots && !module.root.is_dir()) {
                return Err(PackageError::Invalid(format!(
                    "resolved module `{module_id}` has an invalid physical root"
                )));
            }
            for (alias, target) in &module.dependencies {
                let declared = module.manifest.dependencies.get(alias).ok_or_else(|| {
                    PackageError::Invalid(format!(
                        "resolved edge `{alias}` is not declared by module `{module_id}`"
                    ))
                })?;
                let target_module = self.modules.get(target).ok_or_else(|| {
                    PackageError::Invalid(format!(
                        "resolved edge `{alias}` points to missing module `{target}`"
                    ))
                })?;
                if let Dependency::Registry { package, .. } = declared {
                    let ResolvedModuleOrigin::RegistryDependency {
                        package: resolved_package,
                        ..
                    } = &target_module.origin
                    else {
                        return Err(PackageError::Invalid(format!(
                            "registry dependency `{alias}` did not resolve to a registry module"
                        )));
                    };
                    if package != resolved_package {
                        return Err(PackageError::Invalid(format!(
                            "registry dependency `{alias}` resolved to the wrong package"
                        )));
                    }
                }
            }
            if module.dependencies.len() != module.manifest.dependencies.len() {
                return Err(PackageError::Invalid(format!(
                    "module `{module_id}` has unresolved dependency edges"
                )));
            }
        }
        let mut visiting = BTreeSet::new();
        let mut visited = BTreeSet::new();
        self.visit_module(self.project_module_id, &mut visiting, &mut visited)?;
        if visited.len() != self.modules.len() {
            return Err(PackageError::Invalid(
                "resolved source graph contains unreachable modules".to_string(),
            ));
        }
        Ok(())
    }

    fn visit_module(
        &self,
        module_id: Uuid,
        visiting: &mut BTreeSet<Uuid>,
        visited: &mut BTreeSet<Uuid>,
    ) -> Result<(), PackageError> {
        if visited.contains(&module_id) {
            return Ok(());
        }
        if !visiting.insert(module_id) {
            return Err(PackageError::Invalid(format!(
                "package dependency cycle includes module `{module_id}`"
            )));
        }
        for target in self.module(module_id)?.dependencies.values() {
            self.visit_module(*target, visiting, visited)?;
        }
        visiting.remove(&module_id);
        visited.insert(module_id);
        Ok(())
    }
}

fn insert_resolved_module(
    modules: &mut BTreeMap<Uuid, ResolvedModule>,
    module: ResolvedModule,
) -> Result<(), PackageError> {
    let module_id = module.manifest.module_id;
    if modules.insert(module_id, module).is_some() {
        return Err(PackageError::Invalid(format!(
            "resolved package graph contains duplicate moduleId `{module_id}`"
        )));
    }
    Ok(())
}

fn resolved_manifest_dependencies(
    dependencies: &BTreeMap<String, Dependency>,
    locked_path_dependencies: Option<&BTreeMap<String, LockedDependency>>,
    package_module_ids: &BTreeMap<PackageId, Uuid>,
    path_module_ids: &BTreeMap<String, Uuid>,
) -> Result<BTreeMap<String, Uuid>, PackageError> {
    dependencies
        .iter()
        .map(|(alias, dependency)| {
            let module_id = match dependency {
                Dependency::Registry { package, .. } => {
                    *package_module_ids.get(package).ok_or_else(|| {
                        PackageError::Invalid(format!(
                            "registry dependency `{package}` has no locked moduleId"
                        ))
                    })?
                }
                Dependency::Path { path } => {
                    let path = match locked_path_dependencies {
                        Some(edges) => match edges.get(alias) {
                            Some(LockedDependency::Path { path }) => path,
                            _ => {
                                return Err(PackageError::Invalid(format!(
                                    "path dependency `{alias}` has no locked path edge"
                                )));
                            }
                        },
                        None => path,
                    };
                    *path_module_ids.get(path).ok_or_else(|| {
                        PackageError::Invalid(format!(
                            "path dependency `{path}` has no locked moduleId"
                        ))
                    })?
                }
            };
            Ok((alias.clone(), module_id))
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReceipt {
    pub receipt_version: u8,
    pub module_id: Uuid,
    pub package: PackageId,
    pub version: Version,
    pub manifest_sha256: String,
    pub files: BTreeMap<String, FileReceipt>,
    pub exports: BTreeMap<String, ReleaseExportGroup>,
    pub dependencies: BTreeMap<String, Dependency>,
    pub language_version: String,
    pub requires_dawn: VersionReq,
    pub expanded_size: u64,
    pub file_count: usize,
    pub has_audio: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct FileReceipt {
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseExportGroup {
    pub documents: Vec<String>,
    pub objects: Vec<ReleaseExportObject>,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReleaseExportObject {
    pub document: String,
    pub name: String,
    pub kind: ExportObjectKind,
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportObjectKind {
    Project,
    Setup,
    Controller,
    ElementTree,
    PreviewLayout,
    Patch,
    PropDefinition,
    FixtureProfile,
    Curve,
    Gradient,
    Sequence,
    EffectDefinition,
    OperatorDefinition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PackedRelease {
    pub archive: Vec<u8>,
    pub archive_sha256: String,
    pub receipt: ReleaseReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseArchivePlan {
    pub files: BTreeSet<String>,
    pub exports: BTreeMap<String, ReleaseExportGroup>,
}

#[cfg(test)]
fn pack_directory(root: &Utf8Path) -> Result<PackedRelease, PackageError> {
    let mut paths = Vec::new();
    collect_files(root, root, &mut paths)?;
    paths.retain(|path| {
        path != LOCK_FILE
            && path != "dawn-release.json"
            && !path.starts_with("target/")
            && !path.starts_with(".git/")
    });
    let manifest = PackageManifest::read(root)?;
    let exports = manifest
        .exports
        .iter()
        .map(|(name, group)| {
            (
                name.clone(),
                ReleaseExportGroup {
                    documents: group.documents.clone(),
                    objects: Vec::new(),
                },
            )
        })
        .collect();
    pack_directory_with_plan(
        root,
        &ReleaseArchivePlan {
            files: paths.into_iter().collect(),
            exports,
        },
    )
}

pub fn pack_directory_with_plan(
    root: &Utf8Path,
    plan: &ReleaseArchivePlan,
) -> Result<PackedRelease, PackageError> {
    let manifest = PackageManifest::read(root)?;
    let publication = manifest.publication.as_ref().ok_or_else(|| {
        PackageError::Invalid("dawn pack requires publication metadata".to_string())
    })?;
    if manifest
        .dependencies
        .values()
        .any(|dependency| matches!(dependency, Dependency::Path { .. }))
    {
        return Err(PackageError::Invalid(
            "published packages cannot contain path dependencies".to_string(),
        ));
    }
    if !plan.files.contains(MANIFEST_FILE) {
        return Err(PackageError::Invalid(
            "release closure must contain dawn-package.json".to_string(),
        ));
    }
    if plan.files.contains(LOCK_FILE) || plan.files.contains("dawn-release.json") {
        return Err(PackageError::Invalid(
            "release closure cannot contain generated lock or receipt files".to_string(),
        ));
    }
    if plan.exports.len() != manifest.exports.len() {
        return Err(PackageError::Invalid(
            "typed export index does not match manifest exports".to_string(),
        ));
    }
    for (group_name, manifest_group) in &manifest.exports {
        let export = plan.exports.get(group_name).ok_or_else(|| {
            PackageError::Invalid(format!(
                "typed export index is missing group `{group_name}`"
            ))
        })?;
        if export.documents != manifest_group.documents {
            return Err(PackageError::Invalid(format!(
                "typed export index documents differ for group `{group_name}`"
            )));
        }
        let mut object_names = BTreeSet::new();
        for object in &export.objects {
            if !export.documents.contains(&object.document)
                || !valid_object_name(&object.name)
                || !object_names.insert(object.name.as_str())
            {
                return Err(PackageError::Invalid(format!(
                    "typed export index contains an invalid or duplicate object in group `{group_name}`"
                )));
            }
        }
    }
    if let Some(project) = &manifest.project
        && !plan.files.contains(&project.entrypoint)
    {
        return Err(PackageError::Invalid(
            "release closure is missing project.entrypoint".to_string(),
        ));
    }
    for export in manifest.exports.values() {
        for document in &export.documents {
            if !plan.files.contains(document) {
                return Err(PackageError::Invalid(format!(
                    "release closure is missing export document `{document}`"
                )));
            }
        }
    }
    for asset in manifest.assets.keys() {
        if !plan.files.contains(asset) {
            return Err(PackageError::Invalid(format!(
                "release closure is missing declared asset `{asset}`"
            )));
        }
    }
    let paths = plan.files.iter().cloned().collect::<Vec<_>>();
    if paths.len() + 1 > MAX_FILES {
        return Err(PackageError::Invalid(format!(
            "package contains more than {MAX_FILES} files"
        )));
    }
    let mut files = BTreeMap::new();
    let mut folded_paths = BTreeSet::new();
    let mut expanded_size = 0u64;
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    let options = FileOptions::<()>::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    for path in paths {
        validate_relative_path(&path, "release path")?;
        if !folded_paths.insert(path.to_ascii_lowercase()) {
            return Err(PackageError::Invalid(format!(
                "release closure contains case-colliding path `{path}`"
            )));
        }
        let bytes = read_regular_package_file(root, &path)?;
        let size = bytes.len() as u64;
        if !is_supported_package_path(&path) {
            return Err(PackageError::Invalid(format!(
                "unsupported package file type: `{path}`"
            )));
        }
        expanded_size = expanded_size.saturating_add(size);
        if expanded_size > MAX_EXPANDED_BYTES {
            return Err(PackageError::Invalid(format!(
                "expanded package exceeds {MAX_EXPANDED_BYTES} bytes"
            )));
        }
        if size > MAX_TEXT_BYTES && is_text_path(&path) {
            return Err(PackageError::Invalid(format!(
                "text file `{path}` exceeds 1 MB"
            )));
        }
        if is_preview_path(&path) && size > MAX_PREVIEW_BYTES {
            return Err(PackageError::Invalid(format!(
                "preview `{path}` exceeds 5 MB"
            )));
        }
        if path != MANIFEST_FILE && is_audio_path(&path) && !manifest.assets.contains_key(&path) {
            return Err(PackageError::Invalid(format!(
                "audio asset `{path}` is not declared"
            )));
        }
        files.insert(
            path.clone(),
            FileReceipt {
                sha256: sha256_hex(&bytes),
                size,
            },
        );
        writer.start_file(path, options)?;
        writer.write_all(&bytes)?;
    }
    let file_count = files.len();
    let receipt = ReleaseReceipt {
        receipt_version: RELEASE_RECEIPT_VERSION,
        module_id: manifest.module_id,
        package: publication.package.clone(),
        version: publication.version.clone(),
        manifest_sha256: manifest_hash(&manifest)?,
        files,
        exports: plan.exports.clone(),
        dependencies: manifest.dependencies.clone(),
        language_version: manifest.language_version,
        requires_dawn: manifest.requires_dawn,
        expanded_size,
        file_count,
        has_audio: !manifest.assets.is_empty(),
    };
    let receipt_bytes = canonical_json(&receipt)?;
    if receipt_bytes.len() as u64 > MAX_TEXT_BYTES {
        return Err(PackageError::Invalid(
            "dawn-release.json exceeds 1 MB".to_string(),
        ));
    }
    if expanded_size.saturating_add(receipt_bytes.len() as u64) > MAX_EXPANDED_BYTES {
        return Err(PackageError::Invalid(format!(
            "expanded package exceeds {MAX_EXPANDED_BYTES} bytes"
        )));
    }
    writer.start_file("dawn-release.json", options)?;
    writer.write_all(&receipt_bytes)?;
    let archive = writer.finish()?.into_inner();
    if archive.len() > MAX_ARCHIVE_BYTES {
        return Err(PackageError::Invalid(format!(
            "archive exceeds {MAX_ARCHIVE_BYTES} bytes"
        )));
    }
    Ok(PackedRelease {
        archive_sha256: sha256_hex(&archive),
        archive,
        receipt,
    })
}

fn read_regular_package_file(root: &Utf8Path, path: &str) -> Result<Vec<u8>, PackageError> {
    let components = path.split('/').collect::<Vec<_>>();
    let mut current = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        current.push(component);
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() {
            return Err(PackageError::Invalid(format!(
                "symbolic links are not allowed in packages: `{path}`"
            )));
        }
        let is_last = index + 1 == components.len();
        if (is_last && !metadata.is_file()) || (!is_last && !metadata.is_dir()) {
            return Err(PackageError::Invalid(format!(
                "release path is not a regular file: `{path}`"
            )));
        }
    }
    fs::read(current).map_err(Into::into)
}

pub(crate) fn collect_files(
    root: &Utf8Path,
    current: &Utf8Path,
    result: &mut Vec<String>,
) -> Result<(), PackageError> {
    for entry in fs::read_dir(current)? {
        let entry = entry?;
        let path = Utf8PathBuf::from_path_buf(entry.path())
            .map_err(|_| PackageError::Invalid("package path is not UTF-8".to_string()))?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| PackageError::Invalid("package path escaped root".to_string()))?;
        let relative = relative.as_str().replace('\\', "/");
        if entry.file_type()?.is_symlink() {
            return Err(PackageError::Archive(format!(
                "symbolic links are not allowed: {relative}"
            )));
        }
        if entry.file_type()?.is_dir() {
            collect_files(root, &path, result)?;
        } else if entry.file_type()?.is_file() {
            result.push(relative);
        } else {
            return Err(PackageError::Archive(format!(
                "unsupported file type: {relative}"
            )));
        }
    }
    Ok(())
}

fn directory_content_hash(
    root: &Utf8Path,
    manifest: &PackageManifest,
) -> Result<String, PackageError> {
    let mut paths = Vec::new();
    collect_files(root, root, &mut paths)?;
    let nested_modules = manifest
        .dependencies
        .values()
        .filter_map(|dependency| match dependency {
            Dependency::Path { path } => Some(format!("{}/", path.trim_end_matches('/'))),
            Dependency::Registry { .. } => None,
        })
        .collect::<Vec<_>>();
    paths.retain(|path| {
        path != LOCK_FILE
            && path != "dawn-release.json"
            && path != "target"
            && !path.starts_with("target/")
            && path != ".git"
            && !path.starts_with(".git/")
            && !nested_modules.iter().any(|nested| path.starts_with(nested))
    });
    paths.sort();
    let mut hasher = Sha256::new();
    for path in paths {
        let bytes = fs::read(root.join(&path))?;
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update([0]);
        hasher.update(bytes);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

fn is_text_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".dawn")
        || path.ends_with(".json")
        || path.ends_with(".md")
        || path.ends_with(".txt")
        || path == "license"
}

fn is_preview_path(path: &str) -> bool {
    let path = path.to_ascii_lowercase();
    path.ends_with(".png")
        || path.ends_with(".jpg")
        || path.ends_with(".jpeg")
        || path.ends_with(".webp")
}

fn is_audio_path(path: &str) -> bool {
    matches!(
        Utf8Path::new(path)
            .extension()
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "mp3" | "wav" | "ogg" | "flac"
    )
}

fn is_supported_package_path(path: &str) -> bool {
    is_text_path(path) || is_preview_path(path) || is_audio_path(path)
}

pub fn inspect_archive(bytes: &[u8]) -> Result<ReleaseReceipt, PackageError> {
    if bytes.len() > MAX_ARCHIVE_BYTES {
        return Err(PackageError::Archive(format!(
            "archive exceeds {MAX_ARCHIVE_BYTES} bytes"
        )));
    }
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let mut paths = BTreeSet::new();
    let mut folded_paths = BTreeSet::new();
    let mut actual_files = BTreeMap::new();
    let mut expanded = 0u64;
    let mut manifest = None;
    let mut receipt = None;
    let mut file_count = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        let name = entry.name().to_string();
        validate_archive_path(&name)?;
        if !paths.insert(name.clone()) {
            return Err(PackageError::Archive(format!(
                "duplicate archive path: {name}"
            )));
        }
        if !folded_paths.insert(name.to_ascii_lowercase()) {
            return Err(PackageError::Archive(format!(
                "case-colliding archive path: {name}"
            )));
        }
        if entry.is_dir() {
            return Err(PackageError::Archive(format!(
                "directory archive entries are not allowed: {name}"
            )));
        }
        if entry.encrypted() {
            return Err(PackageError::Archive(format!(
                "encrypted archive entries are not allowed: {name}"
            )));
        }
        if let Some(mode) = entry.unix_mode() {
            let file_type = mode & 0o170000;
            if file_type == 0o120000 {
                return Err(PackageError::Archive(format!(
                    "symbolic links are not allowed: {name}"
                )));
            }
            if file_type != 0 && file_type != 0o100000 {
                return Err(PackageError::Archive(format!(
                    "unsupported archive entry type: {name}"
                )));
            }
        }
        file_count += 1;
        if file_count > MAX_FILES {
            return Err(PackageError::Archive(format!(
                "archive contains more than {MAX_FILES} files"
            )));
        }
        if !is_supported_package_path(&name) {
            return Err(PackageError::Archive(format!(
                "unsupported package file type: {name}"
            )));
        }
        let declared_size = entry.size();
        if is_text_path(&name) && declared_size > MAX_TEXT_BYTES {
            return Err(PackageError::Archive(format!(
                "text file `{name}` exceeds 1 MB"
            )));
        }
        if is_preview_path(&name) && declared_size > MAX_PREVIEW_BYTES {
            return Err(PackageError::Archive(format!(
                "preview `{name}` exceeds 5 MB"
            )));
        }
        let remaining = MAX_EXPANDED_BYTES.saturating_sub(expanded);
        let path_limit = if name == MANIFEST_FILE {
            MAX_MANIFEST_BYTES
        } else if is_text_path(&name) {
            MAX_TEXT_BYTES
        } else if is_preview_path(&name) {
            MAX_PREVIEW_BYTES
        } else {
            remaining
        };
        let read_limit = remaining.min(path_limit);
        let mut content = Vec::new();
        entry
            .by_ref()
            .take(read_limit.saturating_add(1))
            .read_to_end(&mut content)?;
        if content.len() as u64 > read_limit {
            return Err(PackageError::Archive(format!(
                "archive entry expands beyond its limit: {name}"
            )));
        }
        let size = content.len() as u64;
        if size != declared_size {
            return Err(PackageError::Archive(format!(
                "archive size metadata mismatch: {name}"
            )));
        }
        expanded = expanded.saturating_add(size);
        actual_files.insert(
            name.clone(),
            FileReceipt {
                sha256: sha256_hex(&content),
                size,
            },
        );
        if name == MANIFEST_FILE {
            if size > MAX_MANIFEST_BYTES {
                return Err(PackageError::Archive("manifest exceeds 64 KB".to_string()));
            }
            manifest = Some(content);
        } else if name == "dawn-release.json" {
            receipt = Some(content);
        }
    }
    let manifest_bytes = manifest
        .ok_or_else(|| PackageError::Archive("root dawn-package.json is required".to_string()))?;
    let manifest: PackageManifest = serde_json::from_slice(&manifest_bytes)?;
    manifest
        .validate_contract()
        .map_err(|error| PackageError::Archive(format!("invalid manifest: {error}")))?;
    let publication = manifest.publication.as_ref().ok_or_else(|| {
        PackageError::Archive("release manifest requires publication metadata".to_string())
    })?;
    if manifest
        .dependencies
        .values()
        .any(|dependency| matches!(dependency, Dependency::Path { .. }))
    {
        return Err(PackageError::Archive(
            "release manifest cannot contain path dependencies".to_string(),
        ));
    }
    let receipt_bytes = receipt
        .ok_or_else(|| PackageError::Archive("dawn-release.json is required".to_string()))?;
    let receipt: ReleaseReceipt = serde_json::from_slice(&receipt_bytes)?;
    if receipt.manifest_sha256 != manifest_hash(&manifest)? {
        return Err(PackageError::Archive(
            "release receipt manifest hash mismatch".to_string(),
        ));
    }
    if receipt.receipt_version != RELEASE_RECEIPT_VERSION {
        return Err(PackageError::Archive(
            "unsupported release receipt version".to_string(),
        ));
    }
    if receipt.module_id != manifest.module_id
        || receipt.package != publication.package
        || receipt.version != publication.version
        || receipt.language_version != manifest.language_version
        || receipt.requires_dawn != manifest.requires_dawn
        || receipt.dependencies != manifest.dependencies
    {
        return Err(PackageError::Archive(
            "release receipt does not match manifest identity or dependencies".to_string(),
        ));
    }
    for (path, file) in &receipt.files {
        if actual_files.get(path) != Some(file) {
            return Err(PackageError::Archive(format!(
                "release receipt hash mismatch: {path}"
            )));
        }
    }
    if receipt.files.len() + 1 != actual_files.len() {
        return Err(PackageError::Archive(
            "release receipt has an incomplete file index".to_string(),
        ));
    }
    if receipt.file_count != receipt.files.len() {
        return Err(PackageError::Archive(
            "release receipt file count mismatch".to_string(),
        ));
    }
    let indexed_size = receipt
        .files
        .values()
        .try_fold(0u64, |total, file| total.checked_add(file.size))
        .ok_or_else(|| PackageError::Archive("release size overflow".to_string()))?;
    if receipt.expanded_size != indexed_size {
        return Err(PackageError::Archive(
            "release receipt expanded size mismatch".to_string(),
        ));
    }
    if receipt.has_audio == manifest.assets.is_empty() {
        return Err(PackageError::Archive(
            "release receipt audio flag mismatch".to_string(),
        ));
    }
    if receipt.exports.len() != manifest.exports.len() {
        return Err(PackageError::Archive(
            "release receipt export index mismatch".to_string(),
        ));
    }
    for (group_name, manifest_group) in &manifest.exports {
        let receipt_group = receipt.exports.get(group_name).ok_or_else(|| {
            PackageError::Archive(format!(
                "release receipt is missing export group `{group_name}`"
            ))
        })?;
        if receipt_group.documents != manifest_group.documents {
            return Err(PackageError::Archive(format!(
                "release receipt documents differ for export group `{group_name}`"
            )));
        }
        let mut object_names = BTreeSet::new();
        for object in &receipt_group.objects {
            if !valid_object_name(&object.name)
                || !receipt_group.documents.contains(&object.document)
                || !object_names.insert(object.name.as_str())
            {
                return Err(PackageError::Archive(format!(
                    "invalid or duplicate object index in export group `{group_name}`"
                )));
            }
        }
    }
    let archive_paths = actual_files.keys().cloned().collect::<BTreeSet<_>>();
    if let Some(project) = &manifest.project {
        require_archived_path(&archive_paths, &project.entrypoint, "project.entrypoint")?;
    }
    for export in manifest.exports.values() {
        for document in &export.documents {
            require_archived_path(&archive_paths, document, "export document")?;
        }
    }
    for path in manifest.assets.keys() {
        require_archived_path(&archive_paths, path, "asset")?;
    }
    for path in actual_files.keys() {
        if is_audio_path(path) && path != MANIFEST_FILE && !manifest.assets.contains_key(path) {
            return Err(PackageError::Archive(format!(
                "audio asset `{path}` is not declared"
            )));
        }
    }
    Ok(receipt)
}

fn require_archived_path(
    archive_paths: &BTreeSet<String>,
    path: &str,
    label: &str,
) -> Result<(), PackageError> {
    if !archive_paths.contains(path) {
        return Err(PackageError::Archive(format!(
            "{label} `{path}` is missing from the archive"
        )));
    }
    Ok(())
}

pub(crate) fn validate_archive_path(path: &str) -> Result<(), PackageError> {
    validate_relative_path(path, "archive path")
}

pub fn dependency_graph(manifest: &PackageManifest) -> BTreeMap<String, String> {
    manifest
        .dependencies
        .iter()
        .map(|(alias, dependency)| {
            let value = match dependency {
                Dependency::Registry { package, version } => {
                    format!("{} {version}", package.as_str())
                }
                Dependency::Path { path } => format!("path:{path}"),
            };
            (alias.clone(), value)
        })
        .collect()
}

/// Replace one file using a fully written, synced temporary file beside it.
/// This does not make a batch of replacements atomic.
pub fn atomic_write(path: &Utf8Path, bytes: &[u8]) -> Result<(), PackageError> {
    let parent = path.parent().ok_or_else(|| {
        PackageError::Invalid(format!(
            "cannot atomically write path without a parent: `{path}`"
        ))
    })?;
    fs::create_dir_all(parent)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn manifest(root: &Utf8Path) -> PackageManifest {
        let entrypoint = root.join("main.dawn");
        fs::write(&entrypoint, "project: {}").expect("fixture");
        PackageManifest {
            manifest_version: MANIFEST_VERSION,
            module_id: Uuid::new_v4(),
            language_version: "0.1".to_string(),
            requires_dawn: VersionReq::parse(">=0.1.0, <1.0.0").expect("version"),
            project: Some(ProjectManifest {
                entrypoint: "main.dawn".to_string(),
            }),
            publication: Some(Publication {
                package: PackageId::new("test/example").expect("package"),
                version: Version::parse("1.0.0").expect("version"),
                display_name: "Example".to_string(),
                summary: "Package contract fixture".to_string(),
                license: "MIT".to_string(),
                tags: vec!["test".to_string()],
            }),
            exports: BTreeMap::from([(
                String::from("main"),
                ExportGroup {
                    documents: vec!["main.dawn".to_string()],
                },
            )]),
            dependencies: BTreeMap::new(),
            assets: BTreeMap::new(),
        }
    }

    #[test]
    fn manifest_and_release_are_deterministic() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let value = manifest(root);
        value.write(root).expect("manifest");
        let first = pack_directory(root).expect("pack");
        let second = pack_directory(root).expect("pack");
        assert_eq!(first.archive, second.archive);
        assert_eq!(
            inspect_archive(&first.archive).expect("inspect"),
            first.receipt
        );
    }

    #[test]
    fn lock_detects_manifest_changes() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let value = manifest(root);
        value.write(root).expect("manifest");
        let lock = Lockfile::new(&value, "https://registry.dawn.dev").expect("lock");
        assert!(lock.validate_manifest(&value).is_ok());
    }

    #[test]
    fn resolver_selects_one_release_and_reports_diamonds() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let mut value = manifest(root);
        let package = PackageId::new("alice/library").expect("package");
        value.dependencies.insert(
            "library".to_string(),
            Dependency::Registry {
                package: package.clone(),
                version: VersionReq::parse("^1.0").expect("requirement"),
            },
        );
        let release = RegistryRelease {
            package: package.clone(),
            version: Version::parse("1.2.0").expect("version"),
            archive_sha256: "a".repeat(64),
            module_id: Uuid::new_v4(),
            dependencies: BTreeMap::new(),
            yanked: false,
            runtime_compatible: true,
        };
        let available = BTreeMap::from([(package.clone(), vec![release])]);
        let lock = resolve_registry(root, &value, "https://registry.dawn.dev", &available)
            .expect("resolve");
        assert_eq!(
            lock.packages.get(&package).expect("locked").version,
            Version::parse("1.2.0").expect("version")
        );

        value.dependencies.insert(
            "conflict".to_string(),
            Dependency::Registry {
                package: package.clone(),
                version: VersionReq::parse("^2.0").expect("requirement"),
            },
        );
        assert!(
            resolve_registry(root, &value, "https://registry.dawn.dev", &available)
                .expect_err("conflict")
                .to_string()
                .contains("dependency resolution failed")
        );
    }

    #[test]
    fn cache_rejects_corrupt_content() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let value = manifest(root);
        value.write(root).expect("manifest");
        let release = pack_directory(root).expect("pack");
        let cache = CacheStore::new(root.join("cache"));
        let path = cache
            .install(&release.archive_sha256, &release.archive, |_| Ok(()))
            .expect("install");
        fs::write(path, b"corrupt").expect("corrupt");
        assert!(cache.read(&release.archive_sha256).is_err());
    }

    #[test]
    fn cache_does_not_install_content_rejected_by_validation() {
        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let value = manifest(root);
        value.write(root).expect("manifest");
        let release = pack_directory(root).expect("pack");
        let cache = CacheStore::new(root.join("cache"));

        let error = cache
            .install(&release.archive_sha256, &release.archive, |_| {
                Err(PackageError::Invalid(
                    "compiler rejected artifact".to_string(),
                ))
            })
            .expect_err("validation must reject the artifact");

        assert!(error.to_string().contains("compiler rejected artifact"));
        assert_eq!(
            cache.status(&release.archive_sha256).expect("cache status"),
            CacheStatus::Missing
        );
    }

    #[test]
    fn concurrent_cache_installs_share_one_verified_entry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let directory = tempdir().expect("tempdir");
        let root = Utf8Path::from_path(directory.path()).expect("utf8");
        let value = manifest(root);
        value.write(root).expect("manifest");
        let release = pack_directory(root).expect("pack");
        let cache = CacheStore::new(root.join("cache"));
        let barrier = Arc::new(Barrier::new(2));
        let validations = Arc::new(AtomicUsize::new(0));
        let archive = Arc::new(release.archive);
        let hash = release.archive_sha256;

        let threads = (0..2)
            .map(|_| {
                let cache = cache.clone();
                let barrier = Arc::clone(&barrier);
                let validations = Arc::clone(&validations);
                let archive = Arc::clone(&archive);
                let hash = hash.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    cache.install(&hash, &archive, |package_root| {
                        assert!(package_root.join(MANIFEST_FILE).is_file());
                        validations.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })
            })
            .collect::<Vec<_>>();
        let installed = threads
            .into_iter()
            .map(|thread| thread.join().expect("install thread").expect("install"))
            .collect::<Vec<_>>();

        assert_eq!(installed[0], installed[1]);
        assert_eq!(validations.load(Ordering::SeqCst), 2);
        assert_eq!(
            cache.status(&hash).expect("cache status"),
            CacheStatus::Ready
        );
    }

    #[test]
    fn shared_protocol_fixtures_match_the_rust_contract() {
        let manifest: PackageManifest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/manifest-v2.json"
        )))
        .expect("manifest fixture");
        manifest.validate_contract().expect("manifest contract");
        assert_eq!(
            manifest_hash(&manifest).expect("manifest hash"),
            "ffd01b9d77fb5eae64f19b24e101da281bda908220b5d168833dbe73ccb7a770"
        );

        let lock: Lockfile = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/lock-v1.json"
        )))
        .expect("lock fixture");
        lock.validate_manifest(&manifest).expect("lock contract");

        let receipt: ReleaseReceipt = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/release-v1.json"
        )))
        .expect("release fixture");
        assert_eq!(receipt.module_id, manifest.module_id);
        assert_eq!(receipt.dependencies, manifest.dependencies);
        assert_eq!(receipt.exports.len(), manifest.exports.len());

        let discovery: RegistryDiscovery = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/discovery-v1.json"
        )))
        .expect("discovery fixture");
        assert_eq!(discovery.registry_version, 1);

        let resolution: RegistryResolveResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/resolve-v1.json"
        )))
        .expect("resolution fixture");
        assert_eq!(
            resolution.package,
            manifest.publication.expect("publication").package
        );

        let download: RegistryDownloadResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/download-v1.json"
        )))
        .expect("download fixture");
        assert_eq!(download.status, RegistryReleaseStatus::Yanked);

        let device_start_request: DeviceLoginRequest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/device-start-request-v1.json"
        )))
        .expect("device start request fixture");
        assert!(matches!(
            device_start_request,
            DeviceLoginRequest::Start {
                registry_version: REGISTRY_PROTOCOL_VERSION,
                ..
            }
        ));
        let _: DeviceStartResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/device-start-response-v1.json"
        )))
        .expect("device start response fixture");
        let _: DeviceTokenResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/device-token-response-v1.json"
        )))
        .expect("device token response fixture");
        let whoami_request: DeviceLoginRequest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/device-whoami-request-v1.json"
        )))
        .expect("device whoami request fixture");
        assert!(matches!(
            whoami_request,
            DeviceLoginRequest::WhoAmI {
                registry_version: REGISTRY_PROTOCOL_VERSION
            }
        ));
        let _: DeviceIdentityResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/device-whoami-response-v1.json"
        )))
        .expect("device whoami response fixture");
        let _: PublishStageRequest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/publish-stage-request-v1.json"
        )))
        .expect("publish stage request fixture");
        let _: PublishStageResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/publish-stage-response-v1.json"
        )))
        .expect("publish stage response fixture");
        let _: PublishFinalizeRequest = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/publish-finalize-request-v1.json"
        )))
        .expect("publish finalize request fixture");
        let _: PublishFinalizeResponse = serde_json::from_str(include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/publish-finalize-response-v1.json"
        )))
        .expect("publish finalize response fixture");
    }

    #[test]
    fn manifest_requires_all_v2_top_level_fields() {
        let fixture = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../protocol/registry-v1/fixtures/manifest-v2.json"
        ));
        let mut value: serde_json::Value = serde_json::from_str(fixture).expect("fixture");
        value.as_object_mut().expect("object").remove("publication");
        assert!(serde_json::from_value::<PackageManifest>(value).is_err());
    }
}
