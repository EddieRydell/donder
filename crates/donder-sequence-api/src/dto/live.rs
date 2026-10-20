//! Live input editors: input devices and the rigs that play from them.
use crate::*;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InputDeviceGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub object_key: String,
    pub device: SetupInputDevice,
    /// Every key a control can read, by its `KeyboardEvent.code` name, in the
    /// language's order.
    pub key_codes: Vec<String>,
    pub input_limits: GuiInputLimits,
}

/// The highest MIDI inputs a control can read.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiInputLimits {
    pub max_note: u8,
    pub max_control_change: u8,
}

/// An input device as a setup lists it and its editor edits it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct SetupInputDevice {
    /// The declared name, or the name the device is stored under.
    pub name: String,
    pub source_ref: GuiObjectRef,
    pub read_only: bool,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GuiInputControl {
    /// Kept across edits; a new control has an id no other control uses.
    pub id: u32,
    /// Unique within the device; rigs refer to it.
    pub name: String,
    pub description: Option<String>,
    pub kind: GuiInputControlKind,
    pub input: GuiControlInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum GuiInputControlKind {
    /// Pressed or released.
    Button,
    /// A position from 0 to 1.
    Dial,
    /// A range of MIDI notes, each its own press.
    Keys,
}

/// What a control reads. A keyboard reads keys; MIDI buttons read notes and
/// control changes, MIDI dials read control changes and pitch bend, and MIDI
/// keys read a range of notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
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
    /// A MIDI note number, 0 to 127; middle C is 60.
    Note {
        note: u8,
    },
    /// MIDI notes from `low` through `high`; a full piano is 21 to 108.
    Notes {
        low: u8,
        high: u8,
    },
    /// A MIDI control change number, 0 to 119; the sustain pedal is 64.
    ControlChange {
        number: u8,
    },
    PitchBend,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigGuiDocument {
    /// The open object's description, edited with `GuiEditCommand::Description`.
    pub description: Option<String>,
    pub path: String,
    pub source_ref: GuiObjectRef,
    pub object_key: String,
    /// The declared name, or the name the rig is stored under.
    pub name: String,
    /// The rig's position among the project's rigs, which Perform starts it by;
    /// `None` when the project does not list it.
    pub perform_index: Option<u32>,
    pub frame_rate: u32,
    pub live_marks: Vec<RigLiveMarks>,
    pub layers: Vec<SequenceLayer>,
    pub composition_graph: SequenceCompositionGraph,
    pub voices: Vec<RigVoice>,
    pub switches: Vec<RigSwitch>,
    pub mappings: Vec<RigMapping>,
    /// The active layout's fixtures and groups, each once, depth-first.
    pub targets: Vec<SequenceLane>,
    pub effect_definitions: Vec<SequenceEffectDefinition>,
    pub curve_library: Vec<SequenceCurveLibraryItem>,
    pub gradient_library: Vec<SequenceGradientLibraryItem>,
    /// The active setup's input devices, which the rig's inputs choose from.
    pub input_devices: Vec<RigInputDevice>,
    pub max_stacked_voices: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigLiveMarks {
    /// The collection's name; marks parameters and `Mark` mappings refer to it.
    pub key: String,
    pub description: Option<String>,
    pub color: String,
}

/// An effect a rig plays: a clip without timing.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigEffect {
    pub layer_id: u32,
    pub target: FixtureTarget,
    pub target_label: String,
    pub scope: SequenceEffectScope,
    pub effect: SequenceEffectReference,
    pub effect_name: String,
    pub params: Vec<SequenceEffectParam>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigVoice {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub effect: RigEffect,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigControlRef {
    pub device: GuiObjectRef,
    pub control: u32,
}

/// A button a switch or mapping reads, with the names the rig's text uses.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigInput {
    pub button: RigControlRef,
    pub device_name: String,
    pub control_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigInputDevice {
    pub device: GuiObjectRef,
    pub name: String,
    pub buttons: Vec<RigInputControl>,
    /// Dials only drive `Control` mappings.
    pub dials: Vec<RigInputControl>,
    /// Keys only trigger effects and add marks.
    pub keys: Vec<RigInputControl>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigInputControl {
    pub control: u32,
    pub name: String,
    /// Whether it plays notes, as values played from a note need.
    pub notes: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigSwitch {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub input: RigInput,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigMapping {
    pub id: u32,
    pub name: String,
    pub description: Option<String>,
    pub input: RigInput,
    /// `None` acts always.
    pub when: Option<RigWhen>,
    pub action: RigMappingAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigWhen {
    On { switch_id: u32 },
    Off { switch_id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigMappingAction {
    Trigger {
        effect: RigEffect,
        length: RigTriggerLength,
        voices: RigVoicePolicy,
        values: Vec<RigPlayedValue>,
    },
    Mark {
        marks: String,
    },
    Control {
        target: RigControlTarget,
        param: String,
    },
}

/// A parameter each press of a trigger sets on the voice it starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct RigPlayedValue {
    pub param: String,
    pub from: RigPlayedSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum RigPlayedSource {
    /// How hard the note was struck; a computer key or pedal plays at 1.
    Velocity,
    /// The note's place in its key range.
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
pub enum RigTriggerLength {
    Fixed { duration_seconds: f64 },
    Held,
    Latched,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigVoicePolicy {
    Retrigger,
    Stack { count: u32 },
    Mono,
}

/// The voices a `Control` mapping sets a parameter of.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigControlTarget {
    Voice {
        voice_id: u32,
    },
    /// Every voice of a `Trigger` mapping.
    Trigger {
        mapping_id: u32,
    },
}

/// A named item of a rig.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigItem {
    LiveMarks { key: String },
    Voice { id: u32 },
    Switch { id: u32 },
    Mapping { id: u32 },
}

/// The effect of a voice or of a `Trigger` mapping.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigEffectOwner {
    Voice { id: u32 },
    Trigger { mapping_id: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigEffectEdit {
    Layer {
        layer_id: u32,
    },
    Target {
        target: FixtureTarget,
    },
    Scope {
        scope: SequenceEffectScope,
    },
    /// Play another effect; its required parameters start from `initial_color`.
    Definition {
        initial_color: String,
        effect: SequenceEffectReference,
    },
    Param {
        name: String,
        value: SequenceEffectParamValue,
    },
}

/// A mapping's action as the editor chooses it.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigActionChoice {
    /// A new trigger plays on the default layer for one second, restarting on
    /// each press.
    Trigger {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
    Mark {
        marks: String,
    },
    Control {
        target: RigControlTarget,
        param: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum RigGuiEdit {
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
    SetLiveMarksColor {
        key: String,
        color: String,
    },
    AddVoice {
        initial_color: String,
        effect: SequenceEffectReference,
        target: FixtureTarget,
    },
    AddSwitch {
        input: RigControlRef,
    },
    AddMapping {
        input: RigControlRef,
        action: RigActionChoice,
    },
    /// Renaming live marks renames every reference to them.
    Rename {
        item: RigItem,
        name: String,
    },
    /// Empty text removes the description.
    SetDescription {
        item: RigItem,
        description: Option<String>,
    },
    /// Deleting live marks leaves the marks parameters that named them without marks.
    Delete {
        item: RigItem,
    },
    EditEffect {
        owner: RigEffectOwner,
        edit: RigEffectEdit,
    },
    SetSwitchInput {
        id: u32,
        input: RigControlRef,
    },
    SetMappingInput {
        id: u32,
        input: RigControlRef,
    },
    SetMappingWhen {
        id: u32,
        when: Option<RigWhen>,
    },
    SetMappingAction {
        id: u32,
        action: RigActionChoice,
    },
    SetTriggerLength {
        mapping_id: u32,
        length: RigTriggerLength,
    },
    SetTriggerVoices {
        mapping_id: u32,
        voices: RigVoicePolicy,
    },
    SetTriggerValues {
        mapping_id: u32,
        values: Vec<RigPlayedValue>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectGuiEdit {
    /// A new rig stored in the project, with the default layer drawn to the output.
    AddRig { initial_color: String },
}
