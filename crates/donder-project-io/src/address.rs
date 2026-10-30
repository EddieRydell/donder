//! Where objects are written: documents, declared names, and the ownership
//! slots of objects written in place. The typed project knows objects only by
//! session ids; the source inventory names each declared one, and an owned
//! object's address follows from its owner's.
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{ObjectId, ObjectKind, Owned, OwnedMember, Setup};
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

/// A stable ownership slot. Collection members use their names or authored
/// local IDs, never list positions, so reordering does not change addresses.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum OwnedObjectSlot {
    Setup,
    Layout,
    Patch,
    /// An owned controller, input device, sequence or rig is named like a
    /// declaration.
    Controller(Identifier),
    InputDevice(Identifier),
    Sequence(Identifier),
    Rig(Identifier),
    Fixture(u32),
}

/// Address of a named object or an owned descendant of a named object.
/// Owned descendants are not symbols and cannot be imported independently.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ObjectAddress {
    root: SourceIdentity,
    owned_path: Vec<OwnedObjectSlot>,
}

impl From<SourceIdentity> for ObjectAddress {
    fn from(root: SourceIdentity) -> Self {
        Self {
            root,
            owned_path: Vec::new(),
        }
    }
}

impl ObjectAddress {
    pub fn owned(&self, slot: OwnedObjectSlot) -> Self {
        let mut child = self.clone();
        child.owned_path.push(slot);
        child
    }

    pub fn parent(&self) -> Option<Self> {
        let mut parent = self.clone();
        parent.owned_path.pop()?;
        Some(parent)
    }

    pub fn root_source(&self) -> &SourceIdentity {
        &self.root
    }

    pub(crate) fn source(&self) -> Option<&SourceIdentity> {
        self.owned_path.is_empty().then_some(&self.root)
    }

    pub fn owned_path(&self) -> &[OwnedObjectSlot] {
        &self.owned_path
    }

    /// The name a controller, input device, sequence or rig is declared or
    /// stored under: its owned slot's name, else its source object's.
    pub fn declared_name(&self) -> &str {
        match self.owned_path.last() {
            Some(
                OwnedObjectSlot::Controller(name)
                | OwnedObjectSlot::InputDevice(name)
                | OwnedObjectSlot::Sequence(name)
                | OwnedObjectSlot::Rig(name),
            ) => name.as_str(),
            _ => self.root.object(),
        }
    }

    pub fn document(&self) -> &Utf8Path {
        self.root.document()
    }

    pub fn document_id(&self) -> &DocumentId {
        self.root.document_id()
    }
}

impl std::fmt::Display for ObjectAddress {
    /// `` `main` `` for a declared object, then each owned slot:
    /// `` `main` › layout ``, `` `main` › controller `front` ``.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}`", self.root.object())?;
        for slot in &self.owned_path {
            match slot {
                OwnedObjectSlot::Setup => write!(f, " › setup")?,
                OwnedObjectSlot::Layout => write!(f, " › layout")?,
                OwnedObjectSlot::Patch => write!(f, " › patch")?,
                OwnedObjectSlot::Controller(name) => {
                    write!(f, " › controller `{}`", name.as_str())?
                }
                OwnedObjectSlot::InputDevice(name) => {
                    write!(f, " › input device `{}`", name.as_str())?
                }
                OwnedObjectSlot::Sequence(name) => write!(f, " › sequence `{}`", name.as_str())?,
                OwnedObjectSlot::Rig(name) => write!(f, " › rig `{}`", name.as_str())?,
                OwnedObjectSlot::Fixture(id) => write!(f, " › fixture {id}")?,
            }
        }
        Ok(())
    }
}

/// A validation error's message, prefixed with the object it concerns.
pub fn validation_message(
    session: &ProjectSession,
    error: &donder_model::ProjectValidationError,
) -> String {
    match session.address_of(error.object) {
        Some(address) => format!("{address}: {}", error.problem),
        None => error.problem.to_string(),
    }
}

/// An object a document declares, in declaration order. The inventory is the
/// only record of a reusable object's document and name.
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

impl OwnedObjectSlot {
    /// The kind of the object owned at this slot of an owner of kind `owner`.
    pub fn child_kind(&self, owner: ObjectKind) -> Option<ObjectKind> {
        use ObjectKind as Kind;
        match (owner, self) {
            (Kind::Project, Self::Setup) => Some(Kind::Setup),
            (Kind::Project, Self::Sequence(_)) => Some(Kind::Sequence),
            (Kind::Project, Self::Rig(_)) => Some(Kind::Rig),
            (Kind::Setup, Self::Layout) => Some(Kind::Layout),
            (Kind::Setup, Self::Patch) => Some(Kind::Patch),
            (Kind::Setup, Self::Controller(_)) => Some(Kind::Controller),
            (Kind::Setup, Self::InputDevice(_)) => Some(Kind::InputDevice),
            (Kind::Layout, Self::Fixture(_)) => Some(Kind::FixtureDefinition),
            _ => None,
        }
    }
}

/// Each member of a collection by the name it is owned under.
fn member<T, I: Copy + Into<ObjectId>>(
    members: &[OwnedMember<T, I>],
    name: &Identifier,
) -> Option<ObjectId>
where
    T: donder_model::Identified<Id = I>,
{
    members
        .iter()
        .find(|source| source.name() == Some(name))
        .map(|source| (*source.id()).into())
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

    /// A declared object's document and name as one identity.
    pub fn source_identity(&self, id: ObjectId) -> Option<SourceIdentity> {
        self.declaration(id).map(|(document, name)| {
            SourceIdentity::from_document(document.clone(), name.as_str().to_string())
        })
    }

    /// Where `id` is written: its declaration, or its owner's address and slot.
    pub fn address_of(&self, id: ObjectId) -> Option<ObjectAddress> {
        if let Some(identity) = self.source_identity(id) {
            return Some(identity.into());
        }
        let mut found = None;
        self.owned_addresses(&mut |owned, address| {
            if owned == id {
                found = Some(address.clone());
            }
        });
        found
    }

    /// Every object's address, for printing many references at once.
    pub(crate) fn addresses(&self) -> HashMap<ObjectId, ObjectAddress> {
        let mut addresses = HashMap::new();
        for (document, source) in &self.source.documents {
            for object in &source.objects {
                addresses.insert(
                    object.id,
                    SourceIdentity::from_document(
                        document.clone(),
                        object.name.as_str().to_string(),
                    )
                    .into(),
                );
            }
        }
        self.owned_addresses(&mut |id, address| {
            addresses.insert(id, address.clone());
        });
        addresses
    }

    /// The object at `address`. Owned fixture definitions have no id.
    pub fn resolve_address(&self, address: &ObjectAddress) -> Option<ObjectId> {
        let mut id = self.declared_id(address.root_source())?;
        for slot in address.owned_path() {
            id = self.owned_child(id, slot)?;
        }
        Some(id)
    }

    fn owned_child(&self, owner: ObjectId, slot: &OwnedObjectSlot) -> Option<ObjectId> {
        let project = &self.project;
        match (owner, slot) {
            (ObjectId::Project(id), OwnedObjectSlot::Setup) if id == project.root().id => {
                match &project.root().setup {
                    Owned::Inline(setup) => Some(setup.id.into()),
                    Owned::Reference(_) => None,
                }
            }
            (ObjectId::Project(id), OwnedObjectSlot::Sequence(name)) if id == project.root().id => {
                member(&project.root().sequences, name)
            }
            (ObjectId::Project(id), OwnedObjectSlot::Rig(name)) if id == project.root().id => {
                member(&project.root().rigs, name)
            }
            (ObjectId::Setup(id), slot) => {
                let setup = project.get::<donder_model::Setup>(id)?;
                match slot {
                    OwnedObjectSlot::Layout => setup.layout.inline().map(|value| value.id.into()),
                    OwnedObjectSlot::Patch => setup.patch.inline().map(|value| value.id.into()),
                    OwnedObjectSlot::Controller(name) => member(&setup.controllers, name),
                    OwnedObjectSlot::InputDevice(name) => member(&setup.input_devices, name),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Visit every object owned in place with its address.
    fn owned_addresses(&self, visit: &mut dyn FnMut(ObjectId, &ObjectAddress)) {
        let project = &self.project;
        let Some(root) = self.address_of_declared(project.root().id.into()) else {
            return;
        };
        if let Owned::Inline(setup) = &project.root().setup {
            let address = root.owned(OwnedObjectSlot::Setup);
            visit(setup.id.into(), &address);
            setup_addresses(setup, &address, visit);
        }
        for source in &project.root().sequences {
            if let OwnedMember::Inline { name, value } = source {
                visit(
                    value.id.into(),
                    &root.owned(OwnedObjectSlot::Sequence(name.clone())),
                );
            }
        }
        for source in &project.root().rigs {
            if let OwnedMember::Inline { name, value } = source {
                visit(
                    value.id.into(),
                    &root.owned(OwnedObjectSlot::Rig(name.clone())),
                );
            }
        }
        for setup in project.reusable::<donder_model::Setup>().values() {
            if let Some(address) = self.address_of_declared(setup.id.into()) {
                setup_addresses(setup, &address, visit);
            }
        }
    }

    fn address_of_declared(&self, id: ObjectId) -> Option<ObjectAddress> {
        self.source_identity(id).map(ObjectAddress::from)
    }
}

fn setup_addresses(
    setup: &Setup,
    address: &ObjectAddress,
    visit: &mut dyn FnMut(ObjectId, &ObjectAddress),
) {
    if let Owned::Inline(layout) = &setup.layout {
        visit(layout.id.into(), &address.owned(OwnedObjectSlot::Layout));
    }
    if let Owned::Inline(patch) = &setup.patch {
        visit(patch.id.into(), &address.owned(OwnedObjectSlot::Patch));
    }
    for source in &setup.controllers {
        if let OwnedMember::Inline { name, value } = source {
            visit(
                value.id.into(),
                &address.owned(OwnedObjectSlot::Controller(name.clone())),
            );
        }
    }
    for source in &setup.input_devices {
        if let OwnedMember::Inline { name, value } = source {
            visit(
                value.id.into(),
                &address.owned(OwnedObjectSlot::InputDevice(name.clone())),
            );
        }
    }
}
