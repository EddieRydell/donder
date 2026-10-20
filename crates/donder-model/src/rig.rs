//! A rig: effects played live from input devices instead of at authored
//! times. Its layers and composition graph are a sequence's.
use crate::effect::{EffectParamValue, EffectRef, EffectScope};
use crate::identity::ObjectIdentity;
use crate::input_device::ControlRef;
use crate::layout::FixtureTarget;
use crate::sequence::{
    MarkCollectionKey, SequenceCompositionGraph, SequenceLayer, SequenceLayerId,
};
use donder_language::DonderDuration;
use donder_runtime_types::{Color, Identifier};
use indexmap::IndexMap;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct RigId(pub ObjectIdentity);

/// The most voices a `Stack` trigger plays at once.
pub const MAX_STACKED_VOICES: u8 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Rig {
    pub id: RigId,
    pub description: Option<String>,
    pub frame_rate: u32,
    pub live_marks: Vec<LiveMarks>,
    pub layers: Vec<SequenceLayer>,
    /// The scene: voices that play for the whole session.
    pub voices: Vec<Voice>,
    pub switches: Vec<Switch>,
    pub mappings: Vec<Mapping>,
    pub composition_graph: SequenceCompositionGraph,
}

/// A mark collection that starts empty each session and gains a mark each
/// time a `Mark` mapping fires.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveMarks {
    pub key: MarkCollectionKey,
    pub description: Option<String>,
    pub display_color: Color,
}

/// An effect a rig plays: a clip without timing.
#[derive(Clone, Debug, PartialEq)]
pub struct RigEffect {
    pub layer_id: SequenceLayerId,
    pub target: FixtureTarget,
    pub scope: EffectScope,
    pub definition: EffectRef,
    /// Marks parameters name the rig's live mark collections.
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RigVoiceId(pub u32);

/// An always-on effect for the whole session.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub id: RigVoiceId,
    /// Unique among the rig's voices and mappings.
    pub name: Identifier,
    pub description: Option<String>,
    pub effect: RigEffect,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RigSwitchId(pub u32);

/// A latching state, off at the start of a session, that each press of its
/// button toggles.
#[derive(Clone, Debug, PartialEq)]
pub struct Switch {
    pub id: RigSwitchId,
    pub name: Identifier,
    pub description: Option<String>,
    pub input: ControlRef,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct RigMappingId(pub u32);

/// What a button or dial does while its condition holds.
#[derive(Clone, Debug, PartialEq)]
pub struct Mapping {
    pub id: RigMappingId,
    /// Unique among the rig's voices and mappings.
    pub name: Identifier,
    pub description: Option<String>,
    pub input: ControlRef,
    /// `None` acts always.
    pub when: Option<When>,
    pub action: MappingAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum When {
    On(RigSwitchId),
    Off(RigSwitchId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum MappingAction {
    /// Start an effect voice.
    Trigger(Box<Trigger>),
    /// Add a mark to a live collection.
    Mark(MarkCollectionKey),
    /// Hold the parameter at the top of its range while a button is held,
    /// and at the bottom otherwise; a dial sets it anywhere in its range.
    Control {
        target: ControlTarget,
        param: Identifier,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Trigger {
    pub effect: RigEffect,
    pub length: TriggerLength,
    pub voices: VoicePolicy,
    /// Parameters each press sets on the voice it starts.
    pub values: Vec<PlayedValue>,
}

/// A parameter a press sets on the voice it starts, from how it was played.
/// The value's `0..1` maps onto the parameter's range as automation does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayedValue {
    pub param: Identifier,
    pub from: PlayedSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlayedSource {
    /// How hard a note was struck; a computer key or a pedal plays at 1.
    Velocity,
    /// The note's place in its control's key range, from its lowest key at 0
    /// to its highest at 1; a single note button is at its place among all
    /// 128 notes.
    Pitch,
    /// The note's name, from C at 0 up the chromatic scale to B at 11/12.
    PitchClass,
    /// The note's name round the circle of fifths: C at 0, G at 1/12, D at
    /// 2/12, and on to F at 11/12.
    Fifths,
}

impl PlayedSource {
    /// Whether the value needs a note, which only MIDI notes and keys carry.
    pub fn needs_note(self) -> bool {
        !matches!(self, Self::Velocity)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TriggerLength {
    /// Plays for this long from each press.
    Fixed(DonderDuration),
    /// Plays while the button is held.
    Held,
    /// A press starts it and the next press stops it.
    Latched,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VoicePolicy {
    /// One voice, restarted by each press.
    Retrigger,
    /// Up to `count` voices; each press takes the next, reusing the oldest.
    Stack { count: u8 },
    /// One voice; presses while it plays are ignored.
    Mono,
}

/// The voices a `Control` mapping sets a parameter of.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ControlTarget {
    Voice(RigVoiceId),
    /// Every voice of a `Trigger` mapping.
    Trigger(RigMappingId),
}

impl Rig {
    /// The effect a `Control` mapping's target plays.
    pub fn control_effect(&self, target: ControlTarget) -> Option<&RigEffect> {
        match target {
            ControlTarget::Voice(id) => self
                .voices
                .iter()
                .find(|voice| voice.id == id)
                .map(|voice| &voice.effect),
            ControlTarget::Trigger(id) => self
                .mappings
                .iter()
                .find(|mapping| mapping.id == id)
                .and_then(|mapping| match &mapping.action {
                    MappingAction::Trigger(trigger) => Some(&trigger.effect),
                    MappingAction::Mark(_) | MappingAction::Control { .. } => None,
                }),
        }
    }

    /// Every effect the rig plays: its voices', then its triggers'.
    pub fn effects(&self) -> impl Iterator<Item = &RigEffect> {
        self.voices
            .iter()
            .map(|voice| &voice.effect)
            .chain(
                self.mappings
                    .iter()
                    .filter_map(|mapping| match &mapping.action {
                        MappingAction::Trigger(trigger) => Some(&trigger.effect),
                        MappingAction::Mark(_) | MappingAction::Control { .. } => None,
                    }),
            )
    }

    pub fn effects_mut(&mut self) -> impl Iterator<Item = &mut RigEffect> {
        self.voices.iter_mut().map(|voice| &mut voice.effect).chain(
            self.mappings
                .iter_mut()
                .filter_map(|mapping| match &mut mapping.action {
                    MappingAction::Trigger(trigger) => Some(&mut trigger.effect),
                    MappingAction::Mark(_) | MappingAction::Control { .. } => None,
                }),
        )
    }

    /// Every control the rig reads: its switches', then its mappings'.
    pub fn inputs(&self) -> impl Iterator<Item = &ControlRef> {
        self.switches
            .iter()
            .map(|switch| &switch.input)
            .chain(self.mappings.iter().map(|mapping| &mapping.input))
    }

    pub(crate) fn inputs_mut(&mut self) -> impl Iterator<Item = &mut ControlRef> {
        self.switches
            .iter_mut()
            .map(|switch| &mut switch.input)
            .chain(self.mappings.iter_mut().map(|mapping| &mut mapping.input))
    }
}

impl crate::ownership::Identified for Rig {
    type Id = RigId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type RigSource = crate::ownership::ValueSource<Box<Rig>, RigId>;

impl AsRef<ObjectIdentity> for RigId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
