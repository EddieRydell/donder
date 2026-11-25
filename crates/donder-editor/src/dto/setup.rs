use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::num::NonZeroU32;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiDocument {
    Project {
        document: ProjectGuiDocument,
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
    Console {
        document: ConsoleGuiDocument,
    },
    Blocked {
        reason: String,
        diagnostics: Vec<IoDiagnostic>,
    },
}

#[derive(Debug, Clone, Eq, Hash, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
/// An object a GUI editor opens. The object's kind picks the editor.
pub struct GuiDocumentRequest {
    pub project_revision: u32,
    pub path: String,
    pub object_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiObjectRef {
    pub path: String,
    pub object_key: String,
    pub kind: ObjectKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiEdit {
    /// Write `value` at each of `sites`: one field of several members edits
    /// them together.
    Put {
        sites: Vec<GuiSite>,
        value: GuiValue,
    },
    /// Remove the object or member at `site`, or the part of a member its
    /// field names; what named it gives way.
    Remove {
        site: GuiSite,
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
    Console {
        edit: ConsoleGuiEdit,
    },
    Project {
        edit: ProjectGuiEdit,
    },
}

/// A value a [`GuiEdit::Put`] writes; the site's field says what it is.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiValue {
    /// A name, which becomes a `snake_case` name the edit reports, or a color.
    Text {
        text: String,
    },
    /// A description, where empty text is none, or a sequence's audio file.
    OptionalText {
        text: Option<String>,
    },
    Seconds {
        seconds: f64,
    },
    /// How long a console clip that plays for the whole session repeats;
    /// none when it does not.
    OptionalSeconds {
        seconds: Option<f64>,
    },
    /// A frame rate.
    Count {
        count: u32,
    },
    /// A clip's layer.
    Id {
        id: u32,
    },
    Target {
        target: FixtureTarget,
    },
    Scope {
        scope: SequenceClipScope,
    },
    /// The effect a clip plays; its required parameters start from
    /// `initial_color`.
    Effect {
        initial_color: String,
        effect: SequenceEffectReference,
    },
    Param {
        value: SequenceParamValue,
    },
    /// The part of an envelope a binding uses.
    Window {
        window: GuiWindow,
    },
    Curve {
        points: Vec<SequenceCurvePoint>,
    },
    Gradient {
        gradient: SequenceGradient,
    },
    Marks {
        marks: Vec<GuiMark>,
    },
    /// An event's actions.
    Actions {
        actions: Vec<ConsoleAction>,
    },
    Controller {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
    },
    InputDevice {
        driver: GuiInputDriver,
        controls: Vec<GuiInputControl>,
    },
    /// The project's routes from fixtures to controllers.
    Routes {
        routes: Vec<GuiPixelRoute>,
    },
}

/// A mark of a collection; a zero duration is instantaneous and a blank
/// label none.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiMark {
    pub(crate) time_seconds: f64,
    pub(crate) duration_seconds: f64,
    pub(crate) label: Option<String>,
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
    /// The edit was refused and the document did not change.
    Refused { problem: GuiProblem },
    /// The edit also changes objects it left alone, which the person confirms
    /// first; until then the document does not change. `changes` says what
    /// changes, a sentence each.
    NeedsConfirmation { changes: Vec<String> },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiEditUpdate {
    pub snapshot: AppSnapshot,
    pub change: GuiDocumentChange,
    /// The sequence selection a selection edit leaves; `None` clears it.
    pub selection: Option<SequenceSelection>,
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
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) points: Vec<SequenceCurvePoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GradientGuiDocument {
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) gradient: SequenceGradient,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectGuiDocument {
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) sequences: Vec<GuiObjectRef>,
    pub consoles: Vec<GuiObjectRef>,
    /// The names a new sequence and console are offered.
    pub(crate) new_sequence_name: String,
    pub(crate) new_console_name: String,
    pub layout_ref: GuiObjectRef,
    /// How the layout's fixtures are wired to the controllers.
    pub outputs: PatchGuiDocument,
    pub controllers: Vec<SetupController>,
    pub input_devices: Vec<SetupInputDevice>,
    /// Every key a new input device's control can read, in the language's order.
    pub(crate) key_codes: Vec<String>,
    pub(crate) input_limits: GuiInputLimits,
    /// The names a new controller and input device are offered.
    pub(crate) new_controller_name: String,
    pub(crate) new_input_device_name: String,
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
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub(crate) controller: SetupController,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupControllerPort {
    pub id: u32,
    /// Free text, unique within the controller; routes refer to it.
    pub name: String,
    /// Empty text means none.
    pub description: Option<String>,
    /// A Donder controller's port's physical output; none on an E1.31 or
    /// Art-Net controller.
    pub donder: Option<SetupDonderOutput>,
}

/// The physical output a Donder controller's port drives.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupDonderOutput {
    /// From 1.
    pub output: u8,
    pub slot_count: u16,
}

/// An E1.31 or Art-Net controller's consecutive universes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiUniverses {
    /// An E1.31 universe, or an Art-Net 15-bit port-address.
    pub start: u16,
    pub count: u16,
    /// Channels per universe.
    pub channels: u16,
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
        universes: GuiUniverses,
        /// From 1; none leaves the controller out of FSEQ files.
        fseq_channel: Option<u32>,
    },
    ArtNet {
        bind_address: String,
        destination: String,
        broadcast: bool,
        universes: GuiUniverses,
        /// From 1; none leaves the controller out of FSEQ files.
        fseq_channel: Option<u32>,
    },
    Donder {
        device: String,
    },
}

impl From<donder_model::Universes> for GuiUniverses {
    fn from(universes: donder_model::Universes) -> Self {
        Self {
            start: universes.start,
            count: universes.count,
            channels: universes.channels,
        }
    }
}

impl From<GuiUniverses> for donder_model::Universes {
    fn from(universes: GuiUniverses) -> Self {
        Self {
            start: universes.start,
            count: universes.count,
            channels: universes.channels,
        }
    }
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
                universes: config.universes.into(),
                fseq_channel: config.fseq_channel.map(NonZeroU32::get),
            },
            ControllerProtocol::ArtNet(config) => Self::ArtNet {
                bind_address: config.bind_address.to_string(),
                destination: config.destination.to_string(),
                broadcast: matches!(config.mode, ArtNetMode::Broadcast),
                universes: config.universes.into(),
                fseq_channel: config.fseq_channel.map(NonZeroU32::get),
            },
            ControllerProtocol::Donder(config) => Self::Donder {
                device: config.device.as_str().to_string(),
            },
        }
    }
}

impl GuiEdit {
    /// Write `value` at `site`.
    pub fn put(site: GuiSite, value: GuiValue) -> Self {
        Self::Put {
            sites: vec![site],
            value,
        }
    }

    /// The site the edit writes: where a refusal shows when the rule it
    /// breaks names no field.
    pub fn site(&self) -> Option<GuiSite> {
        let param = |member, param: &str| {
            GuiSite::of(
                Some(member),
                Some(GuiField::Param {
                    name: param.to_string(),
                }),
            )
        };
        match self {
            Self::Put { sites, .. } => sites.first().cloned(),
            Self::Remove { site } => Some(site.clone()),
            Self::Project { .. } => Some(GuiSite::object(GuiField::Name)),
            Self::Fixture {
                edit: FixtureGuiEdit::MoveHandle { id, .. } | FixtureGuiEdit::ConvertToPixels { id },
            } => Some(GuiSite::of(Some(GuiMember::Shape { id: *id }), None)),
            Self::Console {
                edit: ConsoleGuiEdit::ControlParam { target, .. },
            } => Some(match target {
                SequenceAutomationTarget::ClipParam {
                    clip_id,
                    param: name,
                } => param(GuiMember::Clip { id: *clip_id }, name),
                SequenceAutomationTarget::CompositionNodeParam {
                    node_id,
                    param: name,
                } => param(GuiMember::Node { id: *node_id }, name),
            }),
            _ => None,
        }
    }

    /// The edit, with the object its site names moved to the file at `path`.
    pub fn with_object_path(mut self, path: String) -> Self {
        if let Self::Put { sites, .. } = &mut self {
            for site in sites {
                if let Some(object) = &mut site.object {
                    object.path.clone_from(&path);
                }
            }
        }
        self
    }
}
