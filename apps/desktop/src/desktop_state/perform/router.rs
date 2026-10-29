//! Routing button presses, struck notes and turned dials to rig commands.
//! The host owns this state: which switches are on, which voice a press took,
//! and when a voice frees up. Devices that play the rig only ever receive the
//! commands.

use donder_elaboration::ArmedRig;
use donder_model::{
    ButtonInput, ControlRef, DonderProject, InputControlKind, MappingAction, PlayedQuantity, Rig,
    TriggerLength, VoicePolicy, When,
};
use donder_runtime_types::{RigCommand, RigEvent, SampleTime};

/// A parameter each press sets on the voice it starts.
#[derive(Clone, Debug, PartialEq)]
struct RoutedValue {
    source: PlayedQuantity,
    /// The control that sets it on each of the trigger's voices, in the
    /// order of its voices.
    controls: Box<[u16]>,
}

/// The notes an input plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RoutedNotes {
    /// A computer key, pedal or dial.
    None,
    /// A single note button.
    Note(u8),
    /// A range of keys, each its own press.
    Keys { low: u8, high: u8 },
}

#[derive(Clone, Debug, PartialEq)]
enum RoutedAction {
    Trigger {
        /// The rig's voices for this trigger: one, or the stack's.
        voices: Box<[u16]>,
        length: TriggerLength,
        policy: VoicePolicy,
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
struct RoutedMapping {
    /// The button or dial that drives it, by index in the router's inputs.
    input: usize,
    /// A switch, by index, and the state the mapping needs.
    when: Option<(usize, bool)>,
    action: RoutedAction,
}

/// Mutable routing state for one performance.
#[derive(Clone, Debug)]
pub(crate) struct RigRouter {
    /// The controls the router reads, by input index.
    inputs: Box<[ControlRef]>,
    mappings: Box<[RoutedMapping]>,
    /// The notes each input plays, by index in the router's inputs.
    notes: Box<[RoutedNotes]>,
    /// Each switch's button, by index in the router's inputs.
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
    /// The value a dial last set the mapping's control to.
    turned: Option<f32>,
}

impl RigRouter {
    /// The routing of `rig`'s inputs to the voices, collections and
    /// controls `armed` holds. `None` means an input control is missing.
    pub(crate) fn new(project: &DonderProject, rig: &Rig, armed: &ArmedRig) -> Option<Self> {
        let mut inputs: Vec<ControlRef> = Vec::new();
        let mut input_index =
            |input: &ControlRef| match inputs.iter().position(|known| known == input) {
                Some(index) => index,
                None => {
                    inputs.push(input.clone());
                    inputs.len() - 1
                }
            };
        let switches: Box<[usize]> = rig
            .switches
            .iter()
            .map(|switch| input_index(&switch.input))
            .collect();
        let switch_index = |id| rig.switches.iter().position(|switch| switch.id == id);
        let mappings = rig
            .mappings
            .iter()
            .map(|mapping| {
                Some(RoutedMapping {
                    input: input_index(&mapping.input),
                    when: match mapping.when {
                        None => None,
                        Some(When::On(id)) => Some((switch_index(id)?, true)),
                        Some(When::Off(id)) => Some((switch_index(id)?, false)),
                    },
                    action: match &mapping.action {
                        MappingAction::Trigger(trigger) => RoutedAction::Trigger {
                            values: trigger
                                .values
                                .iter()
                                .zip(armed.played.get(&mapping.id)?)
                                .map(|(value, controls)| RoutedValue {
                                    source: value.from,
                                    controls: controls.clone(),
                                })
                                .collect(),
                            voices: armed.voices.get(&mapping.id)?.clone(),
                            length: trigger.length.clone(),
                            policy: trigger.voices,
                        },
                        MappingAction::Mark(key) => RoutedAction::Mark {
                            collection: *armed.collections.get(key)?,
                        },
                        MappingAction::Control { .. } => RoutedAction::Control {
                            control: *armed.controls.get(&mapping.id)?,
                        },
                    },
                })
            })
            .collect::<Option<Box<[_]>>>()?;
        let notes = inputs
            .iter()
            .map(|input| {
                let control = project
                    .get::<donder_model::InputDevice>(input.device)?
                    .control(input.control)?;
                Some(match &control.kind {
                    InputControlKind::Button(ButtonInput::Note(note)) => RoutedNotes::Note(*note),
                    InputControlKind::Keys(range) => RoutedNotes::Keys {
                        low: range.low,
                        high: range.high,
                    },
                    InputControlKind::Button(
                        ButtonInput::Key(_)
                        | ButtonInput::ControlChange(_)
                        | ButtonInput::Address(_),
                    )
                    | InputControlKind::Dial(_) => RoutedNotes::None,
                })
            })
            .collect::<Option<Box<[_]>>>()?;
        Some(Self::routing(inputs.into(), notes, switches, mappings))
    }

    fn routing(
        inputs: Box<[ControlRef]>,
        notes: Box<[RoutedNotes]>,
        switches: Box<[usize]>,
        mappings: Box<[RoutedMapping]>,
    ) -> Self {
        Self {
            switch_on: vec![false; switches.len()].into(),
            state: vec![MappingState::default(); mappings.len()].into(),
            pressed: Vec::new(),
            inputs,
            notes,
            switches,
            mappings,
        }
    }

    /// The controls the router reads, by input index.
    pub(crate) fn inputs(&self) -> &[ControlRef] {
        &self.inputs
    }

    pub(crate) fn switch_on(&self, switch: usize) -> bool {
        self.switch_on[switch]
    }

    /// Whether `input` is held down, or any of its keys.
    pub(crate) fn pressed(&self, input: usize) -> bool {
        self.pressed.iter().any(|press| press.input == input)
    }

    /// The commands a press of `button` at `time` sends: a computer key or a
    /// pedal, at full velocity. A press of a button that is already down is
    /// a key repeat, and does nothing.
    pub(crate) fn press(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
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
    pub(crate) fn strike(
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
            if mapping.input != press.input
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
                        length,
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
    pub(crate) fn release(&mut self, button: usize, time: SampleTime) -> Vec<RigEvent> {
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
    pub(crate) fn release_note(
        &mut self,
        input: usize,
        note: u8,
        time: SampleTime,
    ) -> Vec<RigEvent> {
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
            if mapping.input != press.input {
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
                    length: TriggerLength::Held,
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
    /// holds, unless the control already has that value.
    pub(crate) fn turn(&mut self, input: usize, value: f32, time: SampleTime) -> Vec<RigEvent> {
        let value = value.clamp(0.0, 1.0);
        let mut events = Vec::new();
        for (mapping, state) in self.mappings.iter().zip(&mut self.state) {
            if mapping.input != input
                || !mapping
                    .when
                    .is_none_or(|(switch, on)| self.switch_on[switch] == on)
                || state.turned == Some(value)
            {
                continue;
            }
            if let RoutedAction::Control { control } = mapping.action {
                state.turned = Some(value);
                events.push(RigEvent {
                    time,
                    command: RigCommand::Control { control, value },
                });
            }
        }
        events
    }

    /// Release every held button and key, as when the performer's window
    /// loses focus.
    pub(crate) fn release_all(&mut self, time: SampleTime) -> Vec<RigEvent> {
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
fn played(source: PlayedQuantity, touch: &Touch, notes: RoutedNotes) -> f32 {
    let Some(note) = touch.note else {
        return match source {
            PlayedQuantity::Velocity => touch.velocity,
            // Validation keeps note sources off inputs without notes.
            PlayedQuantity::Pitch | PlayedQuantity::PitchClass | PlayedQuantity::Fifths => 0.0,
        };
    };
    match source {
        PlayedQuantity::Velocity => touch.velocity,
        PlayedQuantity::Pitch => match notes {
            RoutedNotes::Keys { low, high } if high > low => {
                f32::from(note.saturating_sub(low)) / f32::from(high - low)
            }
            // A range of one key is its own lowest key.
            RoutedNotes::Keys { .. } => 0.0,
            RoutedNotes::Note(_) | RoutedNotes::None => f32::from(note) / 127.0,
        },
        PlayedQuantity::PitchClass => f32::from(note % 12) / 12.0,
        PlayedQuantity::Fifths => f32::from((u16::from(note) * 7 % 12) as u8) / 12.0,
    }
}

#[allow(clippy::too_many_arguments)]
fn trigger(
    state: &mut MappingState,
    voices: &[u16],
    length: &TriggerLength,
    policy: VoicePolicy,
    values: &[RoutedValue],
    played: &[f32],
    key: Option<u8>,
    time: SampleTime,
    commands: &mut Vec<RigCommand>,
) {
    if *length == TriggerLength::Latched && state.playing.iter().any(|(held, _)| *held == key) {
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
        VoicePolicy::Retrigger => 0,
        VoicePolicy::Stack { .. } => {
            let position = state.next % voices.len();
            state.next = (state.next + 1) % voices.len();
            position
        }
        // A held or latched voice is stopped by its own press, so only a
        // fixed length can still be playing at a press.
        VoicePolicy::Mono => {
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
    match length.duration() {
        Some(duration) => {
            state.busy_until = time.checked_add_duration(duration);
        }
        None => {
            // A voice taken from another press is no longer that press's to stop.
            state.playing.retain(|(_, playing)| *playing != voice);
            state.playing.push((key, voice));
        }
    }
}

#[cfg(test)]
mod tests {
    //! Arming the starter's jam rig, and routing its keyboard buttons.
    use super::*;
    use donder_elaboration::{PreparationCache, Retain, arm};
    use donder_runtime_types::Color;

    fn starter() -> DonderProject {
        let path = camino::Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
        donder_project_io::check_project(&path, &Default::default(), &mut Default::default())
            .into_session()
            .unwrap()
            .project
    }

    fn armed(project: &DonderProject) -> (ArmedRig, RigRouter) {
        let rig = project.root().rigs.first().unwrap().id();
        let armed = arm(project, rig, Retain::All, &mut PreparationCache::default()).unwrap();
        let router = RigRouter::new(
            project,
            project.get::<donder_model::Rig>(*rig).unwrap(),
            &armed,
        )
        .unwrap();
        (armed, router)
    }

    /// The router's index of the starter keyboard's button named `name`.
    fn button(project: &DonderProject, router: &RigRouter, name: &str) -> usize {
        router
            .inputs()
            .iter()
            .position(|ControlRef { device, control }| {
                project
                    .get::<donder_model::InputDevice>(*device)
                    .and_then(|device| device.control(*control))
                    .is_some_and(|control| control.name.as_str() == name)
            })
            .unwrap()
    }

    fn at(seconds: f32) -> SampleTime {
        SampleTime::from_ticks((seconds * 1_000_000.0) as u32)
    }

    fn commands(events: Vec<donder_runtime_types::RigEvent>) -> Vec<RigCommand> {
        events.into_iter().map(|event| event.command).collect()
    }

    #[test]
    fn jam_arms_with_a_voice_pool_per_trigger() {
        let project = starter();
        let (_, router) = armed(&project);
        // kick, swell, boost, tap and the shift switch.
        assert_eq!(router.inputs().len(), 5);
    }

    #[test]
    fn jam_keys_light_the_outputs() {
        let project = starter();
        let (armed, mut router) = armed(&project);
        let kick = button(&project, &router, "kick");
        let mut playback = armed.prepared.into_playback();
        let dark = |frame: &[Color]| frame.iter().all(|color| *color == Color::BLACK);
        assert!(
            dark(playback.evaluate(at(0.5)).colors()),
            "nothing plays yet"
        );
        for event in router.press(kick, at(1.0)) {
            playback.apply(event);
        }
        assert!(!dark(playback.evaluate(at(1.1)).colors()), "kick flashes");
        assert!(dark(playback.evaluate(at(1.6)).colors()), "the flash ends");
    }

    #[test]
    fn switches_page_mappings_and_stacks_take_voices_in_turn() {
        let project = starter();
        let (_, mut router) = armed(&project);
        let kick = button(&project, &router, "kick");
        let shift = button(&project, &router, "shift");
        let start = |events: Vec<_>| match commands(events).as_slice() {
            [RigCommand::Start { voice }] => *voice,
            other => panic!("expected one start, got {other:?}"),
        };
        let flash = start(router.press(kick, at(1.0)));
        router.release(kick, at(1.1));
        assert!(commands(router.press(shift, at(2.0))).is_empty());
        router.release(shift, at(2.1));
        let first = start(router.press(kick, at(3.0)));
        router.release(kick, at(3.1));
        let second = start(router.press(kick, at(4.0)));
        router.release(kick, at(4.1));
        assert_ne!(flash, first, "page two plays the burst, not the flash");
        assert_ne!(first, second, "a stack takes its next voice");
    }

    #[test]
    fn held_and_control_buttons_answer_their_release() {
        let project = starter();
        let (_, mut router) = armed(&project);
        let swell = button(&project, &router, "swell");
        let boost = button(&project, &router, "boost");
        let tap = button(&project, &router, "tap");
        let [RigCommand::Start { voice }] = commands(router.press(swell, at(1.0)))[..] else {
            panic!("a held trigger starts on press");
        };
        assert!(
            commands(router.press(swell, at(1.2))).is_empty(),
            "key repeat"
        );
        assert_eq!(
            commands(router.release(swell, at(2.0))),
            [RigCommand::Stop { voice }]
        );
        assert!(matches!(
            commands(router.press(boost, at(3.0)))[..],
            [RigCommand::Control { value: 1.0, .. }]
        ));
        assert!(matches!(
            commands(router.release(boost, at(3.5)))[..],
            [RigCommand::Control { value: 0.0, .. }]
        ));
        assert!(matches!(
            commands(router.press(tap, at(4.0)))[..],
            [RigCommand::Mark { .. }]
        ));
    }

    #[test]
    fn mono_ignores_presses_while_its_voice_plays_and_latch_toggles() {
        let mut router = RigRouter::routing(
            Box::new([]),
            Box::new([RoutedNotes::None; 2]),
            Box::new([]),
            Box::new([
                RoutedMapping {
                    input: 0,
                    when: None,
                    action: RoutedAction::Trigger {
                        voices: Box::new([0]),
                        length: TriggerLength::Fixed(donder_language::DonderDuration(
                            std::time::Duration::from_secs(1),
                        )),
                        policy: VoicePolicy::Mono,
                        values: Box::new([]),
                    },
                },
                RoutedMapping {
                    input: 1,
                    when: None,
                    action: RoutedAction::Trigger {
                        voices: Box::new([1]),
                        length: TriggerLength::Latched,
                        policy: VoicePolicy::Retrigger,
                        values: Box::new([]),
                    },
                },
            ]),
        );
        assert_eq!(
            commands(router.press(0, at(0.0))),
            [RigCommand::Start { voice: 0 }]
        );
        router.release(0, at(0.1));
        assert!(
            commands(router.press(0, at(0.5))).is_empty(),
            "still playing"
        );
        router.release(0, at(0.6));
        assert_eq!(
            commands(router.press(0, at(1.5))),
            [RigCommand::Start { voice: 0 }]
        );
        assert_eq!(
            commands(router.press(1, at(2.0))),
            [RigCommand::Start { voice: 1 }]
        );
        assert!(
            commands(router.release(1, at(2.1))).is_empty(),
            "a latch outlasts its press"
        );
        assert_eq!(
            commands(router.press(1, at(3.0))),
            [RigCommand::Stop { voice: 1 }]
        );
    }
}
