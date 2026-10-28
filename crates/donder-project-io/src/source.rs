use crate::address::{DeclaredObject, DocumentId, SourceIdentity};
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::AssetId;
use donder_model::DonderProject;
use donder_model::ObjectId;
use donder_model::ObjectKind;
use donder_runtime_types::Identifier;
use indexmap::{IndexMap, IndexSet};
use std::collections::BTreeSet;

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
        objects: Vec<(ObjectId, Identifier)>,
    ) -> Result<DocumentId, String> {
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
        let objects = objects
            .into_iter()
            .map(|(id, name)| DeclaredObject::new(id, name))
            .collect();
        let source = SourceDocument::data(Vec::new(), objects)?;
        self.documents.insert(document.clone(), source);
        Ok(document)
    }

    /// Register a new named object in an existing project-owned data document.
    /// The caller inserts its typed value into the same candidate session.
    pub fn add_object(
        &mut self,
        document: &DocumentId,
        id: ObjectId,
        prefix: &str,
    ) -> Result<SourceIdentity, String> {
        Identifier::new(prefix.to_string())
            .map_err(|_| "Invalid source object prefix.".to_string())?;
        let (_, document, source) = self
            .documents
            .get_full_mut(document)
            .ok_or_else(|| "Source document was not found.".to_string())?;
        if source.format() != SourceDocumentFormat::Data
            || matches!(
                id.kind(),
                ObjectKind::EffectDefinition | ObjectKind::OperatorDefinition
            )
        {
            return Err("This object requires a data document.".to_string());
        }
        let name = (1_u32..)
            .map(|index| {
                if index == 1 {
                    prefix.to_owned()
                } else {
                    format!("{prefix}_{index}")
                }
            })
            .find(|key| {
                source
                    .objects
                    .iter()
                    .all(|object| object.name.as_str() != key)
            })
            .and_then(|key| Identifier::new(key).ok())
            .ok_or_else(|| "No source object identifiers remain.".to_string())?;
        source.objects.push(DeclaredObject::new(id, name.clone()));
        Ok(SourceIdentity::from_document(
            document.clone(),
            name.as_str().to_string(),
        ))
    }

    pub fn project_root(&self) -> &Utf8Path {
        &self.workspace.root
    }
    pub fn absolute_path(&self, document: &DocumentId) -> Utf8PathBuf {
        self.workspace.root.join(document.path())
    }
    pub fn document_for_workspace_path(&self, path: &Utf8Path) -> Option<DocumentId> {
        crate::validate_relative_path(path.as_str()).ok()?;
        let document = DocumentId::new(path.to_owned());
        self.documents.contains_key(&document).then_some(document)
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
            .map(DocumentId::path)
            .any(|document_path| {
                document_path == path
                    || document_path
                        .strip_prefix(path)
                        .is_ok_and(|suffix| !suffix.as_str().is_empty())
            })
    }

    pub fn project_document(&self, path: Utf8PathBuf) -> DocumentId {
        DocumentId::new(path)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SourceDocument {
    pub(crate) imports: Vec<ImportEdge>,
    pub(crate) objects: Vec<DeclaredObject>,
    /// A script's text, kept exactly; the GUI never rewrites code. A data
    /// document has none: saving prints it from typed state.
    pub(crate) script: Option<String>,
}

impl SourceDocument {
    /// A data document with its imports and the objects it declares.
    pub fn data(imports: Vec<ImportEdge>, objects: Vec<DeclaredObject>) -> Result<Self, String> {
        Self::new(imports, objects, None)
    }

    /// A script with its text and the definitions it declares.
    pub fn script(objects: Vec<DeclaredObject>, source: String) -> Result<Self, String> {
        Self::new(Vec::new(), objects, Some(source))
    }

    fn new(
        imports: Vec<ImportEdge>,
        objects: Vec<DeclaredObject>,
        script: Option<String>,
    ) -> Result<Self, String> {
        let mut object_ids = IndexSet::new();
        for object in &objects {
            if !object_ids.insert(object.name.clone()) {
                return Err(format!(
                    "duplicate source object `{}`",
                    object.name.as_str()
                ));
            }
            let definition = matches!(
                object.kind(),
                ObjectKind::EffectDefinition | ObjectKind::OperatorDefinition
            );
            if definition != script.is_some() {
                return Err(format!(
                    "source object `{}` is not valid in this document kind",
                    object.name.as_str()
                ));
            }
        }
        Ok(Self {
            imports,
            objects,
            script,
        })
    }

    pub fn imports(&self) -> &[ImportEdge] {
        &self.imports
    }

    pub fn objects(&self) -> &[DeclaredObject] {
        &self.objects
    }

    /// `Data` or `Script`.
    pub fn format(&self) -> SourceDocumentFormat {
        match self.script {
            Some(_) => SourceDocumentFormat::Script,
            None => SourceDocumentFormat::Data,
        }
    }

    /// A script's text.
    pub fn script_source(&self) -> Option<&str> {
        self.script.as_deref()
    }
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

    pub fn documents(&self) -> &[Utf8PathBuf] {
        &self.declaration.documents
    }

    pub fn targets(&self) -> &[DocumentId] {
        &self.targets
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferencedAsset {
    pub id: AssetId,
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

impl ProjectSession {
    /// The asset a sequence's audio names.
    pub fn audio_asset(&self, audio: &donder_model::SequenceAudio) -> Option<&ReferencedAsset> {
        let donder_model::SequenceAudio::Asset(id) = audio else {
            return None;
        };
        self.source
            .referenced_assets
            .iter()
            .find(|asset| asset.id == *id)
    }
}
