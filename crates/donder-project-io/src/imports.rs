pub use ownership::{
    available_reusable_sources, ensure_document_can_reference_object, link_reusable_source,
};
mod ownership;
use crate::loader::{Loader, ResolvedObject};
use crate::source::{ImportEdge, ProjectSession, SourceDocument, SourceObjectKind};
use crate::{
    ExportProjectError, IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoRelatedLocation,
    LoadProjectError, TextRange,
};
use donder_language::data::Reference;
use donder_language::data::Spanned;
use donder_language::{ImportAlias, ImportDeclaration, ImportSource, SourceReference};
use donder_model::{DocumentId, SourceIdentity};
use donder_runtime_types::Identifier;
use indexmap::IndexMap;

#[derive(Clone, Debug)]
pub(crate) struct ParsedImport {
    pub(crate) declaration: ImportDeclaration,
    pub(crate) range: Option<TextRange>,
    pub(crate) alias_span: donder_language::compiler::TextSpan,
    pub(crate) source_ranges: Vec<Option<TextRange>>,
}

fn ensure_document_imports_target(
    session: &mut ProjectSession,
    from_document: &donder_model::DocumentId,
    kind: &SourceObjectKind,
    reference: &str,
    target_document: donder_model::DocumentId,
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
    if !matches!(document.kind, crate::source::SourceDocumentKind::Data) {
        return Err(ExportProjectError::InvalidReference {
            path: from_path.to_path_buf(),
            reference: reference.to_string(),
            message: "Only data documents can declare imports.".into(),
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
    from_document: &donder_model::DocumentId,
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

pub(crate) fn canonical_reference_alias(kind: &SourceObjectKind) -> Option<&'static str> {
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
        name: donder_runtime_types::Identifier::new(identity.object().to_string()).map_err(
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
    /// The declared object a reference starts with, `name` or `alias.name`,
    /// and the segments that follow it.
    pub(crate) fn resolve_declared<'r>(
        &self,
        document: &DocumentId,
        reference: &'r Reference,
    ) -> Result<(ResolvedObject, &'r [Spanned<Identifier>]), LoadProjectError> {
        let unresolved = || self.unresolved(document, reference);
        let segments = &reference.segments;
        let first = segments.first().ok_or_else(unresolved)?;
        let scope = self.visible_objects.get(document).ok_or_else(unresolved)?;
        if let (Some(name), Ok(alias)) = (segments.get(1), ImportAlias::new(first.value.as_str()))
            && let Some(object) = scope.get(&SourceReference::Qualified {
                alias,
                name: name.value.clone(),
            })
        {
            if let Some(import) = self.import_locations.get(document).and_then(|imports| {
                imports
                    .iter()
                    .find(|import| import.declaration.alias.as_str() == first.value.as_str())
            }) {
                self.link(
                    document,
                    first.span,
                    crate::index::LinkTarget::Import {
                        document: document.clone(),
                        span: import.alias_span,
                    },
                );
            }
            if let Some(target) = self.declared_target(object) {
                self.link(document, name.span, target);
            }
            return Ok((object.clone(), &segments[2..]));
        }
        let object = scope
            .get(&SourceReference::Local(first.value.clone()))
            .ok_or_else(unresolved)?;
        if let Some(target) = self.declared_target(object) {
            self.link(document, first.span, target);
        }
        Ok((object.clone(), &segments[1..]))
    }

    /// A reference to exactly one declared object of `kind`.
    pub(crate) fn resolve_reference(
        &self,
        document: &DocumentId,
        reference: &Reference,
        kind: SourceObjectKind,
    ) -> Result<ResolvedObject, LoadProjectError> {
        let (object, rest) = self.resolve_declared(document, reference)?;
        if !rest.is_empty() {
            return Err(self.unresolved(document, reference));
        }
        if object.source_kind() != kind {
            return Err(self.invalid(
                document,
                reference.span,
                format!(
                    "`{}` is {}, not {}",
                    reference.text(),
                    kind_name(&object.source_kind()),
                    kind_name(&kind)
                ),
            ));
        }
        Ok(object)
    }

    pub(crate) fn unresolved(
        &self,
        document: &DocumentId,
        reference: &Reference,
    ) -> LoadProjectError {
        LoadProjectError::InvalidReference {
            path: document.path().to_path_buf(),
            range: self.range(document, reference.span),
            reference: reference.text(),
        }
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
        importer: &donder_model::DocumentId,
        import: &ParsedImport,
    ) -> Result<Vec<donder_model::DocumentId>, LoadProjectError> {
        match &import.declaration.source {
            donder_language::ImportSource::LocalDocuments { documents } => documents
                .iter()
                .enumerate()
                .map(|(index, path)| {
                    crate::validate_document_path(path.as_str()).map_err(|message| {
                        LoadProjectError::InvalidDocument {
                            path: importer.path().to_path_buf(),
                            range: target_range(import, index),
                            message,
                        }
                    })?;
                    let target = donder_model::DocumentId::new(importer.module_id(), path.clone());
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
            fix: None,
            related: vec![IoRelatedLocation {
                path: document.path().to_path_buf(),
                range: previous,
                message: description.to_string(),
            }],
        }],
    }
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
/// An object kind as diagnostics name it.
pub(crate) fn kind_name(kind: &SourceObjectKind) -> &'static str {
    match kind {
        SourceObjectKind::Project => "a project",
        SourceObjectKind::Setup => "a setup",
        SourceObjectKind::Controller => "a controller",
        SourceObjectKind::Layout => "a layout",
        SourceObjectKind::Patch => "a patch",
        SourceObjectKind::FixtureDefinition => "a fixture definition",
        SourceObjectKind::Curve => "a curve",
        SourceObjectKind::Gradient => "a gradient",
        SourceObjectKind::Sequence => "a sequence",
        SourceObjectKind::EffectDefinition => "an effect",
        SourceObjectKind::OperatorDefinition => "an operator",
        SourceObjectKind::EffectInstance => "a clip",
    }
}
