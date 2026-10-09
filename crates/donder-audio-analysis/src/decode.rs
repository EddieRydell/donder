use std::path::Path;

use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::AnalysisError;

/// Decode the default audio track and average its channels.
pub(crate) fn decode_mono(path: &Path) -> Result<(Vec<f64>, u32), AnalysisError> {
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
    let mut mono = Vec::new();
    let mut rate = None;
    let mut interleaved = Vec::<f32>::new();
    while let Some(packet) = format.next_packet().map_err(decode_error)? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A corrupt packet loses only its own samples; the stream continues.
            Err(Error::DecodeError(_)) => continue,
            Err(error) => return Err(decode_error(error)),
        };
        let channels = decoded.spec().channels().count();
        if channels == 0 {
            return Err(AnalysisError::UnsupportedChannels);
        }
        rate = Some(decoded.spec().rate());
        interleaved.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut interleaved);
        mono.extend(interleaved.chunks_exact(channels).map(|frame| {
            frame.iter().map(|&sample| f64::from(sample)).sum::<f64>() / channels as f64
        }));
    }
    let rate = rate.ok_or(AnalysisError::NoAudioTrack)?;
    Ok((mono, rate))
}
