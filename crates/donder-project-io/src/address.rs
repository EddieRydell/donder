//! Where objects are written: documents and declared names. The typed project
//! knows objects only by session ids; the source inventory names each one.
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{ObjectId, ObjectKind};
use donder_runtime_types::Identifier;
use std::collections::HashMap;

use crate::source::{ProjectSession, SourceProject};

/// A Donder document, by its path relative to the project root.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct DocumentId(Utf8PathBuf);

impl DocumentId {
    pub fn new(path: Utf8PathBuf) -> Self {
        Self(path)
    }

    pub fn path(&self) -> &Utf8Path {
        &self.0
    }
}

/// A declared object's document and name. Names are only unique inside their
/// declaring document.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SourceIdentity {
    document: DocumentId,
    object: String,
}

impl SourceIdentity {
    pub fn from_document(document: DocumentId, object: String) -> Self {
        Self { document, object }
    }

    pub fn document(&self) -> &Utf8Path {
        self.document.path()
    }

    pub fn document_id(&self) -> &DocumentId {
        &self.document
    }

    pub fn object(&self) -> &str {
        &self.object
    }
}

/// `problem`, prefixed with the object it concerns.
pub(crate) fn object_problem(
    identity: &SourceIdentity,
    problem: &donder_model::ValidationProblem,
) -> String {
    format!("{}: {problem}", identity.object())
}

/// A validation error's message, prefixed with the object it concerns.
pub fn validation_message(
    session: &ProjectSession,
    error: &donder_model::ProjectValidationError,
) -> String {
    match session.source_identity(error.object) {
        Some(identity) => object_problem(&identity, &error.problem),
        None => error.problem.to_string(),
    }
}

/// An object a document declares, in declaration order. The inventory is the
/// only record of an object's document and name.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct DeclaredObject {
    pub(crate) id: ObjectId,
    pub(crate) name: Identifier,
}

impl DeclaredObject {
    pub(crate) fn new(id: ObjectId, name: Identifier) -> Self {
        Self { id, name }
    }

    pub fn id(&self) -> ObjectId {
        self.id
    }

    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn kind(&self) -> ObjectKind {
        self.id.kind()
    }
}

impl SourceProject {
    /// The document and name a declared object is written under.
    fn declaration(&self, id: ObjectId) -> Option<(&DocumentId, &Identifier)> {
        self.documents.iter().find_map(|(document, source)| {
            source
                .objects
                .iter()
                .find(|object| object.id == id)
                .map(|object| (document, &object.name))
        })
    }

    /// The name a declared object is written under.
    pub fn declared_name(&self, id: ObjectId) -> Option<Identifier> {
        self.declaration(id).map(|(_, name)| name.clone())
    }

    /// The declared object a document and name write.
    fn declared_id(&self, identity: &SourceIdentity) -> Option<ObjectId> {
        self.documents
            .get(identity.document_id())?
            .objects
            .iter()
            .find(|object| object.name.as_str() == identity.object())
            .map(|object| object.id)
    }
}

impl ProjectSession {
    pub fn declaration(&self, id: ObjectId) -> Option<(&DocumentId, &Identifier)> {
        self.source.declaration(id)
    }

    pub fn declared_id(&self, identity: &SourceIdentity) -> Option<ObjectId> {
        self.source.declared_id(identity)
    }

    /// The kind of an object this session declares.
    pub(crate) fn declared_kind(&self, identity: &SourceIdentity) -> ObjectKind {
        self.declared_id(identity)
            .unwrap_or_else(|| unreachable!("the identity is one of the session's declarations"))
            .kind()
    }

    /// A declared object's document and name as one identity, when the
    /// session's project has it: for ids from outside the session, such as a
    /// refused candidate's.
    pub fn source_identity(&self, id: ObjectId) -> Option<SourceIdentity> {
        self.declaration(id).map(|(document, name)| {
            SourceIdentity::from_document(document.clone(), name.as_str().to_string())
        })
    }

    /// Where `id`, an object of the session's project, is declared.
    pub fn address_of(&self, id: impl Into<ObjectId>) -> SourceIdentity {
        self.source_identity(id.into())
            .unwrap_or_else(|| unreachable!("every object of a session is declared"))
    }

    /// Every object's declaration, for printing many references at once.
    pub(crate) fn addresses(&self) -> HashMap<ObjectId, SourceIdentity> {
        let mut addresses = HashMap::new();
        for (document, source) in &self.source.documents {
            for object in &source.objects {
                addresses.insert(
                    object.id,
                    SourceIdentity::from_document(
                        document.clone(),
                        object.name.as_str().to_string(),
                    ),
                );
            }
        }
        addresses
    }
}
