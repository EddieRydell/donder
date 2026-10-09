//! Beat and downbeat detection from song audio, entirely in-process.
//!
//! Audio is decoded, mixed to mono and resampled, turned into the log-mel
//! spectrogram the model was trained on, and run through the small Beat This!
//! model (Foscarin, Schlüter and Widmer, ISMIR 2024; MIT licensed, see
//! `models/LICENSE`). Peaks in its framewise predictions are the beats and
//! downbeats.
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

mod decode;
mod model;
mod peaks;
mod spectrogram;

use std::fmt;
use std::path::Path;

/// Beat and downbeat times in seconds, ascending. Every downbeat is also a beat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BeatTimes {
    pub beats: Vec<f64>,
    pub downbeats: Vec<f64>,
}

#[derive(Debug)]
pub enum AnalysisError {
    Open(std::io::Error),
    NoAudioTrack,
    UnsupportedChannels,
    Decode(String),
    Resample(String),
    Spectrogram(String),
    Model(String),
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open(error) => write!(formatter, "Could not open the audio file: {error}"),
            Self::NoAudioTrack => formatter.write_str("The file has no audio track."),
            Self::UnsupportedChannels => formatter.write_str("The audio has no channels."),
            Self::Decode(message) => write!(formatter, "Could not decode the audio: {message}"),
            Self::Resample(message) => write!(formatter, "Could not resample the audio: {message}"),
            Self::Spectrogram(message) => {
                write!(formatter, "Could not analyze the audio: {message}")
            }
            Self::Model(message) => write!(formatter, "Beat detection failed: {message}"),
        }
    }
}

/// Detect the beats and downbeats of an audio file.
pub fn detect_beats(path: &Path) -> Result<BeatTimes, AnalysisError> {
    let (mono, rate) = decode::decode_mono(path)?;
    let signal = spectrogram::resample(&mono, rate)?;
    let spectrogram = spectrogram::log_mel(&signal)?;
    let (beat, downbeat) = model::predict(&spectrogram)?;
    Ok(peaks::beat_times(&beat, &downbeat))
}
