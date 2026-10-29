//! Sparse evaluation of one authored clip from an already prepared show.
//! Raster dimensions, scheduling, show storage, and caching belong to the host.
use crate::format;
use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use donder_runtime_types::{Color, SampleDuration, SampleTime, SampleTimeError};

use crate::arena::{Carve, Size};
use crate::dsl::{NoSignals, ParamsRef, StripSlots, params_at, scratch_shape};
use crate::evaluation::{PixelSet, RunPixel, clip_evaluator};
use crate::resources::LiveView;
use crate::view::Show;
use crate::workspace::{Timing, carve_scratch, scratch_size};

#[derive(Clone, Copy)]
pub struct ShowClip<'a> {
    show: Show<'a>,
    index: usize,
}

impl<'a> ShowClip<'a> {
    pub(crate) fn new(show: Show<'a>, index: usize) -> Self {
        Self { show, index }
    }

    fn record(&self) -> &'a format::Clip {
        &self.show.s.clips[self.index]
    }

    /// Sample the center frame of a raster column inside the authored clip.
    pub fn raster_column_time(
        &self,
        column: usize,
        columns: usize,
    ) -> Result<SampleTime, SampleTimeError> {
        if columns == 0 || column >= columns {
            return Err(SampleTimeError::OutOfRange);
        }
        let rate = self.frame_rate();
        let micros = donder_runtime_types::MICROS_PER_SECOND;
        // The first frame starting at or after `ticks`.
        let frame_from = |ticks: u32| {
            let (frame, rest) = donder_runtime_types::mul_div(ticks, rate, micros)
                .ok_or(SampleTimeError::OutOfRange)?;
            Ok(frame + u32::from(rest != 0))
        };
        let ticks = self.start_time().as_ticks();
        let start_frame = frame_from(ticks)?;
        let end_frame = frame_from(ticks.saturating_add(self.duration().as_ticks()))?;
        let active_frames = end_frame.saturating_sub(start_frame).max(1);
        // floor((column + 0.5) * active_frames / columns), exactly.
        let (offset, _) =
            donder_runtime_types::mul_div(2 * column as u32 + 1, active_frames, 2 * columns as u32)
                .ok_or(SampleTimeError::OutOfRange)?;
        let frame =
            (start_frame + offset.min(active_frames - 1)).min(self.frame_count().saturating_sub(1));
        donder_runtime_types::sample_time_from_frame(frame, self.frame_rate())
    }

    pub fn start_time(&self) -> SampleTime {
        SampleTime::from_ticks(self.record().start)
    }

    pub fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.record().duration)
    }

    pub fn frame_rate(&self) -> u32 {
        self.show.frame_rate()
    }

    pub fn frame_count(&self) -> u32 {
        self.show.frame_count()
    }

    pub fn target_pixel_count(&self) -> usize {
        crate::targets::TargetView::of(
            self.show,
            &self.show.s.targets[self.record().target as usize],
        )
        .len()
    }

    /// Select evenly spaced cells while preserving their original sampling
    /// coordinates. Equal contexts share one VM sample.
    pub fn raster(&self, rows: usize) -> ClipRaster<'a> {
        let show = self.show;
        let clip = self.record();
        let program = show.program(clip.program);
        let target = crate::targets::TargetView::of(show, &show.s.targets[clip.target as usize]);
        let spatial = program.flag(format::PROGRAM_USES_SPATIAL);
        let sections = program.flag(format::PROGRAM_USES_SECTIONS);
        let indices = evenly_sample_indices(target.len(), rows);
        let row_count = indices.len();
        let mut groups = Vec::<RasterGroup>::new();
        let mut pixels = Vec::<RunPixel>::new();
        let mut group_by_context = BTreeMap::new();
        for (row, local) in indices.into_iter().enumerate() {
            let pixel = target.pixel(local);
            let geometry = if spatial {
                target.spatial_context(local, &pixel, show)
            } else {
                donder_runtime_types::SpatialContext {
                    position: [0.0; 2],
                    min: [0.0; 2],
                    max: [0.0; 2],
                }
            };
            let section = target.sections.pixel(local);
            let key = (
                sections.then_some(section),
                pixel.pixel_index,
                pixel.pixel_count,
                [
                    geometry.position[0],
                    geometry.position[1],
                    geometry.min[0],
                    geometry.min[1],
                    geometry.max[0],
                    geometry.max[1],
                ]
                .map(f32::to_bits),
            );
            if let Some(&group) = group_by_context.get(&key) {
                let group: &mut RasterGroup = &mut groups[group];
                group.rows.push(row);
            } else {
                group_by_context.insert(key, groups.len());
                pixels.push(RunPixel {
                    target_index: local,
                    pixel,
                });
                groups.push(RasterGroup { rows: vec![row] });
            }
        }
        let mut slots = StripSlots::default();
        slots.include(program);
        let mut size = Size::default();
        slots.size(&mut size);
        let scratch = scratch_shape(show, clip.automation, clip.params);
        scratch_size(&mut size, scratch);
        ClipRaster {
            clip: *self,
            output: vec![Color::BLACK; row_count],
            groups,
            pixels,
            slots,
            scratch,
            buffer: vec![0; size.bytes.div_ceil(4)].into_boxed_slice(),
            fresh: true,
        }
    }
}

/// Reusable sparse evaluator borrowing a show.
pub struct ClipRaster<'a> {
    clip: ShowClip<'a>,
    output: Vec<Color>,
    groups: Vec<RasterGroup>,
    /// A pixel of each group.
    pixels: Vec<RunPixel>,
    slots: StripSlots,
    scratch: [usize; 4],
    /// The strip workspace and the automation scratch.
    buffer: Box<[u32]>,
    /// Whether the buffer has not been carved yet.
    fresh: bool,
}

/// Rows sharing one sampling context.
struct RasterGroup {
    rows: Vec<usize>,
}

impl ClipRaster<'_> {
    pub fn evaluate(&mut self, time: SampleTime) -> &[Color] {
        let show = self.clip.show;
        let clip = self.clip.record();
        let timing = Timing {
            start: clip.start,
            duration: clip.duration,
        };
        if !timing.is_active(time) {
            self.output.fill(Color::BLACK);
            return &self.output;
        }
        let bytes = bytemuck::cast_slice_mut(&mut self.buffer);
        let mut arena = if self.fresh {
            Carve::new(bytes)
        } else {
            Carve::again(bytes)
        };
        self.fresh = false;
        let (Some(mut workspace), Some(mut scratch)) = (
            self.slots.carve(&mut arena),
            carve_scratch(&mut arena, self.scratch),
        ) else {
            unreachable!("the buffer holds the sampler's size")
        };
        let params = params_at(
            show,
            &mut scratch,
            ParamsRef {
                params: clip.params,
                plan: clip.automation,
                origin: clip.start,
            },
            time,
            &[],
            LiveView::default(),
        );
        let sampler = clip_evaluator(show, clip, timing, time, 1, params);
        let (groups, output) = (&self.groups, &mut self.output);
        sampler.evaluate_pixels(
            groups.len(),
            PixelSet::Listed(&self.pixels),
            &mut workspace,
            &mut NoSignals,
            |group, color| {
                for &row in &groups[group].rows {
                    output[row] = color;
                }
            },
        );
        &self.output
    }
}

fn evenly_sample_indices(source_count: usize, sample_count: usize) -> Vec<usize> {
    let sample_count = source_count.min(sample_count);
    if sample_count == 0 {
        return Vec::new();
    }
    if sample_count == 1 {
        return vec![0];
    }
    let last_source = (source_count - 1) as u32;
    let last_sample = (sample_count - 1) as u32;
    // round(sample * last_source / last_sample), ties up.
    (0..sample_count as u32)
        .map(|sample| {
            let Some((index, rest)) =
                donder_runtime_types::mul_div(sample, last_source, last_sample)
            else {
                unreachable!("sample <= last_sample, so the index is at most last_source")
            };
            (index + u32::from(rest >= last_sample - last_sample / 2)) as usize
        })
        .collect()
}
