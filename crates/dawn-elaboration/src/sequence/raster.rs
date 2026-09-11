//! Canonical effect-raster evaluation for sequence clips.
//!
//! This module prepares Dawn effects and samples their evaluated colors. It does
//! not own UI request scheduling, raster caching, pixel transport, or Canvas
//! decoding; those belong to the desktop adapter and frontend respectively.

use crate::PreparedEffect;
use crate::RenderError;
use crate::sequence::color::black;
use crate::sequence::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
use crate::sequence::effects::sampling::{
    PreparedSampledEffectPixel, PreparedSampledEffectPixels, TargetColorAddress,
    evenly_sample_indices, prepare_sampled_effect_pixel_groups,
    render_sampled_effect_target_colors,
};
use crate::sequence::fixtures::{PreparedFixture, prepare_fixtures};
use crate::sequence::targets::PreparedTargetCache;
use crate::sequence::targets::PreparedTargetPixel;
use crate::sequence::timeline::{
    first_frame_at_or_after, frame_at_or_before, prepare_timing, sample_time_for_frame,
};
use dawn_language::dsl::{BytecodeProgram, DslBindCache, VmWorkspace};
use dawn_language::effect::{EffectDefinitionId, EffectInstId};
use dawn_language::layout::FixtureInstanceId;
use dawn_language::model::DawnProject;
use dawn_language::sequence::{Sequence, SequenceId};
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_sequence;
use dawn_language::values::{Color, SampleDuration, SampleTime};
use indexmap::{IndexMap, IndexSet};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterRenderer {
    environments: Box<[dawn_runtime::bindings::PreparedParameterEnvironment]>,
    frame_rate: u32,
    frame_count: u32,
    index_start_frame: u32,
    start_time: SampleTime,
    duration: SampleDuration,
    target: Arc<[PreparedTargetPixel]>,
    targets: Box<[Arc<[PreparedTargetPixel]>]>,
    effects: Box<[PreparedEffect]>,
    programs: Box<[Arc<BytecodeProgram>]>,
}

#[derive(Clone, Debug)]
pub struct PreparedEffectRasterSample {
    row_count: usize,
    effect_pixels: Box<[PreparedSampledEffectPixels]>,
}

#[derive(Debug)]
pub struct EffectRasterWorkspace {
    parameters: dawn_runtime::bindings::ParameterWorkspace,
    effect_vm: VmWorkspace,
    automation: Vec<Option<dawn_runtime::signal::EffectAutomationWorkspace>>,
}

pub struct EffectRasterPrepareBatch<'a> {
    project: &'a DawnProject,
    sequence: &'a Sequence,
    fixtures: Vec<PreparedFixture>,
    fixture_ids: IndexSet<FixtureInstanceId>,
    groups: IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
    frame_rate: u32,
    frame_count: u32,
    bind_cache: DslBindCache,
    sample_programs: IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    target_cache: PreparedTargetCache,
}

impl PreparedEffectRasterRenderer {
    pub fn workspace(&self) -> EffectRasterWorkspace {
        let mut effect_vm = VmWorkspace::default();
        for effect in &self.effects {
            if let Some(program) = effect.implementation.dsl_program() {
                effect_vm.reserve(&self.programs[program as usize]);
            }
        }
        EffectRasterWorkspace {
            parameters: dawn_runtime::bindings::ParameterWorkspace::new(
                &self.environments,
                usize::from(!self.environments.is_empty()),
            ),
            effect_vm,
            automation: self
                .effects
                .iter()
                .map(PreparedEffect::automation_workspace)
                .collect(),
        }
    }
    pub fn prepare(
        project: &DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
        effect_id: &EffectInstId,
    ) -> Result<Self, RenderError> {
        let mut batch = EffectRasterPrepareBatch::prepare(project, setup_id, sequence_id)?;
        batch.prepare_effect(effect_id)
    }

    pub fn start_seconds(&self) -> f32 {
        dawn_language::values::sample_time_seconds_f32(self.start_time)
    }

    pub fn duration_seconds(&self) -> f32 {
        dawn_language::values::sample_duration_seconds_f32(self.duration)
    }

    pub fn frame_rate(&self) -> u32 {
        self.frame_rate
    }

    pub fn target_pixel_count(&self) -> usize {
        self.target.len()
    }

    pub fn sampled_raster_column_seconds(
        &self,
        column_index: usize,
        column_count: usize,
    ) -> Result<f32, RenderError> {
        if column_count == 0 || column_index >= column_count {
            return Err(RenderError::InvalidTiming {
                reason: "raster column index must be within a non-empty raster".to_string(),
            });
        }
        let end_time = self
            .start_time
            .checked_add_duration(self.duration)
            .ok_or_else(|| RenderError::InvalidTiming {
                reason: "effect end exceeds the runtime clock range".to_string(),
            })?;
        let end_frame = first_frame_at_or_after(end_time, self.frame_rate);
        let active_frame_count = end_frame.saturating_sub(self.index_start_frame).max(1);
        let sample_offset = (((column_index as f32 + 0.5) * active_frame_count as f32
            / column_count as f32)
            .floor() as u32)
            .min(active_frame_count.saturating_sub(1));
        let sample_time =
            sample_time_for_frame(self.index_start_frame + sample_offset, self.frame_rate)?;
        Ok(dawn_language::values::sample_time_seconds_f32(sample_time))
    }

    pub fn prepare_sampled_raster(&self, row_count: usize) -> PreparedEffectRasterSample {
        let sample_indices = evenly_sample_indices(self.target.len(), row_count);
        let row_count = sample_indices.len();
        let mut sample_lookup = HashMap::<TargetColorAddress, Vec<usize>>::new();
        for (row_index, target_index) in sample_indices.into_iter().enumerate() {
            if let Some(pixel) = self.target.get(target_index) {
                sample_lookup
                    .entry(TargetColorAddress {
                        fixture_index: pixel.fixture_index(),
                        fixture_pixel_index: pixel.fixture_pixel_index(),
                    })
                    .or_default()
                    .push(row_index);
            }
        }
        let effect_pixels = self
            .effects
            .iter()
            .map(|effect| {
                let pixels = self.targets[effect.target as usize]
                    .iter()
                    .filter_map(|pixel| {
                        sample_lookup
                            .get(&TargetColorAddress {
                                fixture_index: pixel.fixture_index(),
                                fixture_pixel_index: pixel.fixture_pixel_index(),
                            })
                            .map(|rows| PreparedSampledEffectPixel {
                                pixel: pixel.clone(),
                                rows: rows.clone(),
                            })
                    })
                    .collect::<Vec<_>>();
                let groups = prepare_sampled_effect_pixel_groups(&pixels);
                PreparedSampledEffectPixels { pixels, groups }
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        PreparedEffectRasterSample {
            row_count,
            effect_pixels,
        }
    }

    pub fn render_sampled_raster_column_with_workspace(
        &self,
        sample: &PreparedEffectRasterSample,
        audio_seconds: f32,
        workspace: &mut EffectRasterWorkspace,
    ) -> Result<Vec<Color>, RenderError> {
        if !audio_seconds.is_finite() {
            return Err(RenderError::InvalidTiming {
                reason: "audio seconds must be finite".to_string(),
            });
        }
        let max_frame = self.frame_count.saturating_sub(1);
        let sample_time = dawn_language::values::sample_time_from_seconds_f32(audio_seconds)
            .map_err(|_| RenderError::InvalidTiming {
                reason: "audio seconds exceed the runtime clock range".to_string(),
            })?;
        let frame_index = frame_at_or_before(sample_time, self.frame_rate).min(max_frame);
        self.render_sampled_raster_column_at_with_workspace(sample, frame_index, workspace)
    }

    fn render_sampled_raster_column_at_with_workspace(
        &self,
        sample: &PreparedEffectRasterSample,
        frame_index: u32,
        workspace: &mut EffectRasterWorkspace,
    ) -> Result<Vec<Color>, RenderError> {
        let sample_time = sample_time_for_frame(frame_index, self.frame_rate)?;
        let mut rendered = vec![black(); sample.row_count];

        for (effect_index, effect) in self.effects.iter().enumerate() {
            if !effect.is_active(sample_time) {
                continue;
            }
            let Some(effect_pixels) = sample.effect_pixels.get(effect_index) else {
                continue;
            };
            let bound = match effect.implementation {
                crate::PreparedEffectImplementation::Bound { environment, .. } => Some(
                    workspace
                        .parameters
                        .resolve(&self.environments, environment, sample_time)?,
                ),
                _ => None,
            };
            render_sampled_effect_target_colors(
                &self.programs,
                effect,
                effect_pixels,
                &mut rendered,
                sample_time,
                &mut workspace.effect_vm,
                (workspace.automation[effect_index].as_mut(), bound),
            )?;
        }

        Ok(rendered)
    }
}

impl<'a> EffectRasterPrepareBatch<'a> {
    pub fn prepare(
        project: &'a DawnProject,
        setup_id: &SetupId,
        sequence_id: &SequenceId,
    ) -> Result<Self, RenderError> {
        let setup = project
            .setups
            .get(setup_id)
            .ok_or_else(|| RenderError::MissingSetup {
                setup_id: setup_id.clone(),
            })?;
        let layout = project
            .layouts
            .get(&setup.layout)
            .ok_or(RenderError::MissingLayout)?;
        let sequence =
            project
                .sequences
                .get(sequence_id)
                .ok_or_else(|| RenderError::MissingSequence {
                    sequence_id: sequence_id.clone(),
                })?;
        validate_sequence(project, sequence).map_err(|error| RenderError::BadGraph {
            message: error.message,
        })?;
        let timing = prepare_timing(sequence)?;

        let (fixtures, groups) = prepare_fixtures(project, layout)?;
        let fixture_ids = fixtures
            .iter()
            .map(|fixture| fixture.id)
            .collect::<IndexSet<_>>();
        let frame_rate = sequence.frame_rate;

        Ok(Self {
            project,
            sequence,
            fixtures,
            fixture_ids,
            groups,
            frame_rate,
            frame_count: timing.frame_count,
            bind_cache: DslBindCache::default(),
            sample_programs: IndexMap::new(),
            target_cache: PreparedTargetCache::default(),
        })
    }

    pub fn prepare_effect(
        &mut self,
        effect_id: &EffectInstId,
    ) -> Result<PreparedEffectRasterRenderer, RenderError> {
        let effect = self
            .sequence
            .effects
            .iter()
            .find(|effect| effect.id == *effect_id)
            .ok_or(RenderError::MissingEffectInstance {
                effect_id: effect_id.clone(),
            })?;
        let mut effects = Vec::new();
        let mut generated_child_count = 0usize;
        let mut environments = Vec::new();
        let target = prepare_effect_inst(
            PrepareEffectContext {
                environments: &mut environments,
                project: self.project,
                sequence: self.sequence,
                fixtures: &self.fixtures,
                fixture_ids: &self.fixture_ids,
                groups: &self.groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut self.bind_cache,
                sample_programs: &mut self.sample_programs,
                target_cache: &mut self.target_cache,
            },
            effect,
        )?;

        let start_time =
            dawn_language::values::sample_time_from_dawn_time(&effect.start).map_err(|_| {
                RenderError::InvalidTiming {
                    reason: "effect start exceeds the runtime clock range".to_string(),
                }
            })?;
        let duration = dawn_language::values::sample_duration_from_dawn_duration(&effect.duration)
            .map_err(|_| RenderError::InvalidTiming {
                reason: "effect duration exceeds the runtime clock range".to_string(),
            })?;
        let index_start_frame = first_frame_at_or_after(start_time, self.frame_rate);
        let mut environments = environments.into_boxed_slice();
        crate::sequence::effects::retained::compact_environments(&mut environments, &mut effects)?;
        dawn_runtime::bindings::PreparedParameterEnvironment::validate_all(&environments)?;
        Ok(PreparedEffectRasterRenderer {
            environments,
            frame_rate: self.frame_rate,
            frame_count: self.frame_count,
            index_start_frame,
            start_time,
            duration,
            target,
            effects: effects.into_boxed_slice(),
            programs: self.sample_programs.values().cloned().collect(),
            targets: self.target_cache.sample_targets.clone().into_boxed_slice(),
        })
    }
}
