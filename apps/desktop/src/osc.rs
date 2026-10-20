//! OSC input for performances: UDP messages sent to an input device's port,
//! on every network interface, handed on as each arrives. A port that cannot
//! be bound yet is tried again every second.

use std::net::{Ipv4Addr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rosc::{OscPacket, OscType};

use crate::desktop_state::lock_unpoisoned;

/// How often a listener checks whether the performance stopped, and how long
/// it waits before binding a port again.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// An OSC message a performance reads: its address, and its first argument
/// as a number, or `None` when it has no arguments.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OscMessage {
    pub(crate) address: String,
    pub(crate) value: Option<f32>,
}

impl OscMessage {
    /// The message, or `None` when its first argument is not a finite number
    /// or a boolean.
    fn read(message: rosc::OscMessage) -> Option<Self> {
        let value = match message.args.first() {
            None => None,
            Some(OscType::Float(value)) => Some(*value),
            Some(OscType::Double(value)) => Some(*value as f32),
            Some(OscType::Int(value)) => Some(*value as f32),
            Some(OscType::Long(value)) => Some(*value as f32),
            Some(OscType::Bool(value)) => Some(if *value { 1.0 } else { 0.0 }),
            Some(_) => return None,
        };
        if value.is_some_and(|value| !value.is_finite()) {
            return None;
        }
        Some(Self {
            address: message.addr,
            value,
        })
    }
}

/// Every message of a packet, in order, with bundles unpacked.
fn messages(packet: OscPacket, out: &mut Vec<OscMessage>) {
    match packet {
        OscPacket::Message(message) => out.extend(OscMessage::read(message)),
        OscPacket::Bundle(bundle) => {
            for packet in bundle.content {
                messages(packet, out);
            }
        }
    }
}

/// The UDP ports a performance listens on. Dropping it closes them all.
pub(crate) struct OscInputs {
    stop: Arc<AtomicBool>,
    listeners: Arc<Mutex<Listeners>>,
}

/// The ports bound so far, and why each port not bound yet failed to bind.
#[derive(Default)]
struct Listeners {
    bound: Vec<u16>,
    failing: Vec<(u16, String)>,
}

/// The ports listening now, and why a port that is not listening failed to
/// bind.
#[derive(Clone, Default)]
pub(crate) struct OscStatus {
    pub(crate) ports: Vec<u16>,
    pub(crate) error: Option<String>,
}

type OnMessage = Arc<dyn Fn(u16, OscMessage) + Send + Sync>;

impl OscInputs {
    /// Listen on each of `ports` and call `on_message` with the port for each
    /// message that arrives.
    pub(crate) fn start(
        ports: Vec<u16>,
        on_message: impl Fn(u16, OscMessage) + Send + Sync + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let listeners = Arc::new(Mutex::new(Listeners::default()));
        let on_message: OnMessage = Arc::new(on_message);
        for port in ports {
            let stop = stop.clone();
            let listeners = listeners.clone();
            let on_message = on_message.clone();
            std::thread::spawn(move || listen(port, &stop, &listeners, &on_message));
        }
        Self { stop, listeners }
    }

    pub(crate) fn status(&self) -> OscStatus {
        let listeners = lock_unpoisoned(&self.listeners);
        let mut ports = listeners.bound.clone();
        ports.sort_unstable();
        OscStatus {
            ports,
            error: listeners.failing.first().map(|(_, error)| error.clone()),
        }
    }
}

impl Drop for OscInputs {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn listen(port: u16, stop: &AtomicBool, listeners: &Mutex<Listeners>, on_message: &OnMessage) {
    let socket = loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, port)).and_then(|socket| {
            socket
                .set_read_timeout(Some(POLL_INTERVAL))
                .map(|()| socket)
        }) {
            Ok(socket) => break socket,
            Err(failure) => {
                let error = format!("Could not listen for OSC on port {port}: {failure}");
                let mut listeners = lock_unpoisoned(listeners);
                listeners.failing.retain(|(failing, _)| *failing != port);
                listeners.failing.push((port, error));
                drop(listeners);
                std::thread::sleep(POLL_INTERVAL);
            }
        }
    };
    {
        let mut listeners = lock_unpoisoned(listeners);
        listeners.failing.retain(|(failing, _)| *failing != port);
        listeners.bound.push(port);
    }
    let mut buffer = vec![0; rosc::decoder::MTU];
    let mut received = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        // A timeout only lets the loop see that the performance stopped.
        let Ok((length, _)) = socket.recv_from(&mut buffer) else {
            continue;
        };
        // A packet that is not OSC is ignored.
        let Ok((_, packet)) = rosc::decoder::decode_udp(&buffer[..length]) else {
            continue;
        };
        messages(packet, &mut received);
        for message in received.drain(..) {
            on_message(port, message);
        }
    }
}
