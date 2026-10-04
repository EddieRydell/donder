//! Sparse sampling of one authored clip from an already prepared sequence.
//! Raster dimensions, scheduling, image storage, and caching belong to the host.
use crate::dsl::{AutomationPlan, BatchWorkspace};
use crate::evaluation::RunPixel;
use crate::signal::{EffectAutomationWorkspace, PreparedClip, PreparedEffect, SignalGraph};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::{collections::BTreeMap, vec, vec::Vec};

#[derive(Clone, Copy)]
pub struct SequenceClip<'a> {
    pub(crate) graph: SignalGraph<'a>,
    pub(crate) clip: &'a PreparedClip,
}

impl<'a> SequenceClip<'a> {
    /// Sample the center frame of a raster column inside the authored clip.
    pub fn raster_column_time(
        &self,
        column: usize,
        columns: usize,
    ) -> Result<SampleTime, crate::values::SampleTimeError> {
        if columns == 0 || column >= columns {
            return Err(crate::values::SampleTimeError::OutOfRange);
        }
        let rate = u64::from(self.frame_rate());
        let ticks = u64::from(self.start_time().as_ticks());
        let end = ticks + u64::from(self.duration().as_ticks());
        let micros = u64::from(crate::values::MICROS_PER_SECOND);
        let start_frame = (ticks * rate).div_ceil(micros);
        let end_frame = (end * rate).div_ceil(micros);
        let active_frames = end_frame.saturating_sub(start_frame).max(1);
        let offset = (((column as f32 + 0.5) * active_frames as f32 / columns as f32).floor()
            as u64)
            .min(active_frames - 1);
        let frame = (start_frame + offset).min(u64::from(self.frame_count().saturating_sub(1)));
        crate::values::sample_time_from_frame(frame as u32, self.frame_rate())
    }
    fn effect(&self) -> &'a PreparedEffect<AutomationPlan> {
        &self.graph.data.effects[self.clip.effect]
    }

    pub fn start_time(&self) -> SampleTime {
        self.effect().start_time
    }
    pub fn duration(&self) -> SampleDuration {
        self.effect().duration
    }
    pub fn frame_rate(&self) -> u32 {
        self.graph.frame_rate
    }
    pub fn frame_count(&self) -> u32 {
        self.graph.frame_count
    }
    pub fn target_pixel_count(&self) -> usize {
        self.graph.target(self.effect().target).len()
    }

    /// Select evenly spaced cells while preserving their original sampling
    /// coordinates. Equal contexts share one VM sample.
    pub fn sampler(&self, rows: usize) -> ClipSampler<'a> {
        let effect = self.effect();
        let target = self.graph.target(effect.target);
        let indices = evenly_sample_indices(target.len(), rows);
        let row_count = indices.len();
        let program = self.graph.sample_program(effect.program);
        let mut workspace = BatchWorkspace::default();
        workspace.reserve(program.bytecode(), program.batch());
        let mut groups = Vec::<SampleGroup>::new();
        let mut group_by_context = BTreeMap::new();
        for (row, local) in indices.into_iter().enumerate() {
            let pixel = target.pixel(local);
            let spatial = *self.graph.spatial_context(
                program.uses_spatial_context(),
                effect.target,
                local,
                &pixel,
            );
            let section = self.graph.targets[effect.target].sections.pixel(local);
            let key = (
                program.uses_sections().then_some(section),
                pixel.pixel_index,
                pixel.pixel_count,
                pixel.pixel_fraction.to_bits(),
                [
                    spatial.position[0],
                    spatial.position[1],
                    spatial.min[0],
                    spatial.min[1],
                    spatial.max[0],
                    spatial.max[1],
                ]
                .map(f32::to_bits),
            );
            if let Some(&group) = group_by_context.get(&key) {
                let group: &mut SampleGroup = &mut groups[group];
                group.rows.push(row);
            } else {
                group_by_context.insert(key, groups.len());
                groups.push(SampleGroup {
                    pixel: RunPixel {
                        target_index: local,
                        pixel,
                    },
                    rows: vec![row],
                });
            }
        }
        ClipSampler {
            clip: *self,
            output: vec![Color::BLACK; row_count],
            groups,
            automation: effect.automation_workspace(),
            workspace,
        }
    }
}

/// Reusable sparse evaluator borrowing immutable, admitted playback data.
pub struct ClipSampler<'a> {
    clip: SequenceClip<'a>,
    output: Vec<Color>,
    groups: Vec<SampleGroup>,
    automation: Option<EffectAutomationWorkspace>,
    workspace: BatchWorkspace,
}

/// Rows sharing one sampling context, and a pixel with that context.
struct SampleGroup {
    pixel: RunPixel,
    rows: Vec<usize>,
}

impl ClipSampler<'_> {
    pub fn evaluate(&mut self, time: SampleTime) -> &[Color] {
        let effect = self.clip.effect();
        if !effect.is_active(time) {
            self.output.fill(Color::BLACK);
            return &self.output;
        }
        let params = match &mut self.automation {
            Some(automation) => automation.params_at(time),
            None => &effect.bound_params,
        };
        let (groups, output) = (&self.groups, &mut self.output);
        effect.with_sampler(self.clip.graph, time, params, |sampler| {
            sampler.sample_pixels(
                self.clip.graph,
                effect.target,
                groups.len(),
                |group| groups[group].pixel,
                &mut self.workspace,
                |group, color| {
                    for &row in &groups[group].rows {
                        output[row] = color;
                    }
                },
            );
        });
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
