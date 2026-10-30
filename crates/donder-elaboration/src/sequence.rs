//! A sequence's show: each clip at its authored time.
pub(crate) mod composition;
pub(crate) mod programs;
pub use programs::PreparationCache;
mod retention;
mod routing;

use crate::selection::Selection;
use crate::stage::{self, Parts};
use donder_language::compiler::ProgramConstants;
use donder_runtime::PreparedShow;
use donder_runtime_types::sample_duration_seconds_f32;

pub(crate) use retention::cells as retained_cells;
pub(crate) use routing::{prepare as prepare_routes, targets as route_targets};

pub(crate) fn prepare(
    selected: Selection<'_>,
    compact: bool,
    cache: &mut PreparationCache,
) -> PreparedShow {
    let accepted = selected.show;
    let sequence = accepted.sequence();
    let parts = Parts {
        layers: &sequence.layers,
        graph: &sequence.composition_graph,
        operators: accepted.operators().collect(),
        timing: accepted.timing(),
    };
    stage::build(
        &selected,
        parts,
        compact,
        cache,
        |clips| {
            for clip in accepted.clips() {
                let authored = clip.clip();
                if clips.drops(&authored.layer_id, &authored.target) {
                    continue;
                }
                let (target, pixel_count) = clips.target(&authored.target, &authored.scope);
                let program = clips.lower(
                    clip.invocation(),
                    ProgramConstants {
                        pixel_count,
                        duration_seconds: Some(sample_duration_seconds_f32(clip.duration())),
                    },
                );
                let handle = clips.builder.clip(
                    Some(authored.id.0),
                    &program,
                    clip.start(),
                    clip.duration(),
                    target,
                );
                clips.layer(&authored.layer_id, handle);
            }
        },
        |_, _, ()| {},
    )
}
