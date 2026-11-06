//! Where objects are written: documents, declared names, and the ownership
//! slots of objects written in place. The typed project knows objects only by
//! session ids; the source inventory names each declared one, and an owned
//! object's address follows from its owner's.
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{DonderProject, Identified, ObjectId, ObjectKind, Owned, OwnedMember, Setup};
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
    /// An owned controller, input device, sequence or console is named like a
    /// declaration.
    Controller(Identifier),
    InputDevice(Identifier),
    Sequence(Identifier),
    Console(Identifier),
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

    /// The name a controller, input device, sequence or console is declared or
    /// stored under: its owned slot's name, else its source object's.
    pub fn declared_name(&self) -> &str {
        match self.owned_path.last().and_then(OwnedObjectSlot::field) {
            Some((_, Some(name))) => name.as_str(),
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
                OwnedObjectSlot::Console(name) => write!(f, " › console `{}`", name.as_str())?,
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

/// A field of a project or setup that owns objects in place: one object, or
/// a collection of members named like declarations. This is the one table of
/// the ownership tree; slots, references and traversals follow from it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwningField {
    Setup,
    Sequences,
    Consoles,
    Layout,
    Patch,
    Controllers,
    InputDevices,
}

impl OwningField {
    pub(crate) const ALL: [Self; 7] = [
        Self::Setup,
        Self::Sequences,
        Self::Consoles,
        Self::Layout,
        Self::Patch,
        Self::Controllers,
        Self::InputDevices,
    ];

    /// The field's name in documents and references.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Sequences => "sequences",
            Self::Consoles => "consoles",
            Self::Layout => "layout",
            Self::Patch => "patch",
            Self::Controllers => "controllers",
            Self::InputDevices => "input_devices",
        }
    }

    /// The kind of object holding the field, and the kind it owns.
    pub(crate) fn kinds(self) -> (ObjectKind, ObjectKind) {
        use ObjectKind as Kind;
        match self {
            Self::Setup => (Kind::Project, Kind::Setup),
            Self::Sequences => (Kind::Project, Kind::Sequence),
            Self::Consoles => (Kind::Project, Kind::Console),
            Self::Layout => (Kind::Setup, Kind::Layout),
            Self::Patch => (Kind::Setup, Kind::Patch),
            Self::Controllers => (Kind::Setup, Kind::Controller),
            Self::InputDevices => (Kind::Setup, Kind::InputDevice),
        }
    }

    /// The slot of the object the field owns: the field itself, or its
    /// collection member `name`.
    pub(crate) fn slot(self, name: Option<Identifier>) -> Option<OwnedObjectSlot> {
        Some(match (self, name) {
            (Self::Setup, None) => OwnedObjectSlot::Setup,
            (Self::Layout, None) => OwnedObjectSlot::Layout,
            (Self::Patch, None) => OwnedObjectSlot::Patch,
            (Self::Sequences, Some(name)) => OwnedObjectSlot::Sequence(name),
            (Self::Consoles, Some(name)) => OwnedObjectSlot::Console(name),
            (Self::Controllers, Some(name)) => OwnedObjectSlot::Controller(name),
            (Self::InputDevices, Some(name)) => OwnedObjectSlot::InputDevice(name),
            _ => return None,
        })
    }

    /// Whether the field is a collection of named members.
    pub(crate) fn is_collection(self) -> bool {
        self.slot(None).is_none()
    }
}

impl OwnedObjectSlot {
    /// The field owning this slot and, for a collection, the member's name.
    /// An owned fixture definition belongs to a layout item, not a field.
    pub(crate) fn field(&self) -> Option<(OwningField, Option<&Identifier>)> {
        Some(match self {
            Self::Setup => (OwningField::Setup, None),
            Self::Layout => (OwningField::Layout, None),
            Self::Patch => (OwningField::Patch, None),
            Self::Sequence(name) => (OwningField::Sequences, Some(name)),
            Self::Console(name) => (OwningField::Consoles, Some(name)),
            Self::Controller(name) => (OwningField::Controllers, Some(name)),
            Self::InputDevice(name) => (OwningField::InputDevices, Some(name)),
            Self::Fixture(_) => return None,
        })
    }

    /// The kind of the object owned at this slot of an owner of kind `owner`.
    pub fn child_kind(&self, owner: ObjectKind) -> Option<ObjectKind> {
        match self.field() {
            Some((field, _)) => {
                let (holder, kind) = field.kinds();
                (holder == owner).then_some(kind)
            }
            None => (owner == ObjectKind::Layout).then_some(ObjectKind::FixtureDefinition),
        }
    }
}

/// Visit each object `owner` owns in place, with its slot.
fn owned_children(
    project: &DonderProject,
    owner: ObjectId,
    visit: &mut dyn FnMut(OwnedObjectSlot, ObjectId),
) {
    fn single<T: Identified<Id = I>, I: Copy + Into<ObjectId>>(
        field: OwningField,
        owned: &Owned<T, I>,
        visit: &mut dyn FnMut(OwnedObjectSlot, ObjectId),
    ) {
        if let (Owned::Inline(value), Some(slot)) = (owned, field.slot(None)) {
            visit(slot, (*value.id()).into());
        }
    }
    fn members<T: Identified<Id = I>, I: Copy + Into<ObjectId>>(
        field: OwningField,
        members: &[OwnedMember<T, I>],
        visit: &mut dyn FnMut(OwnedObjectSlot, ObjectId),
    ) {
        for member in members {
            if let OwnedMember::Inline { name, value } = member
                && let Some(slot) = field.slot(Some(name.clone()))
            {
                visit(slot, (*value.id()).into());
            }
        }
    }
    match owner {
        ObjectId::Project(id) if id == project.root().id => {
            let root = project.root();
            single(OwningField::Setup, &root.setup, visit);
            members(OwningField::Sequences, &root.sequences, visit);
            members(OwningField::Consoles, &root.consoles, visit);
        }
        ObjectId::Setup(id) => {
            if let Some(setup) = project.get::<Setup>(id) {
                single(OwningField::Layout, &setup.layout, visit);
                single(OwningField::Patch, &setup.patch, visit);
                members(OwningField::Controllers, &setup.controllers, visit);
                members(OwningField::InputDevices, &setup.input_devices, visit);
            }
        }
        _ => {}
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
        let mut found = None;
        owned_children(&self.project, owner, &mut |child, id| {
            if &child == slot {
                found = Some(id);
            }
        });
        found
    }

    /// Visit every object owned in place with its address.
    fn owned_addresses(&self, visit: &mut dyn FnMut(ObjectId, &ObjectAddress)) {
        for (document, source) in &self.source.documents {
            for object in &source.objects {
                let address = SourceIdentity::from_document(
                    document.clone(),
                    object.name.as_str().to_string(),
                )
                .into();
                self.owned_below(object.id, &address, visit);
            }
        }
    }

    /// Visit the objects `owner`, at `address`, owns at any depth.
    fn owned_below(
        &self,
        owner: ObjectId,
        address: &ObjectAddress,
        visit: &mut dyn FnMut(ObjectId, &ObjectAddress),
    ) {
        owned_children(&self.project, owner, &mut |slot, id| {
            let child = address.owned(slot);
            visit(id, &child);
            self.owned_below(id, &child, visit);
        });
    }
}
