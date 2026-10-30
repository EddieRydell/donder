use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDocument {
    Patch {
        document: PatchGuiDocument,
    },
    Project {
        document: ProjectGuiDocument,
    },
    Setup {
        document: SetupGuiDocument,
    },
    Sequence {
        document: SequenceGuiDocument,
    },
    Layout {
        document: LayoutGuiDocument,
    },
    Fixture {
        document: FixtureGuiDocument,
    },
    Curve {
        document: CurveGuiDocument,
    },
    Gradient {
        document: GradientGuiDocument,
    },
    Controller {
        document: ControllerGuiDocument,
    },
    InputDevice {
        document: InputDeviceGuiDocument,
    },
    Rig {
        document: RigGuiDocument,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<IoDiagnostic>,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
/// An object a GUI editor opens: a declared object, or an object owned beneath
/// one. The object's kind picks the editor.
pub struct GuiDocumentRequest {
    pub owned_path: Vec<OwnedObjectSlot>,
    pub project_revision: u32,
    pub path: String,
    pub object_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiObjectRef {
    pub owned_path: Vec<OwnedObjectSlot>,
    pub path: String,
    pub object_key: String,
    pub kind: ObjectKind,
    pub id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiEditCommand {
    Ownership {
        member: OwnershipMember,
        edit: GuiOwnershipEdit,
    },
    Patch {
        routes: Vec<GuiPixelRoute>,
    },
    Setup {
        edit: SetupGuiEdit,
    },
    Sequence {
        edit: SequenceGuiEdit,
    },
    Layout {
        edit: LayoutGuiEdit,
    },
    Fixture {
        edit: FixtureGuiEdit,
    },
    Curve {
        points: Vec<SequenceCurvePoint>,
    },
    Gradient {
        stops: Vec<SequenceGradientStop>,
    },
    Controller {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
    },
    InputDevice {
        driver: GuiInputDriver,
        controls: Vec<GuiInputControl>,
    },
    Rig {
        edit: RigGuiEdit,
    },
    Project {
        edit: ProjectGuiEdit,
    },
    /// The name becomes a `snake_case` name, which the edit reports; renaming
    /// marks renames every parameter and mapping that names them.
    Rename {
        item: GuiItem,
        name: String,
    },
    /// Empty text removes the description.
    SetDescription {
        item: GuiItem,
        description: Option<String>,
    },
    SetColor {
        item: GuiItem,
        color: String,
    },
    /// Parameters that named deleted marks keep working with no marks.
    Delete {
        item: GuiItem,
    },
}

/// A part of the open object a generic edit names: the object itself, or one
/// of its clips, mark collections (a rig's live marks), layers, graph nodes or
/// rig behaviors.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiItem {
    Object,
    Clip { id: u32 },
    MarkCollection { key: String },
    Layer { id: u32 },
    GraphNode { node_id: String },
    Voice { id: u32 },
    Switch { id: u32 },
    Mapping { id: u32 },
}

/// How an edit changed the open GUI document, relative to the document at the
/// request's project revision.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDocumentChange {
    /// The whole document.
    Document { document: GuiDocument },
    /// A sequence whose clips mostly did not change. `document` holds every
    /// field, but its `clips` only the changed and added clips; `clip_ids`
    /// lists every clip in order.
    SequenceClips {
        document: SequenceGuiDocument,
        clip_ids: Vec<u32>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiEditUpdate {
    pub snapshot: AppSnapshot,
    pub change: GuiDocumentChange,
    /// The sequence selection a selection edit leaves; `None` clears it.
    pub selection: Option<SequenceSelection>,
    /// The items a copy, cut or paste took, and those it could not.
    pub copied_count: u32,
    pub skipped_count: u32,
    /// The names or keys of the items the edit created, in order.
    pub created: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum BufferExternalState {
    Current,
    ChangedOnDisk,
    DeletedOnDisk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurveGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GradientGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub available_declarations: Vec<GuiObjectRef>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) setup: GuiObjectRef,
    pub(crate) sequences: Vec<GuiObjectRef>,
    pub rigs: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub available_declarations: Vec<GuiObjectRef>,
    pub(crate) path: String,
    pub(crate) source_ref: GuiObjectRef,
    pub(crate) object_key: String,
    pub layout_ref: GuiObjectRef,
    pub patch_ref: GuiObjectRef,
    pub controllers: Vec<SetupController>,
    pub(crate) available_controllers: Vec<SetupController>,
    pub input_devices: Vec<SetupInputDevice>,
    pub available_input_devices: Vec<SetupInputDevice>,
    /// Every key a new input device's control can read, in the language's order.
    pub(crate) key_codes: Vec<String>,
    pub(crate) input_limits: GuiInputLimits,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupController {
    pub(crate) label: String,
    pub source_ref: GuiObjectRef,
    pub(crate) config: SetupControllerConfig,
    pub(crate) ports: Vec<SetupControllerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ControllerGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) controller: SetupController,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupControllerPort {
    pub id: u32,
    /// Unique within the controller; patch routes refer to it.
    pub name: String,
    pub address: u16,
    pub slot_count: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupGuiEdit {
    AddController {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
    },
    AttachController {
        controller: GuiObjectRef,
    },
    DetachController {
        controller: GuiObjectRef,
        remove_outputs: bool,
    },
    AddInputDevice {
        driver: GuiInputDriver,
        controls: Vec<GuiInputControl>,
    },
    AttachInputDevice {
        device: GuiObjectRef,
    },
    DetachInputDevice {
        device: GuiObjectRef,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupControllerConfig {
    E131 {
        sender_name: String,
        bind_address: String,
        priority: u8,
        destination: Option<String>,
    },
    ArtNet {
        bind_address: String,
        destination: String,
        broadcast: bool,
    },
    Donder {
        device: String,
    },
}

impl From<&donder_model::ControllerProtocol> for SetupControllerConfig {
    fn from(protocol: &donder_model::ControllerProtocol) -> Self {
        use donder_model::{ArtNetMode, ControllerProtocol, E131Mode};
        match protocol {
            ControllerProtocol::E131(config) => Self::E131 {
                sender_name: config.sender_name.clone(),
                bind_address: config.bind_address.to_string(),
                priority: config.priority,
                destination: match &config.mode {
                    E131Mode::Multicast => None,
                    E131Mode::Unicast { destination } => Some(destination.to_string()),
                },
            },
            ControllerProtocol::ArtNet(config) => Self::ArtNet {
                bind_address: config.bind_address.to_string(),
                destination: config.destination.to_string(),
                broadcast: matches!(config.mode, ArtNetMode::Broadcast),
            },
            ControllerProtocol::Donder(config) => Self::Donder {
                device: config.device.as_str().to_string(),
            },
        }
    }
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "type", rename_all = "camelCase")]
/// A stable ownership slot: owned members are named, never addressed by position.
pub enum OwnedObjectSlot {
    Setup,
    Layout,
    Patch,
    /// Owned controllers, input devices, sequences and rigs are named.
    Controller {
        name: String,
    },
    InputDevice {
        name: String,
    },
    Sequence {
        name: String,
    },
    Rig {
        name: String,
    },
    Fixture {
        id: u32,
    },
}

impl From<&donder_project_io::OwnedObjectSlot> for OwnedObjectSlot {
    fn from(slot: &donder_project_io::OwnedObjectSlot) -> Self {
        use donder_project_io::OwnedObjectSlot as Slot;
        match slot {
            Slot::Setup => Self::Setup,
            Slot::Layout => Self::Layout,
            Slot::Patch => Self::Patch,
            Slot::Controller(name) => Self::Controller {
                name: name.as_str().to_string(),
            },
            Slot::InputDevice(name) => Self::InputDevice {
                name: name.as_str().to_string(),
            },
            Slot::Sequence(name) => Self::Sequence {
                name: name.as_str().to_string(),
            },
            Slot::Rig(name) => Self::Rig {
                name: name.as_str().to_string(),
            },
            Slot::Fixture(id) => Self::Fixture { id: *id },
        }
    }
}
impl TryFrom<&OwnedObjectSlot> for donder_project_io::OwnedObjectSlot {
    type Error = String;

    fn try_from(slot: &OwnedObjectSlot) -> Result<Self, String> {
        let name = |name: &str| {
            donder_runtime_types::Identifier::new(name.to_string())
                .map_err(|_| format!("`{name}` is not an object name."))
        };
        Ok(match slot {
            OwnedObjectSlot::Setup => Self::Setup,
            OwnedObjectSlot::Layout => Self::Layout,
            OwnedObjectSlot::Patch => Self::Patch,
            OwnedObjectSlot::Controller { name: text } => Self::Controller(name(text)?),
            OwnedObjectSlot::InputDevice { name: text } => Self::InputDevice(name(text)?),
            OwnedObjectSlot::Sequence { name: text } => Self::Sequence(name(text)?),
            OwnedObjectSlot::Rig { name: text } => Self::Rig(name(text)?),
            OwnedObjectSlot::Fixture { id } => Self::Fixture(*id),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
/// The member of the open object an ownership edit changes: the object a slot
/// holds, owned or linked, or a layout's fixture, whose definition several
/// fixtures may link.
pub enum OwnershipMember {
    Object { source: GuiObjectRef },
    Fixture { id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiOwnershipEdit {
    UseExisting {
        declaration: GuiObjectRef,
    },
    MakeIndependent,
    MakeReusable {
        name: String,
        storage: ReusableStorage,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ReusableStorage {
    SameFile,
    NewFile,
}
