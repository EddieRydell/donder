//! Perform mode: one rig played live from the setup's input devices. A
//! performance is not an edit; it never touches project documents or history.
//! Each input is stamped on the session clock, routed to rig commands, and the
//! commands fan out in order to live output and the Preview.

use std::collections::HashMap;

use super::{DesktopState, lock_unpoisoned};
use crate::midi::{Held, MidiInputs, MidiMessage};
use crate::midi_song::{MidiSong, SongPlayer, load_sound_font};
use crate::rendering::{RigOutput, SessionClock};
use donder_elaboration::{PreparationCache, PrepareOutputs, RigRouter};
use donder_model::{
    ButtonInput, ControllerId, DialInput, DonderProject, Heard, InputControlKind, InputDeviceId,
    InputDriver, KeyCode, KeyRange, Measure, RigId,
};
use donder_music::{Listener, Noticed, Played};
use donder_runtime::PreparedRig;
use donder_runtime_types::{RigEvent, SampleTime, sample_time_seconds_f32};
use donder_sequence_api::{
    AudioTransportState, LiveOutputState, PerformButton, PerformMidi, PerformRig, PerformSession,
    PerformSnapshot, PerformSong, PerformSwitch,
};

#[derive(Default)]
pub(crate) struct PerformService {
    performance: Option<Performance>,
    /// Numbers performances, so the Preview tells one from the next.
    generation: u32,
    revision: u32,
    /// Keeps effects lowered between performances.
    preparation: PreparationCache,
}

/// What the Preview needs to play a performance.
pub(crate) struct PreviewPerformance {
    pub(crate) generation: u32,
    pub(crate) archive: Vec<u8>,
    /// Each fixture's id and pixel count, in prepared order.
    pub(crate) fixtures: Vec<(u32, usize)>,
}

/// A MIDI input of a device the performance plays from.
struct MidiBinding {
    /// Ports whose name contains this, ignoring case, or every port.
    port: Option<String>,
    reads: MidiRead,
    /// The router input.
    input: usize,
}

/// A listener the performance plays from: the music on its ports, and the
/// router inputs its moments and measures play.
struct Listening {
    /// Ports whose name contains this, ignoring case, or every port.
    port: Option<String>,
    device: InputDeviceId,
    listener: Listener,
    moments: Vec<(Heard, usize)>,
    measures: Vec<(Measure, usize)>,
}

/// How often a listener notices the passing of time: beats, silence and
/// fading measures.
const LISTEN_TICK: std::time::Duration = std::time::Duration::from_millis(10);

/// What a MIDI control reads.
#[derive(Clone, Copy)]
enum MidiRead {
    Note(u8),
    Keys(KeyRange),
    /// A control change read as a button, as a pedal is.
    Pedal(u8),
    /// A control change read as a dial.
    Dial(u8),
    PitchBend,
}

/// The MIDI control change of the sustain pedal.
const SUSTAIN_PEDAL: u8 = 64;

fn session_seconds(time: SampleTime) -> f64 {
    f64::from(time.as_ticks()) / 1_000_000.0
}

/// Play what a listener noticed: its moments press and release their
/// buttons, and its measures turn their dials.
fn play_noticed(
    listening: &Listening,
    router: &mut RigRouter,
    noticed: &[Noticed],
    time: SampleTime,
    events: &mut Vec<RigEvent>,
) {
    for noticed in noticed {
        match *noticed {
            Noticed::Pressed(heard) | Noticed::Released(heard) => {
                for &(_, input) in listening
                    .moments
                    .iter()
                    .filter(|(moment, _)| *moment == heard)
                {
                    events.extend(if matches!(noticed, Noticed::Pressed(_)) {
                        router.press(input, time)
                    } else {
                        router.release(input, time)
                    });
                }
            }
            Noticed::Measured(measure, value) => {
                for &(_, input) in listening
                    .measures
                    .iter()
                    .filter(|(dial, _)| *dial == measure)
                {
                    events.extend(router.turn(input, value, time));
                }
            }
        }
    }
}

/// Whether a device that reads `wanted` reads the port named `port`: one
/// whose name contains it, ignoring case, or any port.
fn reads_port(wanted: Option<&str>, port: &str) -> bool {
    wanted.is_none_or(|wanted| port.to_lowercase().contains(&wanted.to_lowercase()))
}

struct Performance {
    generation: u32,
    rig: PerformRig,
    clock: SessionClock,
    prepared: PreparedRig,
    /// The prepared rig, encoded once for the Preview.
    archive: Vec<u8>,
    fixtures: Vec<(u32, usize)>,
    router: RigRouter,
    /// Each router button's device and control names.
    buttons: Box<[PerformButton]>,
    /// The router buttons each key presses.
    keys: HashMap<KeyCode, Vec<usize>>,
    /// The router inputs MIDI plays.
    midi: Vec<MidiBinding>,
    /// Listeners to the music on MIDI ports.
    listening: Vec<Listening>,
    /// The MIDI ports the performance is connected to, when it plays from
    /// MIDI.
    midi_inputs: Option<MidiInputs>,
    /// The MIDI song playing into the performance.
    song: Option<SongPlayer>,
    switches: Box<[String]>,
    controllers: Vec<ControllerId>,
    /// Every command so far, for an evaluator that joins mid-performance.
    events: Vec<RigEvent>,
}

impl Performance {
    fn session(&self) -> PerformSession {
        let mut keys = self
            .keys
            .keys()
            .map(|key| key.name().to_string())
            .collect::<Vec<_>>();
        keys.sort();
        PerformSession {
            rig: self.rig.clone(),
            elapsed_seconds: f64::from(sample_time_seconds_f32(self.clock.now())),
            switches: self
                .switches
                .iter()
                .enumerate()
                .map(|(index, name)| PerformSwitch {
                    name: name.clone(),
                    on: self.router.switch_on(index),
                })
                .collect(),
            held: self
                .buttons
                .iter()
                .enumerate()
                .filter(|(index, _)| self.router.pressed(*index))
                .map(|(_, button)| button.clone())
                .collect(),
            keys,
            midi: self.midi_inputs.as_ref().map(|inputs| {
                let status = inputs.status();
                PerformMidi {
                    ports: status.ports,
                    error: status.error,
                }
            }),
            song: self
                .song
                .as_ref()
                .filter(|song| !song.finished())
                .map(|song| PerformSong {
                    name: song.name.clone(),
                    position_seconds: song.position_seconds(),
                    length_seconds: song.length_seconds,
                }),
        }
    }

    fn output(&self) -> RigOutput {
        RigOutput::new(
            self.prepared.clone(),
            &self.events,
            self.controllers.clone(),
            self.clock,
        )
    }
}

impl DesktopState {
    pub(crate) fn perform_snapshot(&self) -> PerformSnapshot {
        let rigs = self
            .project_session()
            .map(|session| project_rigs(&session.project))
            .unwrap_or_default();
        let perform = lock_unpoisoned(&self.perform);
        PerformSnapshot {
            revision: perform.revision,
            rigs,
            session: perform.performance.as_ref().map(Performance::session),
        }
    }

    pub(crate) fn is_performing(&self) -> bool {
        lock_unpoisoned(&self.perform).performance.is_some()
    }

    /// Arm the project's `rig`th rig on the active setup and start its session.
    /// Sequence playback stops; live output that was on follows the performance.
    pub(crate) fn start_performing(
        &self,
        project_revision: u32,
        rig: u32,
    ) -> Result<PerformSnapshot, String> {
        let session = self
            .project_session()
            .ok_or("Open a valid project to perform.")?;
        if project_revision != self.snapshot().project_revision {
            return Err("The project changed. Choose the rig to perform again.".into());
        }
        let project = &session.project;
        let source = project
            .root()
            .rigs
            .get(rig as usize)
            .ok_or("The rig is no longer in the project.")?;
        let rig_id = source.id().clone();
        let described = PerformRig {
            index: rig,
            name: rig_id.0.declared_name().to_string(),
            path: rig_id.0.document().to_string(),
        };
        check_armable(project, &rig_id, &described.name)?;
        // Output that is on plays the new performance in place of what it played.
        let live = matches!(
            lock_unpoisoned(&self.live_output).snapshot().state,
            LiveOutputState::Preparing | LiveOutputState::Holding | LiveOutputState::Streaming
        );
        if matches!(
            self.audio_snapshot().state,
            AudioTransportState::Playing | AudioTransportState::Paused
        ) {
            self.audio_stop();
        }
        {
            let mut perform = lock_unpoisoned(&self.perform);
            let armed = donder_elaboration::arm(
                project,
                &rig_id,
                PrepareOutputs::All,
                &mut perform.preparation,
            )
            .ok_or_else(|| {
                format!(
                    "Rig {} cannot be prepared on the active setup.",
                    described.name
                )
            })?;
            let setup = project
                .setup(project.root().setup.id())
                .ok_or("The active setup is missing.")?;
            let mut keys: HashMap<KeyCode, Vec<usize>> = HashMap::new();
            let mut midi = Vec::new();
            let mut listening: Vec<Listening> = Vec::new();
            let mut buttons = Vec::with_capacity(armed.buttons.len());
            for (index, button) in armed.buttons.iter().enumerate() {
                let device = project
                    .input_device(&button.device)
                    .ok_or("An input device of the rig is missing.")?;
                let control = device
                    .controls
                    .iter()
                    .find(|control| control.id == button.control)
                    .ok_or("A button of the rig is missing.")?;
                buttons.push(PerformButton {
                    device: device.id.0.declared_name().to_string(),
                    control: control.name.as_str().to_string(),
                });
                if let InputDriver::Listen { port } = &device.driver {
                    let index_of = match listening.iter().position(|item| item.device == device.id)
                    {
                        Some(found) => found,
                        None => {
                            listening.push(Listening {
                                port: port.clone(),
                                device: device.id.clone(),
                                listener: Listener::new(&[], &[]),
                                moments: Vec::new(),
                                measures: Vec::new(),
                            });
                            listening.len() - 1
                        }
                    };
                    let item = &mut listening[index_of];
                    match control.kind {
                        InputControlKind::Button(ButtonInput::Heard(heard)) => {
                            item.moments.push((heard, index));
                        }
                        InputControlKind::Dial(DialInput::Measured(measure)) => {
                            item.measures.push((measure, index));
                        }
                        _ => return Err("A listener control reads a MIDI or key input.".into()),
                    }
                    continue;
                }
                // Every control of the setup with this key, note or pedal plays.
                let reads = match control.kind {
                    InputControlKind::Button(ButtonInput::Key(code)) => {
                        keys.entry(code).or_default().push(index);
                        continue;
                    }
                    InputControlKind::Button(ButtonInput::Note(note)) => MidiRead::Note(note),
                    InputControlKind::Button(ButtonInput::ControlChange(number)) => {
                        MidiRead::Pedal(number)
                    }
                    InputControlKind::Dial(DialInput::ControlChange(number)) => {
                        MidiRead::Dial(number)
                    }
                    InputControlKind::Dial(DialInput::PitchBend) => MidiRead::PitchBend,
                    InputControlKind::Keys(range) => MidiRead::Keys(range),
                    InputControlKind::Button(ButtonInput::Heard(_))
                    | InputControlKind::Dial(DialInput::Measured(_)) => {
                        return Err("A MIDI or keyboard control reads a listener's input.".into());
                    }
                };
                let InputDriver::Midi { port } = &device.driver else {
                    return Err("A keyboard control reads a MIDI input.".into());
                };
                midi.push(MidiBinding {
                    port: port.clone(),
                    reads,
                    input: index,
                });
            }
            for item in &mut listening {
                let heard = item
                    .moments
                    .iter()
                    .map(|(heard, _)| *heard)
                    .collect::<Vec<_>>();
                let measures = item
                    .measures
                    .iter()
                    .map(|(measure, _)| *measure)
                    .collect::<Vec<_>>();
                item.listener = Listener::new(&heard, &measures);
            }
            let generation = perform.generation.wrapping_add(1);
            let midi_inputs = (!midi.is_empty() || !listening.is_empty()).then(|| {
                let ports = midi
                    .iter()
                    .map(|binding| binding.port.clone())
                    .chain(listening.iter().map(|item| item.port.clone()))
                    .collect::<Vec<_>>();
                let state = self.weak.clone();
                let lost_state = self.weak.clone();
                MidiInputs::start(
                    move |name| ports.iter().any(|port| reads_port(port.as_deref(), name)),
                    move |port, message| {
                        if let Some(state) = state.upgrade() {
                            DesktopState(state).perform_midi(generation, port, message);
                        }
                    },
                    move |port, held| {
                        if let Some(state) = lost_state.upgrade() {
                            DesktopState(state).perform_midi_lost(generation, port, &held);
                        }
                    },
                )
            });
            let sequence = armed.prepared.sequence();
            let performance = Performance {
                generation,
                rig: described,
                clock: SessionClock::start(sequence.duration()),
                archive: armed.prepared.encode(),
                fixtures: sequence
                    .fixtures()
                    .iter()
                    .map(|fixture| (fixture.id, fixture.pixel_count as usize))
                    .collect(),
                prepared: armed.prepared,
                router: armed.router,
                buttons: buttons.into(),
                keys,
                midi,
                listening,
                midi_inputs,
                song: None,
                switches: project
                    .rig(&rig_id)
                    .ok_or("The rig is no longer in the project.")?
                    .switches
                    .iter()
                    .map(|switch| switch.name.as_str().to_string())
                    .collect(),
                controllers: crate::rendering::setup_controllers(setup),
                events: Vec::new(),
            };
            perform.generation = performance.generation;
            perform.revision = perform.revision.wrapping_add(1);
            let listens = !performance.listening.is_empty();
            perform.performance = Some(performance);
            if listens {
                let state = self.weak.clone();
                std::thread::spawn(move || {
                    loop {
                        std::thread::sleep(LISTEN_TICK);
                        let Some(state) = state.upgrade() else {
                            break;
                        };
                        if !DesktopState(state).perform_listen_tick(generation) {
                            break;
                        }
                    }
                });
            }
        }
        self.preview_wake.notify();
        if live && let Err(error) = self.set_live_output_active(true) {
            self.update_snapshot(|snapshot| snapshot.status = error);
        }
        Ok(self.perform_snapshot())
    }

    /// End the performance: its routing and held buttons go with it, its
    /// output blacks out and the Preview returns to the sequence.
    pub(crate) fn stop_performing(&self) -> PerformSnapshot {
        let stopped = {
            let mut perform = lock_unpoisoned(&self.perform);
            let stopped = perform.performance.take().is_some();
            if stopped {
                perform.revision = perform.revision.wrapping_add(1);
            }
            stopped
        };
        if stopped {
            let output = lock_unpoisoned(&self.live_output).disable();
            self.update_snapshot(|snapshot| snapshot.live_output = output);
            self.preview_wake.notify();
        }
        self.perform_snapshot()
    }

    /// Play the MIDI file at `path` into the performance, as if a pianist
    /// played it on a port named after the file, in place of any song
    /// playing, heard on the settings' SoundFont.
    pub(crate) fn perform_play_song(&self, path: &str) -> Result<PerformSnapshot, String> {
        let song = MidiSong::load(path)?;
        let sound_font = load_sound_font(
            self.snapshot()
                .settings
                .song_sound_font
                .as_deref()
                .ok_or("Choose a SoundFont under Settings > Perform to hear songs on.")?,
        )?;
        let generation = {
            let perform = lock_unpoisoned(&self.perform);
            let performance = perform
                .performance
                .as_ref()
                .ok_or("Perform a rig to play a song into it.")?;
            let hears = performance
                .midi
                .iter()
                .map(|binding| binding.port.as_deref())
                .chain(
                    performance
                        .listening
                        .iter()
                        .map(|item| item.port.as_deref()),
                )
                .any(|port| port.is_none());
            if !hears {
                return Err(format!(
                    "No MIDI device of rig {} reads every port, so it cannot hear a song. Set a device's port to none.",
                    performance.rig.name
                ));
            }
            performance.generation
        };
        // The song before lifts what it held before this one plays.
        if let Some(previous) = self.take_song() {
            previous.stop();
        }
        let name = song.name.clone();
        let state = self.weak.clone();
        let player = SongPlayer::start(song, &sound_font, move |message| {
            if let Some(state) = state.upgrade() {
                DesktopState(state).perform_midi(generation, &name, message);
            }
        })?;
        {
            let mut perform = lock_unpoisoned(&self.perform);
            let performance = perform
                .performance
                .as_mut()
                .filter(|performance| performance.generation == generation)
                .ok_or("The performance ended before the song could start.")?;
            performance.song = Some(player);
            perform.revision = perform.revision.wrapping_add(1);
        }
        Ok(self.perform_snapshot())
    }

    /// Stop the song playing into the performance; the keys and pedals it
    /// held are lifted.
    pub(crate) fn perform_stop_song(&self) -> PerformSnapshot {
        if let Some(song) = self.take_song() {
            song.stop();
        }
        self.perform_snapshot()
    }

    /// The song playing into the performance, taken out of it.
    fn take_song(&self) -> Option<SongPlayer> {
        let mut perform = lock_unpoisoned(&self.perform);
        let song = perform.performance.as_mut()?.song.take()?;
        perform.revision = perform.revision.wrapping_add(1);
        Some(song)
    }

    /// A key, by its `KeyboardEvent.code`, pressed or released. Keys the rig
    /// does not play, and keys while nothing is performing, do nothing.
    pub(crate) fn perform_key(&self, code: &str, pressed: bool) -> PerformSnapshot {
        if let Some(key) = KeyCode::from_name(code) {
            self.route(|performance, time| {
                let buttons = performance.keys.get(&key)?.clone();
                Some(
                    buttons
                        .into_iter()
                        .flat_map(|button| {
                            if pressed {
                                performance.router.press(button, time)
                            } else {
                                performance.router.release(button, time)
                            }
                        })
                        .collect(),
                )
            });
        }
        self.perform_snapshot()
    }

    /// A MIDI message from `port` while performance `generation` plays.
    /// Notes strike note buttons and keys, pedals press and release buttons,
    /// and control changes and pitch bend turn dials.
    fn perform_midi(&self, generation: u32, port: &str, message: MidiMessage) {
        self.route(|performance, time| {
            if performance.generation != generation {
                return None;
            }
            let mut events = Vec::new();
            let played = match message {
                MidiMessage::NoteOn { note, velocity } => Some(Played::NoteOn {
                    note,
                    velocity: f32::from(velocity) / 127.0,
                }),
                MidiMessage::NoteOff { note } => Some(Played::NoteOff { note }),
                MidiMessage::ControlChange {
                    number: SUSTAIN_PEDAL,
                    value,
                } => Some(Played::Sustain(value >= 64)),
                MidiMessage::ControlChange { .. } | MidiMessage::PitchBend { .. } => None,
            };
            if let Some(played) = played {
                let Performance {
                    listening, router, ..
                } = &mut *performance;
                for item in listening.iter_mut() {
                    if reads_port(item.port.as_deref(), port) {
                        let noticed = item.listener.hear(played, session_seconds(time));
                        play_noticed(item, router, &noticed, time, &mut events);
                    }
                }
            }
            for binding in &performance.midi {
                if !reads_port(binding.port.as_deref(), port) {
                    continue;
                }
                let router = &mut performance.router;
                let input = binding.input;
                events.extend(match (binding.reads, message) {
                    (MidiRead::Note(read), MidiMessage::NoteOn { note, velocity })
                        if read == note =>
                    {
                        router.strike(input, note, f32::from(velocity) / 127.0, time)
                    }
                    (MidiRead::Keys(range), MidiMessage::NoteOn { note, velocity })
                        if range.contains(note) =>
                    {
                        router.strike(input, note, f32::from(velocity) / 127.0, time)
                    }
                    (MidiRead::Note(read), MidiMessage::NoteOff { note }) if read == note => {
                        router.release_note(input, note, time)
                    }
                    (MidiRead::Keys(range), MidiMessage::NoteOff { note })
                        if range.contains(note) =>
                    {
                        router.release_note(input, note, time)
                    }
                    // A pedal is down at 64 and above.
                    (MidiRead::Pedal(read), MidiMessage::ControlChange { number, value })
                        if read == number =>
                    {
                        if value >= 64 {
                            router.press(input, time)
                        } else {
                            router.release(input, time)
                        }
                    }
                    (MidiRead::Dial(read), MidiMessage::ControlChange { number, value })
                        if read == number =>
                    {
                        router.turn(input, f32::from(value) / 127.0, time)
                    }
                    (MidiRead::PitchBend, MidiMessage::PitchBend { value }) => {
                        router.turn(input, f32::from(value) / 16383.0, time)
                    }
                    _ => Vec::new(),
                });
            }
            (!events.is_empty()).then_some(events)
        });
    }

    /// A port that went away while performance `generation` plays: the notes
    /// it held are released and the pedals it held lifted, while dials keep
    /// their place.
    fn perform_midi_lost(&self, generation: u32, port: &str, held: &Held) {
        for &note in &held.notes {
            self.perform_midi(generation, port, MidiMessage::NoteOff { note });
        }
        self.route(|performance, time| {
            if performance.generation != generation {
                return None;
            }
            let mut events = Vec::new();
            let Performance {
                midi,
                listening,
                router,
                ..
            } = performance;
            for binding in midi.iter() {
                if let MidiRead::Pedal(number) = binding.reads
                    && held.control_changes.contains(&number)
                    && reads_port(binding.port.as_deref(), port)
                {
                    events.extend(router.release(binding.input, time));
                }
            }
            if held.control_changes.contains(&SUSTAIN_PEDAL) {
                for item in listening.iter_mut() {
                    if reads_port(item.port.as_deref(), port) {
                        let noticed = item
                            .listener
                            .hear(Played::Sustain(false), session_seconds(time));
                        play_noticed(item, router, &noticed, time, &mut events);
                    }
                }
            }
            (!events.is_empty()).then_some(events)
        });
    }

    /// Let performance `generation`'s listeners notice the passing of time.
    /// `false` once that performance has ended.
    fn perform_listen_tick(&self, generation: u32) -> bool {
        let mut playing = false;
        self.route(|performance, time| {
            if performance.generation != generation {
                return None;
            }
            playing = true;
            let mut events = Vec::new();
            let Performance {
                listening, router, ..
            } = performance;
            for item in listening.iter_mut() {
                let noticed = item.listener.tick(session_seconds(time));
                play_noticed(item, router, &noticed, time, &mut events);
            }
            (!events.is_empty()).then_some(events)
        });
        playing
    }

    /// Release every held button, as when the performer's window loses focus.
    pub(crate) fn perform_release_all(&self) -> PerformSnapshot {
        self.route(|performance, time| Some(performance.router.release_all(time)));
        self.perform_snapshot()
    }

    /// Stamp an input on the session clock and fan the commands it routes to
    /// out to every evaluator, in order.
    fn route(&self, input: impl FnOnce(&mut Performance, SampleTime) -> Option<Vec<RigEvent>>) {
        let mut perform = lock_unpoisoned(&self.perform);
        let Some(performance) = perform.performance.as_mut() else {
            return;
        };
        let time = performance.clock.now();
        let Some(events) = input(performance, time) else {
            return;
        };
        performance.events.extend_from_slice(&events);
        if !events.is_empty() {
            lock_unpoisoned(&self.live_output).rig_events(&events);
        }
        perform.revision = perform.revision.wrapping_add(1);
        drop(perform);
        self.preview_wake.notify();
    }

    /// Live output for the performance in progress, if any. It holds the
    /// performance while the output starts, so no command is missed.
    pub(super) fn perform_live_output<R>(&self, start: impl FnOnce(Option<RigOutput>) -> R) -> R {
        let perform = lock_unpoisoned(&self.perform);
        start(perform.performance.as_ref().map(Performance::output))
    }

    pub(crate) fn performance_generation(&self) -> Option<u32> {
        lock_unpoisoned(&self.perform)
            .performance
            .as_ref()
            .map(|performance| performance.generation)
    }

    /// The performance's archive and fixtures, for the Preview.
    pub(crate) fn preview_performance(&self) -> Option<PreviewPerformance> {
        lock_unpoisoned(&self.perform)
            .performance
            .as_ref()
            .map(|performance| PreviewPerformance {
                generation: performance.generation,
                archive: performance.archive.clone(),
                fixtures: performance.fixtures.clone(),
            })
    }

    /// The performance's commands from the `from`th on, while `generation` is
    /// still performing.
    pub(crate) fn performance_events(&self, generation: u32, from: usize) -> Option<Vec<RigEvent>> {
        lock_unpoisoned(&self.perform)
            .performance
            .as_ref()
            .filter(|performance| performance.generation == generation)
            .map(|performance| performance.events[from..].to_vec())
    }

    /// The performance's session clock for the Preview, playing from its start.
    pub(crate) fn performance_clock(&self) -> Option<crate::preview::PreviewClock> {
        lock_unpoisoned(&self.perform)
            .performance
            .as_ref()
            .map(|performance| crate::preview::PreviewClock {
                generation: performance.generation,
                state: AudioTransportState::Playing,
                position_seconds: f64::from(sample_time_seconds_f32(performance.clock.now())),
                start_delay_seconds: 0.0,
                playback_speed: donder_runtime::PlaybackRate::NORMAL.into(),
            })
    }

    /// The performance's colors now, as every evaluator renders them.
    #[cfg(test)]
    pub(crate) fn render_performance_colors(&self) -> Option<Vec<donder_runtime_types::Color>> {
        let perform = lock_unpoisoned(&self.perform);
        let performance = perform.performance.as_ref()?;
        let mut playback = performance.prepared.clone().into_playback();
        for event in &performance.events {
            playback.apply(*event);
        }
        Some(playback.evaluate(performance.clock.now()).colors().to_vec())
    }
}

fn project_rigs(project: &DonderProject) -> Vec<PerformRig> {
    project
        .root()
        .rigs
        .iter()
        .enumerate()
        .map(|(index, source)| PerformRig {
            index: index as u32,
            name: source.id().0.declared_name().to_string(),
            path: source.id().0.document().to_string(),
        })
        .collect()
}

/// Explain why `rig` cannot play on the active setup, before preparing it.
fn check_armable(project: &DonderProject, rig: &RigId, name: &str) -> Result<(), String> {
    let rig = project
        .rig(rig)
        .ok_or("The rig is no longer in the project.")?;
    let setup = project
        .setup(project.root().setup.id())
        .ok_or("The active setup is missing.")?;
    let layout = setup.layout.id();
    if rig.effects().any(|effect| &effect.target.layout != layout) {
        return Err(format!(
            "Rig {name} targets a layout other than the active setup's."
        ));
    }
    if let Some(input) = rig.inputs().find(|input| {
        !setup
            .input_devices
            .iter()
            .any(|device| device.id() == &input.device)
    }) {
        return Err(format!(
            "Rig {name} plays from input device {}, which the active setup does not list.",
            input.device.0.declared_name()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::UdpSocket;
    use std::time::{Duration, Instant};

    #[test]
    fn a_key_plays_the_starter_jam_rig_to_live_output() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_millis(200)))
            .unwrap();
        let (_temporary, root) = crate::desktop_foundation_tests::tests::starter_copy();
        // Stream the starter's controller to this test over Art-Net.
        let path = root.join("setups/main.data.donder");
        let text = std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n");
        let start = text.find("  protocol: E131 {").unwrap();
        let end = start + text[start..].find("\n  },\n").unwrap() + "\n  },\n".len();
        let text = format!(
            "{}  protocol: ArtNet {{\n    bind_address: \"127.0.0.1:0\",\n    destination: \"{}\",\n    mode: Unicast,\n  }},\n{}",
            &text[..start],
            receiver.local_addr().unwrap(),
            &text[end..],
        )
        .replace(
            "address: Universe { universe: ",
            "address: ArtNetPort { port_address: ",
        );
        std::fs::write(&path, text).unwrap();
        let state = DesktopState::new(|_| {});
        state.open_project_path(root.as_str());
        assert!(
            state.project_session().is_some(),
            "{:?}",
            state.snapshot().diagnostics
        );
        let jam = state
            .perform_snapshot()
            .rigs
            .into_iter()
            .find(|rig| rig.name == "jam")
            .unwrap();
        assert_eq!(jam.path, "rigs/jam.data.donder");
        let revision = state.snapshot().project_revision;
        assert!(state.start_performing(revision + 1, jam.index).is_err());
        assert!(state.start_performing(revision, 99).is_err());

        let session = state
            .start_performing(revision, jam.index)
            .unwrap()
            .session
            .unwrap();
        assert_eq!(session.rig, jam);
        assert!(session.keys.iter().any(|key| key == "KeyA"));
        assert_eq!(session.switches.len(), 1);
        assert!(!session.switches[0].on);
        assert!(session.held.is_empty());
        assert_eq!(
            state.audio_play().status,
            "Stop performing to play a sequence."
        );
        state.set_live_output_active(true).unwrap();

        // Unbound keys and key names do nothing.
        state.perform_key("KeyQ", true);
        state.perform_key("NotAKey", true);
        let pressed = state.perform_key("KeyA", true).session.unwrap();
        assert_eq!(pressed.held.len(), 1);
        assert_eq!(pressed.held[0].device, "keyboard");
        assert_eq!(pressed.held[0].control, "kick");
        let colors = state.render_performance_colors().unwrap();
        assert!(
            colors
                .iter()
                .any(|color| color.red > 0.0 || color.green > 0.0 || color.blue > 0.0)
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut buffer = [0u8; 1024];
        let lit = loop {
            assert!(Instant::now() < deadline, "live output stayed dark");
            // Art-Net DMX data starts after an 18-byte header.
            if let Ok(length) = receiver.recv(&mut buffer)
                && length > 18
                && buffer[18..length].iter().any(|slot| *slot != 0)
            {
                break true;
            }
        };
        assert!(lit);
        assert!(
            state
                .perform_key("KeyA", false)
                .session
                .unwrap()
                .held
                .is_empty()
        );
        let shifted = state.perform_key("ShiftLeft", true).session.unwrap();
        assert!(shifted.switches[0].on);
        let released = state.perform_release_all().session.unwrap();
        assert!(released.held.is_empty());
        assert!(released.switches[0].on);

        let stopped = state.stop_performing();
        assert!(stopped.session.is_none());
        assert!(!state.is_performing());
        assert_ne!(
            state.snapshot().live_output.state,
            LiveOutputState::Streaming
        );
        state.shutdown_live_output();
    }
}
