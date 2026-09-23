use camino::{Utf8Path, Utf8PathBuf};
use uuid::Uuid;

/// Stable identity of a Donder document inside a package module.
///
/// The physical package root is deliberately not part of this value. A
/// resolved package can move in the cache or be updated without changing the
/// identity used by domain objects and history.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct DocumentId {
    module_id: Uuid,
    path: Utf8PathBuf,
}

impl DocumentId {
    pub fn new(module_id: Uuid, path: Utf8PathBuf) -> Self {
        Self { module_id, path }
    }

    pub fn module_id(&self) -> Uuid {
        self.module_id
    }

    pub fn path(&self) -> &Utf8Path {
        &self.path
    }
}

/// Qualified identity for an object declared in a Donder source document.
///
/// Object keys are only unique inside their declaring document. Keeping both
/// parts in the domain model prevents import aliases and same-named objects in
/// different documents from collapsing into one global string namespace. Source
/// loaders validate object keys before constructing domain IDs.
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

    pub fn module_id(&self) -> Uuid {
        self.document.module_id()
    }

    pub fn object(&self) -> &str {
        &self.object
    }
}
