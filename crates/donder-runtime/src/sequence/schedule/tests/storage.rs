use super::*;
use donder_runtime_types::Color;

#[derive(Clone, Copy)]
enum Selection {
    First,
    Split,
    Shared,
}

fn selected_sequence(selected: bool, selection: Selection, invert: bool) -> PreparedSequence {
    let ramp = SampleProgram::admit(program(
        vec![
            Instruction::FloatConst {
                dst: Slot::scalar(0),
                bits: 0.0f32.to_bits(),
            },
            Instruction::Rgb {
                dst: Slot::row(0),
                red: Slot::input(Input::PixelFraction),
                green: Slot::scalar(0),
                blue: Slot::scalar(0),
            },
        ],
        1,
        Slot::row(0),
        banks(1, 0, 0),
        banks(0, 0, 1),
    ))
    .unwrap();
    let green = SampleProgram::admit(program(
        vec![Instruction::ColorConst {
            dst: Slot::scalar(0),
            value: Color {
                red: 0,
                green: 255,
                blue: 0,
            },
        }],
        1,
        Slot::scalar(0),
        banks(0, 0, 1),
        Banks::default(),
    ))
    .unwrap();
    let ramp = SampleInvocation::bind(ramp, vec![]).unwrap();
    let green = SampleInvocation::bind(green, vec![]).unwrap();
    let invert = invert.then(|| {
        let operator = OperatorProgram::admit(
            BytecodeProgram {
                frame_caches: 1,
                ..program(
                    vec![
                        Instruction::Context {
                            dst: Slot::scalar(0),
                            read: ContextRead::Seconds,
                        },
                        Instruction::Sample {
                            dst: Slot::row(0),
                            input: 0,
                            seconds: Slot::scalar(0),
                            pixel: SignalPixel::Current,
                            frame_cache: 0,
                        },
                        Instruction::Invert {
                            dst: Slot::row(1),
                            color: Slot::row(0),
                        },
                    ],
                    1,
                    Slot::row(1),
                    banks(1, 0, 0),
                    banks(0, 0, 2),
                )
            },
            1,
        )
        .unwrap();
        OperatorInvocation::bind(operator, vec![]).unwrap()
    });
    let timing = SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        // Preparation owns pruning. These fixtures describe its full and
        // selected results so runtime storage and output can be compared.
        let fixtures = [10, 20].map(|id| {
            let geometry =
                FixtureGeometry::admit((0..113).map(|cell| [cell as f32, 0.0]).collect()).unwrap();
            builder.fixture(
                id,
                if selected {
                    geometry.select(|cell| {
                        id == 10
                            && (!matches!(selection, Selection::Split) || !(37..76).contains(&cell))
                    })
                } else {
                    geometry
                },
            )
        });
        let a = builder.target([fixtures[0]], TargetScope::PerFixture);
        let b = builder.target([fixtures[1]], TargetScope::PerFixture);
        let window = builder.whole_sequence();
        let signal = if selected {
            if let Some(operator) = &invert {
                let empty = builder.layer(true, []);
                builder.operator(operator, |_| empty)
            } else {
                let red = builder.sample(&ramp, window, a);
                builder.layer(true, [red])
            }
        } else {
            let red = builder.sample(&ramp, window, a);
            let green = builder.sample(&green, window, b);
            let red = builder.layer(true, [red]);
            let green = builder.layer(true, [green]);
            match &invert {
                // The red layer is disconnected; the green layer targets an unselected fixture.
                Some(operator) => builder.operator(operator, |_| green),
                None => builder.mix([red, green]),
            }
        };
        let first = builder.port(0, 1);
        match selection {
            Selection::First => builder.route(first, a, OutputEncoding::Rgb(RgbOrder::Rgb), None),
            Selection::Split => {
                let low = builder.target_slice(a, 0..37);
                let high = builder.target_slice(a, if selected { 37..74 } else { 76..113 });
                builder.route(first, low, OutputEncoding::Rgb(RgbOrder::Rgb), None);
                let second = builder.port(1, 2);
                builder.route(second, high, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            }
            Selection::Shared => {
                builder.route(first, a, OutputEncoding::Rgb(RgbOrder::Rgb), None);
                let second = builder.port(1, 2);
                builder.route(second, a, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            }
        }
        builder.output([signal])
    })
}

fn assert_outputs_match(full: PreparedSequence, compacted: PreparedSequence) {
    let mut full = full.into_playback();
    let mut compacted = compacted.into_playback();
    for ticks in [0, 500_000, 1_000_000, 250_000, 0] {
        let time = SampleTime::from_ticks(ticks);
        assert!(
            full.evaluate(time)
                .outputs()
                .eq(compacted.evaluate(time).outputs())
        );
    }
}

#[test]
fn preselected_fixture_preserves_output() {
    let full = selected_sequence(false, Selection::First, false);
    let compacted = selected_sequence(true, Selection::First, false);
    assert_outputs_match(full, compacted);
}

#[test]
fn disjoint_spans_remap_storage_without_changing_logical_coordinates() {
    let full = selected_sequence(false, Selection::Split, false);
    let compacted = selected_sequence(true, Selection::Split, false);
    let data = compacted.archive_data();
    let target = data.signals.target(data.signals.plan.target);
    assert_eq!(data.signals.pixel_count, 74);
    assert_eq!(target.len(), 74);
    for (physical, logical) in (0..37).chain(76..113).enumerate() {
        assert_eq!(target.pixel(physical).fixture_pixel_index, physical as u32);
        assert_eq!(target.pixel(physical).pixel_index, logical);
        assert_eq!(target.pixel(physical).pixel_count, 113);
        assert_eq!(
            target.pixel(physical).pixel_fraction,
            logical as f32 / 112.0
        );
    }
    assert_eq!(target.pixel(37).fixture_pixel_index, 37);
    assert_eq!(target.pixel(37).pixel_index, 76);
    assert_eq!(data.patch.routes.len(), 2);
    assert_eq!(data.patch.routes[0].pixels, 0..37);
    assert_eq!(data.patch.routes[1].pixels, 37..74);
    assert_outputs_match(full, compacted);
}

#[test]
fn shared_fixture_keeps_both_routes() {
    let full = selected_sequence(false, Selection::Shared, false);
    let compacted = selected_sequence(true, Selection::Shared, false);
    let data = compacted.archive_data();
    assert_eq!(data.signals.fixtures.len(), 1);
    assert_eq!(data.signals.pixel_count, 113);
    assert_eq!(data.patch.routes.len(), 2);
    assert_eq!(data.outputs.len(), 2);
    for (index, route) in data.patch.routes.iter().enumerate() {
        assert_eq!(route.pixels, 0..113);
        assert_eq!(route.frame, index);
        assert_eq!(data.outputs[index].controller_index, index as u32);
        assert_eq!(data.outputs[index].width, 339);
    }
    assert_outputs_match(full, compacted);
}

#[test]
fn prepruned_upstream_samples_keep_white_inverted_output() {
    let full = selected_sequence(false, Selection::First, true);
    let compacted = selected_sequence(true, Selection::First, true);
    let mut playback = compacted.clone().into_playback();
    let frame = playback.evaluate(SampleTime::from_ticks(500_000));
    let bytes = frame.outputs().next().unwrap().bytes;
    assert_eq!(bytes.len(), 339);
    assert!(bytes.iter().all(|&channel| channel == 255));
    assert_outputs_match(full, compacted);
}
