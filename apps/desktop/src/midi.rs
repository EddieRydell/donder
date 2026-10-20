//! MIDI input for performances: the ports a rig plays from, read on midir's
//! threads and handed on as each message arrives. Ports that appear while a
//! performance runs are connected within a second, and ports that go away
//! are dropped.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use midir::{Ignore, MidiInput, MidiInputConnection};

use crate::desktop_state::lock_unpoisoned;

/// How often the watcher looks for ports that appeared or went away.
const PORT_SCAN_INTERVAL: Duration = Duration::from_secs(1);

/// The MIDI messages a performance reads, from any channel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MidiMessage {
    NoteOn {
        note: u8,
        velocity: u8,
    },
    NoteOff {
        note: u8,
    },
    ControlChange {
        number: u8,
        value: u8,
    },
    /// From 0 (fully down) through 8192 (centre) to 16383 (fully up).
    PitchBend {
        value: u16,
    },
}

impl MidiMessage {
    pub(crate) fn parse(bytes: &[u8]) -> Option<Self> {
        let (&status, data) = bytes.split_first()?;
        let byte = |index: usize| data.get(index).copied().filter(|byte| *byte < 0x80);
        match status & 0xF0 {
            0x80 => Some(Self::NoteOff { note: byte(0)? }),
            // A note on with no velocity is how many keyboards send note off.
            0x90 => match (byte(0)?, byte(1)?) {
                (note, 0) => Some(Self::NoteOff { note }),
                (note, velocity) => Some(Self::NoteOn { note, velocity }),
            },
            0xB0 => Some(Self::ControlChange {
                number: byte(0)?,
                value: byte(1)?,
            }),
            0xE0 => Some(Self::PitchBend {
                value: u16::from(byte(0)?) | u16::from(byte(1)?) << 7,
            }),
            _ => None,
        }
    }
}

type OnMessage = Arc<dyn Fn(&str, MidiMessage) + Send + Sync>;
type OnLost = Arc<dyn Fn(&str) + Send + Sync>;

/// The MIDI ports a performance is connected to. Dropping it disconnects
/// them all.
pub(crate) struct MidiInputs {
    stop: Arc<AtomicBool>,
    status: Arc<Mutex<MidiStatus>>,
}

/// The ports connected now, and why the last look for ports failed.
#[derive(Clone, Default)]
pub(crate) struct MidiStatus {
    pub(crate) ports: Vec<String>,
    pub(crate) error: Option<String>,
}

impl MidiInputs {
    /// Connect to every port `wanted` accepts by name, now and as ports
    /// appear, and call `on_message` with the port's name for each message.
    /// A port that goes away calls `on_lost` with its name.
    pub(crate) fn start(
        wanted: impl Fn(&str) -> bool + Send + 'static,
        on_message: impl Fn(&str, MidiMessage) + Send + Sync + 'static,
        on_lost: impl Fn(&str) + Send + Sync + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let status = Arc::new(Mutex::new(MidiStatus::default()));
        let on_message: OnMessage = Arc::new(on_message);
        let on_lost: OnLost = Arc::new(on_lost);
        std::thread::spawn({
            let stop = stop.clone();
            let status = status.clone();
            move || watch(&stop, &status, &wanted, &on_message, &on_lost)
        });
        Self { stop, status }
    }

    pub(crate) fn status(&self) -> MidiStatus {
        lock_unpoisoned(&self.status).clone()
    }
}

impl Drop for MidiInputs {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

struct Connected {
    id: String,
    name: String,
    connection: MidiInputConnection<()>,
}

fn watch(
    stop: &AtomicBool,
    status: &Mutex<MidiStatus>,
    wanted: &dyn Fn(&str) -> bool,
    on_message: &OnMessage,
    on_lost: &OnLost,
) {
    // One client lists the ports for the whole performance; each connection
    // takes a client of its own.
    let mut scanner: Option<MidiInput> = None;
    let mut connected: Vec<Connected> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        let mut error = None;
        if scanner.is_none() {
            match MidiInput::new("Donder") {
                Ok(input) => scanner = Some(input),
                Err(failure) => error = Some(format!("MIDI is unavailable: {failure}")),
            }
        }
        if let Some(scanner) = &scanner {
            let mut available = Vec::new();
            for port in scanner.ports() {
                match scanner.port_name(&port) {
                    Ok(name) if wanted(&name) => available.push((port.id(), name)),
                    Ok(_) => {}
                    Err(failure) => error = Some(format!("A MIDI port has no name: {failure}")),
                }
            }
            let (kept, lost): (Vec<_>, Vec<_>) = connected
                .into_iter()
                .partition(|port| available.iter().any(|(id, _)| *id == port.id));
            connected = kept;
            // Nothing a port held stays down once it is gone.
            for port in lost {
                port.connection.close();
                on_lost(&port.name);
            }
            for (id, name) in available {
                if connected.iter().any(|port| port.id == id) {
                    continue;
                }
                match connect(&id, &name, on_message) {
                    Ok(connection) => connected.push(Connected {
                        id,
                        name,
                        connection,
                    }),
                    Err(failure) => error = Some(format!("Could not connect to {name}: {failure}")),
                }
            }
        }
        *lock_unpoisoned(status) = MidiStatus {
            ports: connected.iter().map(|port| port.name.clone()).collect(),
            error,
        };
        std::thread::sleep(PORT_SCAN_INTERVAL);
    }
}

fn connect(
    id: &str,
    name: &str,
    on_message: &OnMessage,
) -> Result<MidiInputConnection<()>, String> {
    let mut input = MidiInput::new("Donder").map_err(|error| error.to_string())?;
    input.ignore(Ignore::All);
    let port = input
        .find_port_by_id(id)
        .ok_or_else(|| "the port went away".to_string())?;
    let on_message = on_message.clone();
    let port_name = name.to_string();
    input
        .connect(
            &port,
            "donder-perform",
            move |_, bytes, _| {
                if let Some(message) = MidiMessage::parse(bytes) {
                    on_message(&port_name, message);
                }
            },
            (),
        )
        .map_err(|error| error.to_string())
}
