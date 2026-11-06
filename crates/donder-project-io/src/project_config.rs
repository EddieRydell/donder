use camino::{Utf8Path, Utf8PathBuf};
use std::{fs, io, io::Write};

pub const PROJECT_ROOT_FILE: &str = "project.data.donder";
/// The root `Project` declaration's `format`.
pub const PROJECT_FORMAT_VERSION: u8 = 1;

/// The one editable source root of a local project.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectWorkspace {
    pub(crate) root: Utf8PathBuf,
}

impl ProjectWorkspace {
    pub(crate) fn new(root: &Utf8Path) -> Result<Self, String> {
        let root = root
            .canonicalize_utf8()
            .map_err(|error| error.to_string())?;
        if !root.is_dir() {
            return Err("Project root must be a directory".into());
        }
        Ok(Self { root })
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
