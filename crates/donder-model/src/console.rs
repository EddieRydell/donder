//! A console: a sequence built live from input devices. Each action edits it
//! at the time of the event that runs it: `Play` and `Start`..`Stop` place
//! clips, `Mark` adds a mark and `Set` writes a parameter's value. Its layers
//! and composition graph are a sequence's.
use crate::composition::{Composition, MarkCollectionId};
use crate::effect::ClipId;
pub(crate) use crate::identity::ConsoleId;
use crate::input_device::ControlRef;
use crate::sequence::{AutomationTarget, Window};
use donder_language::DonderDuration;
use donder_runtime_types::{Identifier, InputHappening, PlayedSource};

/// The most copies of a clip that `Play` actions play at once.
pub const MAX_COPIES: u8 = 16;

/// A console session's length: the playback clock's range.
pub(crate) const CONSOLE_SESSION: donder_runtime_types::SampleDuration =
    donder_runtime_types::SampleDuration::from_ticks(u32::MAX);

#[derive(Clone, Debug, PartialEq)]
pub struct Console {
    pub description: Option<String>,
    pub frame_rate: u32,
    /// Its mark collections start empty each session and gain a mark each
    /// time a `Mark` action runs. Its played clips play when actions play
    /// them, and its timed clips on the session clock.
    pub composition: Composition,
    /// Clips that play from the session's start until it ends.
    pub always: Vec<Always>,
    pub events: Vec<On>,
}

/// A clip that plays from the session's start, repeating every `length`.
#[derive(Clone, Debug, PartialEq)]
pub struct Always {
    pub clip: ClipId,
    pub length: DonderDuration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct OnId(pub u32);

/// What an input event does: its actions, in order.
#[derive(Clone, Debug, PartialEq)]
pub struct On {
    pub id: OnId,
    pub happening: InputHappening,
    pub control: ControlRef,
    pub actions: Vec<Action>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Play a copy of a clip for its length, on top of the copies playing.
    Play(Play),
    /// Play one copy of a clip until it is stopped, repeating every
    /// `length`; nothing while it plays.
    Start {
        clip: ClipId,
        length: DonderDuration,
    },
    /// Stop every playing copy of a clip.
    Stop { clip: ClipId },
    /// `Start` while no copy of the clip plays, else `Stop`.
    Toggle {
        clip: ClipId,
        length: DonderDuration,
    },
    /// Add a mark to a live collection.
    Mark(MarkCollectionId),
    /// Write the event's value into a parameter: an axis's position, a
    /// press's velocity, or 0 for a release, through `window`.
    Set {
        target: AutomationTarget,
        window: Window,
    },
}

impl Action {
    /// The clip the action starts or stops.
    pub fn clip(&self) -> Option<ClipId> {
        match self {
            Self::Play(play) => Some(play.clip),
            Self::Start { clip, .. } | Self::Stop { clip } | Self::Toggle { clip, .. } => {
                Some(*clip)
            }
            Self::Mark(_) | Self::Set { .. } => None,
        }
    }

    /// The clip the action starts a copy of, from the event's press.
    pub fn started(&self) -> Option<ClipId> {
        match self {
            Self::Play(Play { clip, .. })
            | Self::Start { clip, .. }
            | Self::Toggle { clip, .. } => Some(*clip),
            Self::Stop { .. } | Self::Mark(_) | Self::Set { .. } => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub clip: ClipId,
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
    pub from: PlayedSource,
    pub window: Window,
}

/// Whether a played value needs a note, which only MIDI note buttons carry.
pub fn needs_note(source: PlayedSource) -> bool {
    !matches!(source, PlayedSource::Velocity)
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
    /// The played clip `id`, which actions, `always` and `Set`s can name.
    pub fn clip(&self, id: ClipId) -> Option<&crate::effect::Clip> {
        self.composition.clip(id).filter(|clip| {
            matches!(clip.placement, crate::effect::Placement::Played(_)) && clip.effect().is_some()
        })
    }

    /// Whether `clip` plays from the session's start.
    pub fn is_always(&self, clip: ClipId) -> bool {
        self.always.iter().any(|always| always.clip == clip)
    }

    /// The parameters `Set` actions write, each once, in the order the
    /// actions first name them: the order playback numbers them in.
    pub fn set_targets(&self) -> Vec<&AutomationTarget> {
        let mut targets: Vec<&AutomationTarget> = Vec::new();
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
    pub fn rest(&self, target: &AutomationTarget) -> f32 {
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
    pub fn setter(&self, target: &AutomationTarget) -> Option<&ControlRef> {
        self.events
            .iter()
            .find(|on| {
                on.actions.iter().any(
                    |action| matches!(action, Action::Set { target: set, .. } if set == target),
                )
            })
            .map(|on| &on.control)
    }

    pub fn actions(&self) -> impl Iterator<Item = &Action> {
        self.events.iter().flat_map(|on| &on.actions)
    }

    /// How `clip` plays: as many copies as its `Play` actions ask for at
    /// most, or held. A clip only stopped plays as held, never starting.
    pub fn clip_use(&self, clip: ClipId) -> ClipUse {
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
        self.events.iter().map(|on| &on.control).chain(lengths)
    }
}
