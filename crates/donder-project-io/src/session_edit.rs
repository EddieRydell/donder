//! The ways a session changes. An edit works on a candidate, which the
//! session takes only when its documents still declare exactly the project's
//! objects, each once; the documents then import what the project's
//! references need and record the assets each one uses.
use crate::ExportProjectError;
use crate::address::{DocumentId, SourceIdentity};
use crate::source::{AssetError, ProjectSession};
use camino::Utf8PathBuf;
use donder_model::{AssetId, DonderProject, EditError, Object, ObjectId, ProjectEdit};
use donder_runtime_types::Identifier;
use std::collections::{BTreeMap, BTreeSet, HashSet};

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
/// project's objects, change together.
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

    /// Remove the object `id` and its declaration.
    pub fn remove_object(&mut self, id: ObjectId) -> Result<(), SessionEditError> {
        self.0.project.apply_edits([ProjectEdit::Remove(id)])?;
        self.0.source.remove_object(id);
        Ok(())
    }
}

impl ProjectSession {
    /// Apply `edit`, then check that the documents declare exactly the
    /// project's objects. The session is the caller's candidate: a
    /// refused edit may leave it part edited, and the caller drops it.
    pub fn edit<R, E: From<SessionEditError>>(
        &mut self,
        edit: impl FnOnce(&mut SessionEdit<'_>) -> Result<R, E>,
    ) -> Result<R, E> {
        let result = edit(&mut SessionEdit(self))?;
        if !self.declares_its_objects() {
            return Err(SessionEditError::Source(
                "The edit added or removed an object without its declaration.".into(),
            )
            .into());
        }
        maintain_sources(self).map_err(|error| SessionEditError::Source(error.to_string()))?;
        Ok(result)
    }

    /// Edit the typed project alone; the edit keeps its objects.
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

    /// Apply `edits` to the typed project as one edit; they keep its
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

    /// Whether the documents declare exactly the project's objects,
    /// each once.
    fn declares_its_objects(&self) -> bool {
        let mut objects = project_objects(&self.project).collect::<HashSet<_>>();
        self.source
            .documents
            .values()
            .flat_map(|document| document.objects())
            .all(|object| objects.remove(&object.id()))
            && objects.is_empty()
    }
}

/// Every object of `project`, and its root.
fn project_objects(project: &DonderProject) -> impl Iterator<Item = ObjectId> + '_ {
    fn ids<T: donder_model::ProjectObject>(
        project: &DonderProject,
    ) -> impl Iterator<Item = ObjectId> + '_ {
        project.reusable::<T>().keys().map(|&id| id.into())
    }
    std::iter::once(project.root().id.into())
        .chain(ids::<donder_model::Controller>(project))
        .chain(ids::<donder_model::InputDevice>(project))
        .chain(ids::<donder_model::Layout>(project))
        .chain(ids::<donder_model::Sequence>(project))
        .chain(ids::<donder_model::Console>(project))
        .chain(ids::<donder_model::FixtureDefinition>(project))
        .chain(ids::<donder_model::CurveDefinition>(project))
        .chain(ids::<donder_model::GradientDefinition>(project))
        .chain(ids::<donder_model::ScriptDefinition>(project))
}

/// Keep the documents writing what the project holds after an edit: each
/// imports the documents its references name, every document is reachable
/// from the root, so it is part of the project, and each asset records the
/// documents that use it.
fn maintain_sources(session: &mut ProjectSession) -> Result<(), ExportProjectError> {
    let addresses = session.addresses();
    let address = |id: ObjectId| {
        addresses
            .get(&id)
            .unwrap_or_else(|| unreachable!("every object of a session has an address"))
    };
    // Each object and the objects it refers to.
    let mut references: Vec<(ObjectId, ObjectId)> = Vec::new();
    session
        .project
        .for_each_reference(&mut |from, target| references.push((from, target)));
    let mut visible: BTreeMap<DocumentId, BTreeSet<SourceIdentity>> = BTreeMap::new();
    for (from, target) in references {
        visible
            .entry(address(from).document_id().clone())
            .or_default()
            .insert(address(target).clone());
    }
    for (from, targets) in visible {
        for target in targets {
            let kind = session.declared_kind(&target);
            crate::imports::ensure_document_can_reference_source(session, &from, kind, &target)?;
        }
    }
    import_unreachable_documents(session)?;
    // Asset identity is retained when a sequence moves or is copied; membership
    // records the documents that currently use it, rather than its old owners.
    let mut uses = Vec::new();
    session
        .project
        .for_each_asset(&mut |user, asset| uses.push((user, asset.clone())));
    for asset in &mut session.source.referenced_assets {
        asset.referenced_by.clear();
    }
    for (user, id) in uses {
        let document = address(user).document_id().clone();
        let asset = session
            .source
            .referenced_assets
            .iter_mut()
            .find(|asset| asset.id == id)
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: document.path().to_owned(),
                reference: id.0.to_string(),
                message: "An asset is missing.".into(),
            })?;
        asset.referenced_by.insert(document);
    }
    Ok(())
}

/// Remove `documents` and every object they declare. What other documents
/// held that referred to those objects gives way as the model repairs it, and
/// the other documents stop importing them; the repairs are left in the
/// project for [`donder_model::DonderProject::take_repairs`]. The caller owns
/// the candidate session, and the files.
pub fn remove_documents(
    session: &mut ProjectSession,
    documents: &[DocumentId],
) -> Result<(), donder_model::EditError> {
    let objects = documents
        .iter()
        .filter_map(|document| session.source.documents.get(document))
        .flat_map(|document| document.objects().iter().map(|object| object.id()))
        .collect::<Vec<_>>();
    session
        .project
        .apply_edits(objects.into_iter().map(donder_model::ProjectEdit::Remove))?;
    session
        .source
        .documents
        .retain(|document, _| !documents.contains(document));
    for source in session.source.documents.values_mut() {
        for edge in &mut source.imports {
            edge.targets.retain(|target| !documents.contains(target));
        }
        source.imports.retain(|edge| !edge.targets.is_empty());
    }
    for asset in &mut session.source.referenced_assets {
        asset
            .referenced_by
            .retain(|document| !documents.contains(document));
    }
    session
        .source
        .referenced_assets
        .retain(|asset| !asset.referenced_by.is_empty());
    Ok(())
}

/// Import every document the root does not reach from the root: a project is
/// what its root reaches, so a document an edit added, or one whose only
/// importer it removed, stays part of it.
fn import_unreachable_documents(session: &mut ProjectSession) -> Result<(), ExportProjectError> {
    let root = session.source.entrypoint.clone();
    let mut reached = HashSet::from([root.clone()]);
    let mut pending = vec![root.clone()];
    while let Some(document) = pending.pop() {
        for edge in &session.source.documents[&document].imports {
            for target in &edge.targets {
                if reached.insert(target.clone()) {
                    pending.push(target.clone());
                }
            }
        }
    }
    let unreached = session
        .source
        .documents
        .iter()
        .filter(|(document, _)| !reached.contains(*document))
        .map(|(document, source)| {
            let kind = source
                .objects()
                .first()
                .map_or(donder_model::ObjectKind::Script, |object| object.kind());
            (document.clone(), kind)
        })
        .collect::<Vec<_>>();
    for (document, kind) in unreached {
        crate::imports::ensure_document_imports_target(session, &root, &kind, "", document)?;
    }
    Ok(())
}
