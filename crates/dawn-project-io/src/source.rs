use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::identity::DocumentId;
pub use dawn_language::imports::ImportSource;
use dawn_language::model::DawnProject;
use dawn_language::sequence::AssetId;
use indexmap::{IndexMap, IndexSet};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;
use yaml_serde::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSession {
    pub project: DawnProject,
    pub source: SourceProject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceProject {
    pub source_graph: dawn_package::ResolvedSourceGraph,
    pub entrypoint: Option<DocumentId>,
    pub documents: IndexMap<DocumentId, SourceDocument>,
    pub referenced_assets: Vec<ReferencedAsset>,
}

impl SourceProject {
    /// Copy dependency export imports into a new project-owned document so it can
    /// preserve references to dependency objects already visible to its owner.
    pub fn inherit_dependency_imports(
        &mut self,
        from: &DocumentId,
        to: &DocumentId,
    ) -> Result<(), String> {
        if !self.is_project_owned(to) {
            return Err("Import inheritance requires a project-owned target document.".into());
        }
        let imports = self
            .documents
            .get(from)
            .ok_or_else(|| "Import source document was not found.".to_string())?
            .imports
            .iter()
            .filter(|edge| {
                edge.targets
                    .iter()
                    .any(|target| target.module_id() != from.module_id())
            })
            .cloned()
            .collect::<Vec<_>>();
        let target = self
            .documents
            .get_mut(to)
            .ok_or_else(|| "Import target document was not found.".to_string())?;
        for edge in imports {
            if !target
                .imports
                .iter()
                .any(|existing| existing.declaration == edge.declaration)
            {
                target.imports.push(edge);
            }
        }
        Ok(())
    }

    /// Register a new project-owned YAML document and its typed object inventory.
    /// The caller inserts the corresponding typed values into the same candidate session.
    pub fn add_yaml_document(
        &mut self,
        path: Utf8PathBuf,
        objects: Vec<(SourceObjectKind, String)>,
    ) -> Result<Vec<dawn_language::identity::SourceIdentity>, String> {
        if path.as_str().is_empty()
            || path.is_absolute()
            || path.as_str().contains('\\')
            || !path
                .components()
                .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
        {
            return Err("New document paths must be module-relative paths.".to_string());
        }
        if objects.is_empty() {
            return Err("A new source document must contain an object.".to_string());
        }
        let document = self.project_document(path.clone());
        if self.documents.contains_key(&document) || self.project_root().join(&path).exists() {
            return Err("Source document already exists.".to_string());
        }
        let source_objects = objects
            .iter()
            .map(|(kind, key)| SourceObjectId::new(kind.clone(), key.clone()))
            .collect::<Result<Vec<_>, _>>()?;
        let source = SourceDocument::new(
            Vec::new(),
            source_objects,
            SourceDocumentKind::Dawn {
                original_value: Value::Mapping(yaml_serde::Mapping::new()),
            },
        )?;
        let identities = objects
            .into_iter()
            .map(|(_, key)| {
                dawn_language::identity::SourceIdentity::from_document(document.clone(), key)
            })
            .collect();
        self.documents.insert(document, source);
        Ok(identities)
    }

    /// Register a new named object in an existing project-owned YAML document.
    /// The caller inserts its typed value into the same candidate session.
    pub fn add_object(
        &mut self,
        document: &DocumentId,
        kind: SourceObjectKind,
        prefix: &str,
    ) -> Result<dawn_language::identity::SourceIdentity, String> {
        if !self.is_project_owned(document) {
            return Err("New objects require a project-owned document.".to_string());
        }
        Identifier::new(prefix.to_string())
            .map_err(|_| "Invalid source object prefix.".to_string())?;
        let (_, document, source) = self
            .documents
            .get_full_mut(document)
            .ok_or_else(|| "Source document was not found.".to_string())?;
        if !matches!(source.kind, SourceDocumentKind::Dawn { .. })
            || matches!(
                kind,
                SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
            )
        {
            return Err("This object requires a YAML source document.".to_string());
        }
        let key = (1_u32..)
            .map(|index| format!("{prefix}_{index}"))
            .find(|key| source.objects.iter().all(|object| object.id() != key))
            .ok_or_else(|| "No source object identifiers remain.".to_string())?;
        source.objects.push(SourceObjectId::new(kind, key.clone())?);
        Ok(dawn_language::identity::SourceIdentity::from_document(
            document.clone(),
            key,
        ))
    }

    pub fn project_module_id(&self) -> Uuid {
        self.source_graph.project_module_id()
    }

    pub fn project_root(&self) -> &Utf8Path {
        self.source_graph.project_module().root.as_path()
    }

    pub fn module(&self, module_id: Uuid) -> Option<&dawn_package::ResolvedModule> {
        self.source_graph.module(module_id).ok()
    }

    pub fn ownership(&self, document: &DocumentId) -> Option<SourceOwnership> {
        self.module(document.module_id())
            .map(|module| match &module.origin {
                dawn_package::ResolvedModuleOrigin::Project => SourceOwnership::ProjectOwned,
                dawn_package::ResolvedModuleOrigin::PathDependency { declared_path, .. } => {
                    SourceOwnership::PathDependencyOwned {
                        declared_path: declared_path.clone(),
                        module_id: document.module_id(),
                    }
                }
                dawn_package::ResolvedModuleOrigin::RegistryDependency { package, .. } => {
                    SourceOwnership::RegistryReadOnly {
                        package: package.as_str().to_string(),
                        module_id: document.module_id(),
                    }
                }
            })
    }

    pub fn absolute_path(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        self.module(document.module_id())
            .map(|module| module.root.join(document.path()))
    }

    pub fn workspace_module_for_path(
        &self,
        relative_path: &Utf8Path,
    ) -> Option<(Uuid, Utf8PathBuf)> {
        workspace_module_for_path(&self.source_graph, relative_path)
    }

    pub fn document_for_workspace_path(&self, relative_path: &Utf8Path) -> Option<DocumentId> {
        let (module_id, module_relative) = self.workspace_module_for_path(relative_path)?;
        let document_id = DocumentId::new(module_id, module_relative);
        self.documents
            .contains_key(&document_id)
            .then_some(document_id)
    }

    pub fn workspace_path_for_document(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        let module = self.module(document.module_id())?;
        if matches!(
            module.origin,
            dawn_package::ResolvedModuleOrigin::RegistryDependency { .. }
        ) {
            return None;
        }
        let absolute = module.root.join(document.path());
        let relative = absolute.strip_prefix(self.project_root()).ok()?;
        Some(relative.to_path_buf())
    }

    pub fn is_structural_workspace_path(&self, path: &Utf8Path) -> bool {
        self.entrypoint
            .iter()
            .chain(
                self.documents
                    .values()
                    .flat_map(|document| document.imports().iter())
                    .flat_map(|edge| edge.targets().iter()),
            )
            .filter_map(|document_id| self.workspace_path_for_document(document_id))
            .any(|document_path| {
                document_path == path
                    || document_path
                        .strip_prefix(path)
                        .is_ok_and(|suffix| !suffix.as_str().is_empty())
            })
    }

    pub fn project_document(&self, path: Utf8PathBuf) -> DocumentId {
        DocumentId::new(self.project_module_id(), path)
    }

    pub fn is_project_owned(&self, document: &DocumentId) -> bool {
        self.ownership(document) == Some(SourceOwnership::ProjectOwned)
    }

    pub fn is_editable(&self, document: &DocumentId) -> bool {
        matches!(
            self.ownership(document),
            Some(SourceOwnership::ProjectOwned | SourceOwnership::PathDependencyOwned { .. })
        )
    }
}

pub(crate) fn workspace_module_for_path(
    graph: &dawn_package::ResolvedSourceGraph,
    relative_path: &Utf8Path,
) -> Option<(Uuid, Utf8PathBuf)> {
    let absolute = graph.project_module().root.join(relative_path);
    let (module_id, module) = graph
        .modules()
        .iter()
        .filter(|(_, module)| {
            !matches!(
                module.origin,
                dawn_package::ResolvedModuleOrigin::RegistryDependency { .. }
            ) && absolute.starts_with(&module.root)
        })
        .max_by_key(|(_, module)| module.root.components().count())?;
    let module_relative = absolute.strip_prefix(&module.root).ok()?;
    Some((*module_id, module_relative.to_path_buf()))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SourceOwnership {
    ProjectOwned,
    PathDependencyOwned {
        declared_path: String,
        module_id: Uuid,
    },
    RegistryReadOnly {
        package: String,
        module_id: Uuid,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SourceObjectId {
    pub(crate) kind: SourceObjectKind,
    pub(crate) id: String,
}

impl SourceObjectId {
    pub fn new(kind: SourceObjectKind, id: String) -> Result<Self, String> {
        Identifier::new(id.clone())
            .map_err(|_| format!("invalid source object identifier `{id}`"))?;
        Ok(Self { kind, id })
    }

    pub fn kind(&self) -> &SourceObjectKind {
        &self.kind
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum SourceObjectKind {
    Project,
    Setup,
    Controller,
    Layout,
    Patch,
    FixtureDefinition,
    Curve,
    Gradient,
    Sequence,
    EffectDefinition,
    OperatorDefinition,
    EffectInstance,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceDocument {
    pub(crate) imports: Vec<ImportEdge>,
    pub(crate) objects: Vec<SourceObjectId>,
    pub(crate) kind: SourceDocumentKind,
}

impl SourceDocument {
    pub fn new(
        imports: Vec<ImportEdge>,
        objects: Vec<SourceObjectId>,
        kind: SourceDocumentKind,
    ) -> Result<Self, String> {
        let mut object_ids = IndexSet::new();
        for object in &objects {
            if Identifier::new(object.id.clone()).is_err() {
                return Err(format!("invalid source object identifier `{}`", object.id));
            }
            if !object_ids.insert(object.id.clone()) {
                return Err(format!("duplicate source object `{}`", object.id));
            }
            let kind_matches_document = match &kind {
                SourceDocumentKind::Effect { .. } => {
                    object.kind == SourceObjectKind::EffectDefinition
                }
                SourceDocumentKind::Operator { .. } => {
                    object.kind == SourceObjectKind::OperatorDefinition
                }
                SourceDocumentKind::Dawn { .. } => !matches!(
                    object.kind,
                    SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
                ),
            };
            if !kind_matches_document {
                return Err(format!(
                    "source object `{}` is not valid in this document kind",
                    object.id
                ));
            }
        }
        Ok(Self {
            imports,
            objects,
            kind,
        })
    }

    pub fn imports(&self) -> &[ImportEdge] {
        &self.imports
    }

    pub fn objects(&self) -> &[SourceObjectId] {
        &self.objects
    }

    pub fn kind(&self) -> &SourceDocumentKind {
        &self.kind
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SourceDocumentKind {
    Dawn { original_value: Value },
    Effect { source: String },
    Operator { source: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEdge {
    pub(crate) declaration: dawn_language::imports::ImportDeclaration,
    pub(crate) targets: Vec<DocumentId>,
}

impl ImportEdge {
    pub fn declaration(&self) -> &dawn_language::imports::ImportDeclaration {
        &self.declaration
    }

    pub fn alias(&self) -> &str {
        self.declaration.alias.as_str()
    }

    pub fn source(&self) -> &ImportSource {
        &self.declaration.source
    }

    pub fn targets(&self) -> &[DocumentId] {
        &self.targets
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedAsset {
    pub id: AssetId,
    pub module_id: Uuid,
    pub relative_path: Utf8PathBuf,
    pub absolute_path: Utf8PathBuf,
    pub referenced_by: BTreeSet<DocumentId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportReport {
    pub written_files: Vec<Utf8PathBuf>,
    pub copied_assets: Vec<Utf8PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SaveReport {
    pub written_files: Vec<Utf8PathBuf>,
}

pub fn source_file_list(session: &ProjectSession) -> BTreeMap<DocumentId, Vec<String>> {
    session
        .source
        .documents
        .iter()
        .map(|(path, document)| {
            (
                path.clone(),
                document
                    .objects
                    .iter()
                    .map(|object| object.id.clone())
                    .collect(),
            )
        })
        .collect()
}
