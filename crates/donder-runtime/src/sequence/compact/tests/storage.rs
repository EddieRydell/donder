use super::*;
use crate::values::Color;

#[derive(Clone, Copy)]
enum Selection {
    FirstFixture,
    SplitFixture,
    SharedFixture,
    UnpatchedPort,
}

fn selected_sequence(compacted: bool, selection: Selection, invert: bool) -> PreparedSequence {
    let mut cache = DslBindCache::default();
    let ramp = SampleProgram::admit(
        program(
            vec![
                Instruction::LoadFloatConst {
                    dst: FloatSlot(0),
                    bits: 0.0f32.to_bits(),
                },
                Instruction::ContextRead {
                    dst: NumberSlot::Float(FloatSlot(1)),
                    read: ContextRead::PixelFraction,
                },
                Instruction::Rgb {
                    dst: ColorSlot(0),
                    red: FloatSlot(1),
                    green: FloatSlot(0),
                    blue: FloatSlot(0),
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ],
            SlotLayout {
                floats: 2,
                colors: 1,
                ..Default::default()
            },
            1,
        ),
        Box::new([]),
    )
    .unwrap();
    let green = SampleProgram::admit(
        BytecodeProgram {
            uses_pixel_context: false,
            ..program(
                vec![
                    Instruction::LoadColorConst {
                        dst: ColorSlot(0),
                        value: Color {
                            red: 0,
                            green: 255,
                            blue: 0,
                        },
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    colors: 1,
                    ..Default::default()
                },
                0,
            )
        },
        Box::new([]),
    )
    .unwrap();
    let ramp = SampleDefinition::new(ramp)
        .bind(vec![], &mut cache)
        .unwrap();
    let green = SampleDefinition::new(green)
        .bind(vec![], &mut cache)
        .unwrap();
    let invert = invert.then(|| {
        let operator = OperatorProgram::admit(
            program(
                vec![
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(0)),
                        read: ContextRead::Seconds,
                    },
                    Instruction::SignalSample {
                        capability: (),
                        dst: ColorSlot(0),
                        input: 0,
                        seconds: FloatSlot(0),
                        pixel: SignalPixel::Current,
                        frame_cache: 0,
                    },
                    Instruction::ColorInvert {
                        dst: ColorSlot(1),
                        color: ColorSlot(0),
                    },
                    Instruction::ReturnColor(ColorSlot(1)),
                ],
                SlotLayout {
                    floats: 1,
                    colors: 2,
                    ..Default::default()
                },
                1,
            ),
            1,
            Box::new([]),
        )
        .unwrap();
        OperatorDefinition::new(operator)
            .bind(vec![], &mut cache)
            .unwrap()
    });
    let timing = SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        let fixtures = [10, 20].map(|id| {
            builder.fixture(
                id,
                FixtureGeometry::admit((0..113).map(|cell| [cell as f32, 0.0]).collect()).unwrap(),
            )
        });
        let a = builder.target([fixtures[0]], TargetScope::PerFixture);
        let b = builder.target([fixtures[1]], TargetScope::PerFixture);
        let window = builder.whole_sequence();
        let red = builder.sample(&ramp, window, a);
        let green = builder.sample(&green, window, b);
        let red = builder.layer(true, [red]);
        let green = builder.layer(true, [green]);
        let signal = match &invert {
            // The red layer is disconnected; the green layer targets an unselected fixture.
            Some(operator) => builder.operator(operator, |_| green),
            None => builder.mix([red, green]),
        };
        let first = builder.port(0, 1);
        match selection {
            Selection::FirstFixture => {
                builder.route(first, a, OutputEncoding::Rgb(RgbOrder::Rgb), None)
            }
            Selection::SplitFixture => {
                let low = builder.target_slice(a, 0..37);
                let high = builder.target_slice(a, 76..113);
                builder.route(first, low, OutputEncoding::Rgb(RgbOrder::Rgb), None);
                let second = builder.port(1, 2);
                builder.route(second, high, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            }
            Selection::SharedFixture => {
                builder.route(first, a, OutputEncoding::Rgb(RgbOrder::Rgb), None);
                let second = builder.port(1, 2);
                builder.route(second, a, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            }
            Selection::UnpatchedPort => {}
        }
        if compacted {
            builder.compact_to_outputs();
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
fn selected_fixture_removes_unneeded_programs_and_target_records() {
    let full = selected_sequence(false, Selection::FirstFixture, false);
    let compacted = selected_sequence(true, Selection::FirstFixture, false);
    let before = full.archive_data().signals;
    let after = compacted.archive_data().signals;
    assert_eq!(before.programs.len(), 2);
    assert_eq!(after.programs.len(), 1);
    assert_eq!(after.programs[0], before.programs[0]);
    assert_eq!(before.effects.len(), 2);
    assert_eq!(after.effects.len(), 1);
    assert!(after.target_pixels.len() < before.target_pixels.len());
    assert_eq!(before.pixel_count, 226);
    assert_eq!(after.pixel_count, 113);
    assert_outputs_match(full, compacted);
}

#[test]
fn disjoint_spans_remap_storage_without_changing_logical_coordinates() {
    let full = selected_sequence(false, Selection::SplitFixture, false);
    let compacted = selected_sequence(true, Selection::SplitFixture, false);
    let data = compacted.archive_data();
    let target = data.signals.target(data.signals.plan.target);
    assert_eq!(data.signals.pixel_count, 74);
    assert_eq!(target.len(), 74);
    for (physical, logical) in (0..37).chain(76..113).enumerate() {
        assert_eq!(target[physical].fixture_pixel_index, physical as u32);
        assert_eq!(target[physical].pixel_index, logical);
        assert_eq!(target[physical].pixel_count, 113);
        assert_eq!(target[physical].pixel_fraction, logical as f32 / 112.0);
    }
    assert_eq!(target[37].fixture_pixel_index, 37);
    assert_eq!(target[37].pixel_index, 76);
    assert_eq!(data.patch.routes.len(), 2);
    assert_eq!(data.patch.routes[0].pixels, 0..37);
    assert_eq!(data.patch.routes[1].pixels, 37..74);
    assert_outputs_match(full, compacted);
}

#[test]
fn shared_fixture_keeps_both_routes_and_unpatched_port_keeps_none() {
    let full = selected_sequence(false, Selection::SharedFixture, false);
    let compacted = selected_sequence(true, Selection::SharedFixture, false);
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

    let full = selected_sequence(false, Selection::UnpatchedPort, false);
    assert_eq!(full.archive_data().signals.programs.len(), 2);
    let compacted = selected_sequence(true, Selection::UnpatchedPort, false);
    let data = compacted.archive_data();
    assert_eq!(data.outputs.len(), 1);
    assert_eq!(data.outputs[0].width, 0);
    assert!(data.patch.routes.is_empty());
    assert!(data.signals.fixtures.is_empty());
    assert!(data.signals.programs.is_empty());
    assert!(data.signals.effects.is_empty());
    assert!(data.signals.target_pixels.is_empty());
    assert_outputs_match(full, compacted);
}

#[test]
fn pruning_upstream_samples_keeps_invert_program_and_white_output() {
    let full = selected_sequence(false, Selection::FirstFixture, true);
    let compacted = selected_sequence(true, Selection::FirstFixture, true);
    let before = full.archive_data().signals;
    let after = compacted.archive_data().signals;
    assert_eq!(before.programs.len(), 3);
    assert_eq!(before.effects.len(), 2);
    assert!(after.effects.is_empty());
    assert_eq!(after.programs.len(), 1);
    assert!(
        after.programs[0]
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::ColorInvert { .. }))
    );
    let operators: Vec<_> = after
        .plan
        .nodes
        .iter()
        .filter_map(|node| match &node.kind {
            PreparedSignalKind::Operator {
                operator, inputs, ..
            } => Some((operator, inputs)),
            _ => None,
        })
        .collect();
    assert_eq!(operators.len(), 1);
    assert_eq!(operators[0].0.program, 0);
    assert_eq!(operators[0].1.len(), 1);
    assert!(matches!(
        after.plan.nodes[operators[0].1[0]].kind,
        PreparedSignalKind::Layer { .. }
    ));
    assert!(
        after
            .effects_by_layer
            .iter()
            .all(|effects| effects.is_empty())
    );
    let mut playback = compacted.clone().into_playback();
    let frame = playback.evaluate(SampleTime::from_ticks(500_000));
    let bytes = frame.outputs().next().unwrap().bytes;
    assert_eq!(bytes.len(), 339);
    assert!(bytes.iter().all(|&channel| channel == 255));
    assert_outputs_match(full, compacted);
}
