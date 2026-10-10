//! Sequence video export: the Preview, rendered offscreen frame by frame from
//! prepared playback, encoded as H.264 with the sequence's song as AAC, in an
//! MP4 that phones and players open directly.

use std::io::{Seek, SeekFrom, Write};
use std::num::NonZeroU32;

use camino::Utf8Path;
use donder_model::{DonderProject, SequenceId};
use donder_runtime_types::sample_time_from_frame;
use donder_sequence_api::{PreviewAppearance, VideoExportProgress};
use fdk_aac::enc::{AudioObjectType, BitRate, ChannelMode, Encoder, EncoderParams, Transport};
use openh264::OpenH264API;
use openh264::encoder::{
    EncoderConfig, FrameRate, FrameType, IntraFramePeriod, MatrixCoefficients, QpRange,
    RateControlMode, VuiConfig,
};
use openh264::formats::{RgbaSliceU8, YUVBuffer};
use shiguredo_mp4::TrackKind;
use shiguredo_mp4::bitstream::aac::{
    Mp4aSampleEntryConfig, build_mp4a_box, parse_audio_specific_config,
};
use shiguredo_mp4::bitstream::h264::{
    H264NalUnitType, H264SampleEntryConfig, LengthSize, build_avc1_box_from_annexb,
    parse_annexb_nal_units,
};
use shiguredo_mp4::boxes::SampleEntry;
use shiguredo_mp4::descriptors::EsDescriptor;
use shiguredo_mp4::mux::{
    Mp4FileMuxer, Mp4FileMuxerOptions, Sample, estimate_maximum_moov_box_size,
};

use super::geometry::PreviewGeometry;
use super::renderer::PreviewFrameRenderer;
use super::scene::{PreviewColor, PreviewInstance, PreviewScene, PreviewSize, PreviewStyle};

/// Exported videos are 1080p, the size phones and sharing sites expect.
const VIDEO_SIZE: PreviewSize = PreviewSize {
    width: 1920,
    height: 1080,
};
const MAX_FRAMES_PER_SECOND: u32 = 120;
/// H.264 quantizer: low enough that small bright bulbs stay crisp on black.
const VIDEO_QP: u8 = 20;
const KEYFRAME_SECONDS: u32 = 2;
const MICROS_PER_SECOND: u64 = 1_000_000;
const AAC_BITS_PER_SECOND: u32 = 192_000;
/// Samples per channel in one AAC-LC frame.
const AAC_FRAME: usize = 1024;
/// Room for one encoded frame; AAC frames are far smaller.
const AAC_OUTPUT_BYTES: usize = 8192;
const STEREO: usize = 2;
/// Sample fields that `MuxedFile::append` fills in when it writes the data.
const UNPLACED_SAMPLE: Sample = Sample {
    track_kind: TrackKind::Video,
    sample_entry: None,
    keyframe: false,
    timescale: NonZeroU32::MIN,
    duration: 0,
    composition_time_offset: None,
    data_offset: 0,
    data_size: 0,
};

/// Render `sequence` as an MP4 into `output`, with `song` when given,
/// reporting `progress` about once per second of video.
pub(crate) fn export_video(
    project: &DonderProject,
    sequence: &SequenceId,
    song: Option<&Utf8Path>,
    appearance: PreviewAppearance,
    frames_per_second: u32,
    output: &mut (impl Write + Seek),
    mut progress: impl FnMut(VideoExportProgress),
) -> Result<(), String> {
    let video_timescale = NonZeroU32::new(frames_per_second)
        .filter(|fps| fps.get() <= MAX_FRAMES_PER_SECOND)
        .ok_or_else(|| {
            format!("Video frame rate must be between 1 and {MAX_FRAMES_PER_SECOND}.")
        })?;
    let fps = video_timescale.get();
    let style = PreviewStyle::from_appearance(appearance)?;
    let geometry = PreviewGeometry::from_project(project)?;
    let prepared =
        donder_elaboration::prepare(project, sequence, donder_elaboration::PrepareOutputs::All)
            .ok_or("The sequence cannot be played on the active setup.")?;
    if !prepared
        .fixtures()
        .iter()
        .map(|fixture| (fixture.id, fixture.pixel_count))
        .eq(geometry.fixtures.iter().copied())
    {
        return Err("The prepared sequence does not match the active layout.".into());
    }
    let scene = PreviewScene::new(
        0,
        geometry
            .instances
            .into_iter()
            .map(|center_radius| PreviewInstance { center_radius })
            .collect(),
    );
    let rate = prepared.frame_rate();
    let last_frame = prepared.frame_count().saturating_sub(1);
    let duration_micros = u64::from(prepared.duration().as_ticks());
    let mut playback = prepared.into_playback();
    let too_long = || "The sequence is too long to export.".to_string();
    // Enough video frames to cover the whole sequence.
    let total = u32::try_from((duration_micros * u64::from(fps)).div_ceil(MICROS_PER_SECOND))
        .map_err(|_| too_long())?;
    let song = song
        .map(|path| {
            progress(VideoExportProgress::PreparingAudio);
            encode_song(path)
        })
        .transpose()?;

    let audio_frames = song.as_ref().map_or(0, |song| song.frames.len());
    let mut muxer = Mp4FileMuxer::with_options(Mp4FileMuxerOptions {
        // Room for the index at the front, so players can start before the file loads.
        reserved_moov_box_size: estimate_maximum_moov_box_size(&[total as usize, audio_frames]),
        ..Mp4FileMuxerOptions::default()
    })
    .map_err(mux_error)?;
    let mut file = MuxedFile::new(output, muxer.initial_boxes_bytes())?;
    let mut renderer = PreviewFrameRenderer::new(VIDEO_SIZE).map_err(|error| error.to_string())?;
    // Fixed quality, no rate control: every frame of the show is kept.
    let config = EncoderConfig::new()
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .rate_control_mode(RateControlMode::Off)
        .qp(QpRange::new(VIDEO_QP, VIDEO_QP))
        .intra_frame_period(IntraFramePeriod::from_num_frames(fps * KEYFRAME_SECONDS))
        .skip_frames(false)
        // `read_rgba8` converts with the limited-range BT.601 matrix; HD players
        // assume BT.709 unless the stream says otherwise.
        .vui(VuiConfig::bt709().matrix_coefficients(MatrixCoefficients::Smpte170M));
    let mut encoder =
        openh264::encoder::Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|error| format!("Could not start the video encoder: {error}"))?;
    let (width, height) = (VIDEO_SIZE.width as usize, VIDEO_SIZE.height as usize);
    let mut rgba = vec![0u8; width * height * 4];
    let mut yuv = YUVBuffer::new(width, height);
    let mut colors = vec![style.unlit_color(); scene.instances.len()];
    let mut audio_entry = song.as_ref().map(|song| song.entry.clone());
    let mut next_audio = 0usize;
    for index in 0..total {
        if index % fps == 0 {
            progress(VideoExportProgress::Rendering {
                completed: index,
                total,
            });
        }
        let frame = u32::try_from(u64::from(index) * u64::from(rate) / u64::from(fps))
            .map_err(|_| too_long())?
            .min(last_frame);
        let time = sample_time_from_frame(frame, rate).map_err(|_| too_long())?;
        for (target, color) in colors.iter_mut().zip(playback.evaluate(time).colors()) {
            *target = PreviewColor::opaque([color.red, color.green, color.blue]);
        }
        renderer
            .render(&scene, &colors, style, &mut rgba)
            .map_err(|error| error.to_string())?;
        yuv.read_rgba8(RgbaSliceU8::new(&rgba, (width, height)));
        let bitstream = encoder
            .encode(&yuv)
            .map_err(|error| format!("Could not encode the video: {error}"))?;
        // Only IDR frames are safe random-access points for players.
        let keyframe = bitstream.frame_type() == FrameType::IDR;
        let annexb = bitstream.to_vec();
        if annexb.is_empty() {
            return Err("The video encoder skipped a frame.".into());
        }
        // The first frame carries the parameter sets that describe the stream.
        let sample_entry = (index == 0)
            .then(|| {
                build_avc1_box_from_annexb(
                    &annexb,
                    &H264SampleEntryConfig {
                        length_size: LengthSize::FourBytes,
                    },
                )
                .map(SampleEntry::Avc1)
                .map_err(|error| format!("Could not describe the video stream: {error}"))
            })
            .transpose()?;
        file.append(
            &mut muxer,
            &video_sample(&annexb)?,
            Sample {
                track_kind: TrackKind::Video,
                sample_entry,
                keyframe,
                timescale: video_timescale,
                duration: 1,
                ..UNPLACED_SAMPLE
            },
        )?;
        // Interleave each second of the song after its second of video.
        if let Some(song) = song
            .as_ref()
            .filter(|_| (index + 1) % fps == 0 || index + 1 == total)
        {
            let end = u64::from(index + 1) * u64::from(song.sample_rate);
            while let Some(data) = song.frames.get(next_audio)
                && (next_audio * AAC_FRAME) as u64 * u64::from(fps) < end
            {
                file.append(
                    &mut muxer,
                    data,
                    Sample {
                        track_kind: TrackKind::Audio,
                        sample_entry: audio_entry.take(),
                        keyframe: true,
                        timescale: song.timescale,
                        duration: AAC_FRAME as u32,
                        ..UNPLACED_SAMPLE
                    },
                )?;
                next_audio += 1;
            }
        }
    }
    progress(VideoExportProgress::Saving);
    let finalized = muxer.finalize().map_err(mux_error)?;
    for (offset, bytes) in finalized.offset_and_bytes_pairs() {
        file.write_at(offset, bytes)?;
    }
    Ok(())
}

/// The muxer's output file, tracking where the next sample's data starts.
struct MuxedFile<'a, W: Write + Seek> {
    output: &'a mut W,
    position: u64,
}

impl<'a, W: Write + Seek> MuxedFile<'a, W> {
    fn new(output: &'a mut W, initial: &[u8]) -> Result<Self, String> {
        output.write_all(initial).map_err(write_error)?;
        Ok(Self {
            output,
            position: initial.len() as u64,
        })
    }

    /// Write `data` and record it as `sample`, placed where it was written.
    fn append(
        &mut self,
        muxer: &mut Mp4FileMuxer,
        data: &[u8],
        sample: Sample,
    ) -> Result<(), String> {
        self.output.write_all(data).map_err(write_error)?;
        muxer
            .append_sample(&Sample {
                data_offset: self.position,
                data_size: data.len(),
                ..sample
            })
            .map_err(mux_error)?;
        self.position += data.len() as u64;
        Ok(())
    }

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> Result<(), String> {
        self.output
            .seek(SeekFrom::Start(offset))
            .map_err(write_error)?;
        self.output.write_all(bytes).map_err(write_error)
    }
}

/// One encoded picture as an MP4 sample: its NAL units, each prefixed with its
/// length. Parameter sets live in the sample entry, not in samples.
fn video_sample(annexb: &[u8]) -> Result<Vec<u8>, String> {
    let units = parse_annexb_nal_units(annexb)
        .map_err(|error| format!("Could not read the encoded video: {error}"))?;
    let mut sample = Vec::with_capacity(annexb.len());
    for unit in units.iter().filter(|unit| {
        !matches!(
            unit.nal_unit_type,
            H264NalUnitType::Sps | H264NalUnitType::Pps
        )
    }) {
        let length = u32::try_from(unit.data.len())
            .map_err(|_| "An encoded video frame is too large.".to_string())?;
        sample.extend_from_slice(&length.to_be_bytes());
        sample.extend_from_slice(unit.data);
    }
    Ok(sample)
}

/// The song as raw AAC-LC frames of `AAC_FRAME` samples each, from its first sample.
struct EncodedSong {
    sample_rate: u32,
    timescale: NonZeroU32,
    entry: SampleEntry,
    frames: Vec<Vec<u8>>,
}

/// Decode the song to stereo and encode it as AAC-LC.
fn encode_song(path: &Utf8Path) -> Result<EncodedSong, String> {
    let audio_error = |message: String| format!("Could not read the sequence audio: {message}");
    let audio = donder_audio_analysis::decode_audio(path.as_std_path())
        .map_err(|error| audio_error(error.to_string()))?;
    let encoder = Encoder::new(EncoderParams {
        bit_rate: BitRate::Cbr(AAC_BITS_PER_SECOND),
        sample_rate: audio.sample_rate,
        transport: Transport::Raw,
        channels: ChannelMode::Stereo,
        audio_object_type: AudioObjectType::Mpeg4LowComplexity,
    })
    .map_err(|error| audio_error(error.to_string()))?;
    let info = encoder
        .info()
        .map_err(|error| audio_error(error.to_string()))?;
    let config = info
        .confBuf
        .get(..info.confSize as usize)
        .ok_or_else(|| audio_error("the AAC encoder gave no stream description".into()))?;
    let config =
        parse_audio_specific_config(config).map_err(|error| audio_error(error.to_string()))?;
    // Mono is duplicated to both channels; channels past the second are dropped.
    let mut samples: Vec<i16> = Vec::with_capacity(audio.samples.len() / audio.channels * STEREO);
    for frame in audio.samples.chunks_exact(audio.channels) {
        let left = frame[0];
        let right = frame.get(1).copied().unwrap_or(left);
        samples.extend([to_i16(left), to_i16(right)]);
    }
    // fdk-aac delays its output by `nDelay` samples. Leading silence rounds the
    // delay up to whole frames, which are dropped, so the first kept frame starts
    // at the song's first sample. Trailing silence flushes the encoder's tail.
    let delay = info.nDelay as usize;
    let lead = (AAC_FRAME - delay % AAC_FRAME) % AAC_FRAME;
    let skipped = (delay + lead) / AAC_FRAME;
    let mut padded = vec![0i16; lead * STEREO];
    padded.extend_from_slice(&samples);
    padded.resize(padded.len() + (delay + AAC_FRAME) * STEREO, 0);
    let mut produced = 0usize;
    let mut frames = Vec::new();
    let mut output = vec![0u8; AAC_OUTPUT_BYTES];
    let mut input = padded.as_slice();
    while !input.is_empty() {
        let chunk = &input[..input.len().min(AAC_FRAME * STEREO)];
        let encoded = encoder
            .encode(chunk, &mut output)
            .map_err(|error| audio_error(error.to_string()))?;
        if encoded.input_consumed == 0 && encoded.output_size == 0 {
            return Err(audio_error(
                "the AAC encoder stopped accepting samples".into(),
            ));
        }
        input = &input[encoded.input_consumed..];
        if encoded.output_size > 0 {
            if produced >= skipped {
                frames.push(output[..encoded.output_size].to_vec());
            }
            produced += 1;
        }
    }
    let largest = frames.iter().map(Vec::len).max().unwrap_or(0);
    let mp4a = build_mp4a_box(
        &config,
        &Mp4aSampleEntryConfig {
            es_id: EsDescriptor::MIN_ES_ID,
            buffer_size_db: largest as u32,
            max_bitrate: AAC_BITS_PER_SECOND,
            avg_bitrate: AAC_BITS_PER_SECOND,
        },
    )
    .map_err(|error| audio_error(error.to_string()))?;
    Ok(EncodedSong {
        sample_rate: audio.sample_rate,
        timescale: NonZeroU32::new(audio.sample_rate)
            .ok_or_else(|| audio_error("the audio has no sample rate".into()))?,
        entry: SampleEntry::Mp4a(mp4a),
        frames,
    })
}

fn to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

fn mux_error(error: shiguredo_mp4::mux::MuxError) -> String {
    format!("Could not write the MP4: {error}")
}

fn write_error(error: std::io::Error) -> String {
    format!("Could not save the video: {error}")
}
