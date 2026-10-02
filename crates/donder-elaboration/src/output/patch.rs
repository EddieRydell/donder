use crate::selection::SelectedPort;
use donder_language::layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind};
use donder_language::patch::Patch;
use donder_runtime::patch::{PreparedPatch, PreparedPixelRoute};
use donder_runtime::signal::PreparedSignalGraph;
use indexmap::IndexMap;
use std::ops::Range;

/// Lower routes from the loaded project's already-validated patch. Targets and
/// port widths have been checked at the authoring boundary; output selection
/// changes which routes are retained, not their pixel coordinate domain.
pub(crate) fn prepare_patch(
    layout: &Layout,
    patch: &Patch,
    signal: &PreparedSignalGraph,
    ports: &[SelectedPort<'_>],
) -> PreparedPatch {
    let instances: IndexMap<_, _> = signal
        .fixtures
        .iter()
        .zip(&signal.fixture_pixel_offsets)
        .map(|(fixture, &offset)| (FixtureInstanceId(fixture.id), (offset, fixture.pixel_count)))
        .collect();
    fn visit(
        fixtures: &[LayoutFixture],
        instances: &IndexMap<FixtureInstanceId, (usize, usize)>,
        ranges: &mut IndexMap<FixtureInstanceId, Range<usize>>,
        offset: &mut usize,
    ) {
        for fixture in fixtures {
            let start = *offset;
            match &fixture.kind {
                LayoutFixtureKind::Fixture { .. } => {
                    let &(actual, count) = &instances[&fixture.id];
                    *offset = actual + count;
                }
                LayoutFixtureKind::Group { children } => visit(children, instances, ranges, offset),
            }
            ranges.insert(fixture.id, start..*offset);
        }
    }
    let mut ranges = IndexMap::new();
    visit(&layout.fixtures, &instances, &mut ranges, &mut 0);
    let mut routes = Vec::new();
    let mut lookups = Vec::new();
    for route in &patch.routes {
        // Selected-controller preparation intentionally excludes other physical outputs.
        let Some(frame) = ports
            .iter()
            .position(|port| *port.controller == route.controller && port.port.id == route.port)
        else {
            continue;
        };
        let target = &ranges[&route.target.fixture];
        let pixels = if let Some(span) = route.pixels {
            let start = target.start + span.start as usize;
            let end = start + span.count as usize;
            start..end
        } else {
            target.clone()
        };
        if pixels.is_empty() {
            continue;
        }
        let lookup = if route.gamma == 1.0 && route.brightness == 1.0 {
            None
        } else {
            let table = std::array::from_fn(|index| {
                ((index as f32 / 255.0).powf(route.gamma) * route.brightness * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as u8
            });
            let index = match lookups.iter().position(|existing| existing == &table) {
                Some(index) => index,
                None => {
                    let index = lookups.len();
                    lookups.push(table);
                    index
                }
            };
            Some(index)
        };
        routes.push(PreparedPixelRoute {
            pixels,
            frame,
            start_slot: usize::from(route.start_slot),
            encoding: route.encoding,
            lookup,
        });
    }
    PreparedPatch {
        routes: routes.into_boxed_slice(),
        lookups: lookups.into_boxed_slice(),
    }
}
