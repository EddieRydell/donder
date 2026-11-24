use crate::document::text_cache::DocumentTextCache;
pub(super) fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    let mut cache = DocumentTextCache::default();
    let mut writes = std::collections::BTreeMap::new();
    for (id, document) in &session.source.documents {
        let path = output_root.join(id.path());
        let expected = read_previous(&path)?;
        writes.insert(
            id.path().to_path_buf(),
            SourceTextWrite {
                text: document_text(session, id, document, &mut cache)?,
                expected,
            },
        );
    }
    write_source_texts(output_root, &writes)
}

#[derive(Clone, Debug)]
pub struct SourceTextWrite {
    pub text: String,
    /// Last observed disk bytes. None means the file must not exist.
    pub expected: Option<Vec<u8>>,
}

pub(crate) fn read_previous(path: &Utf8Path) -> Result<Option<Vec<u8>>, ExportProjectError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(source) => Err(ExportProjectError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Persist exactly these sources, checking every precondition before writing.
/// A failed write restores the files already touched and reports rollback errors.
pub fn write_source_texts(
    root: &Utf8Path,
    writes: &std::collections::BTreeMap<Utf8PathBuf, SourceTextWrite>,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    let mut files = Vec::with_capacity(writes.len());
    for (relative, write) in writes {
        crate::validate_relative_path(relative.as_str()).map_err(|message| {
            ExportProjectError::Io {
                path: relative.clone(),
                source: io::Error::other(message),
            }
        })?;
        let path = root.join(relative);
        if read_previous(&path)? != write.expected {
            return Err(ExportProjectError::Io {
                path,
                source: io::Error::other(
                    "Source changed on disk; resolve the external conflict before saving",
                ),
            });
        }
        files.push(FileReplacement {
            path,
            bytes: write.text.as_bytes(),
            previous: write.expected.as_deref(),
        });
    }
    replace_files(&files)?;
    Ok(writes.keys().cloned().collect())
}

/// A file to write, and the bytes it holds now: `None` when it does not exist.
pub(crate) struct FileReplacement<'a> {
    pub(crate) path: Utf8PathBuf,
    pub(crate) bytes: &'a [u8],
    pub(crate) previous: Option<&'a [u8]>,
}

/// Write each file in order, rechecking just before each write that it still
/// holds its previous bytes. When one fails, the files already written are
/// restored, and the error names any that could not be.
pub(crate) fn replace_files(files: &[FileReplacement<'_>]) -> Result<(), ExportProjectError> {
    for (index, file) in files.iter().enumerate() {
        let result = (|| {
            let actual = match fs::read(&file.path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            if actual.as_deref() != file.previous {
                return Err(io::Error::other("Source changed during save"));
            }
            crate::atomic_write(&file.path, file.bytes)
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for written in files[..index].iter().rev() {
                let rollback = match written.previous {
                    Some(bytes) => crate::atomic_write(&written.path, bytes),
                    None => match fs::remove_file(&written.path) {
                        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
                        _ => Ok(()),
                    },
                };
                if let Err(error) = rollback {
                    failures.push(format!("{}: {error}", written.path));
                }
            }
            return Err(ExportProjectError::Io {
                path: file.path.clone(),
                source: io::Error::other(if failures.is_empty() {
                    error.to_string()
                } else {
                    format!("{error}; rollback failed: {}", failures.join(", "))
                }),
            });
        }
    }
    Ok(())
}

pub(super) fn document_text(
    session: &ProjectSession,
    document_id: &DocumentId,
    document: &SourceDocument,
    cache: &mut DocumentTextCache,
) -> Result<String, ExportProjectError> {
    match document.script_source() {
        None => crate::document::save::data_document_text(session, document_id, document, cache),
        Some(source) => Ok(source.to_string()),
    }
}

use fs_err as fs;
use std::io;

use crate::address::DocumentId;
use camino::{Utf8Path, Utf8PathBuf};

use crate::ExportProjectError;
use crate::source::{ProjectSession, SourceDocument};
