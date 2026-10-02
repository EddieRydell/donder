use super::*;
use crate::dsl::bytecode::ArithmeticOp;
use crate::values::Color;

fn sequence(
    compacted: bool,
    per_fixture: bool,
    second_count: usize,
    operator: bool,
) -> PreparedSequence {
    let sample = SampleProgram::admit(
        program(
            vec![
                Instruction::LoadIntConst {
                    dst: IntSlot(0),
                    value: 3,
                },
                Instruction::SectionQuery {
                    dst: IntSlot(1),
                    width: IntSlot(0),
                    index: true,
                },
                Instruction::SectionQuery {
                    dst: IntSlot(2),
                    width: IntSlot(0),
                    index: false,
                },
                Instruction::IntToFloat {
                    dst: FloatSlot(0),
                    src: IntSlot(1),
                },
                Instruction::IntToFloat {
                    dst: FloatSlot(1),
                    src: IntSlot(2),
                },
                Instruction::FloatArithmeticConst {
                    dst: FloatSlot(0),
                    op: ArithmeticOp::Divide,
                    value: FloatSlot(0),
                    constant_bits: 10.0_f32.to_bits(),
                    constant_left: false,
                },
                Instruction::FloatArithmeticConst {
                    dst: FloatSlot(1),
                    op: ArithmeticOp::Divide,
                    value: FloatSlot(1),
                    constant_bits: 10.0_f32.to_bits(),
                    constant_left: false,
                },
                Instruction::LoadFloatConst {
                    dst: FloatSlot(2),
                    bits: 0.0_f32.to_bits(),
                },
                Instruction::Rgb {
                    dst: ColorSlot(0),
                    red: FloatSlot(0),
                    green: FloatSlot(1),
                    blue: FloatSlot(2),
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ],
            SlotLayout {
                ints: 3,
                floats: 3,
                colors: 1,
                ..Default::default()
            },
            1,
        ),
        Box::new([]),
    )
    .unwrap();
    let operator = operator.then(|| {
        let operator =
            OperatorProgram::admit(sample.clone().into_parts().0, 1, Box::new([])).unwrap();
        OperatorDefinition::new(operator)
            .bind(vec![], &mut DslBindCache::default())
            .unwrap()
    });
    let invocation = SampleDefinition::new(sample)
        .bind(vec![], &mut DslBindCache::default())
        .unwrap();
    let timing = SequenceTiming::admit(
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(60).unwrap(),
        NonZeroU32::new(1_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        let fixtures = [7, second_count].map(|count| {
            builder.fixture(
                count as u32,
                FixtureGeometry::admit((0..count).map(|index| [index as f32, 0.0]).collect())
                    .unwrap(),
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
        let route = builder.target_slice(route, 2..6);
        let port = builder.port(0, 0);
        builder.route(port, route, OutputEncoding::Rgb(RgbOrder::Rgb), None);
        if compacted {
            builder.compact_to_outputs();
        }
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
                crate::wire::encode_sequence(&sequence(true, per_fixture, second_count, false))
                    .unwrap();
            let selected =
                crate::wire::decode_sequence(&bytes, crate::wire::LoadLimits::default()).unwrap();
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
fn section_queries_in_operators_keep_original_fixture_context_after_compaction() {
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
