use crate::selection::Selection;
use crate::stage::Stage;
use donder_model::FixtureId;
use donder_model::PixelRouteId;
use donder_runtime::ShowBuilder;
use donder_runtime_types::{FixtureGeometry, TargetGeometry, TargetScope};
use indexmap::IndexMap;
use std::ops::Range;

pub(crate) fn targets(
    selected: &Selection<'_>,
    geometry: &[FixtureGeometry],
    members: &IndexMap<FixtureId, Vec<(usize, Range<u32>)>>,
) -> IndexMap<PixelRouteId, TargetGeometry> {
    selected
        .pixel_routes
        .iter()
        .filter(|route| {
            let placed = &selected.routes[&route.id].placement;
            selected.outputs.iter().any(|output| {
                &route.controller == output.controller && placed.output == output.output
            })
        })
        .map(|route| {
            let target = TargetGeometry::parts(
                members[&route.target]
                    .iter()
                    .map(|(index, cells)| (*index, &geometry[*index], cells.clone())),
                TargetScope::PerFixture,
            );
            let target = match route.pixels {
                Some(span) => {
                    target.slice(span.start as usize..span.start as usize + span.count as usize)
                }
                None => target,
            };
            (route.id, target)
        })
        .collect()
}

/// Each selected output's routes, in channel order, with their gamma and
/// brightness as a lookup table in 1/256 of a byte. An E1.31 or Art-Net
/// controller is one output of all its channels, so a route, and a pixel,
/// runs on across universe boundaries; the transport splits the bytes into
/// universes.
pub(crate) fn prepare(builder: &mut ShowBuilder<'_>, selected: &Selection<'_>, stage: &Stage) {
    for selected_output in &selected.outputs {
        let output = builder.port(
            selected_output.controller_index,
            selected_output.output.runtime_port(),
        );
        let mut routes = selected
            .pixel_routes
            .iter()
            .filter(|route| {
                &route.controller == selected_output.controller
                    && selected.routes[&route.id].placement.output == selected_output.output
            })
            .collect::<Vec<_>>();
        routes.sort_by_key(|route| selected.routes[&route.id].placement.channels.start);
        let mut cursor = 0;
        for route in routes {
            let target = builder.target(&stage.routes[&route.id]);
            let played = &selected.routes[&route.id];
            let encoding = played.encoding;
            let lookup = (route.gamma != 1.0 || route.brightness != 1.0).then(|| {
                std::array::from_fn(|index| {
                    (donder_model::patched_level(
                        index as f32 / 255.0,
                        route.gamma,
                        route.brightness,
                    ) * 255.0
                        * 256.0)
                        .round()
                        .clamp(0.0, 255.0 * 256.0) as u16
                })
            });
            // Admission checked non-overlap and the complete output width.
            let start = played.placement.channels.start as usize;
            builder.padding(output, start - cursor);
            let count = builder.route(
                output,
                target,
                encoding,
                lookup,
                route.dither,
                route.reverse,
            );
            cursor = start + count * encoding.channel_count();
        }
        builder.padding(output, selected_output.width as usize - cursor);
    }
}
