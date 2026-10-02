use crate::selection::Selection;
use donder_language::layout::FixtureInstanceId;
use donder_runtime::{FixtureHandle, SequenceBuilder, TargetScope};
use indexmap::IndexMap;

pub(super) fn prepare<'id>(
    builder: &mut SequenceBuilder<'id>,
    selected: &Selection<'_>,
    targets: &IndexMap<FixtureInstanceId, Vec<FixtureHandle<'id>>>,
) {
    let mut lookups = Vec::new();
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
            let target = builder.target(
                targets[&route.target.fixture].iter().copied(),
                TargetScope::PerFixture,
            );
            let target = match route.pixels {
                Some(span) => builder.target_slice(
                    target,
                    span.start as usize..span.start as usize + span.count as usize,
                ),
                None => target,
            };
            let count = builder.target_pixel_count(target);
            if count == 0 {
                continue;
            }
            let encoding = selected.encodings[&route.id];
            let lookup = if route.gamma == 1.0 && route.brightness == 1.0 {
                None
            } else {
                let table = std::array::from_fn(|index| {
                    ((index as f32 / 255.0).powf(route.gamma) * route.brightness * 255.0)
                        .round()
                        .clamp(0.0, 255.0) as u8
                });
                Some(
                    match lookups.iter().find(|(existing, _)| existing == &table) {
                        Some((_, handle)) => *handle,
                        None => {
                            let handle = builder.lookup(table);
                            lookups.push((table, handle));
                            handle
                        }
                    },
                )
            };
            // Admission checked non-overlap and the complete port width.
            let start = usize::from(route.start_slot);
            builder.padding(output, start - cursor);
            builder.route(output, target, encoding, lookup);
            cursor = start + count * encoding.channel_count();
        }
        builder.padding(output, usize::from(port.port.slot_count) - cursor);
    }
}
