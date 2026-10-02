//! Sparse sampling of one authored clip from an already prepared sequence.
//! Raster dimensions, scheduling, image storage, and caching belong to the host.
use crate::dsl::{AutomationPlan, SpatialContext, VmWorkspace};
use crate::signal::{EffectAutomationWorkspace, PreparedClip, PreparedEffect, SignalGraph};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::{collections::BTreeMap, vec, vec::Vec};

#[derive(Clone, Copy)]
pub struct SequenceClip<'a> {
    pub(crate) graph: SignalGraph<'a>,
    pub(crate) clip: &'a PreparedClip,
}

impl<'a> SequenceClip<'a> {
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
        let mut vm = VmWorkspace::default();
        vm.reserve(program.bytecode());
        let mut groups = Vec::<SampleGroup>::new();
        let mut group_by_context = BTreeMap::new();
        let target_start = self.graph.targets[effect.target].pixels.start;
        for (row, local) in indices.into_iter().enumerate() {
            let pixel = &target[local];
            let spatial = *self
                .graph
                .spatial_context(program.uses_spatial_context(), target_start + local);
            let context = SampleContext {
                index: pixel.pixel_index,
                count: pixel.pixel_count,
                fraction: pixel.pixel_fraction,
                spatial,
                section: self.graph.targets[effect.target].sections.pixel(local),
            };
            let key = (
                program.uses_sections().then_some(context.section),
                context.index,
                context.count,
                context.fraction.to_bits(),
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
                    context,
                    rows: vec![row],
                });
            }
        }
        ClipSampler {
            clip: *self,
            output: vec![Color::BLACK; row_count],
            groups,
            automation: effect.automation_workspace(),
            vm,
        }
    }
}

/// Reusable sparse evaluator borrowing immutable, admitted playback data.
pub struct ClipSampler<'a> {
    clip: SequenceClip<'a>,
    output: Vec<Color>,
    groups: Vec<SampleGroup>,
    automation: Option<EffectAutomationWorkspace>,
    vm: VmWorkspace,
}

struct SampleGroup {
    context: SampleContext,
    rows: Vec<usize>,
}

#[derive(Clone, Copy)]
struct SampleContext {
    index: usize,
    count: usize,
    section: crate::sections::SectionPixel,
    fraction: f32,
    spatial: SpatialContext,
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
        effect.with_sampler(self.clip.graph, time, params, |sampler| {
            for group in &self.groups {
                let context = group.context;
                let color = sampler.sample_spatial_context(
                    context.index,
                    context.count,
                    context.fraction,
                    context.section,
                    &context.spatial,
                    &mut self.vm,
                );
                for &row in &group.rows {
                    self.output[row] = color;
                }
            }
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
