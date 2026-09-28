use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
use std::{fs, io, io::Write};
use uuid::Uuid;

pub const PROJECT_ROOT_FILE: &str = "project.donder";
pub const PROJECT_FORMAT_VERSION: u8 = 1;

/// Workspace identity stored in the root document's `workspace` block.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProjectMetadata {
    pub format_version: u8,
    pub project_id: Uuid,
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            project_id: Uuid::new_v4(),
        }
    }
}

impl ProjectMetadata {
    pub fn read(root: &Utf8Path) -> Result<Self, crate::LoadProjectError> {
        let path = root.join(PROJECT_ROOT_FILE);
        let text = fs::read_to_string(&path).map_err(|source| crate::LoadProjectError::Io {
            path: PROJECT_ROOT_FILE.into(),
            source,
        })?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, crate::LoadProjectError> {
        let path = Utf8Path::new(PROJECT_ROOT_FILE);
        let document = crate::diagnostics::parse_yaml_value(path, text)?;
        Self::from_document(&document)
    }

    pub(crate) fn from_document(
        document: &yaml_serde::Value,
    ) -> Result<Self, crate::LoadProjectError> {
        let path = Utf8Path::new(PROJECT_ROOT_FILE);
        let invalid =
            |value: &yaml_serde::Value, message: String| crate::LoadProjectError::InvalidDocument {
                path: path.into(),
                range: crate::diagnostics::source_range_for_value(path, value),
                message,
            };
        let value = document
            .as_mapping()
            .and_then(|map| map.get("workspace"))
            .ok_or_else(|| {
                invalid(
                    document,
                    "project.donder must contain a workspace metadata block".into(),
                )
            })?;
        let metadata =
            crate::loader::mapping::parse_mapping(path, value, "workspace metadata", |fields| {
                let format_version = u8::try_from(fields.u32("format_version")?)
                    .map_err(|error| invalid(value, error.to_string()))?;
                let project_id = Uuid::parse_str(fields.string("project_id")?)
                    .map_err(|error| invalid(value, format!("Invalid project_id: {error}")))?;
                Ok(Self {
                    format_version,
                    project_id,
                })
            })?;
        metadata
            .validate()
            .map_err(|message| invalid(value, message))?;
        Ok(metadata)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.format_version != PROJECT_FORMAT_VERSION {
            return Err(format!(
                "Unsupported project format {}; expected {}",
                self.format_version, PROJECT_FORMAT_VERSION
            ));
        }
        if self.project_id.is_nil() {
            return Err("Project ID must not be nil".into());
        }
        Ok(())
    }

    /// Initialize a root document without replacing existing workspace metadata.
    pub fn initialize_document(&self, source: &str) -> Result<String, String> {
        self.validate()?;
        let value = crate::diagnostics::parse_yaml_value(Utf8Path::new(PROJECT_ROOT_FILE), source)
            .map_err(|error| error.to_string())?;
        let document = value
            .as_mapping()
            .ok_or("document root must be a mapping")?;
        if document.contains_key("workspace") {
            return Err("project.donder already contains a workspace block".into());
        }
        let mut root = yaml_serde::Mapping::new();
        root.insert(
            yaml_serde::Value::String("workspace".into()),
            yaml_serde::to_value(self).map_err(|error| error.to_string())?,
        );
        root.extend(document.clone());
        yaml_serde::to_string(&root).map_err(|error| error.to_string())
    }
}

/// The one editable source root of a local project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectWorkspace {
    pub root: Utf8PathBuf,
    pub metadata: ProjectMetadata,
}

impl ProjectWorkspace {
    pub fn new(root: &Utf8Path, metadata: ProjectMetadata) -> Result<Self, String> {
        metadata.validate()?;
        let root = root
            .canonicalize_utf8()
            .map_err(|error| error.to_string())?;
        if !root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        Ok(Self { root, metadata })
    }
}

/// Validate a portable project-root-relative file path before resolving it on disk.
pub fn validate_relative_path(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.contains('\\')
        || value.contains(':')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || value.split('/').any(|part| {
            part.is_empty()
                || part == "."
                || part == ".."
                || part.ends_with('.')
                || part.ends_with(' ')
                || is_reserved_component(part)
        })
    {
        return Err(format!("`{value}` must be a safe project-relative path"));
    }
    Ok(())
}

pub fn validate_document_path(value: &str) -> Result<(), String> {
    validate_relative_path(value)?;
    if !value.ends_with(".donder") {
        return Err(format!("`{value}` must be a Donder document"));
    }
    Ok(())
}

fn is_reserved_component(component: &str) -> bool {
    let basename = component
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    matches!(basename.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|prefix| {
            basename.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            })
        })
}

/// Replace one file using a fully written, synced temporary file beside it.
/// This does not make a batch of replacements atomic.
pub fn atomic_write(path: &Utf8Path, bytes: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other(format!("Path has no parent: {path}")))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}
