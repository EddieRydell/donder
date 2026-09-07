use dawn_language::dsl::DslBindCache;
use dawn_language::model::DawnProject;
use dawn_language::sequence::{Sequence, SequenceId};
use dawn_language::setup::SetupId;
use dawn_language::validation::validate_sequence;
use indexmap::{IndexMap, IndexSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::composition::{PrepareGraphContext, prepare_signal_graph};
use super::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
use super::elements::prepare_elements;
use super::renderer::RenderError;
use super::targets::PreparedTargetCache;
use super::timeline::prepare_timing;
use crate::{PreparedLayer, PreparedSignalGraph};

static NEXT_SEQUENCE_ID: AtomicU32 = AtomicU32::new(1);

pub fn elaborate_sequence(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence_id: &SequenceId,
) -> Result<PreparedSignalGraph, RenderError> {
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
    prepare_validated_sequence(project, setup_id, sequence)
}

pub(crate) fn prepare_validated_sequence(
    project: &DawnProject,
    setup_id: &SetupId,
    sequence: &Sequence,
) -> Result<PreparedSignalGraph, RenderError> {
    let setup = project
        .setups
        .get(setup_id)
        .ok_or_else(|| RenderError::MissingSetup {
            setup_id: setup_id.clone(),
        })?;
    let tree = project
        .element_trees
        .get(&setup.elements)
        .ok_or(RenderError::MissingElementTree)?;
    let timing = prepare_timing(sequence)?;

    let (elements, groups) = prepare_elements(project, tree)?;
    let mut pixel_count = 0;
    let element_cell_offsets = elements
        .iter()
        .map(|element| {
            let offset = pixel_count;
            pixel_count += element.pixel_count;
            offset
        })
        .collect::<Vec<_>>();
    let element_ids = elements
        .iter()
        .map(|element| element.id)
        .collect::<IndexSet<_>>();
    let mut effects = Vec::with_capacity(sequence.effects.len());
    let mut generated_child_count = 0usize;
    let mut bind_cache = DslBindCache::default();
    let mut sample_programs = IndexMap::new();
    let mut environments = Vec::new();
    let mut target_cache = PreparedTargetCache::default();
    let layers = sequence
        .layers
        .iter()
        .map(|layer| PreparedLayer {
            enabled: layer.enabled,
        })
        .collect::<Vec<_>>();
    let mut effects_by_layer = vec![Vec::new(); layers.len()];
    for effect in &sequence.effects {
        let layer_index = sequence
            .layers
            .iter()
            .position(|layer| layer.id == effect.layer_id)
            .ok_or_else(|| RenderError::BadGraph {
                message: format!(
                    "effect {} references missing layer {}",
                    effect.id.0, effect.layer_id.0
                ),
            })?;
        let first_prepared_effect = effects.len();
        prepare_effect_inst(
            PrepareEffectContext {
                environments: &mut environments,
                project,
                sequence,
                elements: &elements,
                element_ids: &element_ids,
                groups: &groups,
                effects: &mut effects,
                generated_child_count: &mut generated_child_count,
                bind_cache: &mut bind_cache,
                sample_programs: &mut sample_programs,
                target_cache: &mut target_cache,
            },
            effect,
        )?;
        effects_by_layer[layer_index].extend(first_prepared_effect..effects.len());
    }
    for layer_effects in &mut effects_by_layer {
        layer_effects.sort_unstable_by(|left, right| {
            effects[*left]
                .start_time
                .cmp(&effects[*right].start_time)
                .then(left.cmp(right))
        });
    }
    for (slot, automation) in effects
        .iter_mut()
        .filter_map(|effect| effect.automation.as_mut())
        .enumerate()
    {
        automation.workspace_slot =
            u32::try_from(slot).map_err(|_| RenderError::GeneratorPrepare {
                message: "too many automated effects".to_string(),
            })?;
    }
    let frame_rate = sequence.frame_rate;
    let mut programs = sample_programs
        .into_values()
        .map(Arc::unwrap_or_clone)
        .collect::<Vec<_>>();
    let plan = prepare_signal_graph(
        PrepareGraphContext {
            project,
            sequence,
            elements: &elements,
            programs: &mut programs,
            targets: &mut target_cache,
        },
        &sequence.composition_graph,
    )?;
    let mut target_pixels = Vec::new();
    let targets = target_cache
        .sample_targets
        .into_iter()
        .map(|pixels| {
            let start = u32::try_from(target_pixels.len()).map_err(|_| RenderError::BadTarget)?;
            let len = u32::try_from(pixels.len()).map_err(|_| RenderError::BadTarget)?;
            let end = start.checked_add(len).ok_or(RenderError::BadTarget)?;
            target_pixels.extend_from_slice(&pixels);
            let count = pixels
                .iter()
                .map(|pixel| pixel.pixel_count)
                .max()
                .unwrap_or(0);
            Ok(dawn_runtime::signal::PreparedTarget {
                pixels: start..end,
                sample_count: if len > count { count } else { 0 },
            })
        })
        .collect::<Result<Box<[_]>, RenderError>>()?;
    let mut environments = environments.into_boxed_slice();
    super::effects::retained::compact_environments(&mut environments, &mut effects)?;
    dawn_runtime::bindings::PreparedParameterEnvironment::validate_all(&environments)?;
    Ok(PreparedSignalGraph {
        parameter_environments: environments,
        workspace_key: NEXT_SEQUENCE_ID.fetch_add(1, Ordering::Relaxed),
        frame_rate,
        frame_count: timing.frame_count,
        duration: timing.duration,
        elements: elements
            .iter()
            .map(|element| dawn_runtime::signal::PreparedElement {
                id: element.id.0,
                pixel_count: element.pixel_count,
            })
            .collect(),
        element_cell_offsets: element_cell_offsets.into_boxed_slice(),
        pixel_count,
        effects: effects.into_boxed_slice(),
        programs: programs.into_boxed_slice(),
        targets,
        target_pixels: target_pixels.into_boxed_slice(),
        effects_by_layer: effects_by_layer
            .into_iter()
            .map(Vec::into_boxed_slice)
            .collect(),
        layers: layers.into_boxed_slice(),
        plan,
    })
}
