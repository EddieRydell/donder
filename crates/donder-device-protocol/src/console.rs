//! A performance on a controller: the editor connects to [`PORT`] over TCP
//! and sends [`Message`]s, each [`MESSAGE_BYTES`] long. Each connection joins
//! the performance from its start: the controller plays the loaded console
//! from the beginning, applies the replayed inputs at their times, then each
//! live input when it arrives. When the connection ends, the controller
//! releases every held control.

use donder_runtime_types::{ConsoleEvent, InputEvent, InputHappening, SampleTime};

/// The TCP port a controller takes a performance's inputs on.
pub const PORT: u16 = 81;

/// Bytes of an encoded [`ConsoleEvent`].
pub const EVENT_BYTES: usize = 12;
/// Bytes of an encoded [`Message`].
pub const MESSAGE_BYTES: usize = 1 + EVENT_BYTES;

const NO_NOTE: u8 = u8::MAX;
const REPLAYED: u8 = 0;
const LIVE: u8 = 1;

/// What a performance connection carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Message {
    /// An input the performance took before the controller joined, at its
    /// time on the session clock.
    Replayed(ConsoleEvent),
    /// An input the controller applies when it arrives.
    Live(InputEvent),
}

/// An event as its time in ticks, its control, happening and note (255 for
/// none), and its value.
pub fn encode_event(event: &ConsoleEvent) -> [u8; EVENT_BYTES] {
    let input = event.input;
    let mut bytes = [0; EVENT_BYTES];
    bytes[..4].copy_from_slice(&event.time.as_ticks().to_le_bytes());
    bytes[4..6].copy_from_slice(&input.control.to_le_bytes());
    bytes[6] = match input.happening {
        InputHappening::Press => 0,
        InputHappening::Release => 1,
        InputHappening::Move => 2,
    };
    bytes[7] = input.note.unwrap_or(NO_NOTE);
    bytes[8..].copy_from_slice(&input.value.to_le_bytes());
    bytes
}

/// The event `bytes` encode, or `None` for bytes no event encodes.
pub fn decode_event(bytes: &[u8; EVENT_BYTES]) -> Option<ConsoleEvent> {
    let [t0, t1, t2, t3, c0, c1, happening, note, v0, v1, v2, v3] = *bytes;
    Some(ConsoleEvent {
        time: SampleTime::from_ticks(u32::from_le_bytes([t0, t1, t2, t3])),
        input: InputEvent {
            control: u16::from_le_bytes([c0, c1]),
            note: (note != NO_NOTE).then_some(note),
            happening: match happening {
                0 => InputHappening::Press,
                1 => InputHappening::Release,
                2 => InputHappening::Move,
                _ => return None,
            },
            value: f32::from_le_bytes([v0, v1, v2, v3]),
        },
    })
}

/// A message as its kind, then its event; a live input's time is zero.
pub fn encode_message(message: &Message) -> [u8; MESSAGE_BYTES] {
    let (kind, event) = match *message {
        Message::Replayed(event) => (REPLAYED, event),
        Message::Live(input) => (
            LIVE,
            ConsoleEvent {
                time: SampleTime::from_ticks(0),
                input,
            },
        ),
    };
    let mut bytes = [0; MESSAGE_BYTES];
    bytes[0] = kind;
    bytes[1..].copy_from_slice(&encode_event(&event));
    bytes
}

/// The message `bytes` encode, or `None` for bytes no message encodes.
pub fn decode_message(bytes: &[u8; MESSAGE_BYTES]) -> Option<Message> {
    let (kind, event) = bytes.split_first()?;
    let event = decode_event(event.try_into().ok()?)?;
    match *kind {
        REPLAYED => Some(Message::Replayed(event)),
        LIVE => Some(Message::Live(event.input)),
        _ => None,
    }
}
