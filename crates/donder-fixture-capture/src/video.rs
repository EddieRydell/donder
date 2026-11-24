//! Brightness frames from a phone's MP4 or MOV video, H.264 or HEVC.
use shiguredo_mp4::Decode;
use shiguredo_mp4::boxes::{MoovBox, SampleEntry};
use shiguredo_mp4::demux::{Input, Mp4FileDemuxer};
use shiguredo_mp4::{BoxHeader, BoxType, TrackKind};

use crate::CaptureError;

/// Frames are averaged down until their longer side is at most this many
/// samples; enough to tell neighboring lights apart in a framed fixture.
const MAX_FRAME_SIDE: usize = 960;

/// A downscaled brightness frame, 0 to 1, in raster order.
pub(crate) struct Luma {
    pub(crate) width: usize,
    pub(crate) height: usize,
    pub(crate) samples: Vec<f32>,
}

enum Codec {
    H264,
    H265,
}

/// A video track ready to decode.
pub(crate) struct Video<'a> {
    bytes: &'a [u8],
    codec: Codec,
    length_size: u8,
    /// Parameter sets, each with a four-byte length.
    parameters: Vec<u8>,
    /// Each sample's bytes, in decoding order.
    samples: Vec<(usize, usize)>,
    /// Presentation times in seconds, in display order.
    times: Vec<f32>,
    /// How the player turns stored frames for display: x' = a·x + c·y,
    /// y' = b·x + d·y, as `[a, b, c, d]`.
    pub(crate) orientation: [f32; 4],
}

fn length_prefixed(units: &[Vec<u8>]) -> Vec<u8> {
    units
        .iter()
        .flat_map(|unit| {
            (unit.len() as u32)
                .to_be_bytes()
                .into_iter()
                .chain(unit.iter().copied())
        })
        .collect()
}

impl<'a> Video<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self, CaptureError> {
        let mut demuxer = Mp4FileDemuxer::new();
        demuxer.handle_input(Input {
            position: 0,
            data: bytes,
        });
        let track = demuxer
            .tracks()
            .map_err(|_| CaptureError::NotVideo)?
            .iter()
            .find(|track| track.kind == TrackKind::Video)
            .cloned()
            .ok_or(CaptureError::NotVideo)?;
        let timescale = track.timescale.get() as f32;
        let mut entry = None;
        let mut samples = Vec::new();
        let mut times = Vec::new();
        while let Some(sample) = demuxer.next_sample().map_err(|_| CaptureError::NotVideo)? {
            if sample.track.track_id != track.track_id {
                continue;
            }
            if entry.is_none() {
                entry = sample.sample_entry.cloned();
            }
            let start = usize::try_from(sample.data_offset).map_err(|_| CaptureError::NotVideo)?;
            if start + sample.data_size > bytes.len() {
                return Err(CaptureError::NotVideo);
            }
            samples.push((start, sample.data_size));
            let presentation =
                sample.timestamp as i64 + sample.composition_time_offset.unwrap_or(0);
            times.push(presentation as f32 / timescale);
        }
        times.sort_by(f32::total_cmp);
        let (codec, length_size, parameters) = match entry.ok_or(CaptureError::NotVideo)? {
            SampleEntry::Avc1(avc) => (
                Codec::H264,
                avc.avcc_box.length_size_minus_one.get() + 1,
                length_prefixed(&[avc.avcc_box.sps_list, avc.avcc_box.pps_list].concat()),
            ),
            SampleEntry::Hev1(hevc) => (
                Codec::H265,
                hevc.hvcc_box.length_size_minus_one.get() + 1,
                length_prefixed(&hevc_parameters(&hevc.hvcc_box)),
            ),
            SampleEntry::Hvc1(hevc) => (
                Codec::H265,
                hevc.hvcc_box.length_size_minus_one.get() + 1,
                length_prefixed(&hevc_parameters(&hevc.hvcc_box)),
            ),
            _ => return Err(CaptureError::UnsupportedCodec),
        };
        Ok(Self {
            bytes,
            codec,
            length_size,
            parameters,
            samples,
            times,
            orientation: orientation(bytes, track.track_id).ok_or(CaptureError::NotVideo)?,
        })
    }

    /// Visit each frame in display order with its time, until `visit`
    /// returns false.
    pub(crate) fn frames(
        &self,
        mut visit: impl FnMut(f32, &Luma) -> bool,
    ) -> Result<(), CaptureError> {
        let mut times = self.times.iter().copied();
        let mut show = |frame: Luma| times.next().is_some_and(|time| visit(time, &frame));
        let threads = std::thread::available_parallelism().map_or(1, |threads| threads.get());
        let parameters = &self.parameters;
        let units = self
            .samples
            .iter()
            .map(|&(start, size)| &self.bytes[start..start + size]);
        match self.codec {
            Codec::H264 => {
                let mut decoder = rust_h264::decoder::OrderedDecoder::new();
                let length_size = usize::from(self.length_size);
                for nal in rust_h264::nal::parse_avcc(parameters, 4) {
                    decoder.decode_nal(&nal).map_err(decode_error)?;
                }
                for sample in units {
                    for nal in rust_h264::nal::parse_avcc(sample, length_size) {
                        for frame in decoder.decode_nal(&nal).map_err(decode_error)? {
                            if !show(h264_luma(&frame)) {
                                return Ok(());
                            }
                        }
                    }
                }
                for frame in decoder.flush() {
                    if !show(h264_luma(&frame)) {
                        return Ok(());
                    }
                }
            }
            Codec::H265 => {
                let mut decoder = rust_h265::Decoder::new()
                    .with_display_order()
                    .with_threads(threads);
                for nal in rust_h265::parse_hvcc(parameters, 4) {
                    decoder.decode_nal(&nal).map_err(decode_error)?;
                }
                for sample in units {
                    for nal in rust_h265::parse_hvcc(sample, self.length_size) {
                        decoder.decode_nal(&nal).map_err(decode_error)?;
                    }
                    while let Some(frame) = decoder.next_output() {
                        if !show(h265_luma(&frame)?) {
                            return Ok(());
                        }
                    }
                }
                while let Some(frame) = decoder.flush() {
                    if !show(h265_luma(&frame)?) {
                        return Ok(());
                    }
                }
                if let Some(error) = decoder.next_error() {
                    return Err(decode_error(error));
                }
            }
        }
        Ok(())
    }
}

fn hevc_parameters(hvcc: &shiguredo_mp4::boxes::HvccBox) -> Vec<Vec<u8>> {
    hvcc.nalu_arrays
        .iter()
        .flat_map(|array| array.nalus.iter().cloned())
        .collect()
}

fn decode_error(error: impl core::fmt::Debug) -> CaptureError {
    CaptureError::Decode(format!("{error:?}"))
}

/// Average `sample(x, y)` over blocks so the longer side fits.
fn downscale(width: usize, height: usize, sample: impl Fn(usize, usize) -> f32) -> Luma {
    let factor = (width.max(height) / MAX_FRAME_SIDE).max(1);
    let (scaled_width, scaled_height) = (width / factor, height / factor);
    let area = (factor * factor) as f32;
    let mut samples = Vec::with_capacity(scaled_width * scaled_height);
    for row in 0..scaled_height {
        for column in 0..scaled_width {
            let mut sum = 0.0;
            for y in row * factor..(row + 1) * factor {
                for x in column * factor..(column + 1) * factor {
                    sum += sample(x, y);
                }
            }
            samples.push(sum / area);
        }
    }
    Luma {
        width: scaled_width,
        height: scaled_height,
        samples,
    }
}

fn h264_luma(frame: &rust_h264::decoder::Frame) -> Luma {
    let width = frame.width as usize;
    let scale = 1.0 / ((1u32 << frame.bit_depth) - 1) as f32;
    downscale(width, frame.height as usize, |x, y| {
        f32::from(frame.sample(&frame.y, y * width + x)) * scale
    })
}

fn h265_luma(frame: &rust_h265::Frame) -> Result<Luma, CaptureError> {
    let width = frame.width as usize;
    let height = frame.height as usize;
    let scale = 1.0 / ((1u32 << frame.bit_depth) - 1) as f32;
    if let Some(samples) = frame.y.as_u8() {
        return Ok(downscale(width, height, |x, y| {
            f32::from(samples[y * width + x]) * scale
        }));
    }
    let samples = frame
        .y
        .as_u16()
        .ok_or_else(|| CaptureError::Decode("a frame has no brightness samples".into()))?;
    Ok(downscale(width, height, |x, y| {
        f32::from(samples[y * width + x]) * scale
    }))
}

/// The track header's display matrix; phones film portrait as turned
/// landscape frames.
fn orientation(bytes: &[u8], track_id: u32) -> Option<[f32; 4]> {
    let mut rest = bytes;
    while let Ok((header, payload)) = BoxHeader::decode_header_and_payload(rest) {
        let size = header.external_size() + payload.len();
        if header.box_type == BoxType::Normal(*b"moov")
            && let Ok((moov, _)) = MoovBox::decode(&rest[..size])
            && let Some(trak) = moov
                .trak_boxes
                .iter()
                .find(|trak| trak.tkhd_box.track_id == track_id)
        {
            let matrix = trak.tkhd_box.matrix;
            return Some(
                [matrix[0], matrix[1], matrix[3], matrix[4]].map(|value| value as f32 / 65536.0),
            );
        }
        rest = rest.get(size..)?;
    }
    None
}
