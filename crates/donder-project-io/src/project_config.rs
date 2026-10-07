use camino::{Utf8Path, Utf8PathBuf};
use std::{fs, io, io::Write};
use uuid::Uuid;

pub const PROJECT_ROOT_FILE: &str = "project.data.donder";
pub const PROJECT_FORMAT_VERSION: u8 = 1;

/// Workspace identity: the root `Project` declaration's `format` and `id`.
#[derive(Clone, Debug, Eq, PartialEq)]
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
        let (document, diagnostics) = crate::document::read(text);
        if !diagnostics.is_empty() {
            return Err(crate::LoadProjectError::InvalidData {
                path: path.into(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| crate::diagnostics::data_diagnostic(path, text, diagnostic))
                    .collect(),
            });
        }
        let (span, project) = document
            .declarations
            .iter()
            .find_map(|(_, span, declaration)| match declaration {
                crate::document::Declaration::Project(project) => Some((*span, project)),
                _ => None,
            })
            .ok_or_else(|| crate::LoadProjectError::InvalidDocument {
                path: path.into(),
                range: None,
                message: format!("{PROJECT_ROOT_FILE} must declare a `Project`"),
            })?;
        let invalid = |message: String| crate::LoadProjectError::InvalidDocument {
            path: path.into(),
            range: Some(crate::diagnostics::byte_range(text, span.start, span.end)),
            message,
        };
        let metadata = Self {
            format_version: project.format,
            project_id: Uuid::parse_str(&project.id)
                .map_err(|error| invalid(format!("Invalid project id: {error}")))?,
        };
        metadata.validate().map_err(invalid)?;
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
    if !value.ends_with(donder_language::data::SCRIPT_SUFFIX) {
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
