//! Sparse evaluation of one authored clip from an already prepared show.
//! Raster dimensions, scheduling, show storage, and caching belong to the host.
use crate::format;
use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use donder_runtime_types::{Color, MICROS_PER_SECOND, SampleDuration, SampleTime, mul_div};

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

    /// The show frames inside the authored clip, one raster column each.
    pub fn raster_frames(&self) -> core::ops::Range<u32> {
        let rate = self.frame_rate();
        // The first frame starting at or after `ticks`.
        let frame_from = |ticks: u32| {
            let Some((frame, rest)) = mul_div(ticks, rate, MICROS_PER_SECOND) else {
                unreachable!("frame rates are below a million")
            };
            frame + u32::from(rest != 0)
        };
        let ticks = self.start_time().as_ticks();
        let start_frame = frame_from(ticks);
        let end_frame = frame_from(ticks.saturating_add(self.duration().as_ticks()));
        let last_frame = self.frame_count().saturating_sub(1);
        let start_frame = start_frame.min(last_frame);
        start_frame..end_frame.min(last_frame + 1).max(start_frame + 1)
    }

    /// The start of one of the show's frames.
    pub fn frame_time(&self, frame: u32) -> SampleTime {
        self.show.frame_time(frame)
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

    fn frame_count(&self) -> u32 {
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
        let structure = program.flag(format::PROGRAM_USES_STRUCTURE);
        let indices = evenly_sample_indices(target.len(), rows);
        let row_count = indices.len();
        let mut groups = Vec::<RasterGroup>::new();
        let mut pixels = Vec::<RunPixel>::new();
        let mut group_by_context = BTreeMap::new();
        for (row, local) in indices.into_iter().enumerate() {
            let pixel = target.pixel(local);
            let (position, extent) = if spatial {
                target.spatial_context(local, &pixel, show)
            } else {
                ([0.0; 3], crate::dsl::Extent::default())
            };
            let place = structure.then(|| {
                let place = target.structure(local);
                (place.member, place.strand, place.along)
            });
            let section = target.sections.pixel(local);
            let key = (
                sections.then_some(section),
                place,
                pixel.pixel_index,
                pixel.pixel_count,
                position.map(f32::to_bits),
                extent.min.map(f32::to_bits),
                extent.max.map(f32::to_bits),
                (extent.members, extent.strands, extent.strand_length),
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
        let mut workspace = self.slots.carve(&mut arena);
        let mut scratch = carve_scratch(&mut arena, self.scratch);
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
        let sampler = clip_evaluator(show, clip, timing, time, params);
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
