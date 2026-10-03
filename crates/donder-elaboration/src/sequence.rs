//! Resolve selected sampling domains before assembling owner-branded playback storage.
mod composition;
mod programs;
mod retention;
mod routing;

use crate::selection::Selection;
use donder_language::effect::EffectScope;
use donder_language::execution::TargetScope;
use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind};
use donder_language::operator::composition_graph_output_dependencies;
use donder_language::sequence::CompositionGraphNodeKind;
use donder_runtime::PreparedSequence;
use indexmap::IndexMap;

pub(crate) fn prepare(selected: Selection<'_>, compact: bool) -> PreparedSequence {
    let geometry = selected
        .geometry
        .iter()
        .map(|(_, geometry)| geometry.clone())
        .collect::<Vec<_>>();
    let indexes = selected
        .geometry
        .iter()
        .enumerate()
        .map(|(index, (id, _))| (*id, index))
        .collect::<IndexMap<_, _>>();
    let mut targets = IndexMap::new();
    collect_targets(&selected.layout.fixtures, &indexes, &mut targets);
    let routes = routing::targets(&selected, &geometry, &targets);
    let sequence = selected.sequence.sequence();
    let dependencies = composition_graph_output_dependencies(&sequence.composition_graph);
    let mut programs = programs::Programs::default();
    let cells = retention::cells(&selected, &geometry, &routes, &dependencies, compact);
    let has_pixels = cells.iter().any(|cells| !cells.is_empty());
    let required_layers = sequence
        .composition_graph
        .nodes
        .iter()
        .filter_map(|node| {
            if dependencies.contains(&node.id)
                && let CompositionGraphNodeKind::Layer { layer_id } = &node.kind
            {
                Some(layer_id)
            } else {
                None
            }
        })
        .collect::<std::collections::HashSet<_>>();
    PreparedSequence::build(selected.sequence.timing().clone(), |builder| {
        let fixtures = selected
            .geometry
            .iter()
            .enumerate()
            .map(|(index, (id, geometry))| {
                let geometry = if compact {
                    geometry.select(|cell| cells[index].binary_search(&cell).is_ok())
                } else {
                    geometry.clone()
                };
                (index, builder.fixture(id.0, geometry))
            })
            .collect::<IndexMap<_, _>>();
        let mut layer_effects = sequence
            .layers
            .iter()
            .map(|layer| (&layer.id, Vec::new()))
            .collect::<IndexMap<_, _>>();
        let enabled_layers = sequence
            .layers
            .iter()
            .filter(|layer| layer.enabled)
            .map(|layer| &layer.id)
            .collect::<std::collections::HashSet<_>>();
        let windows = builder.windows().collect::<Vec<_>>();
        for ((accepted, window), timing) in selected
            .sequence
            .effects()
            .zip(windows)
            .zip(selected.sequence.timing().windows())
        {
            let effect = accepted.instance();
            if compact
                && (!has_pixels
                    || !enabled_layers.contains(&effect.layer_id)
                    || !required_layers.contains(&effect.layer_id))
            {
                continue;
            }
            let scope = match effect.scope {
                EffectScope::PerFixture => TargetScope::PerFixture,
                EffectScope::WholeTarget => TargetScope::WholeTarget,
            };
            let members = &targets[&effect.target.fixture];
            if compact && members.iter().all(|&index| cells[index].is_empty()) {
                continue;
            }
            let target = builder.target(members.iter().map(|index| fixtures[index]), scope);
            let mut counts = members
                .iter()
                .map(|&index| geometry[index].positions().len() as i32);
            let pixel_count = match scope {
                TargetScope::WholeTarget => Some(counts.sum()),
                TargetScope::PerFixture => counts
                    .next()
                    .filter(|first| counts.all(|count| count == *first)),
            };
            let invocation = programs.sample(
                accepted.execution(),
                donder_language::dsl::ProgramConstants {
                    pixel_count,
                    duration_seconds: Some(donder_language::values::sample_duration_seconds_f32(
                        donder_language::values::SampleDuration::from_ticks(timing.duration.get()),
                    )),
                },
            );
            let prepared = builder.sample(&invocation, window, target);
            builder.clip(effect.id.0, prepared);
            layer_effects[&effect.layer_id].push(prepared);
        }
        let layers = sequence
            .layers
            .iter()
            .filter(|layer| !compact || has_pixels && required_layers.contains(&layer.id))
            .map(|layer| {
                (
                    &layer.id,
                    builder.layer(layer.enabled, layer_effects[&layer.id].iter().copied()),
                )
            })
            .collect::<IndexMap<_, _>>();
        let root = if compact && !has_pixels {
            builder.output([])
        } else {
            composition::prepare(builder, selected.sequence, &layers, &mut programs)
        };
        routing::prepare(builder, &selected, &geometry, &targets, &cells, &fixtures);
        root
    })
}

/// Every group denotes its ordered leaf fixtures, including empty groups.
fn collect_targets(
    nodes: &[LayoutFixture],
    fixtures: &IndexMap<FixtureInstanceId, usize>,
    targets: &mut IndexMap<FixtureInstanceId, Vec<usize>>,
) -> Vec<usize> {
    let mut members = Vec::new();
    for node in nodes {
        let target = match &node.kind {
            LayoutFixtureKind::Fixture { .. } => vec![fixtures[&node.id]],
            LayoutFixtureKind::Group { children } => collect_targets(children, fixtures, targets),
        };
        members.extend(target.iter().copied());
        targets.insert(node.id, target);
    }
    members
}
