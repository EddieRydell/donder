//! A console: a sequence built live from input devices. Each action edits it
//! at the time of the event that runs it: `Play` and `Start`..`Stop` place
//! clips, `Mark` adds a mark and `Set` writes an automation value. Its layers
//! and composition graph are a sequence's.
use crate::effect::{ClipScope, EffectDefinitionId, ParamValue};
pub(crate) use crate::identity::ConsoleId;
use crate::input_device::ControlRef;
use crate::layout::FixtureTarget;
use crate::sequence::{
    AutomationTarget, MarkCollectionKey, SequenceCompositionGraph, SequenceLayer, SequenceLayerId,
};
use donder_language::DonderDuration;
use donder_runtime_types::{Color, Identifier};
use indexmap::IndexMap;

/// The most copies of a clip that `Play` actions play at once.
pub const MAX_COPIES: u8 = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Console {
    pub id: ConsoleId,
    pub description: Option<String>,
    pub frame_rate: u32,
    pub live_marks: Vec<LiveMarks>,
    pub automations: Vec<LiveAutomation>,
    pub layers: Vec<SequenceLayer>,
    pub clips: Vec<ConsoleClip>,
    /// Clips that play for the whole session.
    pub always: Vec<ConsoleClipId>,
    pub events: Vec<On>,
    pub composition_graph: SequenceCompositionGraph,
}

/// A mark collection that starts empty each session and gains a mark each
/// time a `Mark` action runs.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveMarks {
    pub key: MarkCollectionKey,
    pub description: Option<String>,
    pub display_color: Color,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct LiveAutomationId(pub u32);

/// A value in `0..1` that `Set` actions write, mapped onto each bound
/// parameter's declared range as sequence automation is. It holds `value`
/// until the first `Set`.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveAutomation {
    pub id: LiveAutomationId,
    pub name: Identifier,
    pub description: Option<String>,
    pub value: f32,
    /// The parameters it sets: a clip's, on every copy of the clip, or an
    /// operator node's.
    pub bindings: Vec<ConsoleAutomationTarget>,
}

/// A parameter a live automation sets, bound as sequence automation binds.
pub type ConsoleAutomationTarget = AutomationTarget<ConsoleClipId>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ConsoleClipId(pub u32);

/// A clip a console plays: a sequence clip without timing.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleClip {
    pub id: ConsoleClipId,
    pub name: Identifier,
    pub description: Option<String>,
    pub layer_id: SequenceLayerId,
    pub target: FixtureTarget,
    pub scope: ClipScope,
    pub definition: EffectDefinitionId,
    /// Marks parameters name the console's live mark collections.
    pub param_overrides: IndexMap<Identifier, ParamValue>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct OnId(pub u32);

/// What an input event does: its actions, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct On {
    pub id: OnId,
    pub event: InputEvent,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum InputEvent {
    /// A button pressed; a press of a held button does nothing.
    Press(ControlRef),
    Release(ControlRef),
    /// An axis moved.
    Move(ControlRef),
}

impl InputEvent {
    pub fn control(&self) -> &ControlRef {
        match self {
            Self::Press(control) | Self::Release(control) | Self::Move(control) => control,
        }
    }

    pub fn control_mut(&mut self) -> &mut ControlRef {
        match self {
            Self::Press(control) | Self::Release(control) | Self::Move(control) => control,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Play a copy of a clip for its length, on top of the copies playing.
    Play(Play),
    /// Play one copy of a clip until it is stopped; nothing while it plays.
    Start {
        clip: ConsoleClipId,
        values: Vec<PlayedValue>,
    },
    /// Stop every playing copy of a clip.
    Stop { clip: ConsoleClipId },
    /// `Start` while no copy of the clip plays, else `Stop`.
    Toggle {
        clip: ConsoleClipId,
        values: Vec<PlayedValue>,
    },
    /// Add a mark to a live collection.
    Mark(MarkCollectionKey),
    /// Write the event's value into a live automation: an axis's position,
    /// a press's velocity, or 0 for a release.
    Set(LiveAutomationId),
}

impl Action {
    /// The clip the action starts or stops.
    pub fn clip(&self) -> Option<ConsoleClipId> {
        match self {
            Self::Play(play) => Some(play.clip),
            Self::Start { clip, .. } | Self::Stop { clip } | Self::Toggle { clip, .. } => {
                Some(*clip)
            }
            Self::Mark(_) | Self::Set(_) => None,
        }
    }

    /// The parameters the action sets on the copy it starts.
    pub fn values(&self) -> &[PlayedValue] {
        match self {
            Self::Play(play) => &play.values,
            Self::Start { values, .. } | Self::Toggle { values, .. } => values,
            Self::Stop { .. } | Self::Mark(_) | Self::Set(_) => &[],
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub clip: ConsoleClipId,
    /// How many copies of the clip play at once, from 1 to [`MAX_COPIES`];
    /// when all play, the oldest is reused.
    pub copies: u8,
    pub length: PlayLength,
    /// Parameters the press sets on the copy it starts.
    pub values: Vec<PlayedValue>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayLength {
    Fixed(DonderDuration),
    /// From `shortest` at 0 to `longest` at 1 of a value read when the copy
    /// starts.
    Scaled {
        by: LengthSource,
        shortest: DonderDuration,
        longest: DonderDuration,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum LengthSource {
    /// The press's velocity; a computer key presses at 1.
    Velocity,
    /// A live automation's current value.
    Automation(LiveAutomationId),
}

impl PlayLength {
    /// A length every copy plays; `None` when each start sets its own.
    /// Admission checked that a fixed length fits the playback clock.
    pub fn fixed(&self) -> Option<donder_runtime_types::SampleDuration> {
        match self {
            Self::Fixed(duration) => Some(sample_duration(duration)),
            Self::Scaled { .. } => None,
        }
    }
}

/// A validated duration on the playback clock.
pub fn sample_duration(duration: &DonderDuration) -> donder_runtime_types::SampleDuration {
    donder_runtime_types::SampleDuration::from_ticks(duration.as_micros_rounded() as u32)
}

/// A parameter a press sets on the copy it starts, from how it was played.
/// The value's `0..1` maps onto the parameter's range as automation does.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlayedValue {
    pub param: Identifier,
    pub from: PlayedQuantity,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlayedQuantity {
    /// How hard a note was struck; a computer key or a pedal plays at 1.
    Velocity,
    /// The note's place in its button's note range, from its lowest note at
    /// 0 to its highest at 1; a button of one note is at its place among all
    /// 128 notes.
    Pitch,
    /// The note's name, from C at 0 up the chromatic scale to B at 11/12.
    PitchClass,
    /// The note's name round the circle of fifths: C at 0, G at 1/12, D at
    /// 2/12, and on to F at 11/12.
    Fifths,
}

impl PlayedQuantity {
    /// Whether the value needs a note, which only MIDI note buttons carry.
    pub fn needs_note(self) -> bool {
        !matches!(self, Self::Velocity)
    }
}

/// How a clip plays: copies `Play` actions spawn, or one copy that `Start`,
/// `Toggle` or `always` plays until stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipUse {
    /// Up to this many copies.
    Played(u8),
    Held,
}

impl Console {
    pub fn clip(&self, id: ConsoleClipId) -> Option<&ConsoleClip> {
        self.clips.iter().find(|clip| clip.id == id)
    }

    pub fn automation(&self, id: LiveAutomationId) -> Option<&LiveAutomation> {
        self.automations
            .iter()
            .find(|automation| automation.id == id)
    }

    /// The live automation that sets `target`; validation allows one.
    pub fn automation_setting(&self, target: &ConsoleAutomationTarget) -> Option<&LiveAutomation> {
        self.automations
            .iter()
            .find(|automation| automation.bindings.contains(target))
    }

    /// Forget every live automation binding to a deleted clip.
    pub fn forget_clip(&mut self, clip: ConsoleClipId) {
        self.remove_bindings(
            |target| matches!(target, AutomationTarget::ClipParam { clip_id, .. } if *clip_id == clip),
        );
    }

    /// Forget every live automation binding to a deleted graph node.
    pub fn forget_node(&mut self, node: &crate::sequence::CompositionGraphNodeId) {
        self.remove_bindings(|target| {
            matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == node)
        });
    }

    fn remove_bindings(&mut self, matches: impl Fn(&ConsoleAutomationTarget) -> bool) {
        for automation in &mut self.automations {
            automation.bindings.retain(|target| !matches(target));
        }
    }

    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.events.iter().flat_map(|on| &on.actions)
    }

    /// How `clip` plays: as many copies as its `Play` actions ask for at
    /// most, or held. A clip only stopped plays as held, never starting.
    pub fn clip_use(&self, clip: ConsoleClipId) -> ClipUse {
        self.actions()
            .filter_map(|action| match action {
                Action::Play(play) if play.clip == clip => Some(play.copies),
                _ => None,
            })
            .max()
            .map_or(ClipUse::Held, ClipUse::Played)
    }

    /// Every control the console reads, in event order.
    pub fn inputs(&self) -> impl Iterator<Item = &ControlRef> {
        self.events.iter().map(|on| on.event.control())
    }
}

impl crate::ownership::Identified for Console {
    type Id = ConsoleId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type OwnedConsole = crate::ownership::OwnedMember<Console, ConsoleId>;
