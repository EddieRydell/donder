use dawn_language::fixture::FixtureDefinitionId;
use dawn_language::layout::LayoutId;
mod sequence;
mod setup;
mod values;

use sequence::sequence_value;
use setup::{controller_value, fixture_definition_value, layout_value, patch_value, setup_value};
use values::{curve_value, gradient_value, string_value, typed_object, write_source_reference};

pub(super) fn write_source_documents(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<Vec<Utf8PathBuf>, ExportProjectError> {
    validate_source_inventory(session)?;
    let mut writes = std::collections::BTreeMap::new();
    for (id, document) in &session.source.documents {
        if !session.source.is_project_owned(id) {
            continue;
        }
        let path = output_root.join(id.path());
        let expected = read_previous(&path)?;
        writes.insert(
            id.path().to_path_buf(),
            SourceTextWrite {
                text: document_text(session, id, document)?,
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
            dawn_package::atomic_write(path, write.text.as_bytes()).map_err(io::Error::other)
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for (_, written_path, previous) in prepared[..index].iter().rev() {
                let rollback = match &previous.expected {
                    Some(bytes) => {
                        dawn_package::atomic_write(written_path, bytes).map_err(io::Error::other)
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
) -> Result<String, ExportProjectError> {
    match &document.kind {
        SourceDocumentKind::Dawn { .. } => {
            let mut root = Mapping::new();
            if !document.imports.is_empty() {
                root.insert(
                    string_value("imports"),
                    import_decls_value(&document.imports),
                );
            }
            for object in &document.objects {
                let value = serialize_source_object(session, document_id, object)?;
                root.insert(string_value(&object.id), value);
            }
            yaml_serde::to_string(&Value::Mapping(root)).map_err(|source| {
                ExportProjectError::Serialize {
                    path: document_id.path().to_path_buf(),
                    source,
                }
            })
        }
        SourceDocumentKind::Effect { source } => Ok(source.clone()),
        SourceDocumentKind::Operator { source } => Ok(source.clone()),
    }
}

pub(super) fn validate_source_inventory(
    session: &ProjectSession,
) -> Result<(), ExportProjectError> {
    let project = &session.project;
    let identities = std::iter::once((SourceObjectKind::Project, &project.root.id.0))
        .chain(
            project
                .setups
                .keys()
                .map(|id| (SourceObjectKind::Setup, &id.0)),
        )
        .chain(
            project
                .controllers
                .keys()
                .map(|id| (SourceObjectKind::Controller, &id.0)),
        )
        .chain(
            project
                .layouts
                .keys()
                .map(|id| (SourceObjectKind::Layout, &id.0)),
        )
        .chain(
            project
                .patches
                .keys()
                .map(|id| (SourceObjectKind::Patch, &id.0)),
        )
        .chain(
            project
                .sequences
                .keys()
                .map(|id| (SourceObjectKind::Sequence, &id.0)),
        )
        .chain(
            project
                .definitions
                .fixtures
                .definitions
                .keys()
                .map(|id| (SourceObjectKind::FixtureDefinition, &id.0)),
        )
        .chain(
            project
                .definitions
                .curves
                .definitions
                .keys()
                .map(|id| (SourceObjectKind::Curve, &id.0)),
        )
        .chain(
            project
                .definitions
                .gradients
                .definitions
                .keys()
                .map(|id| (SourceObjectKind::Gradient, &id.0)),
        )
        .chain(
            project
                .definitions
                .effects
                .definitions
                .keys()
                .map(|id| (SourceObjectKind::EffectDefinition, &id.0)),
        )
        .chain(
            project
                .definitions
                .operators
                .definitions
                .keys()
                .map(|id| (SourceObjectKind::OperatorDefinition, &id.0)),
        );
    let mut typed: indexmap::IndexSet<_> = identities
        .map(|(kind, identity)| {
            (
                identity.document_id().clone(),
                SourceObjectId {
                    kind,
                    id: identity.object().to_string(),
                },
            )
        })
        .collect();
    for (document_id, document) in &session.source.documents {
        for object in &document.objects {
            if !typed.swap_remove(&(document_id.clone(), object.clone())) {
                return Err(missing_typed_object(document_id, object));
            }
        }
    }
    if let Some((document, object)) = typed.first() {
        return Err(ExportProjectError::InvalidReference {
            path: document.path().to_path_buf(),
            reference: object.id.clone(),
            message: "typed project object is missing from the source inventory".to_string(),
        });
    }
    Ok(())
}

pub(super) fn qualified_identity(
    session: &ProjectSession,
    document: &DocumentId,
    id: &SourceObjectId,
) -> Option<SourceIdentity> {
    session
        .source
        .documents
        .get(document)
        .is_some_and(|document| document.objects.contains(id))
        .then(|| SourceIdentity::from_document(document.clone(), id.id.clone()))
}

pub(super) fn serialize_source_object(
    session: &ProjectSession,
    from_document: &DocumentId,
    id: &SourceObjectId,
) -> Result<Value, ExportProjectError> {
    match id.kind {
        SourceObjectKind::Project => project_root_value(session, from_document),
        SourceObjectKind::Setup => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let setup = session
                .project
                .setups
                .get(&SetupId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            setup_value(session, from_document, setup)
        }
        SourceObjectKind::Controller => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let controller = session
                .project
                .controllers
                .get(&ControllerId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            controller_value(controller)
        }
        SourceObjectKind::Layout => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let layout = session
                .project
                .layouts
                .get(&LayoutId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            layout_value(session, from_document, layout)
        }
        SourceObjectKind::Patch => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let patch = session
                .project
                .patches
                .get(&PatchId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            patch_value(session, from_document, patch)
        }
        SourceObjectKind::FixtureDefinition => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let definition = session
                .project
                .definitions
                .fixtures
                .definitions
                .get(&FixtureDefinitionId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            fixture_definition_value(definition)
        }
        SourceObjectKind::Curve => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let curve = session
                .project
                .definitions
                .curves
                .definitions
                .get(&CurveId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            curve_value(&curve.curve)
        }
        SourceObjectKind::Gradient => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let gradient = session
                .project
                .definitions
                .gradients
                .definitions
                .get(&GradientId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            gradient_value(&gradient.gradient)
        }
        SourceObjectKind::Sequence => {
            let identity = qualified_identity(session, from_document, id)
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            let sequence = session
                .project
                .sequences
                .get(&SequenceId(identity))
                .ok_or_else(|| missing_typed_object(from_document, id))?;
            sequence_value(session, from_document, sequence)
        }
        SourceObjectKind::EffectDefinition
        | SourceObjectKind::OperatorDefinition
        | SourceObjectKind::EffectInstance => Err(ExportProjectError::InvalidReference {
            path: from_document.path().to_path_buf(),
            reference: id.id.clone(),
            message: "DSL definitions are preserved as source documents".to_string(),
        }),
    }
}

pub(super) fn missing_typed_object(
    document: &DocumentId,
    id: &SourceObjectId,
) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: document.path().to_path_buf(),
        reference: id.id.clone(),
        message: "typed project object is missing".to_string(),
    }
}

pub(super) fn import_decls_value(imports: &[ImportEdge]) -> Value {
    Value::Sequence(
        imports
            .iter()
            .map(|import| {
                let mut value = Mapping::new();
                let mut from = Mapping::new();
                match &import.declaration.source {
                    ImportSource::LocalDocuments { documents } => {
                        from.insert(
                            string_value("documents"),
                            Value::Sequence(
                                documents
                                    .iter()
                                    .map(|path| Value::String(path.to_string()))
                                    .collect(),
                            ),
                        );
                    }
                    ImportSource::DependencyExport { dependency, export } => {
                        from.insert(
                            string_value("dependency"),
                            Value::String(dependency.clone()),
                        );
                        from.insert(string_value("export"), Value::String(export.clone()));
                    }
                };
                value.insert(string_value("from"), Value::Mapping(from));
                value.insert(
                    string_value("as"),
                    Value::String(import.declaration.alias.to_string()),
                );
                Value::Mapping(value)
            })
            .collect(),
    )
}

pub(super) fn project_root_value(
    session: &ProjectSession,
    from_document: &DocumentId,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("project");
    value.insert(
        string_value("setup"),
        Value::String(write_source_reference(
            session,
            from_document,
            SourceObjectKind::Setup,
            &session.project.root.setup.0,
        )?),
    );
    value.insert(
        string_value("sequences"),
        Value::Sequence(
            session
                .project
                .root
                .sequences
                .iter()
                .map(|sequence| {
                    write_source_reference(
                        session,
                        from_document,
                        SourceObjectKind::Sequence,
                        &sequence.0,
                    )
                    .map(Value::String)
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}
use std::{fs, io};

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::controller::ControllerId;
use dawn_language::effect::{CurveId, GradientId};
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::patch::PatchId;
use dawn_language::sequence::SequenceId;
use dawn_language::setup::SetupId;
use yaml_serde::{Mapping, Value};

use crate::ExportProjectError;
use crate::source::{
    ImportEdge, ImportSource, ProjectSession, SourceDocument, SourceDocumentKind, SourceObjectId,
    SourceObjectKind,
};
