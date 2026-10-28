use donder_language::compiler::compile_script;
use donder_runtime::PreparedShow;
use donder_runtime_types::BoundProgram;
use donder_runtime_types::Color;
use donder_runtime_types::FixtureGeometry;
use donder_runtime_types::SampleTime;
use donder_runtime_types::{SampleDuration, TargetGeometry, TargetScope};
use donder_test_support::playback;

/// Fixtures of `sizes` pixels under one whole-target effect whose colors
/// encode each pixel's place in its fixture and the time.
fn sequence(sizes: &[usize], operator: Option<&BoundProgram>) -> PreparedShow {
    let effect = compile_script(
        "effect Positions { sample { rgb(pixel.index / 255.0, time / 8.0, pixel.fraction) } }",
    )
    .unwrap()
    .remove(0);
    let sample = playback::program(&effect);
    PreparedShow::build(playback::timing(8_000_000), |builder| {
        let fixtures: Vec<_> = sizes
            .iter()
            .map(|&size| FixtureGeometry::admit(vec![[0.0; 2]; size].into()).unwrap())
            .collect();
        for (id, geometry) in fixtures.iter().enumerate() {
            builder.fixture(id as u32, geometry.clone());
        }
        let target = builder.target(&TargetGeometry::new(
            fixtures.iter().enumerate(),
            TargetScope::WholeTarget,
        ));
        let effect = builder.clip(
            None,
            &sample,
            SampleTime::from_ticks(0),
            SampleDuration::from_ticks(8_000_000),
            target,
        );
        let layer = builder.layer(true, [effect]);
        let output = operator.map_or(layer, |operator| builder.operator(operator, |_| layer));
        Some(output)
    })
}

/// `query` over fixtures of `sizes` shows at each pixel `n` the source pixel
/// `indices[n]` (black for `None`).
fn assert_query(sizes: &[usize], query: &str, indices: &[Option<usize>]) {
    let invocation = playback::invoke(
        &compile_script(&format!(
            "operator Spatial {{ input source; sample {{ {query} }} }}"
        ))
        .unwrap()
        .remove(0),
        &[],
    );
    assert!(invocation.reads_fixture_pixels(), "{query}");
    let lowered = playback::lower(&invocation);
    let mut workspace = sequence(sizes, Some(&lowered)).into_playback();
    let mut source_workspace = sequence(sizes, None).into_playback();
    for ticks in [0, 500_000, 2_000_000, 100_000, 0] {
        let time = SampleTime::from_ticks(ticks);
        let source = source_workspace.evaluate(time).colors();
        let expected = indices
            .iter()
            .map(|index| index.map_or(Color::BLACK, |index| source[index]))
            .collect::<Vec<_>>();
        assert_eq!(
            workspace.evaluate(time).colors(),
            expected,
            "{query}, time={ticks}"
        );
    }
}

#[test]
fn spatial_queries_match_explicit_source_pixels() {
    // Each query and the source pixel each output pixel shows.
    for (query, indices) in [
        (
            "source.at(target.count - 1 - pixel.index)",
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
            "source.at(pixel.index + 1)",
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
        ("source.at(-1)", vec![None; 8]),
        // A reduction gathers every pixel of the current fixture.
        (
            "max for i in 0..4 { source.at(i) }",
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
    ] {
        assert_query(&[4, 4], query, &indices);
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
    let queries: [(&str, Address); 3] = [
        (
            "source.at(target.count - 1 - pixel.index)",
            |first, size, index| Some(first + size - 1 - index),
        ),
        ("source.at(pixel.index + 120)", |first, size, index| {
            (index + 120 < size).then_some(first + index + 120)
        }),
        // The source brightens with the index, so the reduction shows its
        // last tap inside the fixture.
        (
            "max for i in 0..4 { source.at(i * 40) }",
            |first, size, _| Some(first + 40 * ((size - 1) / 40).min(3)),
        ),
    ];
    for (query, address) in queries {
        assert_query(&SIZES, query, &indices(address));
    }
}
