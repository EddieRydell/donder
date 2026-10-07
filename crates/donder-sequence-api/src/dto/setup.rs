use super::*;

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
    Blocked {
        reason: String,
        diagnostics: Vec<ProjectDiagnostic>,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiDocumentRequest {
    pub owned_path: Vec<GuiOwnedStep>,
    pub project_revision: u32,
    pub path: String,
    pub view: DocumentViewId,
    pub object_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiObjectRef {
    pub owned_path: Vec<GuiOwnedStep>,
    pub module_id: String,
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
        slot: GuiOwnershipSlot,
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
    /// Set the open object's description; empty text removes it.
    Description {
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiEditResult {
    pub snapshot: AppSnapshot,
    pub document: GuiDocument,
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
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum DocumentViewId {
    Text,
    Project,
    Setup,
    Layout,
    Fixture,
    Patch,
    Controller,
    Sequence,
    Curve,
    Gradient,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CurveGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub object_key: String,
    pub points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GradientGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub object_key: String,
    pub stops: Vec<SequenceGradientStop>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub available_sources: Vec<GuiObjectRef>,
    pub path: String,
    pub object_key: String,
    pub setup: GuiObjectRef,
    pub sequences: Vec<GuiObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub available_sources: Vec<GuiObjectRef>,
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    pub layout_ref: GuiObjectRef,
    pub patch_ref: GuiObjectRef,
    pub layout_read_only: bool,
    pub patch_read_only: bool,
    pub controllers: Vec<SetupController>,
    pub available_controllers: Vec<SetupController>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupController {
    pub label: String,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
    pub config: SetupControllerConfig,
    pub ports: Vec<SetupControllerPort>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ControllerGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub object_key: String,
    pub controller: SetupController,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SetupControllerConfig {
    E131 {
        source_name: String,
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

impl From<&donder_language::controller::ControllerProtocol> for SetupControllerConfig {
    fn from(protocol: &donder_language::controller::ControllerProtocol) -> Self {
        use donder_language::controller::{ArtNetMode, ControllerProtocol, E131Mode};
        match protocol {
            ControllerProtocol::E131(config) => Self::E131 {
                source_name: config.source_name.clone(),
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
pub enum GuiOwnedStep {
    Setup,
    Layout,
    Patch,
    /// Owned controllers and sequences are named.
    Controller {
        name: String,
    },
    Sequence {
        name: String,
    },
    Fixture {
        id: u32,
    },
}

impl From<&donder_language::identity::OwnedObjectSlot> for GuiOwnedStep {
    fn from(slot: &donder_language::identity::OwnedObjectSlot) -> Self {
        use donder_language::identity::OwnedObjectSlot;
        match slot {
            OwnedObjectSlot::Setup => Self::Setup,
            OwnedObjectSlot::Layout => Self::Layout,
            OwnedObjectSlot::Patch => Self::Patch,
            OwnedObjectSlot::Controller(name) => Self::Controller {
                name: name.as_str().to_string(),
            },
            OwnedObjectSlot::Sequence(name) => Self::Sequence {
                name: name.as_str().to_string(),
            },
            OwnedObjectSlot::Fixture(id) => Self::Fixture { id: *id },
        }
    }
}
impl TryFrom<&GuiOwnedStep> for donder_language::identity::OwnedObjectSlot {
    type Error = String;

    fn try_from(slot: &GuiOwnedStep) -> Result<Self, String> {
        let name = |name: &str| {
            donder_language::dsl::Identifier::new(name.to_string())
                .map_err(|_| format!("`{name}` is not an object name."))
        };
        Ok(match slot {
            GuiOwnedStep::Setup => Self::Setup,
            GuiOwnedStep::Layout => Self::Layout,
            GuiOwnedStep::Patch => Self::Patch,
            GuiOwnedStep::Controller { name: text } => Self::Controller(name(text)?),
            GuiOwnedStep::Sequence { name: text } => Self::Sequence(name(text)?),
            GuiOwnedStep::Fixture { id } => Self::Fixture(*id),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiOwnershipSlot {
    Setup,
    Sequence { index: u32 },
    Layout,
    Patch,
    Controller { index: u32 },
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
        source: GuiObjectRef,
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
