use serde::{Deserialize, Serialize};

use crate::persistence::PersistedWindowState;
use donder_sequence_api::{AudioTransportState, PlaybackSpeed, PreviewAppearance};

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
    /// Fixture instances and the encoded archive travel as the frame's binary
    /// payload, not in the JSON header.
    ReplaceContent {
        revision: u64,
        #[serde(skip)]
        instances: Vec<[f32; 4]>,
        #[serde(skip)]
        sequence: Option<Vec<u8>>,
    },
    SetClock {
        generation: u32,
        state: AudioTransportState,
        position_seconds: f32,
        start_delay_seconds: f32,
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
    GeometryChanged { window: PreviewWindowState },
    Closed { window: PreviewWindowState },
    QuitRequested,
    Error { message: String },
}

// Each message is one frame: a little-endian u32 length and a JSON header,
// then a little-endian u32 length and a binary payload.

pub(crate) fn write_command(
    writer: &mut impl std::io::Write,
    command: &PreviewCommand,
) -> Result<(), String> {
    let payload = match command {
        PreviewCommand::ReplaceContent {
            instances,
            sequence,
            ..
        } => content_payload(instances, sequence.as_deref())?,
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
    if let PreviewCommand::ReplaceContent {
        instances,
        sequence,
        ..
    } = &mut command
    {
        (*instances, *sequence) = read_content_payload(&payload)?;
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

/// Instance count, the instances' f32 components, a sequence flag, then the archive.
fn content_payload(instances: &[[f32; 4]], sequence: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let count = frame_length(instances.len()).map_err(|error| error.to_string())?;
    let mut payload =
        Vec::with_capacity(5 + instances.len() * 16 + sequence.map_or(0, <[u8]>::len));
    payload.extend_from_slice(&count.to_le_bytes());
    for component in instances.iter().flatten() {
        payload.extend_from_slice(&component.to_le_bytes());
    }
    payload.push(u8::from(sequence.is_some()));
    payload.extend_from_slice(sequence.unwrap_or_default());
    Ok(payload)
}

/// Fixture instances and the encoded archive, if any.
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
    let sequence = match rest.split_first().ok_or_else(invalid)? {
        (0, []) => None,
        (1, archive) => Some(archive.to_vec()),
        _ => return Err(invalid()),
    };
    Ok((instances, sequence))
}

#[cfg(test)]
mod tests {
    use super::{PreviewCommand, read_command, write_command};

    #[test]
    fn content_round_trips_through_a_binary_frame() {
        let mut bytes = Vec::new();
        for sequence in [None, Some(vec![]), Some(vec![7, 0, 255])] {
            write_command(
                &mut bytes,
                &PreviewCommand::ReplaceContent {
                    revision: 3,
                    instances: vec![[1.0, -2.5, 0.0, 4.25], [5.0, 6.0, 7.0, 8.0]],
                    sequence: sequence.clone(),
                },
            )
            .unwrap();
            write_command(&mut bytes, &PreviewCommand::Focus).unwrap();
            let mut reader = bytes.as_slice();
            let Some(PreviewCommand::ReplaceContent {
                revision,
                instances,
                sequence: decoded,
            }) = read_command(&mut reader).unwrap()
            else {
                panic!("expected content");
            };
            assert_eq!(revision, 3);
            assert_eq!(instances, [[1.0, -2.5, 0.0, 4.25], [5.0, 6.0, 7.0, 8.0]]);
            assert_eq!(decoded, sequence);
            assert!(matches!(
                read_command(&mut reader),
                Ok(Some(PreviewCommand::Focus))
            ));
            assert!(matches!(read_command(&mut reader), Ok(None)));
            bytes.clear();
        }
    }
}
