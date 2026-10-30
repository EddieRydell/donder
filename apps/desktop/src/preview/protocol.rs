use serde::{Deserialize, Serialize};

use crate::persistence::PersistedWindowState;
use donder_editor::dto::{AudioTransportState, PlaybackSpeed, PreviewAppearance};
use donder_runtime_types::{ConsoleCommand, ConsoleEvent, SampleDuration, SampleTime};

pub(crate) const PREVIEW_HOST_ARGUMENT: &str = "--donder-preview-host";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewStartup {
    pub(crate) appearance: PreviewAppearance,
    pub(crate) window: Option<PreviewWindowState>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PreviewWindowState {
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) maximized: bool,
}

impl From<PersistedWindowState> for PreviewWindowState {
    fn from(value: PersistedWindowState) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
            maximized: value.maximized,
        }
    }
}

impl From<PreviewWindowState> for PersistedWindowState {
    fn from(value: PreviewWindowState) -> Self {
        Self {
            x: value.x,
            y: value.y,
            width: value.width,
            height: value.height,
            maximized: value.maximized,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PreviewCommand {
    /// Fixture instances and the encoded show travel as the frame's binary
    /// payload, not in the JSON header.
    ReplaceContent {
        revision: u64,
        #[serde(skip)]
        instances: Vec<[f32; 4]>,
        #[serde(skip)]
        /// The encoded show the Preview plays: a sequence's or a console's.
        show: Option<Vec<u8>>,
    },
    /// Commands for the console the content plays, in the order they apply. They
    /// travel as the frame's binary payload.
    ConsoleEvents {
        #[serde(skip)]
        events: Vec<ConsoleEvent>,
    },
    SetClock {
        generation: u32,
        state: AudioTransportState,
        position_seconds: f64,
        start_delay_seconds: f64,
        playback_speed: PlaybackSpeed,
    },
    SetAppearance {
        appearance: PreviewAppearance,
    },
    Focus,
    Close,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum PreviewEvent {
    Ready,
    GeometryChanged {
        window: PreviewWindowState,
    },
    Closed {
        window: PreviewWindowState,
    },
    QuitRequested,
    /// A key pressed or released in the Preview, by its `KeyboardEvent.code` name.
    Key {
        code: String,
        pressed: bool,
    },
    /// The Preview lost focus, so every key it held is up.
    KeysReleased,
    Error {
        message: String,
    },
}

// Each message is one frame: a little-endian u32 length and a JSON header,
// then a little-endian u32 length and a binary payload.

pub(crate) fn write_command(
    writer: &mut impl std::io::Write,
    command: &PreviewCommand,
) -> Result<(), String> {
    let payload = match command {
        PreviewCommand::ReplaceContent {
            instances, show, ..
        } => content_payload(instances, show.as_deref())?,
        PreviewCommand::ConsoleEvents { events } => console_events_payload(events),
        _ => Vec::new(),
    };
    write_frame(writer, command, &payload)
}

pub(crate) fn read_command(
    reader: &mut impl std::io::Read,
) -> Result<Option<PreviewCommand>, String> {
    let Some((mut command, payload)) = read_frame::<PreviewCommand>(reader)? else {
        return Ok(None);
    };
    match &mut command {
        PreviewCommand::ReplaceContent {
            instances, show, ..
        } => (*instances, *show) = read_content_payload(&payload)?,
        PreviewCommand::ConsoleEvents { events } => {
            *events = read_console_events_payload(&payload)?;
        }
        _ => {}
    }
    Ok(Some(command))
}

pub(crate) fn write_event(
    writer: &mut impl std::io::Write,
    event: &PreviewEvent,
) -> Result<(), String> {
    write_frame(writer, event, &[])
}

pub(crate) fn read_event(reader: &mut impl std::io::Read) -> Result<Option<PreviewEvent>, String> {
    Ok(read_frame(reader)?.map(|(event, _)| event))
}

fn write_frame(
    writer: &mut impl std::io::Write,
    header: &impl Serialize,
    payload: &[u8],
) -> Result<(), String> {
    let header = serde_json::to_vec(header)
        .map_err(|error| format!("Cannot encode Preview message: {error}"))?;
    let write = |writer: &mut dyn std::io::Write| -> std::io::Result<()> {
        for part in [&header[..], payload] {
            writer.write_all(&frame_length(part.len())?.to_le_bytes())?;
            writer.write_all(part)?;
        }
        writer.flush()
    };
    write(writer).map_err(|error| format!("Cannot write Preview message: {error}"))
}

fn read_frame<T: serde::de::DeserializeOwned>(
    reader: &mut impl std::io::Read,
) -> Result<Option<(T, Vec<u8>)>, String> {
    let mut length = [0u8; 4];
    match reader.read_exact(&mut length) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(error) => return Err(format!("Cannot read Preview message: {error}")),
    }
    let header = read_part(reader, length)?;
    reader
        .read_exact(&mut length)
        .map_err(|error| format!("Cannot read Preview message: {error}"))?;
    let payload = read_part(reader, length)?;
    let header = serde_json::from_slice(&header)
        .map_err(|error| format!("Cannot decode Preview message: {error}"))?;
    Ok(Some((header, payload)))
}

fn read_part(reader: &mut impl std::io::Read, length: [u8; 4]) -> Result<Vec<u8>, String> {
    let mut part = vec![0u8; u32::from_le_bytes(length) as usize];
    reader
        .read_exact(&mut part)
        .map_err(|error| format!("Cannot read Preview message: {error}"))?;
    Ok(part)
}

fn frame_length(length: usize) -> std::io::Result<u32> {
    u32::try_from(length).map_err(|_| std::io::Error::other("Preview message is too large"))
}

const NO_ARCHIVE: u8 = 0;
const SHOW: u8 = 1;

/// Instance count, the instances' f32 components, whether a show
/// follows, then the show.
fn content_payload(instances: &[[f32; 4]], show: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let count = frame_length(instances.len()).map_err(|error| error.to_string())?;
    let (kind, bytes) = match show {
        None => (NO_ARCHIVE, &[][..]),
        Some(bytes) => (SHOW, bytes),
    };
    let mut payload = Vec::with_capacity(5 + instances.len() * 16 + bytes.len());
    payload.extend_from_slice(&count.to_le_bytes());
    for component in instances.iter().flatten() {
        payload.extend_from_slice(&component.to_le_bytes());
    }
    payload.push(kind);
    payload.extend_from_slice(bytes);
    Ok(payload)
}

/// Fixture instances and the encoded show, if any.
type PreviewContent = (Vec<[f32; 4]>, Option<Vec<u8>>);

fn read_content_payload(payload: &[u8]) -> Result<PreviewContent, String> {
    let invalid = || "Preview content payload is malformed.".to_string();
    let (count, rest) = payload.split_first_chunk::<4>().ok_or_else(invalid)?;
    let count = u32::from_le_bytes(*count) as usize;
    let (components, rest) = rest
        .split_at_checked(count.checked_mul(16).ok_or_else(invalid)?)
        .ok_or_else(invalid)?;
    let instances = components
        .as_chunks::<16>()
        .0
        .iter()
        .map(|instance| {
            let (components, _) = instance.as_chunks::<4>();
            core::array::from_fn(|index| f32::from_le_bytes(components[index]))
        })
        .collect();
    let show = match rest.split_first().ok_or_else(invalid)? {
        (&NO_ARCHIVE, []) => None,
        (&SHOW, show) => Some(show.to_vec()),
        _ => return Err(invalid()),
    };
    Ok((instances, show))
}

/// Start a voice for the duration that follows.
const START: u8 = 0;
const STOP: u8 = 1;
const MARK: u8 = 2;
const SET: u8 = 3;
/// Start a voice until it is stopped.
const HOLD: u8 = 4;
const CONSOLE_EVENT_BYTES: usize = 11;

/// Each event as its time in ticks, its command kind, the voice, collection or
/// automation index, and four bytes more: a start's duration in ticks, a set's
/// value, or zero.
fn console_events_payload(events: &[ConsoleEvent]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(events.len() * CONSOLE_EVENT_BYTES);
    for event in events {
        let (kind, index, more) = match event.command {
            ConsoleCommand::Start {
                voice,
                duration: Some(duration),
            } => (START, voice, duration.as_ticks().to_le_bytes()),
            ConsoleCommand::Start {
                voice,
                duration: None,
            } => (HOLD, voice, [0; 4]),
            ConsoleCommand::Stop { voice } => (STOP, voice, [0; 4]),
            ConsoleCommand::Mark { collection } => (MARK, collection, [0; 4]),
            ConsoleCommand::Set { automation, value } => (SET, automation, value.to_le_bytes()),
        };
        payload.extend_from_slice(&event.time.as_ticks().to_le_bytes());
        payload.push(kind);
        payload.extend_from_slice(&index.to_le_bytes());
        payload.extend_from_slice(&more);
    }
    payload
}

fn read_console_events_payload(payload: &[u8]) -> Result<Vec<ConsoleEvent>, String> {
    let invalid = || "Console event payload is malformed.".to_string();
    let (events, rest) = payload.as_chunks::<CONSOLE_EVENT_BYTES>();
    if !rest.is_empty() {
        return Err(invalid());
    }
    events
        .iter()
        .map(|event| {
            let [t0, t1, t2, t3, kind, i0, i1, m0, m1, m2, m3] = *event;
            let index = u16::from_le_bytes([i0, i1]);
            let more = [m0, m1, m2, m3];
            let command = match kind {
                START => ConsoleCommand::Start {
                    voice: index,
                    duration: Some(SampleDuration::from_ticks(u32::from_le_bytes(more))),
                },
                HOLD => ConsoleCommand::Start {
                    voice: index,
                    duration: None,
                },
                STOP => ConsoleCommand::Stop { voice: index },
                MARK => ConsoleCommand::Mark { collection: index },
                SET => ConsoleCommand::Set {
                    automation: index,
                    value: f32::from_le_bytes(more),
                },
                _ => return Err(invalid()),
            };
            Ok(ConsoleEvent {
                time: SampleTime::from_ticks(u32::from_le_bytes([t0, t1, t2, t3])),
                command,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        PreviewCommand, PreviewEvent, read_command, read_event, write_command, write_event,
    };
    use donder_runtime_types::{ConsoleCommand, ConsoleEvent, SampleDuration, SampleTime};

    #[test]
    fn content_round_trips_through_a_binary_frame() {
        let mut bytes = Vec::new();
        for show in [None, Some(vec![]), Some(vec![7, 0, 255])] {
            write_command(
                &mut bytes,
                &PreviewCommand::ReplaceContent {
                    revision: 3,
                    instances: vec![[1.0, -2.5, 0.0, 4.25], [5.0, 6.0, 7.0, 8.0]],
                    show: show.clone(),
                },
            )
            .unwrap();
            write_command(&mut bytes, &PreviewCommand::Focus).unwrap();
            let mut reader = bytes.as_slice();
            let Some(PreviewCommand::ReplaceContent {
                revision,
                instances,
                show: decoded,
            }) = read_command(&mut reader).unwrap()
            else {
                panic!("expected content");
            };
            assert_eq!(revision, 3);
            assert_eq!(instances, [[1.0, -2.5, 0.0, 4.25], [5.0, 6.0, 7.0, 8.0]]);
            assert_eq!(decoded, show);
            assert!(matches!(
                read_command(&mut reader),
                Ok(Some(PreviewCommand::Focus))
            ));
            assert!(matches!(read_command(&mut reader), Ok(None)));
            bytes.clear();
        }
    }

    #[test]
    fn console_events_and_preview_keys_round_trip() {
        let events = vec![
            ConsoleEvent {
                time: SampleTime::from_ticks(1_250_000),
                command: ConsoleCommand::Start {
                    voice: 2,
                    duration: Some(SampleDuration::from_ticks(400_000)),
                },
            },
            ConsoleEvent {
                time: SampleTime::from_ticks(1_250_000),
                command: ConsoleCommand::Start {
                    voice: 3,
                    duration: None,
                },
            },
            ConsoleEvent {
                time: SampleTime::from_ticks(1_250_000),
                command: ConsoleCommand::Set {
                    automation: 1,
                    value: 0.5,
                },
            },
            ConsoleEvent {
                time: SampleTime::from_ticks(u32::MAX),
                command: ConsoleCommand::Mark { collection: 0 },
            },
        ];
        let mut bytes = Vec::new();
        write_command(
            &mut bytes,
            &PreviewCommand::ConsoleEvents {
                events: events.clone(),
            },
        )
        .unwrap();
        write_command(
            &mut bytes,
            &PreviewCommand::ConsoleEvents { events: vec![] },
        )
        .unwrap();
        let mut reader = bytes.as_slice();
        let Some(PreviewCommand::ConsoleEvents { events: decoded }) =
            read_command(&mut reader).unwrap()
        else {
            panic!("expected console events");
        };
        assert_eq!(decoded, events);
        assert!(matches!(
            read_command(&mut reader),
            Ok(Some(PreviewCommand::ConsoleEvents { events })) if events.is_empty()
        ));

        let mut bytes = Vec::new();
        write_event(
            &mut bytes,
            &PreviewEvent::Key {
                code: "KeyA".into(),
                pressed: true,
            },
        )
        .unwrap();
        write_event(&mut bytes, &PreviewEvent::KeysReleased).unwrap();
        let mut reader = bytes.as_slice();
        assert!(matches!(
            read_event(&mut reader),
            Ok(Some(PreviewEvent::Key { code, pressed: true })) if code == "KeyA"
        ));
        assert!(matches!(
            read_event(&mut reader),
            Ok(Some(PreviewEvent::KeysReleased))
        ));
    }
}
