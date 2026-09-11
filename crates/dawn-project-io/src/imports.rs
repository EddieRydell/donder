use crate::diagnostics::{
    source_range_for_field_value, source_range_for_scalar, source_range_for_value,
};
use crate::loader::Loader;
use crate::loader::parse::{ResolvedObject, string_field};
use crate::source::{ImportEdge, ProjectSession, SourceDocument, SourceObjectKind};
use crate::{
    ExportProjectError, IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoRelatedLocation,
    LoadProjectError, TextRange,
};
use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::identity::{DocumentId, SourceIdentity};
use dawn_language::imports::{ImportAlias, ImportDeclaration, ImportSource, SourceReference};
use indexmap::{IndexMap, IndexSet};
use yaml_serde::{Mapping, Value};

#[derive(Clone, Debug)]
pub(crate) struct ParsedImport {
    pub(crate) declaration: ImportDeclaration,
    pub(crate) range: Option<TextRange>,
    pub(crate) source_ranges: Vec<Option<TextRange>>,
}

pub(crate) fn parse_imports(
    path: &Utf8Path,
    map: &Mapping,
) -> Result<Vec<ParsedImport>, LoadProjectError> {
    let Some(imports) = map.get(Value::String("imports".to_string())) else {
        return Ok(Vec::new());
    };
    let imports = imports
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: source_range_for_value(path, imports),
            message: "imports must be a sequence".to_string(),
        })?;
    imports
        .iter()
        .map(|import| {
            let import_mapping =
                import
                    .as_mapping()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "import must be a mapping".to_string(),
                    })?;
            require_exact_mapping_keys(path, import_mapping, &["from", "as"], "import")?;
            let from_value = import_mapping
                .get(Value::String("from".to_string()))
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: "import is missing `from`".to_string(),
                })?;
            let from_mapping =
                from_value
                    .as_mapping()
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "import `from` must be a structured mapping".to_string(),
                    })?;
            let source = if let Some(documents_value) =
                from_mapping.get(Value::String("documents".to_string()))
            {
                require_exact_mapping_keys(
                    path,
                    from_mapping,
                    &["documents"],
                    "local import source",
                )?;
                let raw_documents = documents_value.as_sequence().ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "local import `documents` must be a non-empty sequence"
                            .to_string(),
                    }
                })?;
                if raw_documents.is_empty() {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "local import `documents` must be a non-empty sequence"
                            .to_string(),
                    });
                }
                let mut documents = Vec::new();
                for raw_document in raw_documents {
                    let value =
                        raw_document
                            .as_str()
                            .ok_or_else(|| LoadProjectError::InvalidDocument {
                                path: path.to_path_buf(),
                                range: None,
                                message: "local import `documents` must contain document paths"
                                    .to_string(),
                            })?;
                    documents.push(Utf8PathBuf::from(value));
                }
                dawn_language::imports::ImportSource::LocalDocuments { documents }
            } else {
                require_exact_mapping_keys(
                    path,
                    from_mapping,
                    &["dependency", "export"],
                    "dependency import source",
                )?;
                let dependency = from_mapping
                    .get(Value::String("dependency".to_string()))
                    .and_then(Value::as_str)
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "dependency import requires a dependency alias".to_string(),
                    })?;
                let export = from_mapping
                    .get(Value::String("export".to_string()))
                    .and_then(Value::as_str)
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: None,
                        message: "dependency import requires an export group".to_string(),
                    })?;
                dawn_language::imports::ImportSource::DependencyExport {
                    dependency: dependency.to_string(),
                    export: export.to_string(),
                }
            };
            let alias = string_field(path, import, "as")?;
            Ok(ParsedImport {
                declaration: ImportDeclaration {
                    source,
                    alias: ImportAlias::new(alias).map_err(|message| {
                        LoadProjectError::InvalidDocument {
                            path: path.to_path_buf(),
                            range: source_range_for_field_value(path, import, "as"),
                            message,
                        }
                    })?,
                },
                range: source_range_for_value(path, import),
                source_ranges: if let Some(documents) = from_mapping
                    .get(Value::String("documents".into()))
                    .and_then(Value::as_sequence)
                {
                    documents
                        .iter()
                        .map(|value| source_range_for_value(path, value))
                        .collect()
                } else {
                    ["dependency", "export"]
                        .into_iter()
                        .map(|key| {
                            source_range_for_value(path, &from_mapping[Value::String(key.into())])
                        })
                        .collect()
                },
            })
        })
        .collect()
}

fn require_exact_mapping_keys(
    path: &Utf8Path,
    mapping: &Mapping,
    expected: &[&str],
    label: &str,
) -> Result<(), LoadProjectError> {
    let keys = mapping
        .keys()
        .map(|key| key.as_str())
        .collect::<Option<IndexSet<_>>>()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: format!("{label} keys must be strings"),
        })?;
    if keys.len() != expected.len() || expected.iter().any(|key| !keys.contains(key)) {
        return Err(LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: format!("{label} has missing or unknown fields"),
        });
    }
    Ok(())
}

pub(crate) fn validate_import_document_path(
    document: &Utf8Path,
    value: &str,
) -> Result<(), LoadProjectError> {
    if dawn_package::validate_module_relative_dawn_path(value).is_err() {
        return Err(LoadProjectError::InvalidDocument {
            path: document.to_path_buf(),
            range: None,
            message: "local imports must name explicit safe module-relative Dawn documents"
                .to_string(),
        });
    }
    Ok(())
}

fn ensure_document_imports_target(
    session: &mut ProjectSession,
    from_document: &dawn_language::identity::DocumentId,
    kind: &SourceObjectKind,
    reference: &str,
    target_document: dawn_language::identity::DocumentId,
) -> Result<(), ExportProjectError> {
    let from_path = from_document.path();
    let document = session
        .source
        .documents
        .get_mut(from_document)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message: "source document is missing from the source project".to_string(),
        })?;
    if document
        .imports
        .iter()
        .any(|edge| edge.targets.contains(&target_document))
    {
        return Ok(());
    }
    let alias_base =
        canonical_reference_alias(kind).ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message: format!("no canonical import alias exists for {kind:?} references"),
        })?;
    let alias = available_import_alias(document, alias_base).ok_or_else(|| {
        ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message: format!("no import alias remains for `{alias_base}`"),
        }
    })?;
    if target_document.module_id() != from_document.module_id() {
        return Err(ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message:
                "dependency objects must be exposed through an explicitly declared export import"
                    .to_string(),
        });
    }
    document.imports.push(ImportEdge {
        declaration: ImportDeclaration {
            alias: ImportAlias::new(&alias).map_err(|message| {
                ExportProjectError::InvalidReference {
                    path: from_path.to_path_buf(),
                    reference: reference.to_string(),
                    message,
                }
            })?,
            source: ImportSource::LocalDocuments {
                documents: vec![target_document.path().to_path_buf()],
            },
        },
        targets: vec![target_document],
    });
    Ok(())
}

pub fn ensure_document_can_reference_source(
    session: &mut ProjectSession,
    from_document: &dawn_language::identity::DocumentId,
    kind: SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<(), ExportProjectError> {
    validate_reference_target(session, from_document, &kind, identity)?;
    if identity.document_id() == from_document {
        return Ok(());
    }
    ensure_document_imports_target(
        session,
        from_document,
        &kind,
        identity.object(),
        identity.document_id().clone(),
    )
}

fn available_import_alias(document: &SourceDocument, base: &str) -> Option<String> {
    if document
        .imports
        .iter()
        .all(|import| import.declaration.alias.as_str() != base)
    {
        return Some(base.to_string());
    }
    (2_u32..)
        .map(|suffix| format!("{base}_{suffix}"))
        .find(|candidate| {
            document
                .imports
                .iter()
                .all(|import| import.declaration.alias.as_str() != candidate.as_str())
        })
}

fn canonical_reference_alias(kind: &SourceObjectKind) -> Option<&'static str> {
    match kind {
        SourceObjectKind::EffectDefinition => Some("effects"),
        SourceObjectKind::OperatorDefinition => Some("operators"),
        SourceObjectKind::Curve => Some("curves"),
        SourceObjectKind::Gradient => Some("gradients"),
        SourceObjectKind::Sequence => Some("sequences"),
        SourceObjectKind::Project => Some("projects"),
        SourceObjectKind::Setup => Some("setups"),
        SourceObjectKind::Controller => Some("controllers"),
        SourceObjectKind::Layout => Some("layouts"),
        SourceObjectKind::Patch => Some("patches"),
        SourceObjectKind::FixtureDefinition => Some("fixtures"),
        SourceObjectKind::EffectInstance => None,
    }
}

pub(crate) fn write_effect_reference(
    session: &ProjectSession,
    from_document: &DocumentId,
    reference: &dawn_language::effect::EffectRef,
) -> Result<String, ExportProjectError> {
    use dawn_language::effect::{EffectRef, builtin_effect_definition};
    match reference {
        EffectRef::Builtin(builtin) => Ok(SourceReference::Builtin(
            dawn_language::dsl::Identifier::new(
                builtin_effect_definition(*builtin).source_name.clone(),
            )
            .map_err(|error| ExportProjectError::InvalidReference {
                path: from_document.path().to_path_buf(),
                reference: builtin_effect_definition(*builtin).source_name.clone(),
                message: format!("invalid built-in source identifier: {error:?}"),
            })?,
        )
        .to_string()),
        EffectRef::Custom(target) => write_source_reference(
            session,
            from_document,
            SourceObjectKind::EffectDefinition,
            &target.0,
        ),
    }
}

pub(crate) fn write_source_reference(
    session: &ProjectSession,
    from_document: &DocumentId,
    kind: SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<String, ExportProjectError> {
    validate_reference_target(session, from_document, &kind, identity)?;
    if identity.document_id() == from_document {
        return Ok(identity.object().to_string());
    }
    let alias = session
        .source
        .documents
        .get(from_document)
        .into_iter()
        .flat_map(|document| &document.imports)
        .find(|edge| {
            edge.targets
                .iter()
                .any(|target| target == identity.document_id())
        })
        .map(|edge| edge.declaration.alias.clone())
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.path().to_path_buf(),
            reference: identity.object().to_string(),
            message: format!(
                "no import alias makes the {kind:?} target visible from this document"
            ),
        })?;
    Ok(SourceReference::Qualified {
        alias,
        name: dawn_language::dsl::Identifier::new(identity.object().to_string()).map_err(
            |error| ExportProjectError::InvalidReference {
                path: from_document.path().to_path_buf(),
                reference: identity.object().to_string(),
                message: format!("invalid source identifier: {error:?}"),
            },
        )?,
    }
    .to_string())
}

impl Loader {
    pub(crate) fn link_generated_effects(&mut self) -> Result<(), LoadProjectError> {
        for (id, definition) in &mut self.definitions.effects.definitions {
            let mut targets = Vec::with_capacity(definition.emitted_references.len());
            for occurrence in &definition.emitted_references {
                let range = self.documents.get(id.0.document_id()).and_then(|document| {
                    if let crate::source::SourceDocumentKind::Effect { source } = &document.kind {
                        Some(crate::diagnostics::byte_range(
                            source,
                            occurrence.span.start,
                            occurrence.span.end,
                        ))
                    } else {
                        None
                    }
                });
                let target = lookup_effect_reference(&self.visible_objects, id.0.document_id(), &occurrence.reference)
                    .ok_or_else(|| LoadProjectError::InvalidDocument {
                        path: id.0.document().to_path_buf(), range,
                        message: format!("generated child reference `{}` must resolve to an effect definition in the generator document's scope", occurrence.reference),
                    })?;
                targets.push(target);
            }
            definition.generated_effect_targets = targets.into_boxed_slice();
        }
        for (id, definition) in &self.definitions.effects.definitions {
            for (emission, target) in definition
                .emitted_references
                .iter()
                .zip(&definition.generated_effect_targets)
            {
                let child = self.definitions.effects.resolve(target).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: id.0.document().to_path_buf(),
                        range: None,
                        message: "linked generated child definition is missing".to_string(),
                    }
                })?;
                dawn_language::dsl::validate_emission(emission, &child.params).map_err(
                    |error| {
                        let range = self.documents.get(id.0.document_id()).and_then(|document| {
                            if let crate::source::SourceDocumentKind::Effect { source } =
                                &document.kind
                            {
                                Some(crate::diagnostics::byte_range(
                                    source,
                                    error.span.start,
                                    error.span.end,
                                ))
                            } else {
                                None
                            }
                        });
                        LoadProjectError::InvalidDocument {
                            path: id.0.document().to_path_buf(),
                            range,
                            message: error.message,
                        }
                    },
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn resolve_reference(
        &self,
        document_id: &DocumentId,
        reference: &str,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let range = source_range_for_scalar(document_id.path(), reference);
        SourceReference::parse(reference)
            .ok()
            .and_then(|reference| lookup_reference(&self.visible_objects, document_id, &reference))
            .cloned()
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: document_id.path().to_path_buf(),
                range,
                reference: reference.to_string(),
            })
    }
    pub(crate) fn load_imports(
        &mut self,
        document_id: &DocumentId,
        imports: &[ParsedImport],
    ) -> Result<Vec<ImportEdge>, LoadProjectError> {
        self.import_locations
            .insert(document_id.clone(), imports.to_vec());
        let mut edges = Vec::with_capacity(imports.len());
        for import in imports {
            let targets = self.resolve_import(document_id, import)?;
            // Local inventories are indexed before traversal. A revisited
            // document ends traversal; scopes are constructed after all roots.
            for target in &targets {
                self.load_document(target)?;
            }
            edges.push(ImportEdge {
                declaration: import.declaration.clone(),
                targets,
            });
        }
        Ok(edges)
    }

    pub(crate) fn build_document_scopes(&mut self) -> Result<(), LoadProjectError> {
        for (document_id, document) in &self.documents {
            let declarations = &self.import_locations[document_id];
            let mut aliases = IndexMap::new();
            let mut targets = IndexMap::new();
            let mut imported = Vec::new();
            for (index, edge) in document.imports.iter().enumerate() {
                let location = &declarations[index];
                if let Some(previous) = aliases.insert(edge.declaration.alias.clone(), index) {
                    return Err(import_collision(
                        document_id,
                        location.range.clone(),
                        format!("duplicate import alias `{}`", edge.declaration.alias),
                        declarations[previous].range.clone(),
                        "first import with this alias",
                    ));
                }
                let mut names = IndexMap::new();
                for (target_index, target) in edge.targets.iter().enumerate() {
                    let range = target_range(location, target_index);
                    if let Some(previous) = targets.insert(target.clone(), range.clone()) {
                        return Err(import_collision(
                            document_id,
                            range,
                            format!(
                                "document `{}:{}` is imported more than once; each target must have one alias",
                                target.module_id(),
                                target.path()
                            ),
                            previous,
                            "first import of this document",
                        ));
                    }
                    for (reference, object) in &self.visible_objects[target] {
                        let SourceReference::Local(name) = reference else {
                            continue;
                        };
                        if let Some(previous) = names.insert(name.clone(), range.clone()) {
                            return Err(import_collision(
                                document_id,
                                range.clone(),
                                format!(
                                    "duplicate exported object `{}` in import alias `{}`",
                                    name.as_str(),
                                    edge.declaration.alias
                                ),
                                previous,
                                "first document exposing this name",
                            ));
                        }
                        imported.push((
                            SourceReference::Qualified {
                                alias: edge.declaration.alias.clone(),
                                name: name.clone(),
                            },
                            object.clone(),
                        ));
                    }
                }
            }
            self.visible_objects
                .get_mut(document_id)
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: document_id.path().to_path_buf(),
                    range: None,
                    message: "document inventory is missing".into(),
                })?
                .extend(imported);
        }
        Ok(())
    }

    pub(crate) fn resolve_import(
        &self,
        importer: &dawn_language::identity::DocumentId,
        import: &ParsedImport,
    ) -> Result<Vec<dawn_language::identity::DocumentId>, LoadProjectError> {
        match &import.declaration.source {
            dawn_language::imports::ImportSource::LocalDocuments { documents } => documents
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    validate_import_document_path(importer.path(), path.as_str()).map_err(
                        |error| {
                            crate::diagnostics::with_yaml_location(
                                error,
                                importer.path(),
                                target_range(import, index),
                            )
                        },
                    )?;
                    let target = dawn_language::identity::DocumentId::new(
                        importer.module_id(),
                        path.clone(),
                    );
                    let absolute = self.absolute_document_path(&target)?;
                    if !absolute.is_file() && !self.source_overrides.contains_key(&target) {
                        return Err(LoadProjectError::InvalidDocument {
                            path: importer.path().to_path_buf(),
                            range: target_range(import, index),
                            message: format!("local import target does not exist: {path}"),
                        });
                    }
                    Ok(target)
                })
                .collect(),
            dawn_language::imports::ImportSource::DependencyExport { dependency, export } => {
                for (index, name) in [dependency, export].into_iter().enumerate() {
                    dawn_package::validate_package_reference_name(name).map_err(|error| {
                        LoadProjectError::InvalidDocument {
                            path: importer.path().to_path_buf(),
                            range: import
                                .source_ranges
                                .get(index)
                                .cloned()
                                .flatten()
                                .or_else(|| import.range.clone()),
                            message: error.to_string(),
                        }
                    })?;
                }
                let target_module = self
                    .source_graph
                    .dependency(importer.module_id(), dependency)
                    .map_err(|error| LoadProjectError::InvalidDocument {
                        path: importer.path().to_path_buf(),
                        range: import
                            .source_ranges
                            .first()
                            .cloned()
                            .flatten()
                            .or_else(|| import.range.clone()),
                        message: error.to_string(),
                    })?;
                let group = target_module.manifest.exports.get(export).ok_or_else(|| {
                    LoadProjectError::InvalidDocument {
                        path: importer.path().to_path_buf(),
                        range: import
                            .source_ranges
                            .get(1)
                            .cloned()
                            .flatten()
                            .or_else(|| import.range.clone()),
                        message: format!(
                            "dependency `{dependency}` does not export group `{export}`"
                        ),
                    }
                })?;
                group
                    .documents
                    .iter()
                    .map(|path| {
                        let target = dawn_language::identity::DocumentId::new(
                            target_module.manifest.module_id,
                            Utf8PathBuf::from(path),
                        );
                        let absolute = self.absolute_document_path(&target)?;
                        if !absolute.is_file() && !self.source_overrides.contains_key(&target) {
                            return Err(LoadProjectError::InvalidDocument {
                                path: importer.path().to_path_buf(),
                                range: import.range.clone(),
                                message: format!(
                                    "dependency `{dependency}` export `{export}` is missing `{path}`"
                                ),
                            });
                        }
                        Ok(target)
                    })
                    .collect()
            }
        }
    }
}

fn target_range(import: &ParsedImport, index: usize) -> Option<TextRange> {
    match import.declaration.source {
        ImportSource::LocalDocuments { .. } => import.source_ranges.get(index).cloned().flatten(),
        ImportSource::DependencyExport { .. } => import.source_ranges.get(1).cloned().flatten(),
    }
    .or_else(|| import.range.clone())
}

fn import_collision(
    document: &DocumentId,
    range: Option<TextRange>,
    message: String,
    previous: Option<TextRange>,
    description: &str,
) -> LoadProjectError {
    LoadProjectError::InvalidImports {
        path: document.path().to_path_buf(),
        diagnostics: vec![IoDiagnostic {
            path: document.path().to_path_buf(),
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::DawnLoad,
            message,
            detail: None,
            related: vec![IoRelatedLocation {
                path: document.path().to_path_buf(),
                range: previous,
                message: description.to_string(),
            }],
        }],
    }
}

pub(crate) fn lookup_reference<'a>(
    scopes: &'a IndexMap<DocumentId, IndexMap<SourceReference, ResolvedObject>>,
    document: &DocumentId,
    reference: &SourceReference,
) -> Option<&'a ResolvedObject> {
    scopes.get(document)?.get(reference)
}

fn validate_reference_target(
    session: &ProjectSession,
    from_document: &DocumentId,
    kind: &SourceObjectKind,
    identity: &SourceIdentity,
) -> Result<(), ExportProjectError> {
    session
        .source
        .documents
        .get(identity.document_id())
        .and_then(|document| {
            document
                .objects
                .iter()
                .find(|object| &object.kind == kind && object.id == identity.object())
        })
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from_document.path().to_path_buf(),
            reference: identity.object().to_string(),
            message: "target is missing from its source document".to_string(),
        })?;
    Ok(())
}
pub(crate) fn lookup_effect_reference(
    scopes: &IndexMap<DocumentId, IndexMap<SourceReference, ResolvedObject>>,
    document: &DocumentId,
    reference: &SourceReference,
) -> Option<dawn_language::effect::EffectRef> {
    use dawn_language::effect::{EffectRef, builtin_effect_from_source_name};
    match reference {
        SourceReference::Builtin(name) => {
            builtin_effect_from_source_name(name.as_str()).map(EffectRef::Builtin)
        }
        _ => match lookup_reference(scopes, document, reference)? {
            ResolvedObject::EffectDefinition(target) => Some(EffectRef::Custom(target.clone())),
            _ => None,
        },
    }
}
