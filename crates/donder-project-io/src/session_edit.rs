//! The ways a session changes. An edit works on a candidate, which the
//! session takes only when its documents still declare exactly the project's
//! reusable objects, each once; the documents then import what the project's
//! references need and record the assets each one uses.
use crate::address::{DocumentId, SourceIdentity};
use crate::source::{AssetError, ProjectSession};
use camino::Utf8PathBuf;
use donder_model::{AssetId, DonderProject, EditError, Object, ObjectId, ProjectEdit};
use donder_runtime_types::Identifier;
use std::collections::HashSet;

/// Why a session edit was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum SessionEditError {
    /// The typed project refused it.
    Model(EditError),
    /// The documents refused it: a file or name is taken, or they cannot
    /// write what the project now holds.
    Source(String),
}

impl From<EditError> for SessionEditError {
    fn from(error: EditError) -> Self {
        Self::Model(error)
    }
}

impl std::fmt::Display for SessionEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(error) => error.fmt(f),
            Self::Source(message) => f.write_str(message),
        }
    }
}

/// A candidate session: its typed project, and the documents declaring the
/// project's reusable objects, change together.
pub struct SessionEdit<'s>(&'s mut ProjectSession);

impl SessionEdit<'_> {
    pub fn session(&self) -> &ProjectSession {
        self.0
    }

    pub fn project_mut(&mut self) -> &mut DonderProject {
        &mut self.0.project
    }

    /// Declare `id` in a file of its own as `name`; refused when that file
    /// exists.
    pub fn add_object_document(
        &mut self,
        id: ObjectId,
        name: Identifier,
    ) -> Result<DocumentId, SessionEditError> {
        self.0
            .source
            .add_object_document(id, name)
            .map_err(SessionEditError::Source)
    }

    /// Declare `objects` in a new data document at `path`.
    pub fn add_data_document(
        &mut self,
        path: Utf8PathBuf,
        objects: Vec<(ObjectId, Identifier)>,
    ) -> Result<DocumentId, SessionEditError> {
        self.0
            .source
            .add_data_document(path, objects)
            .map_err(SessionEditError::Source)
    }

    /// Declare `id` in `document` as `prefix`, numbered on a clash.
    pub fn add_object(
        &mut self,
        document: &DocumentId,
        id: ObjectId,
        prefix: &str,
    ) -> Result<SourceIdentity, SessionEditError> {
        self.0
            .source
            .add_object(document, id, prefix)
            .map_err(SessionEditError::Source)
    }

    /// Remove the reusable object `id` and its declaration.
    pub fn remove_object(&mut self, id: ObjectId) -> Result<(), SessionEditError> {
        self.0.project.apply_edits([ProjectEdit::Remove(id)])?;
        self.0.source.remove_object(id);
        Ok(())
    }
}

impl ProjectSession {
    /// Apply `edit`, then check that the documents declare exactly the
    /// project's reusable objects. The session is the caller's candidate: a
    /// refused edit may leave it part edited, and the caller drops it.
    pub fn edit<R, E: From<SessionEditError>>(
        &mut self,
        edit: impl FnOnce(&mut SessionEdit<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        let result = edit(&mut SessionEdit(self))?;
        if !self.declares_reusable_objects() {
            return Err(SessionEditError::Source(
                "The edit added or removed an object without its declaration.".into(),
            )
            .into());
        }
        crate::ownership_edit::maintain_ownership_sources(self)
            .map_err(|error| SessionEditError::Source(error.to_string()))?;
        Ok(result)
    }

    /// Edit the typed project alone; the edit keeps its reusable objects.
    pub fn edit_project<R, E: From<SessionEditError>>(
        &mut self,
        edit: impl FnOnce(&mut DonderProject) -> Result<R, E>,
    ) -> Result<R, E> {
        self.edit(|session| edit(session.project_mut()))
    }

    /// Write one object in place of the object with its id.
    pub fn put(&mut self, object: impl Into<Object>) -> Result<(), SessionEditError> {
        let object = object.into();
        self.edit_project(|project| Ok(project.put(object)?))
    }

    /// Apply `edits` to the typed project as one edit; they keep its reusable
    /// objects.
    pub fn apply_edits(
        &mut self,
        edits: impl IntoIterator<Item = ProjectEdit>,
    ) -> Result<(), SessionEditError> {
        self.edit_project(|project| Ok(project.apply_edits(edits)?))
    }

    /// A fresh id for a new object.
    pub fn new_id(&mut self) -> u32 {
        self.project.new_id()
    }

    /// Give the project `derived` for `derivation`; its objects stay as they
    /// are.
    pub fn with_derived(
        &mut self,
        derivation: donder_model::Derivation,
        derived: donder_model::Derived,
    ) -> Result<(), donder_model::ProjectValidationError> {
        self.project = self.project.with_derived(derivation, derived)?;
        Ok(())
    }

    /// The repairs edits made since they were last taken.
    pub fn take_repairs(&mut self) -> Vec<donder_model::Repair> {
        self.project.take_repairs()
    }

    /// Declare `id` as `name`; refused when its document already declares
    /// that name. References print the new name when their documents are saved.
    pub fn rename_object(&mut self, id: ObjectId, name: Identifier) -> Result<(), String> {
        self.source.rename_object(id, name)
    }

    /// Reference the file at `path`, relative to the project root, from
    /// `document`. An asset already referenced at that path gains the
    /// document; a new one takes the next id.
    pub fn reference_asset(
        &mut self,
        document: &DocumentId,
        path: &str,
    ) -> Result<AssetId, AssetError> {
        let root = self.source.project_root().to_path_buf();
        crate::source::reference_asset(&mut self.source.referenced_assets, &root, document, path)
    }

    /// Whether the documents declare exactly the project's reusable objects,
    /// each once.
    fn declares_reusable_objects(&self) -> bool {
        let mut reusable = reusable_objects(&self.project).collect::<HashSet<_>>();
        self.source
            .documents
            .values()
            .flat_map(|document| document.objects())
            .all(|object| reusable.remove(&object.id()))
            && reusable.is_empty()
    }
}

/// Every reusable object of `project`, and its root.
fn reusable_objects(project: &DonderProject) -> impl Iterator<Item = ObjectId> + '_ {
    fn ids<T: donder_model::Reusable>(
        project: &DonderProject,
    ) -> impl Iterator<Item = ObjectId> + '_ {
        project.reusable::<T>().keys().map(|&id| id.into())
    }
    std::iter::once(project.root().id.into())
        .chain(ids::<donder_model::Setup>(project))
        .chain(ids::<donder_model::Controller>(project))
        .chain(ids::<donder_model::InputDevice>(project))
        .chain(ids::<donder_model::Layout>(project))
        .chain(ids::<donder_model::Patch>(project))
        .chain(ids::<donder_model::Sequence>(project))
        .chain(ids::<donder_model::Console>(project))
        .chain(ids::<donder_model::FixtureDefinition>(project))
        .chain(ids::<donder_model::CurveDefinition>(project))
        .chain(ids::<donder_model::GradientDefinition>(project))
        .chain(ids::<donder_model::EffectDefinition>(project))
        .chain(ids::<donder_model::OperatorDefinition>(project))
}
