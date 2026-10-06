use donder_language::dsl::{
    OperatorInvocation, SignalAddressing, compile_effects, compile_operators,
};
use donder_language::execution::FixtureGeometry;
use donder_language::execution::TargetScope;
use donder_language::values::Color;
use donder_language::values::SampleTime;
use donder_runtime::PreparedSequence;
use donder_test_support::{playback, workload};

const LOCAL: SignalAddressing = SignalAddressing {
    local: true,
    global: false,
};
const GLOBAL: SignalAddressing = SignalAddressing {
    local: false,
    global: true,
};

/// Fixtures of `sizes` pixels under one whole-target effect whose colors
/// encode each pixel's place in its fixture and the sampled time.
fn sequence(sizes: &[usize], operator: Option<&OperatorInvocation>) -> PreparedSequence {
    let effect = compile_effects(
        "effect Positions { sample { rgb(pixel.index / 255.0, time / 8.0, pixel.fraction) } }",
    )
    .unwrap()
    .remove(0);
    let sample = playback::sample(&effect);
    PreparedSequence::build(playback::timing(8_000_000), |builder| {
        let fixtures: Vec<_> = sizes
            .iter()
            .enumerate()
            .map(|(id, &size)| {
                builder.fixture(
                    id as u32,
                    FixtureGeometry::admit(vec![[0.0; 2]; size].into()).unwrap(),
                )
            })
            .collect();
        let target = builder.target(fixtures, TargetScope::WholeTarget);
        let effect = builder.sample(&sample, builder.whole_sequence(), target);
        let layer = builder.layer(true, [effect]);
        let output = operator.map_or(layer, |operator| builder.operator(operator, |_| layer));
        builder.output([output])
    })
}

/// With and without frame caches, `query` over fixtures of `sizes` shows at
/// each pixel `n` the source pixel `indices[n]` (black for `None`) as it was
/// `delay` ticks earlier.
fn assert_query(
    sizes: &[usize],
    query: &str,
    addressing: SignalAddressing,
    delay: u32,
    indices: &[Option<usize>],
) {
    let invocation = compile_operators(&format!(
        "operator Spatial {{ input source; sample {{ {query} }} }}"
    ))
    .unwrap()
    .remove(0)
    .bind(std::iter::empty())
    .unwrap();
    assert_eq!(invocation.addressing(), addressing, "{query}");
    let lowered = playback::lower_operator(&invocation);
    // Every sample time here is query-uniform.
    assert!(
        workload::cached_samples(lowered.program().bytecode()) > 0,
        "{query}"
    );
    let uncached = workload::edit_operator(&lowered, workload::without_frame_caches);
    for (cached, operator) in [(true, &lowered), (false, &uncached)] {
        let mut workspace = sequence(sizes, Some(operator)).into_playback();
        let mut source_workspace = sequence(sizes, None).into_playback();
        for ticks in [0, 500_000, 2_000_000, 100_000, 0] {
            let time = SampleTime::from_ticks(ticks);
            let expected = match ticks.checked_sub(delay) {
                Some(source_ticks) => {
                    let source = source_workspace
                        .evaluate(SampleTime::from_ticks(source_ticks))
                        .colors();
                    indices
                        .iter()
                        .map(|index| index.map_or(Color::BLACK, |index| source[index]))
                        .collect::<Vec<_>>()
                }
                None => vec![Color::BLACK; indices.len()],
            };
            assert_eq!(
                workspace.evaluate(time).colors(),
                expected,
                "{query}, cached={cached}, time={ticks}"
            );
        }
    }
}

#[test]
fn spatial_queries_match_explicit_source_pixels_with_and_without_frame_caches() {
    // Each query, how it addresses pixels, how many ticks earlier it samples,
    // and the source pixel each output pixel shows.
    for (query, addressing, delay, indices) in [
        (
            "source.at(time, target.count - 1 - pixel.index)",
            LOCAL,
            0,
            vec![
                Some(3),
                Some(2),
                Some(1),
                Some(0),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        (
            "source.at(time, pixel.index + 1)",
            LOCAL,
            0,
            vec![
                Some(1),
                Some(2),
                Some(3),
                None,
                Some(5),
                Some(6),
                Some(7),
                None,
            ],
        ),
        (
            "source.at_global(time, 7 - pixel.index)",
            GLOBAL,
            0,
            vec![
                Some(7),
                Some(6),
                Some(5),
                Some(4),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        ("source.at(time, -1)", LOCAL, 0, vec![None; 8]),
        ("source.at_global(time, 8)", GLOBAL, 0, vec![None; 8]),
        (
            "max(source.at_global(time, 1), source.at_global(time, 6))",
            GLOBAL,
            0,
            vec![Some(6); 8],
        ),
        // A reduction gathers every pixel of the current fixture.
        (
            "max for i in 0..4 { source.at(time, i) }",
            LOCAL,
            0,
            vec![
                Some(3),
                Some(3),
                Some(3),
                Some(3),
                Some(7),
                Some(7),
                Some(7),
                Some(7),
            ],
        ),
        // Spatial and temporal changes combine; before the sequence is black.
        (
            "source.at(time - 0.25, target.count - 1 - pixel.index)",
            LOCAL,
            250_000,
            vec![
                Some(3),
                Some(2),
                Some(1),
                Some(0),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
    ] {
        assert_query(&[4, 4], query, addressing, delay, &indices);
    }
}

/// The source pixel of a fixture's pixel, from the fixture's first target
/// pixel, its size and the pixel's index in it.
type Address = fn(usize, usize, usize) -> Option<usize>;

#[test]
fn spatial_queries_reach_pixels_of_other_strips() {
    // Fixtures longer than a strip, so most queries read pixels of another
    // strip, and strips that cross the fixture boundary where nothing splits
    // them there.
    const SIZES: [usize; 2] = [150, 100];
    let indices = |address: Address| {
        let mut indices = Vec::new();
        let mut first = 0;
        for size in SIZES {
            indices.extend((0..size).map(|index| address(first, size, index)));
            first += size;
        }
        indices
    };
    let queries: [(&str, SignalAddressing, u32, Address); 4] = [
        (
            "source.at(time, target.count - 1 - pixel.index)",
            LOCAL,
            0,
            |first, size, index| Some(first + size - 1 - index),
        ),
        (
            "source.at(time, pixel.index + 120)",
            LOCAL,
            0,
            |first, size, index| (index + 120 < size).then_some(first + index + 120),
        ),
        (
            "source.at_global(time, 249 - pixel.index)",
            GLOBAL,
            0,
            |_, _, index| Some(249 - index),
        ),
        // The source brightens with the index, so the reduction shows its
        // last tap inside the fixture.
        (
            "max for i in 0..4 { source.at(time - 0.25, i * 40) }",
            LOCAL,
            250_000,
            |first, size, _| Some(first + 40 * ((size - 1) / 40).min(3)),
        ),
    ];
    for (query, addressing, delay, address) in queries {
        assert_query(&SIZES, query, addressing, delay, &indices(address));
    }
}
