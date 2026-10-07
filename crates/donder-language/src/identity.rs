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

/// A stable ownership slot. Collection members use their names or authored
/// local IDs, never list positions, so reordering does not change addresses.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum OwnedObjectSlot {
    Setup,
    Layout,
    Patch,
    /// An owned controller or sequence is named like a declaration.
    Controller(crate::dsl::Identifier),
    Sequence(crate::dsl::Identifier),
    Fixture(u32),
}

/// Address of a named object or an owned descendant of a named object.
/// Owned descendants are not symbols and cannot be imported independently.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjectIdentity {
    root: SourceIdentity,
    owned_path: Vec<OwnedObjectSlot>,
}

impl From<SourceIdentity> for ObjectIdentity {
    fn from(root: SourceIdentity) -> Self {
        Self {
            root,
            owned_path: Vec::new(),
        }
    }
}

impl ObjectIdentity {
    pub fn owned(&self, slot: OwnedObjectSlot) -> Self {
        let mut child = self.clone();
        child.owned_path.push(slot);
        child
    }

    /// Move this address with its owner, preserving descendant slots.
    pub fn rebase(&mut self, from: &Self, to: &Self) {
        if self.root == from.root && self.owned_path.starts_with(&from.owned_path) {
            let suffix = self.owned_path[from.owned_path.len()..].to_vec();
            *self = to.clone();
            self.owned_path.extend(suffix);
        }
    }

    pub fn parent(&self) -> Option<Self> {
        let mut parent = self.clone();
        parent.owned_path.pop()?;
        Some(parent)
    }

    pub fn root_source(&self) -> &SourceIdentity {
        &self.root
    }

    pub fn source(&self) -> Option<&SourceIdentity> {
        self.owned_path.is_empty().then_some(&self.root)
    }

    pub fn owned_path(&self) -> &[OwnedObjectSlot] {
        &self.owned_path
    }
    pub fn document(&self) -> &Utf8Path {
        self.root.document()
    }
    pub fn document_id(&self) -> &DocumentId {
        self.root.document_id()
    }
    pub fn module_id(&self) -> Uuid {
        self.root.module_id()
    }

    pub fn with_root_source(&self, root: SourceIdentity) -> Self {
        Self {
            root,
            owned_path: self.owned_path.clone(),
        }
    }
}
