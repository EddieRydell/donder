//! Routing button presses to rig commands. The host owns this state: which
//! switches are on, which voice a press took, and when a voice frees up.
//! Devices that play the rig only ever receive the commands.

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

#[derive(Clone, Debug, PartialEq)]
pub enum RoutedAction {
    Trigger {
        /// The rig's voices for this trigger: one, or the stack's.
        voices: Box<[u16]>,
        length: RoutedLength,
        policy: RoutedVoices,
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
    /// Each switch's button, by index in the router's buttons.
    switches: Box<[usize]>,
    switch_on: Box<[bool]>,
    state: Box<[MappingState]>,
    pressed: Box<[bool]>,
}

#[derive(Clone, Debug, Default)]
struct MappingState {
    /// The next voice a stack takes, as a position in its voices.
    next: usize,
    /// Voices a press, hold or latch is playing, to stop on release.
    playing: Vec<u16>,
    /// When the voice a fixed-length mono trigger started ends.
    busy_until: Option<SampleTime>,
    /// Whether the press that the release answers acted at all.
    engaged: bool,
}

impl RigRouter {
    pub fn new(
        button_count: usize,
        switches: Box<[usize]>,
        mappings: Box<[RoutedMapping]>,
    ) -> Self {
        Self {
            switch_on: vec![false; switches.len()].into(),
            state: vec![MappingState::default(); mappings.len()].into(),
            pressed: vec![false; button_count].into(),
            switches,
            mappings,
        }
    }

    pub fn switch_on(&self, switch: usize) -> bool {
        self.switch_on[switch]
    }

    /// Whether `button` is held down.
    pub fn pressed(&self, button: usize) -> bool {
        self.pressed[button]
    }

    /// The commands a press of `button` at `time` sends. A press of a button
    /// that is already down is a key repeat, and does nothing.
    pub fn press(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
        if std::mem::replace(&mut self.pressed[button], true) {
            return Vec::new();
        }
        let mut commands = Vec::new();
        // Conditions see the switches as they were before this press.
        let switch_on = self.switch_on.clone();
        for (switch, &switch_button) in self.switches.iter().enumerate() {
            if switch_button == button {
                self.switch_on[switch] = !self.switch_on[switch];
            }
        }
        for (index, mapping) in self.mappings.iter().enumerate() {
            if mapping.button != button {
                continue;
            }
            let state = &mut self.state[index];
            state.engaged = mapping
                .when
                .is_none_or(|(switch, on)| switch_on[switch] == on);
            if !state.engaged {
                continue;
            }
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
                } => trigger(state, voices, *length, *policy, time, &mut commands),
            }
        }
        commands
            .into_iter()
            .map(|command| RigEvent { time, command })
            .collect()
    }

    /// The commands a release of `button` at `time` sends.
    pub fn release(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
        if !std::mem::replace(&mut self.pressed[button], false) {
            return Vec::new();
        }
        let mut commands = Vec::new();
        for (index, mapping) in self.mappings.iter().enumerate() {
            let state = &mut self.state[index];
            if mapping.button != button || !std::mem::take(&mut state.engaged) {
                continue;
            }
            match &mapping.action {
                RoutedAction::Control { control } => commands.push(RigCommand::Control {
                    control: *control,
                    value: 0.0,
                }),
                RoutedAction::Trigger {
                    length: RoutedLength::Held,
                    ..
                } => commands.extend(
                    state
                        .playing
                        .drain(..)
                        .map(|voice| RigCommand::Stop { voice }),
                ),
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

    /// Release every held button, as when the performer's window loses focus.
    pub fn release_all(&mut self, time: SampleTime) -> Vec<RigEvent> {
        (0..self.pressed.len())
            .flat_map(|button| self.release(button, time))
            .collect()
    }
}

fn trigger(
    state: &mut MappingState,
    voices: &[u16],
    length: RoutedLength,
    policy: RoutedVoices,
    time: SampleTime,
    commands: &mut Vec<RigCommand>,
) {
    if length == RoutedLength::Latched && !state.playing.is_empty() {
        commands.extend(
            state
                .playing
                .drain(..)
                .map(|voice| RigCommand::Stop { voice }),
        );
        return;
    }
    let voice = match policy {
        RoutedVoices::Retrigger => voices[0],
        RoutedVoices::Stack => {
            let voice = voices[state.next % voices.len()];
            state.next = (state.next + 1) % voices.len();
            voice
        }
        // A held or latched voice is stopped by its own button, so only a
        // fixed length can still be playing at a press.
        RoutedVoices::Mono => {
            if state.busy_until.is_some_and(|end| time < end) {
                state.engaged = false;
                return;
            }
            voices[0]
        }
    };
    commands.push(RigCommand::Start { voice });
    match length {
        RoutedLength::Fixed(duration) => {
            state.busy_until = time.checked_add_duration(duration);
        }
        RoutedLength::Held | RoutedLength::Latched => {
            if !state.playing.contains(&voice) {
                state.playing.push(voice);
            }
        }
    }
}
