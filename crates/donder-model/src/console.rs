//! A console: a sequence built live from input devices. Each action edits it
//! at the time of the event that runs it: `Play` and `Start`..`Stop` place
//! clips, `Mark` adds a mark and `Set` writes a parameter's value. Its layers
//! and composition graph are a sequence's.
use crate::effect::{ClipScope, EffectDefinitionId, ParamValue};
pub(crate) use crate::identity::ConsoleId;
use crate::input_device::ControlRef;
use crate::layout::FixtureTarget;
use crate::sequence::{
    AutomationTarget, MarkCollectionKey, SequenceCompositionGraph, SequenceLayer, SequenceLayerId,
    Window,
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

/// A parameter a `Set` action writes: a clip's, on every copy of the clip, or
/// an operator node's. The value's `0..1`, through the action's window, maps
/// onto the parameter's declared range as sequence automation does, and the
/// parameter rests at the window's low end until the first `Set`.
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
    /// Parameters each copy takes from the press that started it.
    pub played: Vec<PlayedValue>,
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
    Start { clip: ConsoleClipId },
    /// Stop every playing copy of a clip.
    Stop { clip: ConsoleClipId },
    /// `Start` while no copy of the clip plays, else `Stop`.
    Toggle { clip: ConsoleClipId },
    /// Add a mark to a live collection.
    Mark(MarkCollectionKey),
    /// Write the event's value into a parameter: an axis's position, a
    /// press's velocity, or 0 for a release, through `window`.
    Set {
        target: ConsoleAutomationTarget,
        window: Window,
    },
}

impl Action {
    /// The clip the action starts or stops.
    pub fn clip(&self) -> Option<ConsoleClipId> {
        match self {
            Self::Play(play) => Some(play.clip),
            Self::Start { clip } | Self::Stop { clip } | Self::Toggle { clip } => Some(*clip),
            Self::Mark(_) | Self::Set { .. } => None,
        }
    }

    /// The clip the action starts a copy of, from the event's press.
    pub fn started(&self) -> Option<ConsoleClipId> {
        match self {
            Self::Play(Play { clip, .. }) | Self::Start { clip } | Self::Toggle { clip } => {
                Some(*clip)
            }
            Self::Stop { .. } | Self::Mark(_) | Self::Set { .. } => None,
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

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum LengthSource {
    /// The press's velocity; a computer key presses at 1.
    Velocity,
    /// An axis's current position; an axis not yet moved reads 0.
    Axis(ControlRef),
}

impl PlayLength {
    /// A length every copy plays; `None` when each start sets its own.
    /// Admission checked that a fixed length fits the playback clock.
    pub fn fixed(&self) -> Option<donder_runtime_types::SampleDuration> {
        match self {
            Self::Fixed(duration) => Some(crate::clock::sample_duration(duration)),
            Self::Scaled { .. } => None,
        }
    }
}

/// A parameter a copy of a clip takes from how the press that started it was
/// played.
/// The value's `0..1`, through `window`, maps onto the parameter's range as
/// automation does.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayedValue {
    pub param: Identifier,
    pub from: PlayedQuantity,
    pub window: Window,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PlayedQuantity {
    /// How hard a note was struck; a computer key or a pedal plays at 1.
    Velocity,
    /// The note's name, from C at 0 up the chromatic scale to B at 11/12.
    PitchClass,
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

    /// The parameters `Set` actions write, each once, in the order the
    /// actions first name them: the order playback numbers them in.
    pub fn set_targets(&self) -> Vec<&ConsoleAutomationTarget> {
        let mut targets: Vec<&ConsoleAutomationTarget> = Vec::new();
        for action in self.actions() {
            if let Action::Set { target, .. } = action
                && !targets.contains(&target)
            {
                targets.push(target);
            }
        }
        targets
    }

    /// Where a parameter `Set`s write rests until the first: its window's low
    /// end, the value of an axis not yet moved or of a released button.
    pub fn rest(&self, target: &ConsoleAutomationTarget) -> f32 {
        self.actions()
            .find_map(|action| match action {
                Action::Set {
                    target: set,
                    window,
                } if set == target => Some(window.low),
                _ => None,
            })
            .unwrap_or_else(|| unreachable!("set targets name parameters a `Set` writes"))
    }

    /// The control whose events `Set` `target`; validation allows one.
    pub fn setter(&self, target: &ConsoleAutomationTarget) -> Option<&ControlRef> {
        self.events
            .iter()
            .find(|on| {
                on.actions.iter().any(
                    |action| matches!(action, Action::Set { target: set, .. } if set == target),
                )
            })
            .map(|on| on.event.control())
    }

    /// Forget every `Set` of a deleted clip's parameters.
    pub fn forget_clip(&mut self, clip: ConsoleClipId) {
        self.forget_sets(
            |target| matches!(target, AutomationTarget::ClipParam { clip_id, .. } if *clip_id == clip),
        );
    }

    /// Forget every `Set` of a deleted graph node's parameters.
    pub fn forget_node(&mut self, node: &crate::sequence::CompositionGraphNodeId) {
        self.forget_sets(|target| {
            matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == node)
        });
    }

    fn forget_sets(&mut self, matches: impl Fn(&ConsoleAutomationTarget) -> bool) {
        for on in &mut self.events {
            on.actions
                .retain(|action| !matches!(action, Action::Set { target, .. } if matches(target)));
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

    /// Every control the console reads: those its events answer, in event
    /// order, then the axes that scale a length.
    pub fn inputs(&self) -> impl Iterator<Item = &ControlRef> {
        let lengths = self.actions().filter_map(|action| match action {
            Action::Play(Play {
                length:
                    PlayLength::Scaled {
                        by: LengthSource::Axis(control),
                        ..
                    },
                ..
            }) => Some(control),
            _ => None,
        });
        self.events
            .iter()
            .map(|on| on.event.control())
            .chain(lengths)
    }
}

impl crate::ownership::Identified for Console {
    type Id = ConsoleId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type OwnedConsole = crate::ownership::OwnedMember<Console, ConsoleId>;
