//! Selected output cells and the additional input domain reachable operators need.
use crate::selection::Selection;
use donder_language::execution::{FixtureGeometry, TargetGeometry};
use donder_language::patch::PixelRouteId;
use donder_language::sequence::CompositionGraphNodeId;
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashSet};

pub(super) fn cells(
    selected: &Selection<'_>,
    geometry: &[FixtureGeometry],
    routes: &IndexMap<PixelRouteId, TargetGeometry>,
    dependencies: &HashSet<CompositionGraphNodeId>,
    compact: bool,
) -> Vec<Vec<u32>> {
    if !compact {
        return geometry
            .iter()
            .map(|fixture| (0..fixture.positions().len() as u32).collect())
            .collect();
    }
    let mut cells = vec![BTreeSet::new(); geometry.len()];
    for target in routes.values() {
        for pixel in target.pixels() {
            cells[pixel.fixture()].insert(pixel.cell());
        }
    }
    let mut local = false;
    let mut global = false;
    for operator in selected
        .sequence
        .operators()
        .filter(|operator| dependencies.contains(&operator.node().id))
    {
        let addressing = operator.invocation().addressing();
        local |= addressing.local;
        global |= addressing.global;
    }
    // Local reads can address every original cell of selected fixtures. Global
    // reads can address unpatched fixtures, so preserve the original full rig.
    if cells.iter().any(|cells| !cells.is_empty()) && (local || global) {
        for (fixture, cells) in geometry.iter().zip(&mut cells) {
            if global || !cells.is_empty() {
                cells.extend(0..fixture.positions().len() as u32);
            }
        }
    }
    cells
        .into_iter()
        .map(|cells| cells.into_iter().collect())
        .collect()
}
