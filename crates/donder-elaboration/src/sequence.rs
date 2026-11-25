//! A sequence's show: each clip at its authored time.
pub(crate) mod composition;
pub(crate) mod programs;
pub use programs::PreparationCache;
mod retention;
mod routing;

use crate::selection::Selection;
use crate::stage;
use donder_runtime::PreparedShow;

pub(crate) use retention::cells as retained_cells;
pub(crate) use routing::{prepare as prepare_routes, targets as route_targets};

pub(crate) fn prepare(
    selected: Selection<'_>,
    compact: bool,
    cache: &mut PreparationCache,
) -> PreparedShow {
    stage::build(&selected, compact, cache, None)
}
