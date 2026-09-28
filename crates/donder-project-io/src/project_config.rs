use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::{fs, io, io::Write};
use uuid::Uuid;

pub const PROJECT_CONFIG_FILE: &str = "donder.json";
pub const PROJECT_FORMAT_VERSION: u8 = 1;

/// Local project identity and entrypoint. Content is described by the documents themselves.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectConfig {
    pub format_version: u8,
    pub project_id: Uuid,
    pub entrypoint: Utf8PathBuf,
}

impl ProjectConfig {
    pub fn new(entrypoint: Utf8PathBuf) -> Self {
        Self {
            format_version: PROJECT_FORMAT_VERSION,
            project_id: Uuid::new_v4(),
            entrypoint,
        }
    }

    pub fn read(root: &Utf8Path) -> Result<Self, String> {
        let text = fs::read_to_string(root.join(PROJECT_CONFIG_FILE))
            .map_err(|error| error.to_string())?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let config: Self = serde_json::from_str(text).map_err(|error| error.to_string())?;
        config.validate()?;
        Ok(config)
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
        validate_document_path(self.entrypoint.as_str())
    }

    pub fn to_text(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map(|text| text + "\n")
            .map_err(|error| error.to_string())
    }

    pub fn write(&self, root: &Utf8Path) -> Result<(), String> {
        atomic_write(&root.join(PROJECT_CONFIG_FILE), self.to_text()?.as_bytes())
            .map_err(|error| error.to_string())
    }
}

/// The one editable source root of a local project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectWorkspace {
    pub root: Utf8PathBuf,
    pub config: ProjectConfig,
}

impl ProjectWorkspace {
    pub fn new(root: &Utf8Path, config: ProjectConfig) -> Result<Self, String> {
        config.validate()?;
        let root = root
            .canonicalize_utf8()
            .map_err(|error| error.to_string())?;
        if !root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        Ok(Self { root, config })
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
