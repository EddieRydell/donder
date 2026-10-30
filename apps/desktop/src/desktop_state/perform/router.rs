//! Routing button presses, struck notes and moved axes to console commands.
//! The host owns this state: which copies of each clip play and until when,
//! each live automation's value, and which buttons are held. Devices that
//! play the console only ever receive the commands.

use donder_elaboration::ArmedConsole;
use donder_model::{
    Action, Console, ControlRef, DonderProject, InputEvent, LengthSource, NoteRange, PlayLength,
    PlayedQuantity, sample_duration,
};
use donder_runtime_types::{ConsoleCommand, ConsoleEvent, SampleDuration, SampleTime};

/// What a routed event answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Happening {
    Press,
    Release,
    Move,
}

/// An event's actions, in document order.
#[derive(Clone, Debug, PartialEq)]
struct RoutedOn {
    /// The control, by index in the router's inputs.
    input: usize,
    happening: Happening,
    actions: Box<[RoutedAction]>,
}

#[derive(Clone, Debug, PartialEq)]
enum RoutedAction {
    Play {
        /// The clip, by index in the router's clips.
        clip: usize,
        copies: usize,
        length: RoutedLength,
        values: Box<[RoutedValue]>,
    },
    Start {
        clip: usize,
        values: Box<[RoutedValue]>,
    },
    Stop {
        clip: usize,
    },
    Toggle {
        clip: usize,
        values: Box<[RoutedValue]>,
    },
    Mark {
        collection: u16,
    },
    Set {
        /// By index in the router's automations.
        automation: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RoutedLength {
    Fixed(SampleDuration),
    Scaled {
        by: RoutedSource,
        shortest: SampleDuration,
        longest: SampleDuration,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum RoutedSource {
    Velocity,
    /// By index in the router's automations.
    Automation(usize),
}

/// A parameter a press sets on the copy it starts.
#[derive(Clone, Debug, PartialEq)]
struct RoutedValue {
    source: PlayedQuantity,
    /// The automation that sets it on each of the clip's voices.
    automations: Box<[u16]>,
}

/// A copy of a clip that started playing.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Copy {
    started: SampleTime,
    /// `None` plays until stopped.
    ends: Option<SampleTime>,
}

impl Copy {
    fn plays_at(self, time: SampleTime) -> bool {
        self.ends.is_none_or(|end| time < end)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct RoutedClip {
    voices: Box<[u16]>,
    /// The copy each voice last started, if any.
    copies: Box<[Option<Copy>]>,
}

impl RoutedClip {
    fn new(voices: Box<[u16]>) -> Self {
        Self {
            copies: vec![None; voices.len()].into(),
            voices,
        }
    }

    fn plays_at(&self, time: SampleTime) -> bool {
        self.copies.iter().flatten().any(|copy| copy.plays_at(time))
    }
}

/// A live automation and the value it holds now.
#[derive(Clone, Copy, Debug, PartialEq)]
struct RoutedAutomation {
    index: u16,
    value: f32,
}

/// Mutable routing state for one performance.
#[derive(Clone, Debug)]
pub(crate) struct ConsoleRouter {
    /// The controls the router reads, by input index.
    inputs: Box<[ControlRef]>,
    /// The notes each input reads, if it reads notes.
    notes: Box<[Option<NoteRange>]>,
    events: Box<[RoutedOn]>,
    clips: Box<[RoutedClip]>,
    automations: Box<[RoutedAutomation]>,
    /// The clips that play for the whole session, by index.
    always: Box<[usize]>,
    /// The presses held down.
    held: Vec<Press>,
}

/// One press: a button, or one note of a button reading notes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Press {
    input: usize,
    note: Option<u8>,
}

/// What an event carries.
#[derive(Clone, Copy, Debug)]
struct Touch {
    press: Press,
    /// A press's velocity, an axis's position, or 0 for a release.
    value: f32,
}

impl ConsoleRouter {
    /// The routing of `console`'s events to the voices, collections and
    /// automations `armed` holds. `None` means an input control is missing.
    pub(crate) fn new(
        project: &DonderProject,
        console: &Console,
        armed: &ArmedConsole,
    ) -> Option<Self> {
        let mut inputs: Vec<ControlRef> = Vec::new();
        for input in console.inputs() {
            if !inputs.contains(input) {
                inputs.push(input.clone());
            }
        }
        let notes = inputs
            .iter()
            .map(|input| {
                let control = project
                    .get::<donder_model::InputDevice>(input.device)?
                    .control(input.control)?;
                Some(control.kind.notes())
            })
            .collect::<Option<Box<[_]>>>()?;
        let clip = |id| armed.clips.get_index_of(&id);
        let automation = |id| armed.automations.get_index_of(&id);
        let events = console
            .events
            .iter()
            .map(|on| {
                let values = |clip_id, values: &[donder_model::PlayedValue]| {
                    let played = &armed.clips.get(&clip_id)?.played;
                    values
                        .iter()
                        .map(|value| {
                            Some(RoutedValue {
                                source: value.from,
                                automations: played.get(&value.param)?.clone(),
                            })
                        })
                        .collect::<Option<Box<[_]>>>()
                };
                let actions = on
                    .actions
                    .iter()
                    .map(|action| {
                        Some(match action {
                            Action::Play(play) => RoutedAction::Play {
                                clip: clip(play.clip)?,
                                copies: usize::from(play.copies),
                                length: match &play.length {
                                    PlayLength::Fixed(duration) => {
                                        RoutedLength::Fixed(sample_duration(duration))
                                    }
                                    PlayLength::Scaled {
                                        by,
                                        shortest,
                                        longest,
                                    } => RoutedLength::Scaled {
                                        by: match by {
                                            LengthSource::Velocity => RoutedSource::Velocity,
                                            LengthSource::Automation(id) => {
                                                RoutedSource::Automation(automation(*id)?)
                                            }
                                        },
                                        shortest: sample_duration(shortest),
                                        longest: sample_duration(longest),
                                    },
                                },
                                values: values(play.clip, &play.values)?,
                            },
                            Action::Start {
                                clip: id,
                                values: played,
                            } => RoutedAction::Start {
                                clip: clip(*id)?,
                                values: values(*id, played)?,
                            },
                            Action::Stop { clip: id } => RoutedAction::Stop { clip: clip(*id)? },
                            Action::Toggle {
                                clip: id,
                                values: played,
                            } => RoutedAction::Toggle {
                                clip: clip(*id)?,
                                values: values(*id, played)?,
                            },
                            Action::Mark(key) => RoutedAction::Mark {
                                collection: *armed.collections.get(key)?,
                            },
                            Action::Set(id) => RoutedAction::Set {
                                automation: automation(*id)?,
                            },
                        })
                    })
                    .collect::<Option<Box<[_]>>>()?;
                Some(RoutedOn {
                    input: inputs
                        .iter()
                        .position(|input| input == on.event.control())?,
                    happening: match on.event {
                        InputEvent::Press(_) => Happening::Press,
                        InputEvent::Release(_) => Happening::Release,
                        InputEvent::Move(_) => Happening::Move,
                    },
                    actions,
                })
            })
            .collect::<Option<Box<[_]>>>()?;
        let automations = armed
            .automations
            .iter()
            .map(|(id, &index)| {
                Some(RoutedAutomation {
                    index,
                    value: console.automation(*id)?.value,
                })
            })
            .collect::<Option<Box<[_]>>>()?;
        let always = console
            .always
            .iter()
            .map(|id| clip(*id))
            .collect::<Option<Box<[_]>>>()?;
        Some(Self {
            notes,
            events,
            clips: armed
                .clips
                .values()
                .map(|clip| RoutedClip::new(clip.voices.clone()))
                .collect(),
            automations,
            always,
            held: Vec::new(),
            inputs: inputs.into(),
        })
    }

    /// The controls the router reads, by input index.
    pub(crate) fn inputs(&self) -> &[ControlRef] {
        &self.inputs
    }

    /// Whether `input` is held down, or any of its notes.
    pub(crate) fn pressed(&self, input: usize) -> bool {
        self.held.iter().any(|press| press.input == input)
    }

    /// The commands that start the session at `time`: each `always` clip
    /// starts and plays until the session ends.
    pub(crate) fn start(&mut self, time: SampleTime) -> Vec<ConsoleEvent> {
        let mut commands = Vec::new();
        for clip in self.always.clone() {
            self.start_copy(clip, 0, None, &[], 1.0, None, time, &mut commands);
        }
        stamp(commands, time)
    }

    /// The commands a press of `button` at `time` sends: a computer key or a
    /// pedal, at full velocity. A press of a button that is already down is
    /// a key repeat, and does nothing.
    pub(crate) fn press(&mut self, button: usize, time: SampleTime) -> Vec<ConsoleEvent> {
        self.touch(
            Happening::Press,
            Touch {
                press: Press {
                    input: button,
                    note: None,
                },
                value: 1.0,
            },
            time,
        )
    }

    /// The commands striking `note` of the note button `input` with
    /// `velocity` in `0..=1` at `time` sends; each note is its own press.
    pub(crate) fn strike(
        &mut self,
        input: usize,
        note: u8,
        velocity: f32,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(
            Happening::Press,
            Touch {
                press: Press {
                    input,
                    note: Some(note),
                },
                value: velocity.clamp(0.0, 1.0),
            },
            time,
        )
    }

    /// The commands a release of `button` at `time` sends.
    pub(crate) fn release(&mut self, button: usize, time: SampleTime) -> Vec<ConsoleEvent> {
        self.touch(
            Happening::Release,
            Touch {
                press: Press {
                    input: button,
                    note: None,
                },
                value: 0.0,
            },
            time,
        )
    }

    /// The commands releasing `note` of the note button `input` at `time` sends.
    pub(crate) fn release_note(
        &mut self,
        input: usize,
        note: u8,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(
            Happening::Release,
            Touch {
                press: Press {
                    input,
                    note: Some(note),
                },
                value: 0.0,
            },
            time,
        )
    }

    /// The commands a move of the axis `input` to `value`, in `0..=1`, at
    /// `time` sends.
    pub(crate) fn move_axis(
        &mut self,
        input: usize,
        value: f32,
        time: SampleTime,
    ) -> Vec<ConsoleEvent> {
        self.touch(
            Happening::Move,
            Touch {
                press: Press { input, note: None },
                value: value.clamp(0.0, 1.0),
            },
            time,
        )
    }

    /// Release every held button and note, as when the performer's window
    /// loses focus.
    pub(crate) fn release_all(&mut self, time: SampleTime) -> Vec<ConsoleEvent> {
        std::mem::take(&mut self.held)
            .into_iter()
            .flat_map(|press| {
                self.held.push(press);
                self.touch(Happening::Release, Touch { press, value: 0.0 }, time)
            })
            .collect()
    }

    fn touch(&mut self, happening: Happening, touch: Touch, time: SampleTime) -> Vec<ConsoleEvent> {
        let press = touch.press;
        match happening {
            Happening::Press if self.held.contains(&press) => return Vec::new(),
            Happening::Press => self.held.push(press),
            Happening::Release => {
                let Some(held) = self.held.iter().position(|held| *held == press) else {
                    return Vec::new();
                };
                self.held.swap_remove(held);
            }
            Happening::Move => {}
        }
        let mut commands = Vec::new();
        let events = std::mem::take(&mut self.events);
        for on in events
            .iter()
            .filter(|on| on.input == press.input && on.happening == happening)
        {
            for action in &on.actions {
                self.act(action, touch, time, &mut commands);
            }
        }
        self.events = events;
        stamp(commands, time)
    }

    fn act(
        &mut self,
        action: &RoutedAction,
        touch: Touch,
        time: SampleTime,
        commands: &mut Vec<ConsoleCommand>,
    ) {
        let note = touch
            .press
            .note
            .map(|note| (note, self.notes[touch.press.input]));
        match action {
            RoutedAction::Play {
                clip,
                copies,
                length,
                values,
            } => {
                let duration = match *length {
                    RoutedLength::Fixed(duration) => duration,
                    RoutedLength::Scaled {
                        by,
                        shortest,
                        longest,
                    } => {
                        let scale = match by {
                            RoutedSource::Velocity => touch.value,
                            RoutedSource::Automation(automation) => {
                                self.automations[automation].value
                            }
                        };
                        let (shortest, longest) = (shortest.as_ticks(), longest.as_ticks());
                        let span = f64::from(longest - shortest) * f64::from(scale);
                        SampleDuration::from_ticks(shortest + span.round() as u32)
                    }
                };
                // A free copy, or else the oldest of the action's copies.
                let routed = &self.clips[*clip];
                let copies = (*copies).min(routed.voices.len());
                let position = routed.copies[..copies]
                    .iter()
                    .position(|copy| copy.is_none_or(|copy| !copy.plays_at(time)))
                    .unwrap_or_else(|| {
                        (0..copies)
                            .min_by_key(|&position| {
                                routed.copies[position].map(|copy| copy.started)
                            })
                            .unwrap_or(0)
                    });
                self.start_copy(
                    *clip,
                    position,
                    Some(duration),
                    values,
                    touch.value,
                    note,
                    time,
                    commands,
                );
            }
            RoutedAction::Start { clip, values } => {
                if !self.clips[*clip].plays_at(time) {
                    self.start_copy(*clip, 0, None, values, touch.value, note, time, commands);
                }
            }
            RoutedAction::Stop { clip } => self.stop(*clip, time, commands),
            RoutedAction::Toggle { clip, values } => {
                if self.clips[*clip].plays_at(time) {
                    self.stop(*clip, time, commands);
                } else {
                    self.start_copy(*clip, 0, None, values, touch.value, note, time, commands);
                }
            }
            RoutedAction::Mark { collection } => commands.push(ConsoleCommand::Mark {
                collection: *collection,
            }),
            RoutedAction::Set { automation } => {
                let automation = &mut self.automations[*automation];
                if automation.value != touch.value {
                    automation.value = touch.value;
                    commands.push(ConsoleCommand::Set {
                        automation: automation.index,
                        value: touch.value,
                    });
                }
            }
        }
    }

    /// Start the copy at `position` of `clip`, for `duration` or until
    /// stopped, with the values played on it.
    #[allow(clippy::too_many_arguments)]
    fn start_copy(
        &mut self,
        clip: usize,
        position: usize,
        duration: Option<SampleDuration>,
        values: &[RoutedValue],
        velocity: f32,
        note: Option<(u8, Option<NoteRange>)>,
        time: SampleTime,
        commands: &mut Vec<ConsoleCommand>,
    ) {
        let routed = &mut self.clips[clip];
        routed.copies[position] = Some(Copy {
            started: time,
            ends: duration.and_then(|duration| time.checked_add_duration(duration)),
        });
        commands.push(ConsoleCommand::Start {
            voice: routed.voices[position],
            duration,
        });
        // Values follow the start, so a copy plays with its own from the
        // frame it starts.
        for value in values {
            commands.push(ConsoleCommand::Set {
                automation: value.automations[position],
                value: played(value.source, velocity, note),
            });
        }
    }

    /// Stop every copy of `clip` that plays at `time`.
    fn stop(&mut self, clip: usize, time: SampleTime, commands: &mut Vec<ConsoleCommand>) {
        let routed = &mut self.clips[clip];
        for (copy, &voice) in routed.copies.iter_mut().zip(&routed.voices) {
            if copy.is_some_and(|copy| copy.plays_at(time)) {
                commands.push(ConsoleCommand::Stop { voice });
            }
            *copy = None;
        }
    }
}

fn stamp(commands: Vec<ConsoleCommand>, time: SampleTime) -> Vec<ConsoleEvent> {
    commands
        .into_iter()
        .map(|command| ConsoleEvent { time, command })
        .collect()
}

/// A played value, in `0..=1`, from a press at `velocity` of `note` of a
/// button reading `range`, or of no note.
fn played(source: PlayedQuantity, velocity: f32, note: Option<(u8, Option<NoteRange>)>) -> f32 {
    let Some((note, range)) = note else {
        // Validation keeps note sources off buttons without notes.
        return match source {
            PlayedQuantity::Velocity => velocity,
            PlayedQuantity::Pitch | PlayedQuantity::PitchClass | PlayedQuantity::Fifths => 0.0,
        };
    };
    match source {
        PlayedQuantity::Velocity => velocity,
        PlayedQuantity::Pitch => match range {
            Some(NoteRange { low, high }) if high > low => {
                f32::from(note.saturating_sub(low)) / f32::from(high - low)
            }
            // A button of one note is at its place among all 128 notes.
            _ => f32::from(note) / 127.0,
        },
        PlayedQuantity::PitchClass => f32::from(note % 12) / 12.0,
        PlayedQuantity::Fifths => f32::from((u16::from(note) * 7 % 12) as u8) / 12.0,
    }
}

#[cfg(test)]
mod tests {
    //! Arming the starter's jam console, and routing its keyboard buttons.
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

    fn armed(project: &DonderProject) -> (ArmedConsole, ConsoleRouter) {
        let console = project.root().consoles.first().unwrap().id();
        let armed = arm(
            project,
            console,
            Retain::All,
            &mut PreparationCache::default(),
        )
        .unwrap();
        let router =
            ConsoleRouter::new(project, project.get::<Console>(*console).unwrap(), &armed).unwrap();
        (armed, router)
    }

    /// The router's index of the starter keyboard's button named `name`.
    fn button(project: &DonderProject, router: &ConsoleRouter, name: &str) -> usize {
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

    fn seconds(seconds: f32) -> SampleDuration {
        SampleDuration::from_ticks((seconds * 1_000_000.0) as u32)
    }

    fn commands(events: Vec<ConsoleEvent>) -> Vec<ConsoleCommand> {
        events.into_iter().map(|event| event.command).collect()
    }

    /// A router of `events` over clips with the given voices, automations at
    /// the given values, and inputs that read no notes.
    fn routing(
        inputs: usize,
        clips: &[&[u16]],
        automations: &[f32],
        events: Vec<RoutedOn>,
    ) -> ConsoleRouter {
        ConsoleRouter {
            inputs: Box::new([]),
            notes: vec![None; inputs].into(),
            events: events.into(),
            clips: clips
                .iter()
                .map(|voices| RoutedClip::new((*voices).into()))
                .collect(),
            automations: automations
                .iter()
                .zip(0..)
                .map(|(&value, index)| RoutedAutomation { index, value })
                .collect(),
            always: Box::new([]),
            held: Vec::new(),
        }
    }

    fn on(input: usize, happening: Happening, actions: Vec<RoutedAction>) -> RoutedOn {
        RoutedOn {
            input,
            happening,
            actions: actions.into(),
        }
    }

    fn play(clip: usize, copies: usize, length: RoutedLength) -> RoutedAction {
        RoutedAction::Play {
            clip,
            copies,
            length,
            values: Box::new([]),
        }
    }

    #[test]
    fn jam_arms_its_events_and_starts_its_always_clip() {
        let project = starter();
        let (_, mut router) = armed(&project);
        // kick, swell, tap and boost.
        assert_eq!(router.inputs().len(), 4);
        assert!(matches!(
            commands(router.start(at(0.0)))[..],
            [ConsoleCommand::Start { duration: None, .. }]
        ));
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
    fn plays_stack_copies_and_reuse_the_oldest() {
        let fixed = RoutedLength::Fixed(seconds(1.0));
        let mut router = routing(
            1,
            &[&[10, 11]],
            &[],
            vec![on(0, Happening::Press, vec![play(0, 2, fixed)])],
        );
        let mut press = |time| {
            let events = router.press(0, at(time));
            router.release(0, at(time + 0.05));
            commands(events)
        };
        let start = |voice| ConsoleCommand::Start {
            voice,
            duration: Some(seconds(1.0)),
        };
        assert_eq!(press(0.0), [start(10)]);
        assert_eq!(press(0.2), [start(11)], "a second copy plays on top");
        assert_eq!(press(0.4), [start(10)], "all play, so the oldest goes");
        assert_eq!(press(1.3), [start(11)], "the copy started at 0.2 ended");
    }

    #[test]
    fn start_plays_one_copy_until_stopped_and_toggle_flips() {
        let mut router = routing(
            3,
            &[&[0], &[1]],
            &[],
            vec![
                on(
                    0,
                    Happening::Press,
                    vec![RoutedAction::Start {
                        clip: 0,
                        values: Box::new([]),
                    }],
                ),
                on(0, Happening::Release, vec![RoutedAction::Stop { clip: 0 }]),
                on(
                    1,
                    Happening::Press,
                    vec![RoutedAction::Start {
                        clip: 0,
                        values: Box::new([]),
                    }],
                ),
                on(
                    2,
                    Happening::Press,
                    vec![RoutedAction::Toggle {
                        clip: 1,
                        values: Box::new([]),
                    }],
                ),
            ],
        );
        assert_eq!(
            commands(router.press(0, at(1.0))),
            [ConsoleCommand::Start {
                voice: 0,
                duration: None
            }]
        );
        assert!(commands(router.press(0, at(1.2))).is_empty(), "key repeat");
        assert!(
            commands(router.press(1, at(1.5))).is_empty(),
            "the clip already plays"
        );
        assert_eq!(
            commands(router.release(0, at(2.0))),
            [ConsoleCommand::Stop { voice: 0 }]
        );
        assert!(
            commands(router.release(0, at(2.1))).is_empty(),
            "a release of a button not held"
        );
        assert_eq!(
            commands(router.press(2, at(3.0))),
            [ConsoleCommand::Start {
                voice: 1,
                duration: None
            }]
        );
        router.release(2, at(3.1));
        assert_eq!(
            commands(router.press(2, at(4.0))),
            [ConsoleCommand::Stop { voice: 1 }]
        );
    }

    #[test]
    fn marks_and_sets_answer_presses_releases_and_moves() {
        let project = starter();
        let (_, mut router) = armed(&project);
        let boost = button(&project, &router, "boost");
        let tap = button(&project, &router, "tap");
        assert!(matches!(
            commands(router.press(boost, at(3.0)))[..],
            [ConsoleCommand::Set { value: 1.0, .. }]
        ));
        assert!(matches!(
            commands(router.release(boost, at(3.5)))[..],
            [ConsoleCommand::Set { value: 0.0, .. }]
        ));
        assert!(matches!(
            commands(router.press(tap, at(4.0)))[..],
            [ConsoleCommand::Mark { .. }]
        ));

        let mut router = routing(
            1,
            &[],
            &[0.0],
            vec![on(
                0,
                Happening::Move,
                vec![RoutedAction::Set { automation: 0 }],
            )],
        );
        assert_eq!(
            commands(router.move_axis(0, 0.5, at(1.0))),
            [ConsoleCommand::Set {
                automation: 0,
                value: 0.5
            }]
        );
        assert!(
            commands(router.move_axis(0, 0.5, at(1.1))).is_empty(),
            "the automation already holds it"
        );
    }

    #[test]
    fn scaled_lengths_read_velocity_or_an_automation_at_the_start() {
        let scaled = |by| RoutedLength::Scaled {
            by,
            shortest: seconds(1.0),
            longest: seconds(3.0),
        };
        let mut router = routing(
            3,
            &[&[0], &[1]],
            &[0.25],
            vec![
                on(
                    0,
                    Happening::Press,
                    vec![play(0, 1, scaled(RoutedSource::Velocity))],
                ),
                on(
                    1,
                    Happening::Press,
                    vec![play(1, 1, scaled(RoutedSource::Automation(0)))],
                ),
                on(
                    2,
                    Happening::Move,
                    vec![RoutedAction::Set { automation: 0 }],
                ),
            ],
        );
        let lengths = |events: Vec<ConsoleEvent>| {
            commands(events)
                .into_iter()
                .filter_map(|command| match command {
                    ConsoleCommand::Start { duration, .. } => duration,
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(lengths(router.strike(0, 60, 0.5, at(0.0))), [seconds(2.0)]);
        assert_eq!(lengths(router.press(1, at(0.0))), [seconds(1.5)]);
        router.move_axis(2, 1.0, at(0.1));
        router.release(1, at(0.1));
        assert_eq!(lengths(router.press(1, at(5.0))), [seconds(3.0)]);
    }

    #[test]
    fn played_values_follow_the_start_and_release_all_releases_held_notes() {
        let mut router = routing(
            1,
            &[&[0]],
            &[],
            vec![
                on(
                    0,
                    Happening::Press,
                    vec![RoutedAction::Start {
                        clip: 0,
                        values: Box::new([RoutedValue {
                            source: PlayedQuantity::Pitch,
                            automations: Box::new([7]),
                        }]),
                    }],
                ),
                on(0, Happening::Release, vec![RoutedAction::Stop { clip: 0 }]),
            ],
        );
        router.notes = Box::new([Some(NoteRange { low: 60, high: 72 })]);
        assert_eq!(
            commands(router.strike(0, 66, 0.8, at(0.0))),
            [
                ConsoleCommand::Start {
                    voice: 0,
                    duration: None
                },
                ConsoleCommand::Set {
                    automation: 7,
                    value: 0.5
                },
            ]
        );
        assert!(router.pressed(0));
        assert_eq!(
            commands(router.release_all(at(1.0))),
            [ConsoleCommand::Stop { voice: 0 }]
        );
        assert!(!router.pressed(0));
    }
}
