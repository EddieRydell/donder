//! Sparse sampling of one authored clip from an already prepared sequence.
//! Raster dimensions, scheduling, image storage, and caching belong to the host.
use crate::bindings::ParameterWorkspace;
use crate::dsl::{SpatialContext, VmWorkspace};
use crate::signal::{
    EffectAutomationWorkspace, EvaluationError, PreparedClip, PreparedEffectImplementation,
    PreparedSignalGraph,
};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::{collections::BTreeMap, vec, vec::Vec};

#[derive(Clone, Copy)]
pub struct SequenceClip<'a> {
    pub(crate) graph: &'a PreparedSignalGraph,
    pub(crate) clip: &'a PreparedClip,
}

impl<'a> SequenceClip<'a> {
    pub fn start_time(&self) -> SampleTime {
        self.clip.start_time
    }
    pub fn duration(&self) -> SampleDuration {
        self.clip.duration
    }
    pub fn frame_rate(&self) -> u32 {
        self.graph.frame_rate
    }
    pub fn frame_count(&self) -> u32 {
        self.graph.frame_count
    }
    pub fn target_pixel_count(&self) -> usize {
        self.graph.target(self.clip.target).len()
    }

    /// Select evenly spaced target cells once, retaining each effect's original
    /// local indices and spatial scope. Equal contexts share a VM sample.
    pub fn sampler(&self, rows: usize) -> ClipSampler<'a> {
        let target = self.graph.target(self.clip.target);
        let indices = evenly_sample_indices(target.len(), rows);
        let row_count = indices.len();
        let mut rows_by_address = BTreeMap::<(usize, u32), Vec<usize>>::new();
        for (row, index) in indices.into_iter().enumerate() {
            let pixel = &target[index];
            rows_by_address
                .entry((pixel.fixture_index, pixel.fixture_pixel_index))
                .or_default()
                .push(row);
        }
        let mut vm = VmWorkspace::default();
        let effects = self
            .clip
            .effects
            .iter()
            .map(|&index| {
                let effect = &self.graph.effects[index];
                vm.reserve(&self.graph.programs[effect.implementation.dsl_program()]);
                let mut groups = Vec::<SampleGroup>::new();
                let mut group_by_context = BTreeMap::new();
                let target_start = self.graph.targets[effect.target].pixels.start;
                for (local, pixel) in self.graph.target(effect.target).iter().enumerate() {
                    let Some(rows) =
                        rows_by_address.get(&(pixel.fixture_index, pixel.fixture_pixel_index))
                    else {
                        continue;
                    };
                    let spatial = (!self.graph.spatial_contexts.is_empty())
                        .then(|| self.graph.spatial_contexts[target_start + local]);
                    let context = SampleContext {
                        index: pixel.pixel_index,
                        count: pixel.pixel_count,
                        fraction: pixel.pixel_fraction,
                        spatial,
                    };
                    let key = (
                        context.index,
                        context.count,
                        context.fraction.to_bits(),
                        spatial.map(|context| {
                            [
                                context.position[0],
                                context.position[1],
                                context.min[0],
                                context.min[1],
                                context.max[0],
                                context.max[1],
                            ]
                            .map(f32::to_bits)
                        }),
                    );
                    if let Some(&group) = group_by_context.get(&key) {
                        let group: &mut SampleGroup = &mut groups[group];
                        group.rows.extend(rows);
                    } else {
                        group_by_context.insert(key, groups.len());
                        groups.push(SampleGroup {
                            context,
                            rows: rows.clone(),
                        });
                    }
                }
                EffectSamples {
                    index,
                    groups,
                    automation: effect.automation_workspace(),
                }
            })
            .collect();
        ClipSampler {
            graph: self.graph,
            row_count,
            effects,
            vm,
            parameters: ParameterWorkspace::new(
                &self.graph.parameter_environments,
                usize::from(!self.graph.parameter_environments.is_empty()),
            ),
        }
    }
}

/// Reusable sparse clip evaluator. Its borrowed graph cannot change underneath
/// the prepared sample groups or parameter/automation storage.
pub struct ClipSampler<'a> {
    graph: &'a PreparedSignalGraph,
    row_count: usize,
    effects: Vec<EffectSamples>,
    parameters: ParameterWorkspace,
    vm: VmWorkspace,
}

struct EffectSamples {
    index: usize,
    groups: Vec<SampleGroup>,
    automation: Option<EffectAutomationWorkspace>,
}
struct SampleGroup {
    context: SampleContext,
    rows: Vec<usize>,
}
#[derive(Clone, Copy)]
struct SampleContext {
    index: usize,
    count: usize,
    fraction: f32,
    spatial: Option<SpatialContext>,
}

impl ClipSampler<'_> {
    pub fn evaluate(
        &mut self,
        time: SampleTime,
        output: &mut [Color],
    ) -> Result<(), EvaluationError> {
        if output.len() != self.row_count {
            return Err(EvaluationError::InvalidWorkspace);
        }
        output.fill(Color::BLACK);
        for sampled in &mut self.effects {
            let effect = &self.graph.effects[sampled.index];
            if !effect.is_active(time) {
                continue;
            }
            let bound = match effect.implementation {
                PreparedEffectImplementation::Bound { environment, .. } => {
                    Some(self.parameters.resolve(
                        &self.graph.parameter_environments,
                        environment,
                        time,
                    )?)
                }
                _ => None,
            };
            effect.with_sampler(
                &self.graph.programs,
                time,
                sampled.automation.as_mut(),
                bound,
                |sampler| {
                    for group in &sampled.groups {
                        let context = group.context;
                        let color = sampler.sample_spatial_context(
                            context.index,
                            context.count,
                            context.fraction,
                            context.spatial.as_ref(),
                            &mut self.vm,
                        )?;
                        for &row in &group.rows {
                            let target = &mut output[row];
                            target.red = target.red.max(color.red);
                            target.green = target.green.max(color.green);
                            target.blue = target.blue.max(color.blue);
                        }
                    }
                    Ok(())
                },
            )?;
        }
        Ok(())
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
