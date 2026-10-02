//! Lower accepted authoring relationships to owner-branded playback handles.
//! Parameter binding, clock admission and geometry expansion have already happened
//! at project admission. This module only assembles their accepted results.

mod composition;
mod routing;

use crate::selection::Selection;
use donder_language::effect::EffectScope;
use donder_language::layout::{FixtureInstanceId, LayoutFixture, LayoutFixtureKind};
use donder_runtime::{FixtureHandle, PreparedSequence, TargetScope};
use indexmap::IndexMap;

pub(crate) fn prepare(selected: Selection<'_>, compact: bool) -> PreparedSequence {
    PreparedSequence::build(selected.sequence.timing().clone(), |builder| {
        let fixtures = selected
            .geometry
            .iter()
            .map(|(id, geometry)| (*id, builder.fixture(id.0, geometry.clone())))
            .collect::<IndexMap<_, _>>();
        let mut targets = IndexMap::new();
        collect_targets(&selected.layout.fixtures, &fixtures, &mut targets);
        let sequence = selected.sequence.sequence();
        let mut layer_effects = sequence
            .layers
            .iter()
            .map(|layer| (&layer.id, Vec::new()))
            .collect::<IndexMap<_, _>>();
        let windows = builder.windows().collect::<Vec<_>>();
        for (accepted, window) in selected.sequence.effects().zip(windows) {
            let effect = accepted.instance();
            let fixtures = &targets[&effect.target.fixture];
            let scope = match effect.scope {
                EffectScope::PerFixture => TargetScope::PerFixture,
                EffectScope::WholeTarget => TargetScope::WholeTarget,
            };
            let target = builder.target(fixtures.iter().copied(), scope);
            let prepared = builder.sample(accepted.execution(), window, target);
            builder.clip(effect.id.0, prepared);
            layer_effects[&effect.layer_id].push(prepared);
        }
        let layers = sequence
            .layers
            .iter()
            .map(|layer| {
                (
                    &layer.id,
                    builder.layer(layer.enabled, layer_effects[&layer.id].iter().copied()),
                )
            })
            .collect::<IndexMap<_, _>>();
        let root = composition::prepare(builder, selected.sequence, &layers);
        routing::prepare(builder, &selected, &targets);
        if compact {
            builder.compact_to_outputs();
        }
        root
    })
}

/// Every group denotes its ordered leaf fixtures, including empty groups.
fn collect_targets<'id>(
    nodes: &[LayoutFixture],
    fixtures: &IndexMap<FixtureInstanceId, FixtureHandle<'id>>,
    targets: &mut IndexMap<FixtureInstanceId, Vec<FixtureHandle<'id>>>,
) -> Vec<FixtureHandle<'id>> {
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
