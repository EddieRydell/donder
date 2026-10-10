use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::AnalysisError;

/// A song's default audio track, decoded.
#[derive(Clone, Debug)]
pub struct DecodedAudio {
    /// Interleaved samples in [-1, 1], `channels` per frame.
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
}

impl DecodedAudio {
    /// Each frame's channels averaged.
    pub fn mono(&self) -> Vec<f64> {
        self.samples
            .chunks_exact(self.channels)
            .map(|frame| {
                frame.iter().map(|&sample| f64::from(sample)).sum::<f64>() / frame.len() as f64
            })
            .collect()
    }
}

/// Decode the default audio track of the file at `path`.
pub fn decode_audio(path: &Path) -> Result<DecodedAudio, AnalysisError> {
    let file = std::fs::File::open(path).map_err(AnalysisError::Open)?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        hint.with_extension(extension);
    }
    let decode_error = |error: Error| AnalysisError::Decode(error.to_string());
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(decode_error)?;
    let track = format
        .default_track(TrackType::Audio)
        .ok_or(AnalysisError::NoAudioTrack)?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or(AnalysisError::NoAudioTrack)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())
        .map_err(decode_error)?;
    let track_id = track.id;
    let time_base = track.time_base;
    let mut audio: Option<DecodedAudio> = None;
    let mut interleaved = Vec::<f32>::new();
    while let Some(packet) = format.next_packet().map_err(decode_error)? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A corrupt packet becomes silence of its length, so later audio keeps its time.
            Err(Error::DecodeError(message)) => {
                let (Some(audio), Some(time_base)) = (audio.as_mut(), time_base) else {
                    return Err(AnalysisError::Decode(message.to_string()));
                };
                let frames = packet.dur.get()
                    * u64::from(time_base.numer.get())
                    * u64::from(audio.sample_rate)
                    / u64::from(time_base.denom.get());
                let silent = usize::try_from(frames)
                    .map_err(|_| AnalysisError::Decode(message.to_string()))?;
                audio
                    .samples
                    .resize(audio.samples.len() + silent * audio.channels, 0.0);
                continue;
            }
            Err(error) => return Err(decode_error(error)),
        };
        let channels = decoded.spec().channels().count();
        let sample_rate = decoded.spec().rate();
        let audio = audio.get_or_insert_with(|| DecodedAudio {
            samples: Vec::new(),
            channels,
            sample_rate,
        });
        if channels == 0 || channels != audio.channels || sample_rate != audio.sample_rate {
            return Err(AnalysisError::UnsupportedLayout);
        }
        interleaved.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut interleaved);
        audio.samples.extend_from_slice(&interleaved);
    }
    audio.ok_or(AnalysisError::NoAudioTrack)
}
