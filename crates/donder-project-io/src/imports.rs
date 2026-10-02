pub use ownership::{
    available_reusable_sources, ensure_document_can_reference_object, link_reusable_source,
};
mod ownership;
use crate::diagnostics::{
    source_range_for_field_value, source_range_for_scalar, source_range_for_value,
};
use crate::loader::Loader;
use crate::loader::mapping::parse_mapping;
use crate::loader::parse::ResolvedObject;
use crate::source::{ImportEdge, ProjectSession, SourceDocument, SourceObjectKind};
use crate::{
    ExportProjectError, IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoRelatedLocation,
    LoadProjectError, TextRange,
};
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::{DocumentId, SourceIdentity};
use donder_language::imports::{ImportAlias, ImportDeclaration, ImportSource, SourceReference};
use indexmap::IndexMap;
pub(crate) use ownership::write_object_reference;
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
    let Some(imports) = map.get(Value::String("imports".into())) else {
        return Ok(Vec::new());
    };
    let imports = imports
        .as_sequence()
        .ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_owned(),
            range: source_range_for_value(path, imports),
            message: "imports must be a sequence".into(),
        })?;
    imports
        .iter()
        .map(|import| {
            parse_mapping(path, import, "import", |fields| {
                let from = fields.required("from")?;
                let (source, source_ranges) = parse_mapping(path, from, "import source", |source| {
                    if let Some(documents) = source.optional("documents") {
                        let documents = documents
                            .as_sequence()
                            .filter(|values| !values.is_empty())
                            .ok_or_else(|| LoadProjectError::InvalidDocument {
                                path: path.to_owned(),
                                range: source_range_for_value(path, documents),
                                message: "local import `documents` must be a non-empty sequence".into(),
                            })?;
                        let paths = documents
                            .iter()
                            .map(|value| {
                                value.as_str().map(Utf8PathBuf::from).ok_or_else(|| {
                                    LoadProjectError::InvalidDocument {
                                        path: path.to_owned(),
                                        range: source_range_for_value(path, value),
                                        message: "local import `documents` must contain document paths".into(),
                                    }
                                })
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        let ranges = documents
                            .iter()
                            .map(|value| source_range_for_value(path, value))
                            .collect();
                        Ok((ImportSource::LocalDocuments { documents: paths }, ranges))
                    } else {
                        Err(LoadProjectError::InvalidDocument { path: path.to_owned(), range: source_range_for_value(path, from), message: "Import source requires a non-empty documents list".into() })
                    }
                })?;
                let alias = ImportAlias::new(fields.string("as")?).map_err(|message| {
                    LoadProjectError::InvalidDocument {
                        path: path.to_owned(),
                        range: source_range_for_field_value(path, import, "as"),
                        message,
                    }
                })?;
                Ok(ParsedImport {
                    declaration: ImportDeclaration { source, alias },
                    range: source_range_for_value(path, import),
                    source_ranges,
                })
            })
        })
        .collect()
}

pub(crate) fn validate_import_document_path(
    document: &Utf8Path,
    value: &str,
) -> Result<(), LoadProjectError> {
    if crate::validate_document_path(value).is_err() {
        return Err(LoadProjectError::InvalidDocument {
            path: document.to_path_buf(),
            range: None,
            message: "local imports must name explicit safe module-relative Donder documents"
                .to_string(),
        });
    }
    Ok(())
}

fn ensure_document_imports_target(
    session: &mut ProjectSession,
    from_document: &donder_language::identity::DocumentId,
    kind: &SourceObjectKind,
    reference: &str,
    target_document: donder_language::identity::DocumentId,
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
    if !matches!(
        document.kind,
        crate::source::SourceDocumentKind::Donder { .. }
    ) {
        return Err(ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message: "Only YAML documents can declare imports.".into(),
        });
    }
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
            message: "referenced objects must belong to the same project".into(),
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
    from_document: &donder_language::identity::DocumentId,
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
    reference: &donder_language::effect::EffectRef,
) -> Result<String, ExportProjectError> {
    use donder_language::effect::EffectRef;
    match reference {
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
        name: donder_language::dsl::Identifier::new(identity.object().to_string()).map_err(
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
        importer: &donder_language::identity::DocumentId,
        import: &ParsedImport,
    ) -> Result<Vec<donder_language::identity::DocumentId>, LoadProjectError> {
        match &import.declaration.source {
            donder_language::imports::ImportSource::LocalDocuments { documents } => documents
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
                    let target = donder_language::identity::DocumentId::new(
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
        }
    }
}

fn target_range(import: &ParsedImport, index: usize) -> Option<TextRange> {
    match import.declaration.source {
        ImportSource::LocalDocuments { .. } => import.source_ranges.get(index).cloned().flatten(),
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
            code: IoDiagnosticCode::DonderLoad,
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
) -> Option<donder_language::effect::EffectRef> {
    use donder_language::effect::EffectRef;
    match lookup_reference(scopes, document, reference)? {
        ResolvedObject::EffectDefinition(target) => Some(EffectRef::Custom(target.clone())),
        _ => None,
    }
}
