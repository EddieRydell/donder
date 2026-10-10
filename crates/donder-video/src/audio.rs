use camino::Utf8Path;
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::errors::Error;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::VideoError;

const AAC_BITS_PER_SECOND: u32 = 192_000;
/// Samples per channel in one AAC-LC frame.
const AAC_FRAME: usize = 1024;
/// Room for one encoded frame; AAC frames are far smaller.
const AAC_OUTPUT_BYTES: usize = 8192;

/// The song as ADTS-framed AAC frames, each with its first sample's position.
pub(crate) struct EncodedSong {
    pub(crate) sample_rate: u32,
    pub(crate) frames: Vec<(u64, Vec<u8>)>,
}

/// Decode the song to interleaved stereo and encode it as AAC-LC.
pub(crate) fn encode_song(path: &Utf8Path) -> Result<EncodedSong, VideoError> {
    let (samples, sample_rate) = decode_stereo(path)?;
    let encoder = Encoder::new(EncoderParams {
        bit_rate: BitRate::Cbr(AAC_BITS_PER_SECOND),
        sample_rate,
        transport: Transport::Adts,
        channels: ChannelMode::Stereo,
        audio_object_type: AudioObjectType::Mpeg4LowComplexity,
    })
    .map_err(|error| VideoError::Audio(error.to_string()))?;
    // fdk-aac delays its output by `nDelay` samples. Leading silence rounds the
    // delay up to whole frames, which are dropped, so the first kept frame starts
    // at the song's first sample. Trailing silence flushes the encoder's tail.
    let delay = encoder
        .info()
        .map_err(|error| VideoError::Audio(error.to_string()))?
        .nDelay as usize;
    let lead = (AAC_FRAME - delay % AAC_FRAME) % AAC_FRAME;
    let skipped = (delay + lead) / AAC_FRAME;
    let mut padded = vec![0i16; lead * 2];
    padded.extend_from_slice(&samples);
    padded.resize(padded.len() + (delay + AAC_FRAME) * 2, 0);
    let mut produced = 0usize;
    let mut frames = Vec::new();
    let mut output = vec![0u8; AAC_OUTPUT_BYTES];
    let mut input = padded.as_slice();
    while !input.is_empty() {
        let chunk = &input[..input.len().min(AAC_FRAME * 2)];
        let info = encoder
            .encode(chunk, &mut output)
            .map_err(|error| VideoError::Audio(error.to_string()))?;
        if info.input_consumed == 0 && info.output_size == 0 {
            return Err(VideoError::Audio(
                "the AAC encoder stopped accepting samples".into(),
            ));
        }
        input = &input[info.input_consumed..];
        if info.output_size > 0 {
            if produced >= skipped {
                let position = ((produced - skipped) * AAC_FRAME) as u64;
                frames.push((position, output[..info.output_size].to_vec()));
            }
            produced += 1;
        }
    }
    Ok(EncodedSong {
        sample_rate,
        frames,
    })
}

/// Decode the default audio track to interleaved 16-bit stereo; mono is
/// duplicated and extra channels are dropped.
fn decode_stereo(path: &Utf8Path) -> Result<(Vec<i16>, u32), VideoError> {
    let file = std::fs::File::open(path).map_err(|error| VideoError::Audio(error.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(extension) = path.extension() {
        hint.with_extension(extension);
    }
    let decode_error = |error: Error| VideoError::Audio(error.to_string());
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
        .ok_or_else(|| VideoError::Audio("the file has no audio track".into()))?;
    let parameters = track
        .codec_params
        .as_ref()
        .and_then(|parameters| parameters.audio())
        .ok_or_else(|| VideoError::Audio("the file has no audio track".into()))?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(parameters, &AudioDecoderOptions::default())
        .map_err(decode_error)?;
    let track_id = track.id;
    let mut stereo = Vec::new();
    let mut rate = None;
    let mut interleaved = Vec::<f32>::new();
    while let Some(packet) = format.next_packet().map_err(decode_error)? {
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            // A corrupt packet becomes silence of its length, so later audio keeps its time.
            Err(Error::DecodeError(_)) => {
                let silent = packet.dur.get() as usize;
                stereo.resize(stereo.len() + silent * 2, 0);
                continue;
            }
            Err(error) => return Err(decode_error(error)),
        };
        let channels = decoded.spec().channels().count();
        if channels == 0 {
            return Err(VideoError::Audio("the audio has no channels".into()));
        }
        rate = Some(decoded.spec().rate());
        interleaved.resize(decoded.samples_interleaved(), 0.0);
        decoded.copy_to_slice_interleaved(&mut interleaved);
        for frame in interleaved.chunks_exact(channels) {
            let left = frame[0];
            let right = frame.get(1).copied().unwrap_or(left);
            stereo.push(to_i16(left));
            stereo.push(to_i16(right));
        }
    }
    let rate = rate.ok_or_else(|| VideoError::Audio("the file has no audio".into()))?;
    Ok((stereo, rate))
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}
