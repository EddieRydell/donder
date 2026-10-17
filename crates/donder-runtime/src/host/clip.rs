//! Sparse sampling of one authored clip from an already prepared show.
//! Raster dimensions, scheduling, image storage, and caching belong to the host.
use alloc::{boxed::Box, collections::BTreeMap, vec, vec::Vec};
use donder_runtime_types::{Color, SampleDuration, SampleTime, SampleTimeError, image};

use crate::arena::{Carve, Size};
use crate::dsl::StripSlots;
use crate::evaluation::{PixelSource, RunPixel, effect_sampler};
use crate::resources::{LiveView, ParamsView};
use crate::show::Show;
use crate::workspace::{Timing, carve_effect_state, effect_state_size};

#[derive(Clone, Copy)]
pub struct SequenceClip<'a> {
    show: Show<'a>,
    effect: usize,
}

impl<'a> SequenceClip<'a> {
    pub(crate) fn new(show: Show<'a>, effect: usize) -> Self {
        Self { show, effect }
    }

    fn record(&self) -> &'a image::Effect {
        &self.show.s.effects[self.effect]
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
        let rate = u64::from(self.frame_rate());
        let ticks = u64::from(self.start_time().as_ticks());
        let end = ticks + u64::from(self.duration().as_ticks());
        let micros = u64::from(donder_runtime_types::MICROS_PER_SECOND);
        let start_frame = (ticks * rate).div_ceil(micros);
        let end_frame = (end * rate).div_ceil(micros);
        let active_frames = end_frame.saturating_sub(start_frame).max(1);
        // floor((column + 0.5) * active_frames / columns), exactly.
        let offset =
            ((2 * column as u64 + 1) * active_frames / (2 * columns as u64)).min(active_frames - 1);
        let frame = (start_frame + offset).min(u64::from(self.frame_count().saturating_sub(1)));
        donder_runtime_types::sample_time_from_frame(frame as u32, self.frame_rate())
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
    pub fn sampler(&self, rows: usize) -> ClipSampler<'a> {
        let show = self.show;
        let effect = self.record();
        let program = show.program(effect.program);
        let target = crate::targets::TargetView::of(show, &show.s.targets[effect.target as usize]);
        let spatial = program.flag(image::PROGRAM_USES_SPATIAL);
        let sections = program.flag(image::PROGRAM_USES_SECTIONS);
        let indices = evenly_sample_indices(target.len(), rows);
        let row_count = indices.len();
        let mut groups = Vec::<SampleGroup>::new();
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
                let group: &mut SampleGroup = &mut groups[group];
                group.rows.push(row);
            } else {
                group_by_context.insert(key, groups.len());
                pixels.push(RunPixel {
                    target_index: local,
                    pixel,
                });
                groups.push(SampleGroup { rows: vec![row] });
            }
        }
        let mut slots = StripSlots::default();
        slots.include(program);
        let mut size = Size::default();
        slots.size(&mut size);
        effect_state_size(show, self.effect, &mut size);
        ClipSampler {
            clip: *self,
            output: vec![Color::BLACK; row_count],
            groups,
            pixels,
            slots,
            buffer: vec![0; size.bytes.div_ceil(8)].into_boxed_slice(),
            fresh: true,
        }
    }
}

/// Reusable sparse evaluator borrowing an admitted show.
pub struct ClipSampler<'a> {
    clip: SequenceClip<'a>,
    output: Vec<Color>,
    groups: Vec<SampleGroup>,
    /// A pixel of each group.
    pixels: Vec<RunPixel>,
    slots: StripSlots,
    /// The strip workspace and the effect's automation state.
    buffer: Box<[u64]>,
    /// Whether the buffer has not been carved yet.
    fresh: bool,
}

/// Rows sharing one sampling context.
struct SampleGroup {
    rows: Vec<usize>,
}

impl ClipSampler<'_> {
    pub fn evaluate(&mut self, time: SampleTime) -> &[Color] {
        let show = self.clip.show;
        let effect = self.clip.record();
        let timing = Timing {
            start: effect.start,
            duration: effect.duration,
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
        let (Some(mut workspace), Some(mut states)) = (
            self.slots.carve(&mut arena),
            carve_effect_state(&mut arena, show, self.clip.effect),
        ) else {
            unreachable!("the buffer holds the sampler's size")
        };
        let params = if states.records.is_empty() {
            ParamsView::image(show, effect.params)
        } else {
            states.get(0).params_at(show, time, LiveView::default())
        };
        let sampler = effect_sampler(show, effect, timing, time, params);
        let (groups, output) = (&self.groups, &mut self.output);
        sampler.sample_pixels(
            groups.len(),
            PixelSource::Listed(&self.pixels),
            &mut workspace,
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
    let last_source = (source_count - 1) as u128;
    let last_sample = (sample_count - 1) as u128;
    (0..sample_count)
        .map(|sample| ((sample as u128 * last_source + last_sample / 2) / last_sample) as usize)
        .collect()
}
