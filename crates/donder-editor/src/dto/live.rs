//! Live input editors: input devices and the consoles that play from them.
use crate::dto::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceGuiDocument {
    /// The open object's description, edited with a description `GuiEdit::Put`.
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

/// An input device as the project lists it and its editor edits it.
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

/// What a device sent while an input was being learned: the kind of control it
/// suits, and the input it reads. A MIDI input carries the channel it came on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LearnedInput {
    pub kind: GuiInputControlKind,
    pub input: GuiControlInput,
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
    /// The open object's description, edited with a description `GuiEdit::Put`.
    pub(crate) description: Option<String>,
    pub(crate) source_ref: GuiObjectRef,
    /// The declared name.
    pub(crate) name: String,
    /// The console's position among the project's consoles, which Perform
    /// starts it by.
    pub perform_index: u32,
    pub(crate) frame_rate: u32,
    /// Each starts empty every session; `Mark` actions add to it.
    pub(crate) mark_collections: Vec<SequenceMarkCollection>,
    pub layers: Vec<SequenceLayer>,
    pub composition_graph: SequenceCompositionGraph,
    pub clips: Vec<GuiClip>,
    /// The clips that play for the whole session, by id.
    pub(crate) always: Vec<u32>,
    pub events: Vec<ConsoleOn>,
    /// The active layout's fixtures and groups, each once, depth-first.
    pub targets: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub(crate) curve_library: Vec<SequenceCurveLibraryItem>,
    pub(crate) gradient_library: Vec<SequenceGradientLibraryItem>,
    /// The project's input devices, which the console's events choose from.
    pub input_devices: Vec<ConsoleInputDevice>,
    /// The most copies of a clip `Play` actions play at once.
    pub(crate) max_copies: u32,
    /// The length a clip newly played from the session's start repeats every.
    pub(crate) held_length_seconds: f64,
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
    /// What the control reads.
    pub(crate) input: GuiControlInput,
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
    },
    /// Play one copy of a clip until it is stopped, repeating every
    /// `length_seconds`.
    Start {
        clip_id: u32,
        length_seconds: f64,
    },
    Stop {
        clip_id: u32,
    },
    Toggle {
        clip_id: u32,
        length_seconds: f64,
    },
    /// Add a mark to the live mark collection `into`, by id.
    Mark {
        into: u32,
    },
    /// Write the event's value into a parameter: an axis's position, a
    /// press's velocity, or 0 for a release.
    Set {
        target: SequenceAutomationTarget,
        window: GuiWindow,
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

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleLengthSource {
    /// The press's velocity; a computer key presses at 1.
    Velocity,
    /// An axis's position; an axis not yet moved reads 0.
    Axis { control: ConsoleControlRef },
}

/// A parameter a copy takes from how the press that started it was played.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ConsolePlayedValue {
    pub(crate) param: String,
    pub(crate) from: ConsolePlayedQuantity,
    pub(crate) window: GuiWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConsolePlayedQuantity {
    /// How hard the note was struck; a computer key or pedal plays at 1.
    Velocity,
    /// The note's name, C to B up the chromatic scale.
    PitchClass,
}

/// What sets a parameter live.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleParamSource {
    /// An axis when moved, or a button by the press's velocity and 0 on the
    /// release.
    Control { control: ConsoleControlRef },
    /// How the press that started each copy of the clip was played; only a
    /// clip's parameter.
    Played { from: ConsolePlayedQuantity },
}

/// How a control plays a new effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum ConsolePlayMode {
    /// A copy that plays for a second.
    ForTime,
    /// One copy from the press to the release.
    Held,
    /// Start it, or stop it when it plays.
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ConsoleGuiEdit {
    /// A new effect and the actions that play it from a control. A press or
    /// release plays it for a second or toggles it, and `Held` starts it on the
    /// press and stops it on the release.
    AddEffectAction {
        control: ConsoleControlRef,
        event: ConsoleEventKind,
        mode: ConsolePlayMode,
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
    /// A new effect that plays for the whole session.
    AddAlwaysEffect {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
    /// An action added to what a control does on an event.
    AddAction {
        control: ConsoleControlRef,
        event: ConsoleEventKind,
        action: ConsoleAction,
    },
    /// Link a parameter to what sets it live, or to nothing; whatever set it
    /// before lets go. The value reaches the parameter through `window`.
    ControlParam {
        target: SequenceAutomationTarget,
        source: Option<ConsoleParamSource>,
        window: GuiWindow,
    },
    Composition {
        edit: CompositionGuiEdit,
    },
    /// A new clip on the first layer over the whole target, named after its
    /// effect.
    AddClip {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectGuiEdit {
    /// A new sequence of the project in a file of its own named `name`, with
    /// the default layer drawn to the output.
    AddSequence { initial_color: String, name: String },
    /// A new console of the project in a file of its own named `name`, with
    /// the default layer drawn to the output.
    AddConsole { initial_color: String, name: String },
    AddController {
        config: SetupControllerConfig,
        ports: Vec<SetupControllerPort>,
        /// The name of the controller and of its file.
        name: String,
    },
    AddInputDevice {
        driver: GuiInputDriver,
        controls: Vec<GuiInputControl>,
        /// The name of the device and of its file.
        name: String,
    },
}
