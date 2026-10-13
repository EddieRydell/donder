//! Resolve selected sampling domains before assembling owner-branded playback storage.
pub(crate) mod composition;
pub(crate) mod programs;
pub use programs::PreparationCache;
mod retention;
mod routing;

use crate::selection::Selection;
use crate::stage::{Stage, Targets};
use donder_runtime::PreparedSequence;
use indexmap::IndexMap;

pub(crate) use retention::cells as retained_cells;
pub(crate) use routing::{prepare as prepare_routes, targets as route_targets};

pub(crate) fn prepare(
    selected: Selection<'_>,
    compact: bool,
    cache: &mut PreparationCache,
) -> PreparedSequence {
    let sequence = selected.show.sequence();
    let stage = Stage::new(
        &selected,
        &sequence.composition_graph,
        selected.show.operators(),
        compact,
    );
    let mut programs = programs::Programs::new(cache);
    let required_layers = stage.required_layers(&sequence.composition_graph);
    PreparedSequence::build(selected.show.timing().clone(), |builder| {
        let fixtures = stage.fixtures(builder, &selected);
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
        let mut targets = Targets::default();
        let windows = builder.windows().collect::<Vec<_>>();
        for ((accepted, window), timing) in selected
            .show
            .effects()
            .zip(windows)
            .zip(selected.show.timing().windows())
        {
            let effect = accepted.instance();
            if compact
                && (!stage.has_pixels
                    || !enabled_layers.contains(&effect.layer_id)
                    || !required_layers.contains(&effect.layer_id))
            {
                continue;
            }
            if stage.drops(&effect.target) {
                continue;
            }
            let (target, pixel_count) =
                targets.get(builder, &stage, &fixtures, &effect.target, &effect.scope);
            let invocation = programs.sample(
                accepted.execution(),
                donder_language::compiler::ProgramConstants {
                    pixel_count,
                    duration_seconds: Some(donder_runtime_types::sample_duration_seconds_f32(
                        donder_runtime_types::SampleDuration::from_ticks(timing.duration.get()),
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
            .filter(|layer| !compact || stage.has_pixels && required_layers.contains(&layer.id))
            .map(|layer| {
                (
                    &layer.id,
                    builder.layer(layer.enabled, layer_effects[&layer.id].iter().copied()),
                )
            })
            .collect::<IndexMap<_, _>>();
        // A disabled or empty layer is black everywhere.
        let black = sequence
            .layers
            .iter()
            .filter(|layer| !layer.enabled || layer_effects[&layer.id].is_empty())
            .map(|layer| &layer.id)
            .collect::<std::collections::HashSet<_>>();
        let root = if compact && !stage.has_pixels {
            builder.output([])
        } else {
            composition::prepare(
                builder,
                &sequence.composition_graph,
                selected.show.operators(),
                selected.show.timing().duration(),
                &layers,
                &black,
                &mut programs,
            )
        };
        prepare_routes(
            builder,
            &selected,
            &stage.geometry,
            &stage.targets,
            &stage.cells,
            &fixtures,
        );
        root
    })
}
