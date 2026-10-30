//! Perform mode: one rig played live from the setup's input devices. A
//! performance is not an edit; it never touches project documents or history.
//! Each input is stamped on the session clock, routed to rig commands, and the
//! commands fan out in order to live output and the Preview.

use std::collections::HashMap;

mod router;

use super::{DesktopState, lock_unpoisoned};
use crate::midi::{MidiInputs, MidiMessage};
use crate::osc::{OscInputs, OscMessage};
use crate::rendering::{RigOutput, SessionClock};
use donder_editor::dto::{
    AudioTransportState, LiveOutputState, PerformButton, PerformMidi, PerformOsc, PerformRig,
    PerformSession, PerformSnapshot, PerformSwitch,
};
use donder_elaboration::{PreparationCache, Retain};
use donder_model::{
    ButtonInput, ControllerId, DialInput, InputControlKind, InputDevice, InputDriver, KeyCode,
    KeyRange, OscAddress, RigId,
};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedShow;
use donder_runtime_types::{RigEvent, SampleTime, sample_time_seconds_f32};
use router::RigRouter;

#[derive(Default)]
pub(crate) struct PerformService {
    performance: Option<Performance>,
    /// Numbers performances, so the Preview tells one from the next.
    generation: u32,
    revision: u32,
    /// Keeps clips lowered between performances.
    preparation: PreparationCache,
}

/// What the Preview needs to play a performance.
pub(crate) struct PreviewPerformance {
    pub(crate) generation: u32,
    pub(crate) show: Vec<u8>,
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

/// An OSC address of a device the performance plays from.
struct OscBinding {
    /// The UDP port the device listens on.
    port: u16,
    address: OscAddress,
    reads: OscRead,
    /// The router input.
    input: usize,
}

/// What an OSC control reads.
#[derive(Clone, Copy)]
enum OscRead {
    Button,
    /// A dial whose first argument runs from `low` to `high`.
    Dial {
        low: f32,
        high: f32,
    },
}

/// The binding of `device`'s control at router input `input`, which reads
/// `address`.
fn osc_binding(
    device: &InputDevice,
    address: &OscAddress,
    reads: OscRead,
    input: usize,
) -> Result<OscBinding, String> {
    let InputDriver::Osc { port } = device.driver else {
        return Err("A control reads an OSC address its device does not listen for.".into());
    };
    Ok(OscBinding {
        port,
        address: address.clone(),
        reads,
        input,
    })
}

/// Whether a device that reads `wanted` reads the port named `port`: one
/// whose name contains it, ignoring case, or any port.
fn reads_port(wanted: Option<&str>, port: &str) -> bool {
    wanted.is_none_or(|wanted| port.to_lowercase().contains(&wanted.to_lowercase()))
}

/// What a MIDI message does to the control a binding reads.
enum MidiTouch {
    /// A note or pedal goes down, with its velocity in `0..=1`.
    Down {
        number: u8,
        velocity: f32,
    },
    Up {
        number: u8,
    },
    /// A dial turns, to a value in `0..=1`.
    Turn(f32),
}

impl MidiTouch {
    fn of(reads: MidiRead, message: MidiMessage) -> Option<Self> {
        let velocity = |velocity: u8| f32::from(velocity) / 127.0;
        Some(match (reads, message) {
            (
                MidiRead::Note(read),
                MidiMessage::NoteOn {
                    note,
                    velocity: struck,
                },
            ) if read == note => Self::Down {
                number: note,
                velocity: velocity(struck),
            },
            (
                MidiRead::Keys(range),
                MidiMessage::NoteOn {
                    note,
                    velocity: struck,
                },
            ) if range.contains(note) => Self::Down {
                number: note,
                velocity: velocity(struck),
            },
            (MidiRead::Note(read), MidiMessage::NoteOff { note }) if read == note => {
                Self::Up { number: note }
            }
            (MidiRead::Keys(range), MidiMessage::NoteOff { note }) if range.contains(note) => {
                Self::Up { number: note }
            }
            // A pedal is down at 64 and above.
            (MidiRead::Pedal(read), MidiMessage::ControlChange { number, value })
                if read == number =>
            {
                if value >= 64 {
                    Self::Down {
                        number,
                        velocity: 1.0,
                    }
                } else {
                    Self::Up { number }
                }
            }
            (MidiRead::Dial(read), MidiMessage::ControlChange { number, value })
                if read == number =>
            {
                Self::Turn(velocity(value))
            }
            (MidiRead::PitchBend, MidiMessage::PitchBend { value }) => {
                Self::Turn(f32::from(value) / 16383.0)
            }
            _ => return None,
        })
    }
}

struct Performance {
    generation: u32,
    rig: PerformRig,
    clock: SessionClock,
    prepared: PreparedShow,
    /// The prepared rig, encoded once for the Preview.
    show: Vec<u8>,
    fixtures: Vec<(u32, usize)>,
    router: RigRouter,
    /// Each router button's device and control names.
    buttons: Box<[PerformButton]>,
    /// The router buttons each key presses.
    keys: HashMap<KeyCode, Vec<usize>>,
    /// The router inputs MIDI plays.
    midi: Vec<MidiBinding>,
    /// The ports holding each MIDI press down, by router input and note or
    /// pedal number: the router hears the press when the first port holds it
    /// and its release when the last lets go.
    midi_held: HashMap<(usize, u8), Vec<usize>>,
    /// The names of the ports MIDI has come from, by the index `midi_held`
    /// holds them by.
    midi_ports: Vec<String>,
    /// The MIDI ports the performance is connected to, when it plays from
    /// MIDI.
    midi_inputs: Option<MidiInputs>,
    /// The router inputs OSC plays.
    osc: Vec<OscBinding>,
    /// The UDP ports the performance listens on, when it plays from OSC.
    osc_inputs: Option<OscInputs>,
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
            osc: self.osc_inputs.as_ref().map(|inputs| {
                let status = inputs.status();
                PerformOsc {
                    ports: status.ports,
                    error: status.error,
                }
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
            .map(|session| project_rigs(&session))
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
        let rig_id = *source.id();
        let described = perform_rig(&session, rig, rig_id)?;
        check_armable(&session, &rig_id, &described.name)?;
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
        // A rig plays only on streamed controllers.
        self.set_devices_live(false);
        {
            let mut perform = lock_unpoisoned(&self.perform);
            let armed =
                donder_elaboration::arm(project, &rig_id, Retain::All, &mut perform.preparation)
                    .ok_or_else(|| {
                        format!(
                            "Rig {} cannot be prepared on the active setup.",
                            described.name
                        )
                    })?;
            let router = RigRouter::new(
                project,
                project
                    .get::<donder_model::Rig>(rig_id)
                    .ok_or("The rig is no longer in the project.")?,
                &armed,
            )
            .ok_or("An input control of the rig is missing.")?;
            let setup = project
                .get::<donder_model::Setup>(*project.root().setup.id())
                .ok_or("The active setup is missing.")?;
            let mut keys: HashMap<KeyCode, Vec<usize>> = HashMap::new();
            let mut midi = Vec::new();
            let mut osc = Vec::new();
            let mut buttons = Vec::with_capacity(router.inputs().len());
            for (index, button) in router.inputs().iter().enumerate() {
                let device = project
                    .get::<donder_model::InputDevice>(button.device)
                    .ok_or("An input device of the rig is missing.")?;
                let control = device
                    .controls
                    .iter()
                    .find(|control| control.id == button.control)
                    .ok_or("A button of the rig is missing.")?;
                buttons.push(PerformButton {
                    device: declared_name(&session, device.id)?,
                    control: control.name.as_str().to_string(),
                });
                // Every control of the setup with this key, note or pedal plays.
                let reads = match &control.kind {
                    InputControlKind::Button(ButtonInput::Key(code)) => {
                        keys.entry(*code).or_default().push(index);
                        continue;
                    }
                    InputControlKind::Button(ButtonInput::Address(address)) => {
                        osc.push(osc_binding(device, address, OscRead::Button, index)?);
                        continue;
                    }
                    InputControlKind::Dial(DialInput::Address {
                        path: address,
                        low,
                        high,
                    }) => {
                        let reads = OscRead::Dial {
                            low: *low,
                            high: *high,
                        };
                        osc.push(osc_binding(device, address, reads, index)?);
                        continue;
                    }
                    InputControlKind::Button(ButtonInput::Note(note)) => MidiRead::Note(*note),
                    InputControlKind::Button(ButtonInput::ControlChange(number)) => {
                        MidiRead::Pedal(*number)
                    }
                    InputControlKind::Dial(DialInput::ControlChange(number)) => {
                        MidiRead::Dial(*number)
                    }
                    InputControlKind::Dial(DialInput::PitchBend) => MidiRead::PitchBend,
                    InputControlKind::Keys(range) => MidiRead::Keys(*range),
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
            let generation = perform.generation.wrapping_add(1);
            let midi_inputs = (!midi.is_empty()).then(|| {
                let ports = midi
                    .iter()
                    .map(|binding| binding.port.clone())
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
                    move |port| {
                        if let Some(state) = lost_state.upgrade() {
                            DesktopState(state).perform_midi_lost(generation, port);
                        }
                    },
                )
            });
            let osc_inputs = (!osc.is_empty()).then(|| {
                let mut ports = osc.iter().map(|binding| binding.port).collect::<Vec<_>>();
                ports.sort_unstable();
                ports.dedup();
                let state = self.weak.clone();
                OscInputs::start(ports, move |port, message| {
                    if let Some(state) = state.upgrade() {
                        DesktopState(state).perform_osc(generation, port, &message);
                    }
                })
            });
            let prepared = armed.prepared;
            let performance = Performance {
                generation,
                rig: described,
                clock: SessionClock::start(prepared.duration()),
                show: prepared.encode(),
                fixtures: prepared
                    .fixtures()
                    .iter()
                    .map(|fixture| (fixture.id, fixture.pixel_count as usize))
                    .collect(),
                prepared,
                router,
                buttons: buttons.into(),
                keys,
                midi,
                midi_held: HashMap::new(),
                midi_ports: Vec::new(),
                midi_inputs,
                osc,
                osc_inputs,
                switches: project
                    .get::<donder_model::Rig>(rig_id)
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
            perform.performance = Some(performance);
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
            let port = match performance
                .midi_ports
                .iter()
                .position(|known| known == port)
            {
                Some(index) => index,
                None => {
                    performance.midi_ports.push(port.to_string());
                    performance.midi_ports.len() - 1
                }
            };
            let name = &performance.midi_ports[port];
            let mut events = Vec::new();
            for binding in &performance.midi {
                if !reads_port(binding.port.as_deref(), name) {
                    continue;
                }
                let Some(touch) = MidiTouch::of(binding.reads, message) else {
                    continue;
                };
                let router = &mut performance.router;
                let input = binding.input;
                let pedal = matches!(binding.reads, MidiRead::Pedal(_));
                match touch {
                    MidiTouch::Down { number, velocity } => {
                        let holders = performance.midi_held.entry((input, number)).or_default();
                        if holders.contains(&port) {
                            continue;
                        }
                        holders.push(port);
                        // Another port already holds it down.
                        if holders.len() > 1 {
                            continue;
                        }
                        events.extend(if pedal {
                            router.press(input, time)
                        } else {
                            router.strike(input, number, velocity, time)
                        });
                    }
                    MidiTouch::Up { number } => {
                        let Some(holders) = performance.midi_held.get_mut(&(input, number)) else {
                            continue;
                        };
                        let Some(held) = holders.iter().position(|holder| *holder == port) else {
                            continue;
                        };
                        holders.swap_remove(held);
                        if !holders.is_empty() {
                            continue;
                        }
                        performance.midi_held.remove(&(input, number));
                        events.extend(if pedal {
                            router.release(input, time)
                        } else {
                            router.release_note(input, number, time)
                        });
                    }
                    MidiTouch::Turn(value) => events.extend(router.turn(input, value, time)),
                }
            }
            (!events.is_empty()).then_some(events)
        });
    }

    /// A port that went away while performance `generation` plays: the notes
    /// and pedals only it held are released, while dials keep their place.
    fn perform_midi_lost(&self, generation: u32, port: &str) {
        self.route(|performance, time| {
            if performance.generation != generation {
                return None;
            }
            let port = performance
                .midi_ports
                .iter()
                .position(|known| known == port)?;
            let mut events = Vec::new();
            let pedals = |input: usize, midi: &[MidiBinding]| {
                midi.iter().any(|binding| {
                    binding.input == input && matches!(binding.reads, MidiRead::Pedal(_))
                })
            };
            let midi = &performance.midi;
            let router = &mut performance.router;
            performance.midi_held.retain(|&(input, number), holders| {
                holders.retain(|holder| *holder != port);
                if !holders.is_empty() {
                    return true;
                }
                events.extend(if pedals(input, midi) {
                    router.release(input, time)
                } else {
                    router.release_note(input, number, time)
                });
                false
            });
            (!events.is_empty()).then_some(events)
        });
    }

    /// An OSC message to `port` while performance `generation` plays. A button
    /// presses at a value above 0 and releases at 0, and a message with no
    /// value presses and releases it at once; a dial turns to the value's
    /// place in its range.
    fn perform_osc(&self, generation: u32, port: u16, message: &OscMessage) {
        self.route(|performance, time| {
            if performance.generation != generation {
                return None;
            }
            let mut events = Vec::new();
            for binding in &performance.osc {
                if binding.port != port || binding.address.0 != message.address {
                    continue;
                }
                let router = &mut performance.router;
                let input = binding.input;
                match (binding.reads, message.value) {
                    (OscRead::Button, None) => {
                        events.extend(router.press(input, time));
                        events.extend(router.release(input, time));
                    }
                    (OscRead::Button, Some(value)) if value > 0.0 => {
                        events.extend(router.press(input, time));
                    }
                    (OscRead::Button, Some(_)) => events.extend(router.release(input, time)),
                    (OscRead::Dial { low, high }, Some(value)) => {
                        events.extend(router.turn(input, (value - low) / (high - low), time));
                    }
                    (OscRead::Dial { .. }, None) => {}
                }
            }
            (!events.is_empty()).then_some(events)
        });
    }

    /// Release every held button, as when the performer's window loses focus.
    pub(crate) fn perform_release_all(&self) -> PerformSnapshot {
        self.route(|performance, time| {
            performance.midi_held.clear();
            Some(performance.router.release_all(time))
        });
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

    /// The performance's show and fixtures, for the Preview.
    pub(crate) fn preview_performance(&self) -> Option<PreviewPerformance> {
        lock_unpoisoned(&self.perform)
            .performance
            .as_ref()
            .map(|performance| PreviewPerformance {
                generation: performance.generation,
                show: performance.show.clone(),
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
    fn render_performance_colors(&self) -> Option<Vec<donder_runtime_types::Color>> {
        let perform = lock_unpoisoned(&self.perform);
        let performance = perform.performance.as_ref()?;
        let mut playback = performance.prepared.clone().into_playback();
        for event in &performance.events {
            playback.apply(*event);
        }
        Some(playback.evaluate(performance.clock.now()).colors().to_vec())
    }
}

fn project_rigs(session: &ProjectSession) -> Vec<PerformRig> {
    session
        .project
        .root()
        .rigs
        .iter()
        .zip(0..)
        .map(|(source, index)| {
            perform_rig(session, index, *source.id())
                .unwrap_or_else(|_| unreachable!("every rig of a session has an address"))
        })
        .collect()
}

/// The project's `index`th rig as Perform names it.
fn perform_rig(session: &ProjectSession, index: u32, rig: RigId) -> Result<PerformRig, String> {
    let address = session
        .address_of(rig.into())
        .ok_or("The rig is missing from the source inventory.")?;
    Ok(PerformRig {
        index,
        name: address.declared_name().to_string(),
        path: address.document().to_string(),
    })
}

/// The name an object is declared or owned under.
fn declared_name(
    session: &ProjectSession,
    id: impl Into<donder_model::ObjectId>,
) -> Result<String, String> {
    session
        .address_of(id.into())
        .map(|address| address.declared_name().to_string())
        .ok_or_else(|| "An object is missing from the source inventory.".to_string())
}

/// Explain why `rig` cannot play on the active setup, before preparing it.
fn check_armable(session: &ProjectSession, rig: &RigId, name: &str) -> Result<(), String> {
    let project = &session.project;
    let rig = project
        .get::<donder_model::Rig>(*rig)
        .ok_or("The rig is no longer in the project.")?;
    let setup = project
        .get::<donder_model::Setup>(*project.root().setup.id())
        .ok_or("The active setup is missing.")?;
    let layout = setup.layout.id();
    if rig.clips().any(|clip| &clip.target.layout != layout) {
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
            declared_name(session, input.device)?
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
