//! Live input editors: input devices and the consoles that play from them.
use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) object_key: String,
    pub device: SetupInputDevice,
    /// Every key a control can read, by its `KeyboardEvent.code` name, in the
    /// language's order.
    pub key_codes: Vec<String>,
    pub(crate) input_limits: GuiInputLimits,
}

/// The highest MIDI inputs a control can read.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiInputLimits {
    pub(crate) max_note: u8,
    pub(crate) max_control_change: u8,
    /// Channels run from 1 to this.
    pub(crate) max_channel: u8,
}

/// An input device as a setup lists it and its editor edits it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupInputDevice {
    /// The declared name, or the name the device is stored under.
    pub name: String,
    pub source_ref: GuiObjectRef,
    pub driver: GuiInputDriver,
    pub controls: Vec<GuiInputControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiInputDriver {
    Keyboard,
    /// MIDI input ports whose name contains `port`, or every port.
    Midi {
        port: Option<String>,
    },
    /// OSC messages sent to `port` over UDP, on every network interface.
    Osc {
        port: u16,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiInputControl {
    /// Kept across edits; a new control has an id no other control uses.
    pub id: u32,
    /// Unique within the device; consoles refer to it.
    pub name: String,
    pub description: Option<String>,
    pub kind: GuiInputControlKind,
    pub input: GuiControlInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiInputControlKind {
    /// Pressed or released; a button reading a range of notes is pressed by
    /// each of them.
    Button,
    /// A position from 0 to 1.
    Axis,
}

/// What a control reads. A keyboard reads keys; MIDI buttons read a range of
/// notes or a control change, and MIDI axes a control change or pitch bend.
/// OSC buttons read an address, and OSC axes an address and the range its
/// value runs over. A MIDI input reads its `channel`, from 1 to 16, or every
/// channel when it is `None`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum GuiControlInput {
    /// A keyboard key by its `KeyboardEvent.code` name.
    Key {
        code: String,
    },
    /// MIDI notes from `low` through `high`, each its own press; one note
    /// when `low` is `high`. A full piano is 21 to 108 and middle C is 60.
    Notes {
        low: u8,
        high: u8,
        channel: Option<u8>,
    },
    /// A MIDI control change number, 0 to 119; the sustain pedal is 64.
    ControlChange {
        number: u8,
        channel: Option<u8>,
    },
    PitchBend {
        channel: Option<u8>,
    },
    /// An OSC address, such as `/chord/change`.
    Address {
        path: String,
    },
    /// An OSC address whose first argument runs from `low` to `high`.
    AddressRange {
        path: String,
        low: f32,
        high: f32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::SetDescription`.
    pub(crate) description: Option<String>,
    pub(crate) path: String,
    pub(crate) source_ref: GuiObjectRef,
    pub(crate) object_key: String,
    /// The declared name, or the name the console is stored under.
    pub(crate) name: String,
    /// The console's position among the project's consoles, which Perform
    /// starts it by; `None` when the project does not list it.
    pub perform_index: Option<u32>,
    pub(crate) frame_rate: u32,
    pub(crate) live_marks: Vec<ConsoleLiveMarks>,
    pub(crate) automations: Vec<ConsoleAutomation>,
    pub layers: Vec<SequenceLayer>,
    pub composition_graph: SequenceCompositionGraph,
    pub clips: Vec<ConsoleClip>,
    /// The clips that play for the whole session, by id.
    pub(crate) always: Vec<u32>,
    pub events: Vec<ConsoleOn>,
    /// The active layout's fixtures and groups, each once, depth-first.
    pub targets: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub(crate) curve_library: Vec<SequenceCurveLibraryItem>,
    pub(crate) gradient_library: Vec<SequenceGradientLibraryItem>,
    /// The active setup's input devices, which the console's events choose from.
    pub input_devices: Vec<ConsoleInputDevice>,
    /// The most copies of a clip `Play` actions play at once.
    pub(crate) max_copies: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleLiveMarks {
    /// The collection's name; marks parameters and `Mark` actions refer to it.
    pub(crate) key: String,
    pub(crate) description: Option<String>,
    pub(crate) color: String,
}

/// A value in `0..=1` that `Set` actions write, mapped onto each bound
/// parameter's range.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleAutomation {
    pub(crate) id: u32,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// The value it holds until the first `Set`.
    pub(crate) value: f32,
    /// The parameters it sets: of console clips or of graph operator nodes.
    pub(crate) bindings: Vec<SequenceAutomationTarget>,
}

/// A clip a console plays: a sequence clip without timing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleClip {
    pub id: u32,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub layer_id: u32,
    pub(crate) target: FixtureTarget,
    pub(crate) target_label: String,
    pub(crate) scope: SequenceClipScope,
    pub(crate) effect: SequenceEffectReference,
    pub(crate) effect_name: String,
    /// A parameter a live automation sets carries that automation's id as
    /// its `automation`.
    pub(crate) params: Vec<SequenceClipParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleControlRef {
    pub device: GuiObjectRef,
    pub control: u32,
}

/// A control an event reads, with the names the console's text uses.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleInput {
    pub(crate) control: ConsoleControlRef,
    pub(crate) device_name: String,
    pub(crate) control_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleInputDevice {
    pub device: GuiObjectRef,
    pub(crate) name: String,
    pub buttons: Vec<ConsoleInputControl>,
    pub axes: Vec<ConsoleInputControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleInputControl {
    pub control: u32,
    pub(crate) name: String,
    /// Whether it plays notes, as values played from a note need.
    pub(crate) notes: bool,
}

/// What an input event does.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsoleOn {
    pub id: u32,
    pub(crate) event: ConsoleEventKind,
    pub(crate) input: ConsoleInput,
    pub actions: Vec<ConsoleAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConsoleEventKind {
    /// A button pressed.
    Press,
    Release,
    /// An axis moved.
    Move,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleAction {
    /// Play a copy of a clip for its length, on top of the copies playing.
    Play {
        clip_id: u32,
        copies: u32,
        length: ConsolePlayLength,
        values: Vec<ConsolePlayedValue>,
    },
    /// Play one copy of a clip until it is stopped.
    Start {
        clip_id: u32,
        values: Vec<ConsolePlayedValue>,
    },
    Stop {
        clip_id: u32,
    },
    Toggle {
        clip_id: u32,
        values: Vec<ConsolePlayedValue>,
    },
    /// Add a mark to the live mark collection `into`.
    Mark {
        into: String,
    },
    /// Write the event's value into the live automation `into`.
    Set {
        into: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsolePlayLength {
    Fixed {
        duration_seconds: f64,
    },
    /// From `shortest_seconds` at 0 to `longest_seconds` at 1 of `by`.
    Scaled {
        by: ConsoleLengthSource,
        shortest_seconds: f64,
        longest_seconds: f64,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleLengthSource {
    /// The press's velocity; a computer key presses at 1.
    Velocity,
    Automation {
        automation_id: u32,
    },
}

/// A parameter a press sets on the copy it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsolePlayedValue {
    pub(crate) param: String,
    pub(crate) from: ConsolePlayedQuantity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConsolePlayedQuantity {
    /// How hard the note was struck; a computer key or pedal plays at 1.
    Velocity,
    /// The note's place in its button's note range.
    Pitch,
    /// The note's name, C to B up the chromatic scale.
    PitchClass,
    /// The note's name round the circle of fifths.
    Fifths,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleClipEdit {
    Layer {
        layer_id: u32,
    },
    Target {
        target: FixtureTarget,
    },
    Scope {
        scope: SequenceClipScope,
    },
    /// Play another effect; its required parameters start from
    /// `initial_color`, and the played values and automation bindings of the
    /// old effect's parameters go.
    Definition {
        initial_color: String,
        effect: SequenceEffectReference,
    },
    Param {
        name: String,
        value: SequenceParamValue,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleGuiEdit {
    SetFrameRate {
        frame_rate: u32,
    },
    Composition {
        edit: CompositionGuiEdit,
    },
    /// The name becomes a unique `snake_case` name.
    AddLiveMarks {
        name: String,
        color: String,
    },
    /// A new automation at 0 with no bindings; the name becomes a unique
    /// `snake_case` name.
    AddAutomation {
        name: String,
    },
    SetAutomationValue {
        id: u32,
        value: f32,
    },
    SetAutomationBindings {
        id: u32,
        bindings: Vec<SequenceAutomationTarget>,
    },
    /// A new clip on the first layer over the whole target, named after its
    /// effect.
    AddClip {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
    EditClip {
        id: u32,
        edit: ConsoleClipEdit,
    },
    SetAlways {
        clip_ids: Vec<u32>,
    },
    /// A new event with no actions.
    AddEvent {
        event: ConsoleEventKind,
        input: ConsoleControlRef,
    },
    /// Change what an event answers. Actions the new event cannot run go:
    /// all but `Set` for a move, played values for all but a press, and
    /// values played from a note for a control without notes.
    SetEvent {
        id: u32,
        event: ConsoleEventKind,
        input: ConsoleControlRef,
    },
    SetActions {
        id: u32,
        actions: Vec<ConsoleAction>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectGuiEdit {
    /// A new console stored in the project, with the default layer drawn to the output.
    AddConsole { initial_color: String },
}
