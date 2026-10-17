//! Selected output cells and the additional input domain reachable operators need.
use donder_model::AcceptedOperatorInputs;
use donder_model::CompositionGraphNodeId;
use donder_model::PixelRouteId;
use donder_runtime_types::{FixtureGeometry, TargetGeometry};
use indexmap::IndexMap;
use std::collections::{BTreeSet, HashSet};

pub(crate) fn cells<'a>(
    operators: impl Iterator<Item = AcceptedOperatorInputs<'a>>,
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
    // Reads of other pixels of a fixture can address every original cell of
    // the selected fixtures.
    let local = operators
        .filter(|operator| dependencies.contains(&operator.node().id))
        .any(|operator| operator.invocation().reads_fixture_pixels());
    if local {
        for (fixture, cells) in geometry.iter().zip(&mut cells) {
            if !cells.is_empty() {
                cells.extend(0..fixture.positions().len() as u32);
            }
        }
    }
    cells
        .into_iter()
        .map(|cells| cells.into_iter().collect())
        .collect()
}
