use donder_language::dsl::DslBindCache;
use donder_language::layout::Layout;
use donder_language::model::DonderProject;
use donder_language::sequence::Sequence;
use donder_runtime::signal::{PreparedLayer, PreparedSignalGraph};
use indexmap::IndexMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use super::composition::{PrepareGraphContext, prepare_signal_graph};
use super::effects::preparation::{PrepareEffectContext, prepare_effect_inst};
use super::fixtures::prepare_fixtures;
use super::targets::PreparedTargetCache;
use super::timeline::prepare_timing;

static NEXT_SEQUENCE_ID: AtomicU32 = AtomicU32::new(1);

pub(crate) fn prepare_sequence(
    project: &DonderProject,
    layout: &Layout,
    sequence: &Sequence,
) -> PreparedSignalGraph {
    let timing = prepare_timing(sequence);

    let (fixtures, groups): super::fixtures::PreparedFixtures = prepare_fixtures(project, layout);
    let mut pixel_count = 0;
    let fixture_pixel_offsets = fixtures
        .iter()
        .map(|fixture| {
            let offset = pixel_count;
            pixel_count += fixture.pixel_count;
            offset
        })
        .collect::<Vec<_>>();
    let mut effects = Vec::with_capacity(sequence.effects.len());
    let mut clips = Vec::with_capacity(sequence.effects.len());
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
    let layer_indices = sequence
        .layers
        .iter()
        .enumerate()
        .map(|(index, layer)| (&layer.id, index))
        .collect::<IndexMap<_, _>>();
    for effect in &sequence.effects {
        let layer_index = layer_indices[&effect.layer_id];
        let first_prepared_effect = effects.len();
        let target: Arc<[super::targets::PreparedTargetPixel]> = prepare_effect_inst(
            PrepareEffectContext {
                environments: &mut environments,
                project,
                sequence,
                fixtures: &fixtures,
                groups: &groups,
                effects: &mut effects,
                bind_cache: &mut bind_cache,
                sample_programs: &mut sample_programs,
                target_cache: &mut target_cache,
            },
            effect,
        );
        effects_by_layer[layer_index].extend(first_prepared_effect..effects.len());
        clips.push(donder_runtime::signal::PreparedClip {
            id: effect.id.0,
            start_time: donder_language::values::SampleTime::from_ticks(
                effect.start.as_micros_rounded() as u32,
            ),
            duration: donder_language::values::SampleDuration::from_ticks(
                effect.duration.as_micros_rounded() as u32,
            ),
            target: target_cache.sample_target(target),
            effects: (first_prepared_effect..effects.len()).collect(),
        });
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
        automation.workspace_slot = slot;
    }
    let frame_rate = sequence.frame_rate;
    let mut programs = sample_programs
        .into_values()
        .map(Arc::unwrap_or_clone)
        .collect::<Vec<_>>();
    let plan: donder_runtime::signal::SignalPlan = prepare_signal_graph(
        PrepareGraphContext {
            project,
            sequence,
            fixtures: &fixtures,
            programs: &mut programs,
            targets: &mut target_cache,
        },
        &sequence.composition_graph,
    );
    let mut target_pixels = Vec::new();
    let mut spatial_contexts = Vec::new();
    let needs_spatial = programs
        .iter()
        .any(|program| program.uses_spatial_context());
    let targets = target_cache
        .sample_targets
        .into_iter()
        .map(|pixels| {
            let start = target_pixels.len();
            let len = pixels.len();
            target_pixels.extend_from_slice(&pixels);
            let end = target_pixels.len();
            if needs_spatial {
                spatial_contexts.extend(super::targets::spatial_contexts(&pixels, &fixtures));
            }
            let count = pixels
                .iter()
                .map(|pixel| pixel.pixel_count as usize)
                .max()
                .unwrap_or(0);
            donder_runtime::signal::PreparedTarget {
                pixels: start..end,
                sample_count: if len > count { count } else { 0 },
            }
        })
        .collect();
    let mut environments = environments.into_boxed_slice();
    super::effects::retained::compact_environments(&mut environments, &mut effects);
    PreparedSignalGraph {
        parameter_environments: environments,
        workspace_key: NEXT_SEQUENCE_ID.fetch_add(1, Ordering::Relaxed),
        frame_rate,
        frame_count: timing.frame_count,
        duration: timing.duration,
        fixtures: fixtures
            .iter()
            .map(|fixture| donder_runtime::signal::PreparedFixture {
                id: fixture.id.0,
                pixel_count: fixture.pixel_count,
            })
            .collect(),
        fixture_pixel_offsets: fixture_pixel_offsets.into_boxed_slice(),
        pixel_count,
        effects: effects.into_boxed_slice(),
        clips: clips.into_boxed_slice(),
        programs: programs.into_boxed_slice(),
        targets,
        target_pixels: target_pixels.into_boxed_slice(),
        spatial_contexts: spatial_contexts.into_boxed_slice(),
        effects_by_layer: effects_by_layer
            .into_iter()
            .map(Vec::into_boxed_slice)
            .collect(),
        layers: layers.into_boxed_slice(),
        plan,
    }
}
