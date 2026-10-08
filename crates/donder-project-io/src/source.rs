use camino::{Utf8Path, Utf8PathBuf};
pub use donder_language::ImportSource;
use donder_model::AssetId;
use donder_model::DocumentId;
use donder_model::DonderProject;
use donder_runtime_types::Identifier;
use indexmap::{IndexMap, IndexSet};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceDocumentFormat {
    /// `*.data.donder`: declarations the GUI edits.
    Data,
    /// `*.donder`: effects and operators.
    Script,
    Other,
}

pub fn source_document_format(path: &Utf8Path) -> SourceDocumentFormat {
    let Some(file_name) = path.file_name() else {
        return SourceDocumentFormat::Other;
    };
    if file_name.ends_with(donder_language::data::DATA_DOCUMENT_SUFFIX) {
        SourceDocumentFormat::Data
    } else if file_name.ends_with(donder_language::data::SCRIPT_SUFFIX) {
        SourceDocumentFormat::Script
    } else {
        SourceDocumentFormat::Other
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectSession {
    pub project: DonderProject,
    pub source: SourceProject,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceProject {
    pub workspace: crate::ProjectWorkspace,
    pub entrypoint: Option<DocumentId>,
    pub documents: IndexMap<DocumentId, SourceDocument>,
    pub referenced_assets: Vec<ReferencedAsset>,
}

impl SourceProject {
    /// Register a new project-owned data document and its typed object inventory.
    /// The caller inserts the corresponding typed values into the same candidate session.
    pub fn add_data_document(
        &mut self,
        path: Utf8PathBuf,
        objects: Vec<(SourceObjectKind, String)>,
    ) -> Result<Vec<donder_model::SourceIdentity>, String> {
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
        let source = SourceDocument::new(Vec::new(), source_objects, SourceDocumentKind::Data)?;
        let identities = objects
            .into_iter()
            .map(|(_, key)| donder_model::SourceIdentity::from_document(document.clone(), key))
            .collect();
        self.documents.insert(document, source);
        Ok(identities)
    }

    /// Register a new named object in an existing project-owned data document.
    /// The caller inserts its typed value into the same candidate session.
    pub fn add_object(
        &mut self,
        document: &DocumentId,
        kind: SourceObjectKind,
        prefix: &str,
    ) -> Result<donder_model::SourceIdentity, String> {
        if !self.is_project_owned(document) {
            return Err("New objects require a project-owned document.".to_string());
        }
        Identifier::new(prefix.to_string())
            .map_err(|_| "Invalid source object prefix.".to_string())?;
        let (_, document, source) = self
            .documents
            .get_full_mut(document)
            .ok_or_else(|| "Source document was not found.".to_string())?;
        if !matches!(source.kind, SourceDocumentKind::Data)
            || matches!(
                kind,
                SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
            )
        {
            return Err("This object requires a data document.".to_string());
        }
        let key = (1_u32..)
            .map(|index| {
                if index == 1 {
                    prefix.to_owned()
                } else {
                    format!("{prefix}_{index}")
                }
            })
            .find(|key| source.objects.iter().all(|object| object.id() != key))
            .ok_or_else(|| "No source object identifiers remain.".to_string())?;
        source.objects.push(SourceObjectId::new(kind, key.clone())?);
        Ok(donder_model::SourceIdentity::from_document(
            document.clone(),
            key,
        ))
    }

    pub fn project_module_id(&self) -> Uuid {
        self.workspace.metadata.project_id
    }
    pub fn project_root(&self) -> &Utf8Path {
        &self.workspace.root
    }
    pub fn absolute_path(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        self.is_project_owned(document)
            .then(|| self.workspace.root.join(document.path()))
    }
    pub fn workspace_module_for_path(&self, path: &Utf8Path) -> Option<(Uuid, Utf8PathBuf)> {
        crate::validate_relative_path(path.as_str()).ok()?;
        Some((self.project_module_id(), path.to_owned()))
    }
    pub fn document_for_workspace_path(&self, path: &Utf8Path) -> Option<DocumentId> {
        let (id, path) = self.workspace_module_for_path(path)?;
        let document = DocumentId::new(id, path);
        self.documents.contains_key(&document).then_some(document)
    }
    pub fn workspace_path_for_document(&self, document: &DocumentId) -> Option<Utf8PathBuf> {
        self.is_project_owned(document)
            .then(|| document.path().to_owned())
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
        document.module_id() == self.project_module_id()
    }
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
        if !imports.is_empty() && !matches!(kind, SourceDocumentKind::Data) {
            return Err("Only data documents can declare imports.".into());
        }
        let mut object_ids = IndexSet::new();
        for object in &objects {
            if Identifier::new(object.id.clone()).is_err() {
                return Err(format!("invalid source object identifier `{}`", object.id));
            }
            if !object_ids.insert(object.id.clone()) {
                return Err(format!("duplicate source object `{}`", object.id));
            }
            let kind_matches_document = match &kind {
                SourceDocumentKind::Script { .. } => matches!(
                    object.kind,
                    SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
                ),
                SourceDocumentKind::Data => !matches!(
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
    Data,
    /// Scripts keep their source text exactly; the GUI never rewrites code.
    Script {
        source: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportEdge {
    pub(crate) declaration: donder_language::ImportDeclaration,
    pub(crate) targets: Vec<DocumentId>,
}

impl ImportEdge {
    pub fn declaration(&self) -> &donder_language::ImportDeclaration {
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

impl SourceObjectKind {
    /// The kind of an owned child at this slot; source references do not add slots.
    pub fn owned_child_kind(&self, slot: &donder_model::OwnedObjectSlot) -> Option<Self> {
        use donder_model::OwnedObjectSlot;
        match (self, slot) {
            (Self::Project, OwnedObjectSlot::Setup) => Some(Self::Setup),
            (Self::Project, OwnedObjectSlot::Sequence(_)) => Some(Self::Sequence),
            (Self::Setup, OwnedObjectSlot::Layout) => Some(Self::Layout),
            (Self::Setup, OwnedObjectSlot::Patch) => Some(Self::Patch),
            (Self::Setup, OwnedObjectSlot::Controller(_)) => Some(Self::Controller),
            (Self::Layout, OwnedObjectSlot::Fixture(_)) => Some(Self::FixtureDefinition),
            _ => None,
        }
    }
}

impl ProjectSession {
    /// Check an owned address against the typed tree, never against source names.
    pub fn owned_object_exists(
        &self,
        kind: &SourceObjectKind,
        identity: &donder_model::ObjectIdentity,
    ) -> bool {
        use donder_model::{
            ControllerId, FixtureInstanceId, FixtureSource, LayoutFixtureKind, LayoutId,
            OwnedObjectSlot, PatchId, SequenceId, SetupId,
        };
        if identity.source().is_some() {
            return false;
        }
        match kind {
            SourceObjectKind::Setup => self.project.setup(&SetupId(identity.clone())).is_some(),
            SourceObjectKind::Layout => self.project.layout(&LayoutId(identity.clone())).is_some(),
            SourceObjectKind::Patch => self.project.patch(&PatchId(identity.clone())).is_some(),
            SourceObjectKind::Controller => self
                .project
                .controller(&ControllerId(identity.clone()))
                .is_some(),
            SourceObjectKind::Sequence => self
                .project
                .sequence(&SequenceId(identity.clone()))
                .is_some(),
            SourceObjectKind::FixtureDefinition => {
                let Some(OwnedObjectSlot::Fixture(id)) = identity.owned_path().last() else {
                    return false;
                };
                identity
                    .parent()
                    .and_then(|parent| self.project.layout(&LayoutId(parent)))
                    .and_then(|layout| layout.fixture(FixtureInstanceId(*id)))
                    .is_some_and(|fixture| {
                        matches!(
                            fixture.kind,
                            LayoutFixtureKind::Fixture {
                                definition: FixtureSource::Inline(_),
                                ..
                            }
                        )
                    })
            }
            _ => false,
        }
    }
}
