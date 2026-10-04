use super::*;
use core::num::NonZeroU32;
use donder_language::execution::{PixelEncoding, RgbOrder};

fn timing() -> SequenceTiming {
    SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap()
}

#[test]
fn spatial_storage_is_shared_by_physical_fixture_and_only_kept_for_consumers() {
    let spatial = donder_language::dsl::compile_effects(
        "effect Position { color sample() { return rgb(pixel_x(), 0.0, 0.0); } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let plain = donder_language::dsl::compile_effects(
        "effect Plain { color sample() { return rgb(1.0, 0.0, 0.0); } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let sequence = crate::PreparedSequence::build(timing(), |builder| {
        let a = builder.fixture(
            0,
            FixtureGeometry::admit((0..150).map(|index| [index as f32 / 149.0, 0.0]).collect())
                .unwrap(),
        );
        let b = builder.fixture(
            1,
            FixtureGeometry::admit(vec![[0.0, 0.0]; 150].into()).unwrap(),
        );
        let whole = builder.target([a], TargetScope::PerFixture);
        let sliced = builder.target_slice(whole, 50..100);
        let other = builder.target([b], TargetScope::PerFixture);
        let window = builder.whole_sequence();
        let effects = [
            builder.sample(&spatial, window, whole),
            builder.sample(&spatial, window, sliced),
            builder.sample(&plain, window, other),
        ];
        let layer = builder.layer(true, effects);
        builder.output([layer])
    });
    let graph = sequence.archive_data().signals;
    assert_eq!(graph.positions[0].len(), 150);
    assert!(graph.positions[1].is_empty());
    let a = &graph.targets[graph.effects[0].target];
    let b = &graph.targets[graph.effects[1].target];
    let context = b.spatial_context(0, &b.pixels.pixel(0), &graph.positions);
    assert_eq!(
        context,
        a.spatial_context(50, &a.pixels.pixel(50), &graph.positions)
    );
    assert_eq!(context.min, [0.0, 0.0]);
    assert_eq!(context.max, [1.0, 0.0]);
    assert_eq!(b.pixels.pixel(0).pixel_index, 50);
    assert_eq!(b.pixels.pixel(0).pixel_count, 150);
}

#[test]
fn archive_roundtrip_preserves_boundary_controller_metadata_and_output_order() {
    let sequence = crate::sequence::PreparedSequence::build(timing(), |builder| {
        for (controller, port, width) in [(u32::MAX, 3, 2), (0, 8, 1), (u32::MAX - 1, 5, 4)] {
            let output = builder.port(controller, port);
            builder.padding(output, width);
        }
        builder.output([])
    });
    let bytes = crate::archive::encode_sequence(&sequence).unwrap();
    let decoded =
        crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default()).unwrap();
    assert_eq!(decoded.outputs(), sequence.outputs());
    let mut playback = decoded.into_playback();
    let frame = playback.evaluate(SampleTime::from_ticks(0));
    let outputs: Vec<_> = frame
        .outputs()
        .map(|output| {
            (
                output.output.controller_index,
                output.output.port,
                output.bytes,
            )
        })
        .collect();
    assert_eq!(
        outputs,
        vec![
            (u32::MAX, 3, &[0, 0][..]),
            (0, 8, &[0][..]),
            (u32::MAX - 1, 5, &[0; 4][..])
        ]
    );
}

#[test]
fn full_rig_target_accepts_a_fixture_larger_than_u16_indices() {
    let sequence = crate::sequence::PreparedSequence::build(timing(), |builder| {
        let geometry =
            FixtureGeometry::admit((0..65_537).map(|cell| [cell as f32, 0.0]).collect()).unwrap();
        builder.fixture(7, geometry);
        builder.output([])
    });
    let graph = sequence.archive_data().signals;
    let target = graph.target(graph.plan.target);
    assert_eq!(target.len(), 65_537);
    assert_eq!(target.last().unwrap().fixture_pixel_index, 65_536);
    assert_eq!(target.last().unwrap().pixel_index, 65_536);
    let mut playback = sequence.into_playback();
    assert_eq!(
        playback.evaluate(SampleTime::from_ticks(0)).colors().len(),
        65_537
    );
}

#[test]
fn route_slice_preserves_original_logical_and_spatial_scope() {
    let mut builder = SequenceBuilder::new(timing());
    let first = builder.fixture(
        10,
        FixtureGeometry::admit(vec![[0.0, 1.0], [2.0, 3.0]].into()).unwrap(),
    );
    let last = builder.fixture(20, FixtureGeometry::admit(vec![[4.0, 5.0]].into()).unwrap());
    let whole = builder.target([last, first], TargetScope::WholeTarget);
    let sliced = builder.target_slice(whole, 1..usize::MAX);
    let original = &builder.targets[whole.index];
    let subset = &builder.targets[sliced.index];
    assert_eq!(subset.pixels, original.pixels[1..]);
    assert_eq!(subset.spatial, original.spatial[1..]);
    assert_eq!(
        subset.selection.as_ref(),
        [original.selection[0], original.selection[2]]
    );
    assert_eq!(subset.pixels[0].pixel_index, 2);
    assert_eq!(subset.pixels[0].pixel_count, 3);
    assert_eq!(subset.spatial[0].min, [0.0, 1.0]);
    assert_eq!(subset.spatial[0].max, [4.0, 5.0]);
    let output = builder.port(0, 1);
    builder.route(output, sliced, OutputEncoding::Rgb(RgbOrder::Rgb), None);
    assert_eq!(builder.outputs[output.index].bytes.len(), 6);
    assert_eq!(builder.routes[0].pixels, 1..3);
    let end = builder.target_slice(whole, 10..20);
    assert!(builder.targets[end.index].pixels.is_empty());
    let reversed = builder.target_slice(whole, core::ops::Range { start: 2, end: 1 });
    assert!(builder.targets[reversed.index].pixels.is_empty());
}

#[test]
fn encoding_admission_preserves_every_permutation_and_rejects_corrupt_orders() {
    for a in 0..5 {
        for b in 0..5 {
            for c in 0..5 {
                let rgb = PixelEncoding::Rgb { order: [a, b, c] };
                let admitted = OutputEncoding::admit(rgb);
                assert_eq!(admitted.is_some(), rgb.is_valid());
                if let Some(admitted) = admitted {
                    assert_eq!(admitted.channel_count(), 3);
                    assert_eq!(admitted.encoding(), rgb);
                }
                for d in 0..5 {
                    let rgbw = PixelEncoding::Rgbw {
                        order: [a, b, c, d],
                    };
                    let admitted = OutputEncoding::admit(rgbw);
                    assert_eq!(admitted.is_some(), rgbw.is_valid());
                    if let Some(admitted) = admitted {
                        assert_eq!(admitted.channel_count(), 4);
                        assert_eq!(admitted.encoding(), rgbw);
                    }
                }
            }
        }
    }
}
