use crate::imports::parse_imports;
use donder_language::fixture::FixtureDefinitionId;
use donder_language::imports::SourceReference;
use donder_language::layout::LayoutId;
pub(crate) mod parse;
mod resolve;

pub(crate) use parse::mapping;
use parse::{
    ResolvedObject, SourceObjectValue, parse_curve, parse_gradient, sequence_field, string_field,
};
use resolve::DomainResolver;

pub(super) struct Loader {
    pub(crate) source_graph: donder_package::ResolvedSourceGraph,
    pub(crate) entrypoint: Option<donder_language::identity::DocumentId>,
    pub(crate) documents: IndexMap<donder_language::identity::DocumentId, SourceDocument>,
    pub(crate) visible_objects:
        IndexMap<donder_language::identity::DocumentId, IndexMap<SourceReference, ResolvedObject>>,
    pub(crate) import_locations:
        IndexMap<donder_language::identity::DocumentId, Vec<crate::imports::ParsedImport>>,
    pub(crate) loading_documents: IndexSet<donder_language::identity::DocumentId>,
    pub(crate) definitions: ProjectDefinitionStores,
    pub(crate) referenced_assets: Vec<ReferencedAsset>,
    pub(crate) next_asset_id: u32,
    pub(crate) checked_dsl_documents: IndexSet<Utf8PathBuf>,
    pub(crate) source_overrides: IndexMap<donder_language::identity::DocumentId, String>,
}

impl Loader {
    pub(super) fn new(
        source_graph: donder_package::ResolvedSourceGraph,
    ) -> Result<Self, LoadProjectError> {
        source_graph
            .validate()
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from(donder_package::MANIFEST_FILE),
                range: None,
                message: error.to_string(),
            })?;
        let project_module = source_graph
            .module(source_graph.project_module_id())
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: Utf8PathBuf::from(donder_package::MANIFEST_FILE),
                range: None,
                message: error.to_string(),
            })?;
        let entrypoint = project_module.manifest.project.as_ref().map(|project| {
            donder_language::identity::DocumentId::new(
                source_graph.project_module_id(),
                Utf8PathBuf::from(&project.entrypoint),
            )
        });
        if let Some(entrypoint) = &entrypoint {
            let absolute = project_module.root.join(entrypoint.path());
            if !absolute.is_file() {
                return Err(LoadProjectError::InvalidEntrypoint { path: absolute });
            }
        }
        Ok(Self {
            source_graph,
            entrypoint,
            documents: IndexMap::new(),
            visible_objects: IndexMap::new(),
            import_locations: IndexMap::new(),
            loading_documents: IndexSet::new(),
            definitions: ProjectDefinitionStores::default(),
            referenced_assets: Vec::new(),
            next_asset_id: 1,
            source_overrides: IndexMap::new(),
            checked_dsl_documents: IndexSet::new(),
        })
    }

    pub(super) fn source_identity(
        &self,
        document: &donder_language::identity::DocumentId,
        object: String,
    ) -> donder_language::identity::SourceIdentity {
        donder_language::identity::SourceIdentity::from_document(document.clone(), object)
    }

    pub(super) fn load(mut self) -> Result<ProjectSession, LoadProjectError> {
        let compiled = self.compile()?;
        let project = compiled
            .project
            .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
                path: compiled
                    .source
                    .project_root()
                    .join(donder_package::MANIFEST_FILE),
            })?;
        Ok(ProjectSession {
            project,
            source: compiled.source,
        })
    }

    pub(super) fn compile(&mut self) -> Result<crate::CompiledSourceGraph, LoadProjectError> {
        let mut roots = std::collections::BTreeSet::new();
        for (module_id, module) in self.source_graph.modules() {
            for export in module.manifest.exports.values() {
                for document in &export.documents {
                    roots.insert(donder_language::identity::DocumentId::new(
                        *module_id,
                        Utf8PathBuf::from(document),
                    ));
                }
            }
        }
        if let Some(entrypoint) = &self.entrypoint {
            roots.insert(entrypoint.clone());
        }
        for root in &roots {
            self.load_document(root)?;
        }
        self.build_document_scopes()?;
        self.link_generated_effects()?;

        let mut typed = if let Some(entrypoint) = self.entrypoint.clone() {
            self.resolve_project(&entrypoint)?
        } else {
            self.workspace_project(roots.iter().next().cloned().unwrap_or_else(|| {
                donder_language::identity::DocumentId::new(
                    self.source_graph.project_module_id(),
                    Utf8PathBuf::from(donder_package::MANIFEST_FILE),
                )
            }))
        };
        self.resolve_loaded_objects(&mut typed)?;
        if let Some(entrypoint) = &self.entrypoint {
            donder_language::validation::validate_project(&typed).map_err(|error| {
                LoadProjectError::InvalidDocument {
                    path: entrypoint.path().to_path_buf(),
                    range: None,
                    message: format!("project validation failed: {error:?}"),
                }
            })?;
        }
        let definitions = typed.definitions.clone();
        let project = self.entrypoint.as_ref().map(|_| typed);
        Ok(crate::CompiledSourceGraph {
            source: SourceProject {
                source_graph: self.source_graph.clone(),
                entrypoint: self.entrypoint.take(),
                documents: std::mem::take(&mut self.documents),
                referenced_assets: std::mem::take(&mut self.referenced_assets),
            },
            project,
            definitions,
        })
    }

    fn workspace_project(&self, document: donder_language::identity::DocumentId) -> DonderProject {
        let project_identity = donder_language::identity::SourceIdentity::from_document(
            document.clone(),
            "__package_compile_project".to_string(),
        );
        let setup_identity = donder_language::identity::SourceIdentity::from_document(
            document,
            "__package_compile_setup".to_string(),
        );
        DonderProject {
            root: ProjectRoot {
                id: ProjectId(project_identity),
                setup: SetupId(setup_identity),
                sequences: Vec::new(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: self.definitions.clone(),
        }
    }

    fn resolve_loaded_objects(
        &mut self,
        project: &mut DonderProject,
    ) -> Result<(), LoadProjectError> {
        // Every indexed object needs typed state, including unused objects in imported
        // documents. Saving never falls back to an unresolved original YAML value.
        let objects: Vec<_> = self
            .visible_objects
            .iter()
            .flat_map(|(document, visible)| {
                visible
                    .iter()
                    .filter(|(key, _)| matches!(key, SourceReference::Local(_)))
                    .map(|(_, object)| (document.clone(), object.clone()))
            })
            .collect();

        let active_entrypoint = self.entrypoint.clone();
        let mut resolver = DomainResolver {
            loader: self,
            project,
        };
        for (document, object) in objects {
            match object {
                ResolvedObject::Project(id) => {
                    if active_entrypoint.as_ref() != Some(&document)
                        || resolver.project.root.id != id
                    {
                        return Err(LoadProjectError::InvalidDocument {
                            path: document.path().to_path_buf(),
                            range: None,
                            message:
                                "exported project objects must be the active manifest entrypoint"
                                    .to_string(),
                        });
                    }
                }
                ResolvedObject::Setup(id) => {
                    resolver.resolve_setup(&id)?;
                }
                ResolvedObject::Controller(id) => {
                    resolver.resolve_controller(&id)?;
                }
                ResolvedObject::Layout(id) => {
                    resolver.resolve_layout(&id)?;
                }
                ResolvedObject::Patch(id) => {
                    resolver.resolve_patch(&id)?;
                }
                ResolvedObject::FixtureDefinition(id) => {
                    resolver.resolve_fixture(&id)?;
                }
                ResolvedObject::Curve(id) => {
                    resolver.resolve_curve(document.path(), &id)?;
                }
                ResolvedObject::Gradient(id) => {
                    if !resolver
                        .project
                        .definitions
                        .gradients
                        .definitions
                        .contains_key(&id)
                    {
                        return Err(missing_exported_definition(&document, &id.0));
                    }
                }
                ResolvedObject::Sequence(id) => {
                    resolver.resolve_sequence(&id)?;
                }
                ResolvedObject::EffectDefinition(id) => {
                    resolver.resolve_effect_definition(&id)?;
                }
                ResolvedObject::OperatorDefinition(id) => {
                    if !resolver
                        .project
                        .definitions
                        .operators
                        .definitions
                        .contains_key(&id)
                    {
                        return Err(missing_exported_definition(&document, &id.0));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn load_document(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
    ) -> Result<(), LoadProjectError> {
        if self.documents.contains_key(document_id) {
            return Ok(());
        }
        if self.loading_documents.contains(document_id) {
            // Import cycles are valid: each Donder document indexes its local objects
            // before resolving imports, so the active document is already visible.
            return Ok(());
        }
        self.loading_documents.insert(document_id.clone());
        let absolute = self.absolute_document_path(document_id)?;
        let result = match crate::source_document_format(document_id.path()) {
            crate::SourceDocumentFormat::Effect => {
                self.load_effect_document(document_id, &absolute)
            }
            crate::SourceDocumentFormat::Operator => {
                self.load_operator_document(document_id, &absolute)
            }
            crate::SourceDocumentFormat::Donder | crate::SourceDocumentFormat::Other => {
                self.load_donder_document(document_id, &absolute)
            }
        };
        self.loading_documents.shift_remove(document_id);
        result
    }

    pub(super) fn absolute_document_path(
        &self,
        document_id: &donder_language::identity::DocumentId,
    ) -> Result<Utf8PathBuf, LoadProjectError> {
        self.source_graph
            .module(document_id.module_id())
            .map(|module| module.root.join(document_id.path()))
            .map_err(|error| LoadProjectError::InvalidDocument {
                path: document_id.path().to_path_buf(),
                range: None,
                message: error.to_string(),
            })
    }

    pub(super) fn read_source(
        &self,
        document_id: &donder_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<String, LoadProjectError> {
        if let Some(source) = self.source_overrides.get(document_id) {
            return Ok(source.clone());
        }
        fs::read_to_string(absolute).map_err(|source| LoadProjectError::Io {
            path: absolute.to_path_buf(),
            source,
        })
    }

    pub(super) fn load_effect_document(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
        if let Ok(path) = absolute.strip_prefix(&self.source_graph.project_module().root) {
            self.checked_dsl_documents.insert(path.to_path_buf());
        }
        let compiled = compile_effect_document(&source).map_err(|diagnostics| {
            LoadProjectError::InvalidEffect {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::EffectCompile,
                        )
                    })
                    .collect(),
            }
        })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        let imports: Vec<_> = compiled
            .imports
            .iter()
            .map(|import| crate::imports::ParsedImport {
                declaration: import.declaration.clone(),
                range: Some(crate::diagnostics::byte_range(
                    &source,
                    import.span.start,
                    import.span.end,
                )),
                source_ranges: import
                    .source_spans
                    .iter()
                    .map(|span| {
                        Some(crate::diagnostics::byte_range(
                            &source, span.start, span.end,
                        ))
                    })
                    .collect(),
            })
            .collect();
        for effect in compiled.effects {
            let name = effect.effect.name().as_str().to_string();
            let id = EffectDefinitionId(self.source_identity(document_id, name.clone()));
            self.definitions
                .effects
                .insert(id.clone(), EffectDefinition::custom(id.clone(), effect));
            let source_object = SourceObjectId {
                kind: SourceObjectKind::EffectDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                SourceReference::Local(Identifier::new(name.clone()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: document_id.path().to_path_buf(),
                        range: None,
                        message: format!("invalid object name `{name}`"),
                    }
                })?),
                ResolvedObject::EffectDefinition(id),
            );
        }
        self.visible_objects.insert(document_id.clone(), visible);
        let import_edges = self.load_imports(document_id, &imports)?;
        let document =
            SourceDocument::new(import_edges, objects, SourceDocumentKind::Effect { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn load_operator_document(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let source = self.read_source(document_id, absolute)?;
        if let Ok(path) = absolute.strip_prefix(&self.source_graph.project_module().root) {
            self.checked_dsl_documents.insert(path.to_path_buf());
        }
        let compiled = compile_operators(&source).map_err(|diagnostics| {
            LoadProjectError::InvalidOperator {
                path: relative.to_path_buf(),
                diagnostics: diagnostics
                    .into_iter()
                    .map(|diagnostic| {
                        dsl_diagnostic(
                            relative,
                            &source,
                            diagnostic,
                            IoDiagnosticCode::OperatorCompile,
                        )
                    })
                    .collect(),
            }
        })?;
        let mut visible = IndexMap::new();
        let mut objects = Vec::new();
        for operator in compiled {
            let name = operator.name().as_str().to_string();
            let id = OperatorDefinitionId(self.source_identity(document_id, name.clone()));
            let definition = custom_operator_definition(id.clone(), operator);
            self.definitions.operators.insert(id.clone(), definition);
            let source_object = SourceObjectId {
                kind: SourceObjectKind::OperatorDefinition,
                id: name.clone(),
            };
            objects.push(source_object);
            visible.insert(
                SourceReference::Local(Identifier::new(name.clone()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: document_id.path().to_path_buf(),
                        range: None,
                        message: format!("invalid object name `{name}`"),
                    }
                })?),
                ResolvedObject::OperatorDefinition(id),
            );
        }
        self.visible_objects.insert(document_id.clone(), visible);
        let document =
            SourceDocument::new(Vec::new(), objects, SourceDocumentKind::Operator { source })
                .map_err(|message| LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message,
                })?;
        self.import_locations
            .insert(document_id.clone(), Vec::new());
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn load_donder_document(
        &mut self,
        document_id: &donder_language::identity::DocumentId,
        absolute: &Utf8Path,
    ) -> Result<(), LoadProjectError> {
        let relative = document_id.path();
        let text = self.read_source(document_id, absolute)?;
        let value = parse_yaml_value(relative, &text)?;
        let map = mapping(&value).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let imports = parse_imports(relative, map)?;
        let mut visible = IndexMap::new();

        let mut objects = Vec::new();
        for (key, object_value) in map {
            let Some(key) = key.as_str() else {
                return Err(LoadProjectError::InvalidDocument {
                    path: relative.to_path_buf(),
                    range: None,
                    message: "object keys must be strings".to_string(),
                });
            };
            if key == "imports" {
                continue;
            }
            Identifier::new(key.to_string()).map_err(|_| LoadProjectError::InvalidDocument {
                path: relative.to_path_buf(),
                range: source_range_for_value(relative, object_value),
                message: format!("invalid object identifier `{key}`"),
            })?;
            let object_type = string_field(relative, object_value, "type")?;
            let object = match object_type {
                "project" => ResolvedObject::Project(ProjectId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "setup" => ResolvedObject::Setup(SetupId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "controller" => ResolvedObject::Controller(ControllerId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "layout" => ResolvedObject::Layout(LayoutId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "patch" => ResolvedObject::Patch(PatchId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "fixture" => ResolvedObject::FixtureDefinition(FixtureDefinitionId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "curve" => ResolvedObject::Curve(CurveId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "gradient" => ResolvedObject::Gradient(GradientId(
                    self.source_identity(document_id, key.to_string()),
                )),
                "sequence" => ResolvedObject::Sequence(SequenceId(
                    self.source_identity(document_id, key.to_string()),
                )),
                other => {
                    return Err(LoadProjectError::InvalidDocument {
                        path: relative.to_path_buf(),
                        range: source_range_for_field_value(relative, object_value, "type"),
                        message: format!("unsupported object type `{other}`"),
                    });
                }
            };
            if let ResolvedObject::Curve(id) = &object {
                self.definitions.curves.insert(
                    id.clone(),
                    CurveDefinition {
                        curve: parse_curve(relative, object_value)?,
                    },
                );
            }
            if let ResolvedObject::Gradient(id) = &object {
                self.definitions.gradients.insert(
                    id.clone(),
                    GradientDefinition {
                        gradient: parse_gradient(relative, object_value)?,
                    },
                );
            }
            let source_object = SourceObjectId {
                kind: object.source_kind(),
                id: key.to_string(),
            };
            objects.push(source_object);
            visible.insert(
                SourceReference::Local(Identifier::new(key.to_string()).map_err(|_| {
                    LoadProjectError::InvalidDocument {
                        path: document_id.path().to_path_buf(),
                        range: None,
                        message: format!("invalid object name `{key}`"),
                    }
                })?),
                object,
            );
        }

        self.visible_objects.insert(document_id.clone(), visible);

        let import_edges = self.load_imports(document_id, &imports)?;

        let document = SourceDocument::new(
            import_edges,
            objects,
            SourceDocumentKind::Donder {
                original_value: value,
            },
        )
        .map_err(|message| LoadProjectError::InvalidDocument {
            path: relative.to_path_buf(),
            range: None,
            message,
        })?;
        self.documents.insert(document_id.clone(), document);
        Ok(())
    }

    pub(super) fn resolve_project(
        &mut self,
        entrypoint: &donder_language::identity::DocumentId,
    ) -> Result<DonderProject, LoadProjectError> {
        let root_object = self.single_project_object(entrypoint)?;
        let root_id = ProjectId(
            self.source_identity(
                entrypoint,
                Identifier::new(root_object.key.clone())
                    .map_err(|_| LoadProjectError::InvalidDocument {
                        path: entrypoint.path().to_path_buf(),
                        range: None,
                        message: "project object key is not a valid identifier".to_string(),
                    })?
                    .as_str()
                    .to_string(),
            ),
        );
        let setup = self.reference_as_setup(
            entrypoint,
            string_field(entrypoint.path(), root_object.value, "setup")?,
        )?;
        let sequences = sequence_field(entrypoint.path(), root_object.value, "sequences")?
            .iter()
            .map(|reference| self.reference_as_sequence(entrypoint, reference))
            .collect::<Result<Vec<_>, _>>()?;

        let mut project = DonderProject {
            root: ProjectRoot {
                id: root_id,
                setup: setup.clone(),
                sequences: sequences.clone(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: ProjectDefinitionStores::default(),
        };
        project.definitions = self.definitions.clone();

        {
            let mut resolver = DomainResolver {
                loader: self,
                project: &mut project,
            };
            resolver.resolve_setup(&setup)?;
            for sequence in sequences {
                resolver.resolve_sequence(&sequence)?;
            }
        }
        Ok(project)
    }

    fn single_project_object<'a>(
        &'a self,
        document_id: &donder_language::identity::DocumentId,
    ) -> Result<SourceObjectValue<'a>, LoadProjectError> {
        let path = document_id.path();
        let document = self.donder_document(document_id)?;
        let mut found = None;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        for (key, value) in document_map.iter() {
            let Some(key) = key.as_str() else {
                continue;
            };
            if key == "imports" {
                continue;
            }
            if string_field(path, value, "type")? == "project" {
                if found.is_some() {
                    return Err(LoadProjectError::InvalidDocument {
                        path: path.to_path_buf(),
                        range: source_range_for_value(path, value),
                        message: "entrypoint must contain exactly one project object".to_string(),
                    });
                }
                found = Some(SourceObjectValue {
                    key: key.to_string(),
                    value,
                });
            }
        }
        found.ok_or_else(|| LoadProjectError::InvalidDocument {
            path: path.to_path_buf(),
            range: None,
            message: "entrypoint must contain a project object".to_string(),
        })
    }

    pub(super) fn object_value(
        &self,
        id: &ResolvedObject,
    ) -> Result<(donder_language::identity::DocumentId, String, Value), LoadProjectError> {
        let identity = id.source_identity();
        if !self
            .documents
            .get(identity.document_id())
            .is_some_and(|document| {
                document
                    .objects
                    .iter()
                    .any(|object| object.kind == id.source_kind() && object.id == identity.object())
            })
        {
            return Err(LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: id.id_string(),
            });
        }
        let document = self.donder_document(identity.document_id())?;
        let document_map = mapping(document).ok_or_else(|| LoadProjectError::InvalidDocument {
            path: identity.document().to_path_buf(),
            range: None,
            message: "document root must be a mapping".to_string(),
        })?;
        let value = document_map
            .get(Value::String(identity.object().to_string()))
            .ok_or_else(|| LoadProjectError::InvalidReference {
                path: identity.document().to_path_buf(),
                range: None,
                reference: identity.object().to_string(),
            })?
            .clone();
        Ok((
            identity.document_id().clone(),
            identity.object().to_string(),
            value,
        ))
    }

    fn donder_document<'a>(
        &'a self,
        document_id: &donder_language::identity::DocumentId,
    ) -> Result<&'a Value, LoadProjectError> {
        let path = document_id.path();
        let document =
            self.documents
                .get(document_id)
                .ok_or_else(|| LoadProjectError::InvalidDocument {
                    path: path.to_path_buf(),
                    range: None,
                    message: "document not loaded".to_string(),
                })?;
        match &document.kind {
            SourceDocumentKind::Donder { original_value } => Ok(original_value),
            SourceDocumentKind::Effect { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Donder document".to_string(),
            }),
            SourceDocumentKind::Operator { .. } => Err(LoadProjectError::InvalidDocument {
                path: path.to_path_buf(),
                range: None,
                message: "expected YAML Donder document".to_string(),
            }),
        }
    }

    pub(super) fn reference_as_setup(
        &self,
        document_id: &donder_language::identity::DocumentId,
        reference: &str,
    ) -> Result<SetupId, LoadProjectError> {
        let path = document_id.path();
        match self.resolve_reference(document_id, reference)? {
            ResolvedObject::Setup(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
                reference: reference.to_string(),
            }),
        }
    }

    pub(super) fn reference_as_sequence(
        &self,
        document_id: &donder_language::identity::DocumentId,
        reference: &str,
    ) -> Result<SequenceId, LoadProjectError> {
        let path = document_id.path();
        match self.resolve_reference(document_id, reference)? {
            ResolvedObject::Sequence(id) => Ok(id),
            _ => Err(LoadProjectError::InvalidReference {
                path: path.to_path_buf(),
                range: source_range_for_scalar(path, reference),
                reference: reference.to_string(),
            }),
        }
    }
}

fn missing_exported_definition(
    document: &donder_language::identity::DocumentId,
    identity: &donder_language::identity::SourceIdentity,
) -> LoadProjectError {
    LoadProjectError::InvalidReference {
        path: document.path().to_path_buf(),
        range: None,
        reference: identity.object().to_string(),
    }
}

use std::fs;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::controller::ControllerId;
use donder_language::dsl::{Identifier, compile_effect_document, compile_operators};
use donder_language::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use donder_language::model::{DonderProject, ProjectDefinitionStores, ProjectId, ProjectRoot};
use donder_language::operator::{OperatorDefinitionId, custom_operator_definition};
use donder_language::patch::PatchId;
use donder_language::sequence::SequenceId;
use donder_language::setup::SetupId;
use indexmap::{IndexMap, IndexSet};
use yaml_serde::Value;

use crate::diagnostics::{
    dsl_diagnostic, parse_yaml_value, source_range_for_field_value, source_range_for_scalar,
    source_range_for_value,
};
use crate::source::{
    ProjectSession, ReferencedAsset, SourceDocument, SourceDocumentKind, SourceObjectId,
    SourceObjectKind, SourceProject,
};
use crate::{IoDiagnosticCode, LoadProjectError};
