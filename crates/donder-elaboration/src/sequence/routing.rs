use crate::selection::Selection;
use crate::stage::Stage;
use donder_model::FixtureInstanceId;
use donder_model::PixelRouteId;
use donder_runtime::ShowBuilder;
use donder_runtime_types::{FixtureGeometry, TargetGeometry, TargetScope};
use indexmap::IndexMap;

pub(crate) fn targets<S>(
    selected: &Selection<'_, S>,
    geometry: &[FixtureGeometry],
    members: &IndexMap<FixtureInstanceId, Vec<usize>>,
) -> IndexMap<PixelRouteId, TargetGeometry> {
    selected
        .patch
        .routes
        .iter()
        .filter(|route| {
            selected
                .ports
                .iter()
                .any(|port| &route.controller == port.controller && route.port == port.port.id)
        })
        .map(|route| {
            let target = TargetGeometry::new(
                members[&route.target.fixture]
                    .iter()
                    .map(|&index| (index, &geometry[index])),
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

/// Each selected port's routes, in slot order, with their gamma and
/// brightness as a lookup table.
pub(crate) fn prepare<S>(
    builder: &mut ShowBuilder<'_>,
    selected: &Selection<'_, S>,
    stage: &Stage,
) {
    for port in &selected.ports {
        let output = builder.port(port.controller_index, port.port.id.0);
        let mut routes = selected
            .patch
            .routes
            .iter()
            .filter(|route| &route.controller == port.controller && route.port == port.port.id)
            .collect::<Vec<_>>();
        routes.sort_by_key(|route| route.start_slot);
        let mut cursor = 0;
        for route in routes {
            let target = builder.target(&stage.routes[&route.id]);
            let encoding = selected.encodings[&route.id];
            let lookup = (route.gamma != 1.0 || route.brightness != 1.0).then(|| {
                std::array::from_fn(|index| {
                    ((index as f32 / 255.0).powf(route.gamma) * route.brightness * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                })
            });
            // Admission checked non-overlap and the complete port width.
            let start = usize::from(route.start_slot);
            builder.padding(output, start - cursor);
            let count = builder.route(output, target, encoding, lookup);
            cursor = start + count * encoding.channel_count();
        }
        builder.padding(output, usize::from(port.port.slot_count) - cursor);
    }
}
