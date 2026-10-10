//! Sequence video export: the Preview's front view of every pixel, rendered
//! frame by frame from prepared playback, encoded as H.264 with the
//! sequence's song as AAC, in an MP4 that phones and players open directly.
#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod audio;
mod scene;

use std::fmt;

use camino::Utf8Path;
use donder_model::{DonderProject, SequenceId};
use donder_runtime_types::sample_time_from_frame;
use muxide::api::{AacProfile, AudioCodec, MuxerBuilder, VideoCodec};
use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, FrameType, IntraFramePeriod, RateControlMode,
};
use openh264::formats::{RgbSliceU8, YUVBuffer};

/// How the video looks and its size. Colours come from the caller's theme.
#[derive(Clone, Copy, Debug)]
pub struct VideoOptions {
    pub width: u32,
    pub height: u32,
    pub frames_per_second: u32,
    pub background_rgb: [u8; 3],
    pub unlit_rgb: [u8; 3],
    /// Share of the frame the layout fills, as in the Preview.
    pub canvas_fill_ratio: f32,
    /// Smallest drawn pixel radius, as in the Preview.
    pub minimum_radius_pixels: f32,
}

#[derive(Debug)]
pub enum VideoError {
    InvalidOptions(&'static str),
    MissingSetup,
    EmptyLayout,
    Prepare,
    Audio(String),
    Encode(String),
    Mux(String),
}

impl fmt::Display for VideoError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidOptions(message) => formatter.write_str(message),
            Self::MissingSetup => formatter.write_str("The project's setup or layout is missing."),
            Self::EmptyLayout => formatter.write_str("The layout has no pixels to draw."),
            Self::Prepare => {
                formatter.write_str("The sequence cannot be played on the active setup.")
            }
            Self::Audio(message) => {
                write!(formatter, "Could not read the sequence audio: {message}")
            }
            Self::Encode(message) => write!(formatter, "Could not encode the video: {message}"),
            Self::Mux(message) => write!(formatter, "Could not write the MP4: {message}"),
        }
    }
}

impl std::error::Error for VideoError {}

const VIDEO_BITS_PER_SECOND: u32 = 8_000_000;
const KEYFRAME_SECONDS: u32 = 2;

/// Render `sequence` as an MP4, with `audio` (the sequence's song) when given.
pub fn export_video(
    project: &DonderProject,
    sequence: &SequenceId,
    audio: Option<&Utf8Path>,
    options: &VideoOptions,
) -> Result<Vec<u8>, VideoError> {
    validate(options)?;
    let scene = scene::Scene::new(project, options)?;
    let prepared =
        donder_elaboration::prepare(project, sequence, donder_elaboration::PrepareOutputs::All)
            .ok_or(VideoError::Prepare)?;
    let rate = prepared.frame_rate();
    let frame_count = prepared.frame_count();
    let mut playback = prepared.into_playback();
    let fps = options.frames_per_second;
    let video_frames = u64::from(frame_count) * u64::from(fps) / u64::from(rate);
    let song = audio.map(audio::encode_song).transpose()?;

    let mut bytes = Vec::new();
    let mut builder = MuxerBuilder::new(&mut bytes)
        .video(
            VideoCodec::H264,
            options.width,
            options.height,
            f64::from(fps),
        )
        .with_fast_start(true);
    if let Some(song) = &song {
        builder = builder.audio(AudioCodec::Aac(AacProfile::Lc), song.sample_rate, 2);
    }
    let mut muxer = builder
        .build()
        .map_err(|error| VideoError::Mux(error.to_string()))?;
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(VIDEO_BITS_PER_SECOND))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .rate_control_mode(RateControlMode::Bitrate)
        .intra_frame_period(IntraFramePeriod::from_num_frames(fps * KEYFRAME_SECONDS));
    let mut encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
        .map_err(|error| VideoError::Encode(error.to_string()))?;

    let (width, height) = (options.width as usize, options.height as usize);
    let mut rgb = vec![0u8; width * height * 3];
    let mut yuv = YUVBuffer::new(width, height);
    let mut audio_frames = song.as_ref().map(|song| song.frames.iter().peekable());
    for index in 0..video_frames {
        let seconds = index as f64 / f64::from(fps);
        let frame = u32::try_from(index * u64::from(rate) / u64::from(fps))
            .map_err(|_| VideoError::InvalidOptions("The sequence is too long to export."))?;
        let time = sample_time_from_frame(frame, rate)
            .map_err(|_| VideoError::InvalidOptions("The sequence is too long to export."))?;
        scene.draw(&playback.evaluate(time), &mut rgb);
        yuv.read_rgb8(RgbSliceU8::new(&rgb, (width, height)));
        let encoded = encoder
            .encode(&yuv)
            .map_err(|error| VideoError::Encode(error.to_string()))?;
        let keyframe = matches!(encoded.frame_type(), FrameType::IDR | FrameType::I);
        let data = encoded.to_vec();
        if !data.is_empty() {
            muxer
                .write_video(seconds, &data, keyframe)
                .map_err(|error| VideoError::Mux(error.to_string()))?;
        }
        // Keep the song interleaved with the picture, up to the next video frame.
        if let (Some(frames), Some(song)) = (audio_frames.as_mut(), &song) {
            let next = (index + 1) as f64 / f64::from(fps);
            while let Some((position, data)) = frames
                .next_if(|(position, _)| (*position as f64) / f64::from(song.sample_rate) < next)
            {
                muxer
                    .write_audio(*position as f64 / f64::from(song.sample_rate), data)
                    .map_err(|error| VideoError::Mux(error.to_string()))?;
            }
        }
    }
    muxer
        .finish()
        .map_err(|error| VideoError::Mux(error.to_string()))?;
    Ok(bytes)
}

fn validate(options: &VideoOptions) -> Result<(), VideoError> {
    if options.width < 16
        || options.height < 16
        || !options.width.is_multiple_of(2)
        || !options.height.is_multiple_of(2)
    {
        return Err(VideoError::InvalidOptions(
            "Video width and height must be even and at least 16.",
        ));
    }
    if options.frames_per_second == 0 || options.frames_per_second > 120 {
        return Err(VideoError::InvalidOptions(
            "Video frame rate must be between 1 and 120.",
        ));
    }
    if !options.canvas_fill_ratio.is_finite()
        || options.canvas_fill_ratio <= 0.0
        || options.canvas_fill_ratio > 1.0
    {
        return Err(VideoError::InvalidOptions(
            "The canvas fill ratio must be in (0, 1].",
        ));
    }
    if !options.minimum_radius_pixels.is_finite() || options.minimum_radius_pixels < 0.0 {
        return Err(VideoError::InvalidOptions(
            "The minimum pixel radius must not be negative.",
        ));
    }
    Ok(())
}
