//! A performance's inputs streamed to the Donder controllers that play it,
//! one TCP connection each on its own thread, so a slow controller delays no
//! other. A controller joins with every input so far and then hears each new
//! one; a connection that breaks joins again, with every input, until the
//! performance ends.

use std::collections::HashMap;
use std::io::Write;
use std::net::{SocketAddr, TcpStream};
use std::sync::{Mutex, mpsc};
use std::thread;
use std::time::Duration;

use donder_device_protocol::console::{Message, PORT, encode_message};
use donder_runtime_types::ConsoleEvent;

use crate::desktop_state::lock_unpoisoned;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(2);
const RETRY_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Default)]
struct Performance {
    generation: u32,
    events: Vec<ConsoleEvent>,
    /// Each joined controller's link thread, by address; dropping one ends
    /// its connection.
    links: HashMap<SocketAddr, mpsc::Sender<Vec<ConsoleEvent>>>,
}

#[derive(Default)]
pub(crate) struct ConsoleFeed {
    performance: Mutex<Performance>,
}

impl ConsoleFeed {
    /// Performance `generation` armed with `events` so far. Controllers that
    /// joined another one leave it.
    pub(crate) fn perform(&self, generation: u32, events: Vec<ConsoleEvent>) {
        *lock_unpoisoned(&self.performance) = Performance {
            generation,
            events,
            links: HashMap::new(),
        };
    }

    /// New inputs of performance `generation`, in order.
    pub(crate) fn events(&self, generation: u32, events: &[ConsoleEvent]) {
        let mut performance = lock_unpoisoned(&self.performance);
        if performance.generation != generation {
            return;
        }
        performance.events.extend_from_slice(events);
        for link in performance.links.values() {
            let _ = link.send(events.to_vec());
        }
    }

    /// Join the controller at `address`, which plays performance
    /// `generation`'s console on the session clock, to its inputs.
    pub(crate) fn join(&self, generation: u32, address: SocketAddr) {
        let mut performance = lock_unpoisoned(&self.performance);
        if performance.generation != generation {
            return;
        }
        let (sender, receiver) = mpsc::channel();
        let events = performance.events.clone();
        thread::spawn(move || link(address, events, &receiver));
        performance.links.insert(address, sender);
    }

    /// Whether the controller at `address` hears performance `generation`.
    pub(crate) fn has_joined(&self, generation: u32, address: SocketAddr) -> bool {
        let performance = lock_unpoisoned(&self.performance);
        performance.generation == generation && performance.links.contains_key(&address)
    }

    /// End the performance's connections.
    pub(crate) fn stop(&self) {
        let mut performance = lock_unpoisoned(&self.performance);
        performance.events.clear();
        performance.links.clear();
    }
}

/// Keep the controller at `address` connected and hearing `events`, then
/// what `receiver` adds, until the feed lets go of it.
fn link(
    address: SocketAddr,
    mut events: Vec<ConsoleEvent>,
    receiver: &mpsc::Receiver<Vec<ConsoleEvent>>,
) {
    let mut stream = None;
    loop {
        let Some(open) = stream.as_mut() else {
            stream = connect(address, &events);
            if stream.is_none() {
                match receiver.recv_timeout(RETRY_INTERVAL) {
                    Ok(new) => events.extend(new),
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                }
            }
            continue;
        };
        let Ok(new) = receiver.recv() else {
            return;
        };
        let live = new
            .iter()
            .flat_map(|event| encode_message(&Message::Live(event.input)))
            .collect::<Vec<_>>();
        if open.write_all(&live).is_err() {
            stream = None;
        }
        events.extend(new);
    }
}

/// Connect to the controller at `address`, which then plays its console from
/// the start, and replay `events` to it.
fn connect(address: SocketAddr, events: &[ConsoleEvent]) -> Option<TcpStream> {
    let replayed = events
        .iter()
        .flat_map(|event| encode_message(&Message::Replayed(*event)))
        .collect::<Vec<_>>();
    let mut stream =
        TcpStream::connect_timeout(&SocketAddr::new(address.ip(), PORT), CONNECT_TIMEOUT).ok()?;
    stream.set_nodelay(true).ok()?;
    stream.set_write_timeout(Some(CONNECT_TIMEOUT)).ok()?;
    stream.write_all(&replayed).ok()?;
    Some(stream)
}
