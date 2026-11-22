//! FSEQ v2 sequence files, the format Falcon Player (FPP) schedules and plays.
//!
//! Frames are rendered at whole-millisecond steps of show time, each prepared
//! output's bytes at its controller's FSEQ channel and unused channels zero,
//! and compressed with zstd in blocks.

use core::fmt;
use core::num::{NonZeroU8, NonZeroU32};

use donder_model::{Controller, DonderProject, Setup};
use donder_runtime::{Output, PreparedShow};
use donder_runtime_types::SampleTime;

const HEADER_BYTES: usize = 32;
const BLOCK_INDEX_ENTRY_BYTES: usize = 8;
const MAJOR_VERSION: u8 = 2;
const MINOR_VERSION: u8 = 0;
const COMPRESSION_ZSTD: u8 = 1;
/// The block count is one header byte in FSEQ 2.0.
const MAX_BLOCKS: u32 = 255;
/// zstd's default level.
const ZSTD_LEVEL: i32 = 0;
const MICROS_PER_MILLI: u32 = 1_000;
const MILLIS_PER_SECOND: u32 = 1_000;
const PRODUCER: &str = concat!("Donder ", env!("CARGO_PKG_VERSION"));

/// The time between FSEQ frames, in whole milliseconds.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FseqStep(NonZeroU8);

impl FseqStep {
    /// The longest step that plays as smoothly as live output: longer steps
    /// show motion in visible jumps and flicker on dithered routes.
    pub const SMOOTH_MILLIS: u8 = 10;

    pub fn from_millis(millis: u8) -> Option<Self> {
        NonZeroU8::new(millis).map(Self)
    }

    /// The whole-millisecond step closest to an authored frame rate.
    pub fn nearest(frame_rate: u32) -> Self {
        let millis = (MILLIS_PER_SECOND + frame_rate / 2) / frame_rate.max(1);
        Self(NonZeroU8::new(millis.clamp(1, u32::from(u8::MAX)) as u8).unwrap_or(NonZeroU8::MIN))
    }

    pub fn millis(self) -> u8 {
        self.0.get()
    }

    pub fn is_smooth(self) -> bool {
        self.millis() <= Self::SMOOTH_MILLIS
    }
}

#[derive(Debug)]
pub enum FseqError {
    /// The output at this index has no FSEQ channel.
    Unplaced(usize),
    /// The outputs at these indices take some of the same channels.
    Overlap(usize, usize),
    TooManyChannels,
    TooManyFrames,
    HeaderTooLong,
    Compression(std::io::Error),
}

impl fmt::Display for FseqError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unplaced(_) => formatter.write_str("An output has no FSEQ channel."),
            Self::Overlap(_, _) => {
                formatter.write_str("Two outputs take some of the same FSEQ channels.")
            }
            Self::TooManyChannels => {
                formatter.write_str("The selected outputs have too many channels for FSEQ.")
            }
            Self::TooManyFrames => {
                formatter.write_str("The sequence has too many frames for FSEQ.")
            }
            Self::HeaderTooLong => {
                formatter.write_str("The media file name is too long for an FSEQ header.")
            }
            Self::Compression(error) => {
                write!(formatter, "Could not compress FSEQ frames: {error}")
            }
        }
    }
}

/// Render every frame of `sequence` into an FSEQ v2 file, each output at its
/// FSEQ channel from 1. `media_file` names the audio FPP plays with the
/// sequence. `progress` hears the frames rendered and the total, before the
/// first and after each block.
pub fn encode_fseq(
    sequence: PreparedShow,
    fseq_channel: impl Fn(&Output) -> Option<NonZeroU32>,
    step: FseqStep,
    media_file: Option<&str>,
    mut progress: impl FnMut(u32, u32),
) -> Result<Vec<u8>, FseqError> {
    // Without it, Windows runs a render that has no foreground window as
    // background work on efficiency cores, about four times as slowly.
    let _full_speed = power_guard::PowerGuard::no_throttling();
    let mut places = Vec::with_capacity(sequence.outputs().len());
    for (index, output) in sequence.outputs().iter().enumerate() {
        let start = fseq_channel(output)
            .ok_or(FseqError::Unplaced(index))?
            .get()
            - 1;
        let end = start
            .checked_add(output.width)
            .ok_or(FseqError::TooManyChannels)?;
        places.push((index, start as usize..end as usize));
    }
    places.sort_by_key(|(_, range)| range.start);
    for pair in places.windows(2) {
        if pair[1].1.start < pair[0].1.end {
            return Err(FseqError::Overlap(pair[0].0, pair[1].0));
        }
    }
    places.sort_by_key(|(index, _)| *index);
    let channels = places.iter().map(|(_, range)| range.end).max().unwrap_or(0);
    let channel_count = u32::try_from(channels).map_err(|_| FseqError::TooManyChannels)?;
    let step_micros = u32::from(step.millis()) * MICROS_PER_MILLI;
    let frame_count = sequence.duration().as_ticks().div_ceil(step_micros);
    let frames_per_block = frame_count.div_ceil(MAX_BLOCKS).max(1);

    let mut playback = sequence.into_playback();
    let mut blocks = Vec::new();
    let mut raw = Vec::with_capacity(frames_per_block as usize * channels);
    progress(0, frame_count);
    for first in (0..frame_count).step_by(frames_per_block as usize) {
        raw.clear();
        for frame in first..frame_count.min(first + frames_per_block) {
            let ticks = frame
                .checked_mul(step_micros)
                .ok_or(FseqError::TooManyFrames)?;
            let rendered = playback.evaluate(SampleTime::from_ticks(ticks));
            let offset = raw.len();
            raw.resize(offset + channels, 0);
            for (output, (_, range)) in rendered.outputs().zip(&places) {
                raw[offset + range.start..offset + range.end].copy_from_slice(output.bytes);
            }
        }
        let compressed = zstd::bulk::compress(&raw, ZSTD_LEVEL).map_err(FseqError::Compression)?;
        blocks.push((first, compressed));
        progress(frame_count.min(first + frames_per_block), frame_count);
    }

    let mut variables = Vec::new();
    push_variable(&mut variables, *b"sp", PRODUCER)?;
    if let Some(media) = media_file {
        push_variable(&mut variables, *b"mf", media)?;
    }
    let variables_start = HEADER_BYTES + blocks.len() * BLOCK_INDEX_ENTRY_BYTES;
    let data_start = (variables_start + variables.len()).next_multiple_of(4);

    let compressed_bytes: usize = blocks.iter().map(|(_, block)| block.len()).sum();
    let mut file = Vec::with_capacity(data_start + compressed_bytes);
    file.extend_from_slice(b"PSEQ");
    file.extend_from_slice(&header_u16(data_start)?.to_le_bytes());
    file.extend_from_slice(&[MINOR_VERSION, MAJOR_VERSION]);
    file.extend_from_slice(&header_u16(variables_start)?.to_le_bytes());
    file.extend_from_slice(&channel_count.to_le_bytes());
    file.extend_from_slice(&frame_count.to_le_bytes());
    // Step, flags, compression, block count, sparse range count, reserved.
    file.extend_from_slice(&[step.millis(), 0, COMPRESSION_ZSTD, blocks.len() as u8, 0, 0]);
    // FPP treats the unique ID as the file's creation time in microseconds.
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_micros() as u64);
    file.extend_from_slice(&created.to_le_bytes());
    for (first, block) in &blocks {
        let length = u32::try_from(block.len()).map_err(|_| FseqError::TooManyFrames)?;
        file.extend_from_slice(&first.to_le_bytes());
        file.extend_from_slice(&length.to_le_bytes());
    }
    file.extend_from_slice(&variables);
    file.resize(data_start, 0);
    for (_, block) in &blocks {
        file.extend_from_slice(block);
    }
    Ok(file)
}

/// Each prepared output's FSEQ channel from 1: its setup controller's, for
/// an E1.31 or Art-Net controller's universes.
pub fn setup_fseq_channel(project: &DonderProject) -> impl Fn(&Output) -> Option<NonZeroU32> + '_ {
    let setup = project.get::<Setup>(*project.root().setup.id());
    move |output| {
        let source = setup?.controllers.get(output.controller_index as usize)?;
        project.get::<Controller>(*source.id())?.fseq_channel()
    }
}

/// A variable header: its length including this prefix, a two-letter code and
/// a NUL-terminated value.
fn push_variable(out: &mut Vec<u8>, code: [u8; 2], value: &str) -> Result<(), FseqError> {
    let length = header_u16(4 + value.len() + 1)?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(&code);
    out.extend_from_slice(value.as_bytes());
    out.push(0);
    Ok(())
}

fn header_u16(value: usize) -> Result<u16, FseqError> {
    u16::try_from(value).map_err(|_| FseqError::HeaderTooLong)
}
