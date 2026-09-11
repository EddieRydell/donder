use super::errors::SequenceOutputPrepareError;
use super::frame::ControllerPortFrame;
use dawn_language::layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind};
use dawn_language::patch::Patch;
use dawn_runtime::patch::{PreparedPatch, PreparedPixelRoute};
use dawn_runtime::signal::PreparedSignalGraph;
use indexmap::IndexMap;
use std::ops::Range;

pub(super) fn prepare_patch(
    layout: &Layout,
    patch: &Patch,
    signal: &PreparedSignalGraph,
    ports: &[ControllerPortFrame],
) -> Result<PreparedPatch, SequenceOutputPrepareError> {
    let instances: IndexMap<_, _> = signal
        .fixtures
        .iter()
        .zip(&signal.fixture_pixel_offsets)
        .map(|(fixture, &offset)| (FixtureInstanceId(fixture.id), (offset, fixture.pixel_count)))
        .collect();
    fn visit(
        fixtures: &[LayoutFixture],
        instances: &IndexMap<FixtureInstanceId, (usize, usize)>,
        ranges: &mut IndexMap<FixtureInstanceId, Range<u32>>,
        offset: &mut u32,
    ) -> Result<(), SequenceOutputPrepareError> {
        for fixture in fixtures {
            let start = *offset;
            match &fixture.kind {
                LayoutFixtureKind::Fixture { .. } => {
                    let &(actual, count) = instances
                        .get(&fixture.id)
                        .ok_or_else(|| invalid("Prepared fixture is missing."))?;
                    if actual != start as usize {
                        return Err(invalid("Prepared fixture order differs from the layout."));
                    }
                    *offset = offset
                        .checked_add(u32::try_from(count).map_err(|_| invalid("Too many pixels."))?)
                        .ok_or_else(|| invalid("Too many pixels."))?;
                }
                LayoutFixtureKind::Group { children } => {
                    visit(children, instances, ranges, offset)?
                }
            }
            ranges.insert(fixture.id, start..*offset);
        }
        Ok(())
    }
    let mut ranges = IndexMap::new();
    visit(&layout.fixtures, &instances, &mut ranges, &mut 0)?;
    let mut routes = Vec::new();
    let mut lookups = Vec::new();
    for route in &patch.routes {
        // Selected-controller preparation intentionally excludes other physical outputs.
        let Some(frame) = ports
            .iter()
            .position(|port| port.controller == route.controller && port.port == route.port)
        else {
            continue;
        };
        if route.target.layout != layout.id {
            return Err(invalid("Output targets a different layout."));
        }
        let target = ranges
            .get(&route.target.fixture)
            .ok_or_else(|| invalid("Output target is missing."))?;
        let pixels = if let Some(span) = route.pixels {
            let start = target
                .start
                .checked_add(span.start)
                .ok_or_else(|| invalid("Pixel span overflowed."))?;
            let end = start
                .checked_add(span.count)
                .filter(|&end| end <= target.end)
                .ok_or_else(|| invalid("Pixel span exceeds its target."))?;
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
            Some(u16::try_from(index).map_err(|_| invalid("Too many output transforms."))?)
        };
        routes.push(PreparedPixelRoute {
            pixels,
            frame: u32::try_from(frame).map_err(|_| invalid("Too many outputs."))?,
            start_slot: u32::from(route.start_slot),
            encoding: route.encoding,
            lookup,
        });
    }
    Ok(PreparedPatch {
        routes: routes.into_boxed_slice(),
        lookups: lookups.into_boxed_slice(),
    })
}

fn invalid(message: &str) -> SequenceOutputPrepareError {
    SequenceOutputPrepareError::InvalidPatch(message.into())
}
