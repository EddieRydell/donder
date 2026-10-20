//! Routing button presses, struck notes and turned dials to rig commands.
//! The host owns this state: which switches are on, which voice a press took,
//! and when a voice frees up. Devices that play the rig only ever receive the
//! commands.

use donder_music::PitchClass;
use donder_runtime_types::{RigCommand, RigEvent, SampleDuration, SampleTime};

/// How long a triggered voice plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RoutedLength {
    Fixed(SampleDuration),
    /// While its button is held.
    Held,
    /// From one press to the next.
    Latched,
}

/// What a press does when the voices of a trigger are all taken.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutedVoices {
    /// One voice, restarted by every press.
    Retrigger,
    /// Presses take the voices in turn, reusing the oldest.
    Stack,
    /// One voice; presses while it plays are ignored.
    Mono,
}

/// Where a played value comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutedSource {
    Velocity,
    Pitch,
    PitchClass,
    Fifths,
}

/// A parameter each press sets on the voice it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedValue {
    pub source: RoutedSource,
    /// The control that sets it on each of the trigger's voices, in the
    /// order of its voices.
    pub controls: Box<[u16]>,
}

/// The notes an input plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutedNotes {
    /// A computer key, pedal or dial.
    None,
    /// A single note button.
    Note(u8),
    /// A range of keys, each its own press.
    Keys { low: u8, high: u8 },
}

#[derive(Clone, Debug, PartialEq)]
pub enum RoutedAction {
    Trigger {
        /// The rig's voices for this trigger: one, or the stack's.
        voices: Box<[u16]>,
        length: RoutedLength,
        policy: RoutedVoices,
        values: Box<[RoutedValue]>,
    },
    Mark {
        collection: u16,
    },
    Control {
        control: u16,
    },
}

/// A mapping that acts while its condition holds.
#[derive(Clone, Debug, PartialEq)]
pub struct RoutedMapping {
    /// The button or dial that drives it, by index in the router's inputs.
    pub button: usize,
    /// A switch, by index, and the state the mapping needs.
    pub when: Option<(usize, bool)>,
    pub action: RoutedAction,
}

/// Mutable routing state for one performance.
#[derive(Clone, Debug)]
pub struct RigRouter {
    mappings: Box<[RoutedMapping]>,
    /// The notes each input plays, by index in the router's inputs.
    notes: Box<[RoutedNotes]>,
    /// Each switch's button, by index in the router's buttons.
    switches: Box<[usize]>,
    switch_on: Box<[bool]>,
    state: Box<[MappingState]>,
    /// The presses held down: an input, and the key of a range.
    pressed: Vec<Press>,
}

/// One press: a button, or one key of a range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    input: usize,
    key: Option<u8>,
}

/// How a press was played.
#[derive(Clone, Copy, Debug)]
struct Touch {
    press: Press,
    note: Option<u8>,
    velocity: f32,
}

#[derive(Clone, Debug, Default)]
struct MappingState {
    /// The next voice a stack takes, as a position in its voices.
    next: usize,
    /// Voices a press, hold or latch is playing, to stop on release, with
    /// the key of a range that started each.
    playing: Vec<(Option<u8>, u16)>,
    /// When the voice a fixed-length mono trigger started ends.
    busy_until: Option<SampleTime>,
    /// The keys whose press the release answers acted, or `None` for a
    /// button's.
    engaged: Vec<Option<u8>>,
}

impl RigRouter {
    pub fn new(
        notes: Box<[RoutedNotes]>,
        switches: Box<[usize]>,
        mappings: Box<[RoutedMapping]>,
    ) -> Self {
        Self {
            switch_on: vec![false; switches.len()].into(),
            state: vec![MappingState::default(); mappings.len()].into(),
            pressed: Vec::new(),
            notes,
            switches,
            mappings,
        }
    }

    pub fn switch_on(&self, switch: usize) -> bool {
        self.switch_on[switch]
    }

    /// Whether `input` is held down, or any of its keys.
    pub fn pressed(&self, input: usize) -> bool {
        self.pressed.iter().any(|press| press.input == input)
    }

    /// The commands a press of `button` at `time` sends: a computer key or a
    /// pedal, at full velocity. A press of a button that is already down is
    /// a key repeat, and does nothing.
    pub fn press(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
        self.touch(
            Touch {
                press: Press {
                    input: button,
                    key: None,
                },
                note: None,
                velocity: 1.0,
            },
            time,
        )
    }

    /// The commands striking `note` of `input`, a note button or a range of
    /// keys, with `velocity` in `0..=1` at `time` sends.
    pub fn strike(
        &mut self,
        input: usize,
        note: u8,
        velocity: f32,
        time: SampleTime,
    ) -> Vec<RigEvent> {
        let key = matches!(self.notes[input], RoutedNotes::Keys { .. }).then_some(note);
        self.touch(
            Touch {
                press: Press { input, key },
                note: Some(note),
                velocity: velocity.clamp(0.0, 1.0),
            },
            time,
        )
    }

    fn touch(&mut self, touch: Touch, time: SampleTime) -> Vec<RigEvent> {
        let press = touch.press;
        if self.pressed.contains(&press) {
            return Vec::new();
        }
        self.pressed.push(press);
        let mut commands = Vec::new();
        // Conditions see the switches as they were before this press.
        let switch_on = self.switch_on.clone();
        for (switch, &switch_button) in self.switches.iter().enumerate() {
            if switch_button == press.input {
                self.switch_on[switch] = !self.switch_on[switch];
            }
        }
        for (index, mapping) in self.mappings.iter().enumerate() {
            if mapping.button != press.input
                || !mapping
                    .when
                    .is_none_or(|(switch, on)| switch_on[switch] == on)
            {
                continue;
            }
            let state = &mut self.state[index];
            state.engaged.push(press.key);
            match &mapping.action {
                RoutedAction::Mark { collection } => commands.push(RigCommand::Mark {
                    collection: *collection,
                }),
                RoutedAction::Control { control } => commands.push(RigCommand::Control {
                    control: *control,
                    value: 1.0,
                }),
                RoutedAction::Trigger {
                    voices,
                    length,
                    policy,
                    values,
                } => {
                    let played = values
                        .iter()
                        .map(|value| played(value.source, &touch, self.notes[press.input]))
                        .collect::<Vec<_>>();
                    trigger(
                        state,
                        voices,
                        *length,
                        *policy,
                        values,
                        &played,
                        press.key,
                        time,
                        &mut commands,
                    );
                }
            }
        }
        commands
            .into_iter()
            .map(|command| RigEvent { time, command })
            .collect()
    }

    /// The commands a release of `button` at `time` sends.
    pub fn release(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
        self.lift(
            Press {
                input: button,
                key: None,
            },
            time,
        )
    }

    /// The commands releasing `note` of `input`, a note button or a range of
    /// keys, at `time` sends.
    pub fn release_note(&mut self, input: usize, note: u8, time: SampleTime) -> Vec<RigEvent> {
        let key = matches!(self.notes[input], RoutedNotes::Keys { .. }).then_some(note);
        self.lift(Press { input, key }, time)
    }

    fn lift(&mut self, press: Press, time: SampleTime) -> Vec<RigEvent> {
        let Some(held) = self.pressed.iter().position(|held| *held == press) else {
            return Vec::new();
        };
        self.pressed.swap_remove(held);
        let mut commands = Vec::new();
        for (index, mapping) in self.mappings.iter().enumerate() {
            let state = &mut self.state[index];
            if mapping.button != press.input {
                continue;
            }
            let Some(engaged) = state.engaged.iter().position(|key| *key == press.key) else {
                continue;
            };
            state.engaged.swap_remove(engaged);
            match &mapping.action {
                RoutedAction::Control { control } => commands.push(RigCommand::Control {
                    control: *control,
                    value: 0.0,
                }),
                RoutedAction::Trigger {
                    length: RoutedLength::Held,
                    ..
                } => state.playing.retain(|&(key, voice)| {
                    let stops = key == press.key;
                    if stops {
                        commands.push(RigCommand::Stop { voice });
                    }
                    !stops
                }),
                RoutedAction::Trigger { .. } | RoutedAction::Mark { .. } => {}
            }
        }
        commands
            .into_iter()
            .map(|command| RigEvent { time, command })
            .collect()
    }

    /// The commands a turn of the dial `input` to `value`, in `0..=1`, at
    /// `time` sends: it sets every control it drives while its condition
    /// holds.
    pub fn turn(&mut self, input: usize, value: f32, time: SampleTime) -> Vec<RigEvent> {
        let value = value.clamp(0.0, 1.0);
        self.mappings
            .iter()
            .filter(|mapping| {
                mapping.button == input
                    && mapping
                        .when
                        .is_none_or(|(switch, on)| self.switch_on[switch] == on)
            })
            .filter_map(|mapping| match mapping.action {
                RoutedAction::Control { control } => Some(RigEvent {
                    time,
                    command: RigCommand::Control { control, value },
                }),
                RoutedAction::Trigger { .. } | RoutedAction::Mark { .. } => None,
            })
            .collect()
    }

    /// Release every held button and key, as when the performer's window
    /// loses focus.
    pub fn release_all(&mut self, time: SampleTime) -> Vec<RigEvent> {
        std::mem::take(&mut self.pressed)
            .into_iter()
            .flat_map(|press| {
                self.pressed.push(press);
                self.lift(press, time)
            })
            .collect()
    }
}

/// A played value, in `0..=1`, from how `touch` was played on an input that
/// plays `notes`.
fn played(source: RoutedSource, touch: &Touch, notes: RoutedNotes) -> f32 {
    let Some(note) = touch.note else {
        return match source {
            RoutedSource::Velocity => touch.velocity,
            // Validation keeps note sources off inputs without notes.
            RoutedSource::Pitch | RoutedSource::PitchClass | RoutedSource::Fifths => 0.0,
        };
    };
    match source {
        RoutedSource::Velocity => touch.velocity,
        RoutedSource::Pitch => match notes {
            RoutedNotes::Keys { low, high } if high > low => {
                f32::from(note.saturating_sub(low)) / f32::from(high - low)
            }
            _ => f32::from(note) / 127.0,
        },
        RoutedSource::PitchClass => f32::from(note % 12) / 12.0,
        RoutedSource::Fifths => PitchClass(note % 12).fifths(),
    }
}

#[allow(clippy::too_many_arguments)]
fn trigger(
    state: &mut MappingState,
    voices: &[u16],
    length: RoutedLength,
    policy: RoutedVoices,
    values: &[RoutedValue],
    played: &[f32],
    key: Option<u8>,
    time: SampleTime,
    commands: &mut Vec<RigCommand>,
) {
    if length == RoutedLength::Latched && state.playing.iter().any(|(held, _)| *held == key) {
        state.playing.retain(|&(held, voice)| {
            let stops = held == key;
            if stops {
                commands.push(RigCommand::Stop { voice });
            }
            !stops
        });
        return;
    }
    let position = match policy {
        RoutedVoices::Retrigger => 0,
        RoutedVoices::Stack => {
            let position = state.next % voices.len();
            state.next = (state.next + 1) % voices.len();
            position
        }
        // A held or latched voice is stopped by its own press, so only a
        // fixed length can still be playing at a press.
        RoutedVoices::Mono => {
            if state.busy_until.is_some_and(|end| time < end) {
                if let Some(engaged) = state.engaged.iter().rposition(|held| *held == key) {
                    state.engaged.swap_remove(engaged);
                }
                return;
            }
            0
        }
    };
    let voice = voices[position];
    commands.push(RigCommand::Start { voice });
    // Values follow the start, so a voice sounds with its own from the frame
    // it starts.
    for (value, &played) in values.iter().zip(played) {
        commands.push(RigCommand::Control {
            control: value.controls[position],
            value: played,
        });
    }
    match length {
        RoutedLength::Fixed(duration) => {
            state.busy_until = time.checked_add_duration(duration);
        }
        RoutedLength::Held | RoutedLength::Latched => {
            // A voice taken from another press is no longer that press's to stop.
            state.playing.retain(|(_, playing)| *playing != voice);
            state.playing.push((key, voice));
        }
    }
}
