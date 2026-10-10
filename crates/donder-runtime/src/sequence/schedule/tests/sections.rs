use super::*;
use crate::dsl::bytecode::FloatBinary;
use donder_runtime_types::Color;

fn sequence(
    selected: bool,
    per_fixture: bool,
    second_count: usize,
    operator: bool,
) -> PreparedSequence {
    // rgb(section_index(3) / 10, section_count(3) / 10, 0), with the
    // section queries in the body.
    let tenth = |dst| Instruction::FloatBinary {
        op: FloatBinary::Divide,
        dst,
        a: dst,
        b: Slot::scalar(0),
    };
    let sample = SampleProgram::admit(program(
        vec![
            Instruction::IntConst {
                dst: Slot::scalar(0),
                value: 3,
            },
            Instruction::FloatConst {
                dst: Slot::scalar(0),
                bits: 10.0_f32.to_bits(),
            },
            Instruction::FloatConst {
                dst: Slot::scalar(1),
                bits: 0.0_f32.to_bits(),
            },
            Instruction::SectionIndex {
                dst: Slot::row(0),
                width: Slot::scalar(0),
            },
            Instruction::SectionCount {
                dst: Slot::row(1),
                width: Slot::scalar(0),
            },
            Instruction::IntToFloat {
                dst: Slot::row(0),
                a: Slot::row(0),
            },
            Instruction::IntToFloat {
                dst: Slot::row(1),
                a: Slot::row(1),
            },
            tenth(Slot::row(0)),
            tenth(Slot::row(1)),
            Instruction::Rgb {
                dst: Slot::row(0),
                red: Slot::row(0),
                green: Slot::row(1),
                blue: Slot::scalar(1),
            },
        ],
        3,
        Slot::row(0),
        banks(2, 1, 0),
        banks(2, 2, 1),
    ))
    .unwrap();
    let operator = operator.then(|| {
        let operator = OperatorProgram::admit(sample.clone().into_bytecode(), 1).unwrap();
        OperatorInvocation::bind(operator, vec![]).unwrap()
    });
    let invocation = SampleInvocation::bind(sample, vec![]).unwrap();
    let timing = SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        let fixtures = [(7, true), (second_count, false)].map(|(count, routed)| {
            let geometry =
                FixtureGeometry::admit((0..count).map(|index| [index as f32, 0.0]).collect())
                    .unwrap();
            builder.fixture(
                count as u32,
                if selected {
                    geometry.select(|cell| routed && (2..6).contains(&cell))
                } else {
                    geometry
                },
            )
        });
        let scope = if per_fixture {
            TargetScope::PerFixture
        } else {
            TargetScope::WholeTarget
        };
        let target = builder.target([fixtures[1], fixtures[0]], scope);
        let window = builder.whole_sequence();
        let effect = builder.sample(&invocation, window, target);
        builder.clip(7, effect);
        let layer = builder.layer(true, [effect]);
        let layer = operator
            .as_ref()
            .map_or(layer, |operator| builder.operator(operator, |_| layer));
        let route = builder.target([fixtures[0]], TargetScope::PerFixture);
        let route = builder.target_slice(route, if selected { 0..4 } else { 2..6 });
        let port = builder.port(0, 0);
        builder.route(port, route, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        builder.output([layer])
    })
}

fn encoded(index: usize, count: usize) -> Color {
    Color {
        red: (index as f32 / 10.0 * 255.0 + 0.5) as u8,
        green: (count as f32 / 10.0 * 255.0 + 0.5) as u8,
        blue: 0,
    }
}

#[test]
fn section_queries_preserve_fixture_scope_short_sections_and_selected_routes() {
    for per_fixture in [false, true] {
        for second_count in [5_usize, 7] {
            let full = sequence(false, per_fixture, second_count, false);
            let second_sections = second_count.div_ceil(3);
            let expected: Vec<_> = (0..7)
                .map(|index| {
                    encoded(
                        index / 3 + if per_fixture { 0 } else { second_sections },
                        if per_fixture { 3 } else { 3 + second_sections },
                    )
                })
                .chain((0..second_count).map(|index| {
                    encoded(
                        index / 3,
                        if per_fixture {
                            second_sections
                        } else {
                            3 + second_sections
                        },
                    )
                }))
                .collect();
            let mut raster = full.clip(7).unwrap().sampler(7 + second_count);
            assert_eq!(raster.evaluate(SampleTime::from_ticks(0)), expected);
            let bytes =
                crate::archive::encode_sequence(&sequence(true, per_fixture, second_count, false))
                    .unwrap();
            let selected =
                crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default())
                    .unwrap();
            let mut full = full.into_playback();
            let mut selected = selected.into_playback();
            for ticks in [0, 500_000, 0] {
                let time = SampleTime::from_ticks(ticks);
                let frame = full.evaluate(time);
                assert_eq!(frame.colors(), expected);
                assert!(frame.outputs().eq(selected.evaluate(time).outputs()));
            }
        }
    }
}

#[test]
fn section_queries_in_operators_keep_original_fixture_context_with_selected_storage() {
    let mut full = sequence(false, false, 5, true).into_playback();
    let mut selected = sequence(true, false, 5, true).into_playback();
    let expected: Vec<_> = (0..7)
        .map(|index| encoded(index / 3, 3))
        .chain((0..5).map(|index| encoded(index / 3, 2)))
        .collect();
    let frame = full.evaluate(SampleTime::from_ticks(0));
    assert_eq!(frame.colors(), expected);
    assert!(
        frame
            .outputs()
            .eq(selected.evaluate(SampleTime::from_ticks(0)).outputs())
    );
}
