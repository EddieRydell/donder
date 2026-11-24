//! Reading the coded pattern out of the video: where each pixel lit.
use donder_output::{CAPTURE_SLOT_SECONDS, CaptureCode, CaptureSlot};
use glam::Vec2;

use crate::CaptureError;
use crate::video::{Luma, Video};

/// Frames are synchronized on blocks of this many samples a side.
const THUMBNAIL_BLOCK: usize = 8;
/// Blocks whose brightness swings by this share of the widest swing follow
/// the pattern.
const ACTIVE_SHARE: f32 = 0.5;
/// The smallest brightness swing, 0 to 1, the pattern must make where it
/// plays.
const MIN_CONTRAST: f32 = 0.02;
/// Frames dimmer than this share of the swing count as dark.
const DARK_SHARE: f32 = 0.15;
/// The pattern starts when the frame crosses this share of the swing.
const SYNC_SHARE: f32 = 0.5;
/// Dark slots the lead must last, short of its full length for blur.
const LEAD_MARGIN_SLOTS: f32 = 1.0;
/// Frames within this share of a slot from its middle are averaged into it.
const SLOT_WINDOW: f32 = 0.3;
/// An image sample takes part when it brightens by this share of the
/// brightest sample's change, and by at least `MIN_LIT`.
const LIT_SHARE: f32 = 0.2;
const MIN_LIT: f32 = 0.03;
/// Each bit must differ from its inverse by this share of the sample's change.
const BIT_SHARE: f32 = 0.35;
/// A pixel's samples lie within this share of the frame's longer side of its
/// brightest one; farther ones are reflections or noise.
const SPOT_RADIUS_SHARE: f32 = 0.02;

/// Where each of the code's pixels lit, in frame samples with y up as the
/// video is shown, or `None` where it was not seen.
pub(crate) fn locate(
    video: &Video<'_>,
    code: CaptureCode,
) -> Result<Vec<Option<Vec2>>, CaptureError> {
    let mut thumbnails = Vec::new();
    video.frames(|time, frame| {
        thumbnails.push((time, thumbnail(frame)));
        true
    })?;
    let start = cycle_start(&brightness(&thumbnails), code)?;
    let slots = slot_averages(video, code, start)?;
    Ok(decode(&slots, code, video.orientation))
}

/// The frame averaged in blocks of `THUMBNAIL_BLOCK` samples a side.
fn thumbnail(frame: &Luma) -> Vec<f32> {
    let (width, height) = (
        frame.width / THUMBNAIL_BLOCK,
        frame.height / THUMBNAIL_BLOCK,
    );
    let mut blocks = vec![0.0; width * height];
    for y in 0..height * THUMBNAIL_BLOCK {
        for x in 0..width * THUMBNAIL_BLOCK {
            blocks[y / THUMBNAIL_BLOCK * width + x / THUMBNAIL_BLOCK] +=
                frame.samples[y * frame.width + x];
        }
    }
    blocks
}

/// Each frame's brightness where the pattern plays: the blocks that change
/// most over the video, so a fixture filling little of the frame still
/// shows.
fn brightness(thumbnails: &[(f32, Vec<f32>)]) -> Vec<(f32, f32)> {
    let blocks = thumbnails.first().map_or(0, |(_, blocks)| blocks.len());
    let swing = (0..blocks)
        .map(|block| {
            let (low, high) = thumbnails.iter().fold(
                (f32::INFINITY, f32::NEG_INFINITY),
                |(low, high), (_, blocks)| (low.min(blocks[block]), high.max(blocks[block])),
            );
            high - low
        })
        .collect::<Vec<_>>();
    let widest = swing.iter().copied().fold(0.0, f32::max);
    let active = (0..blocks)
        .filter(|&block| swing[block] >= ACTIVE_SHARE * widest)
        .collect::<Vec<_>>();
    let area = (active.len() * THUMBNAIL_BLOCK * THUMBNAIL_BLOCK).max(1) as f32;
    thumbnails
        .iter()
        .map(|(time, blocks)| {
            (
                *time,
                active.iter().map(|&block| blocks[block]).sum::<f32>() / area,
            )
        })
        .collect()
}

/// When the first whole cycle in the video starts.
fn cycle_start(brightness: &[(f32, f32)], code: CaptureCode) -> Result<f32, CaptureError> {
    let (low, high) = brightness.iter().fold(
        (f32::INFINITY, f32::NEG_INFINITY),
        |(low, high), &(_, level)| (low.min(level), high.max(level)),
    );
    if high - low <= MIN_CONTRAST {
        return Err(CaptureError::NoPattern);
    }
    let dark = low + DARK_SHARE * (high - low);
    let sync = low + SYNC_SHARE * (high - low);
    let lead = (code.first_lit_slot() as f32 - LEAD_MARGIN_SLOTS) * CAPTURE_SLOT_SECONDS;
    let end = brightness.last().map_or(0.0, |&(time, _)| time);
    let mut dark_since = None;
    let mut found = false;
    for pair in brightness.windows(2) {
        let [(before_time, before), (time, level)] = [pair[0], pair[1]];
        if before < dark {
            dark_since.get_or_insert(before_time);
        }
        if level >= sync
            && let Some(since) = dark_since
            && time - since >= lead
        {
            let crossing = before_time + (sync - before) / (level - before) * (time - before_time);
            let start = crossing - code.first_lit_slot() as f32 * CAPTURE_SLOT_SECONDS;
            if start + code.cycle_seconds() <= end {
                return Ok(start);
            }
            found = true;
        }
        if level >= dark {
            dark_since = None;
        }
    }
    Err(if found {
        CaptureError::TooShort
    } else {
        CaptureError::NoPattern
    })
}

/// Each slot's frames, averaged.
fn slot_averages(
    video: &Video<'_>,
    code: CaptureCode,
    start: f32,
) -> Result<Vec<Luma>, CaptureError> {
    let mut sums: Vec<(Vec<f32>, u32)> = Vec::new();
    let mut size = (0, 0);
    let end = start + code.cycle_seconds();
    video.frames(|time, frame| {
        if time > end {
            return false;
        }
        let offset = (time - start) / CAPTURE_SLOT_SECONDS;
        if offset < 0.0 || (offset.fract() - 0.5).abs() > SLOT_WINDOW {
            return true;
        }
        if sums.is_empty() {
            size = (frame.width, frame.height);
            sums = (0..code.slots())
                .map(|_| (vec![0.0; frame.samples.len()], 0))
                .collect();
        }
        if let Some((sum, count)) = sums.get_mut(offset as usize) {
            for (total, sample) in sum.iter_mut().zip(&frame.samples) {
                *total += sample;
            }
            *count += 1;
        }
        true
    })?;
    if sums.is_empty() || sums.iter().any(|&(_, count)| count == 0) {
        return Err(CaptureError::FrameRate);
    }
    Ok(sums
        .into_iter()
        .map(|(sum, count)| Luma {
            width: size.0,
            height: size.1,
            samples: sum.into_iter().map(|total| total / count as f32).collect(),
        })
        .collect())
}

/// Read each image sample's code and gather the samples of each pixel.
fn decode(slots: &[Luma], code: CaptureCode, orientation: [f32; 4]) -> Vec<Option<Vec2>> {
    let average = |kind: CaptureSlot| {
        let chosen = (0..code.slots())
            .filter(|&slot| code.slot(slot) == kind)
            .filter_map(|slot| slots.get(slot as usize))
            .collect::<Vec<_>>();
        let len = chosen.first().map_or(0, |slot| slot.samples.len());
        (0..len)
            .map(|index| {
                chosen.iter().map(|slot| slot.samples[index]).sum::<f32>() / chosen.len() as f32
            })
            .collect::<Vec<_>>()
    };
    let (width, height) = slots
        .first()
        .map_or((0, 0), |slot| (slot.width, slot.height));
    let dark = average(CaptureSlot::Dark);
    let lit = average(CaptureSlot::Lit);
    let bits = (0..code.bits())
        .map(|bit| {
            [false, true].map(|inverted| {
                slots
                    .iter()
                    .enumerate()
                    .find(|&(slot, _)| code.slot(slot as u32) == CaptureSlot::Bit { bit, inverted })
                    .map(|(_, luma)| luma)
            })
        })
        .collect::<Vec<_>>();
    let change = lit
        .iter()
        .zip(&dark)
        .map(|(lit, dark)| lit - dark)
        .collect::<Vec<_>>();
    let brightest = change.iter().copied().fold(0.0, f32::max);
    let threshold = (LIT_SHARE * brightest).max(MIN_LIT);
    // Each sample's pixel and its weight.
    let samples = change
        .iter()
        .enumerate()
        .filter(|&(_, &change)| change >= threshold)
        .filter_map(|(index, &change)| {
            let mut value = 0;
            for (bit, [on, off]) in bits.iter().enumerate() {
                let (on, off) = ((*on)?.samples[index], (*off)?.samples[index]);
                if (on - off).abs() < BIT_SHARE * change {
                    return None;
                }
                if on > off {
                    value |= 1 << bit;
                }
            }
            Some((code.pixel(value)?, index, change))
        })
        .collect::<Vec<_>>();
    let position = |index: usize| {
        let (x, y) = ((index % width) as f32 + 0.5, (index / width) as f32 + 0.5);
        let [a, b, c, d] = orientation;
        Vec2::new(a * x + c * y, -(b * x + d * y))
    };
    let mut brightest = vec![None::<(usize, f32)>; code.pixels() as usize];
    for &(pixel, index, change) in &samples {
        let best = &mut brightest[pixel as usize];
        if best.is_none_or(|(_, weight)| change > weight) {
            *best = Some((index, change));
        }
    }
    let radius = SPOT_RADIUS_SHARE * width.max(height) as f32;
    let mut sums = vec![(Vec2::ZERO, 0.0f32); code.pixels() as usize];
    for &(pixel, index, change) in &samples {
        let Some((seed, _)) = brightest[pixel as usize] else {
            continue;
        };
        let point = position(index);
        if point.distance(position(seed)) <= radius {
            let sum = &mut sums[pixel as usize];
            sum.0 += point * change;
            sum.1 += change;
        }
    }
    sums.into_iter()
        .map(|(sum, weight)| (weight > 0.0).then(|| sum / weight))
        .collect()
}
