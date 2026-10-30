use crate::document::text_cache::DocumentTextCache;
pub(super) fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    validate_source_inventory(session)?;
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

fn read_previous(path: &Utf8Path) -> Result<Option<Vec<u8>>, ExportProjectError> {
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
    let mut prepared = Vec::new();
    for (relative, write) in writes {
        if relative.as_str().is_empty()
            || !relative
                .components()
                .all(|part| matches!(part, camino::Utf8Component::Normal(_)))
        {
            return Err(ExportProjectError::Io {
                path: relative.clone(),
                source: io::Error::other("Invalid project source path"),
            });
        }
        let path = root.join(relative);
        let previous = read_previous(&path)?;
        if previous != write.expected {
            return Err(ExportProjectError::Io {
                path,
                source: io::Error::other(
                    "Source changed on disk; resolve the external conflict before saving",
                ),
            });
        }
        prepared.push((relative, path, write));
    }
    for (index, (_, path, write)) in prepared.iter().enumerate() {
        let result = (|| {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            // Recheck immediately before each overwrite as well as before the transaction.
            let actual = match fs::read(path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
            if actual != write.expected {
                return Err(io::Error::other("Source changed during save"));
            }
            crate::atomic_write(path, write.text.as_bytes()).map_err(io::Error::other)
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for (_, written_path, previous) in prepared[..index].iter().rev() {
                let rollback = match &previous.expected {
                    Some(bytes) => {
                        crate::atomic_write(written_path, bytes).map_err(io::Error::other)
                    }
                    None => fs::remove_file(written_path),
                };
                if let Err(error) = rollback {
                    failures.push(format!("{written_path}: {error}"));
                }
            }
            return Err(ExportProjectError::Io {
                path: path.clone(),
                source: io::Error::other(if failures.is_empty() {
                    error.to_string()
                } else {
                    format!("{error}; rollback failed: {}", failures.join(", "))
                }),
            });
        }
    }
    Ok(writes.keys().cloned().collect())
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

/// The inventory declares exactly the typed project's reusable objects, each
/// under a name unique in its document.
pub(super) fn validate_source_inventory(
    session: &ProjectSession,
) -> Result<(), ExportProjectError> {
    let project = &session.project;
    let mut typed: indexmap::IndexSet<ObjectId> = std::iter::once(project.root().id.into())
        .chain(
            project
                .reusable::<donder_model::Setup>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::Controller>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::InputDevice>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::Layout>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::Patch>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::Sequence>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::Console>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::FixtureDefinition>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::CurveDefinition>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::GradientDefinition>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::EffectDefinition>()
                .keys()
                .map(|&id| id.into()),
        )
        .chain(
            project
                .reusable::<donder_model::OperatorDefinition>()
                .keys()
                .map(|&id| id.into()),
        )
        .collect();
    for (document_id, document) in &session.source.documents {
        let mut names = std::collections::HashSet::new();
        for object in &document.objects {
            if !names.insert(&object.name) {
                return Err(ExportProjectError::InvalidReference {
                    path: document_id.path().to_path_buf(),
                    reference: object.name.as_str().to_string(),
                    message: "the document declares this name twice".to_string(),
                });
            }
            if !typed.swap_remove(&object.id) {
                return Err(missing_typed_object(document_id, object));
            }
        }
    }
    if let Some(id) = typed.first() {
        return Err(ExportProjectError::InvalidReference {
            path: crate::PROJECT_ROOT_FILE.into(),
            reference: format!("{:?}", id.kind()),
            message: "typed project object is missing from the source inventory".to_string(),
        });
    }
    Ok(())
}

pub(crate) fn missing_typed_object(
    document: &DocumentId,
    object: &DeclaredObject,
) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: document.path().to_path_buf(),
        reference: object.name.as_str().to_string(),

        message: "typed project object is missing".to_string(),
    }
}

use std::{fs, io};

use crate::address::{DeclaredObject, DocumentId};
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::ObjectId;

use crate::ExportProjectError;
use crate::source::{ProjectSession, SourceDocument};
