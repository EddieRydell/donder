//! Audio feature boundary: transport state in `engine`, Kira adaptation in `backend`.

mod backend;
mod engine;

pub(crate) use engine::{AudioEngine, Timeline, WindowEnd};

/// The extensions of the audio files a sequence can play.
pub(crate) const AUDIO_EXTENSIONS: &[&str] = &["mp3", "wav", "ogg", "flac"];
