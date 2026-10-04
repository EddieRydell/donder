use super::playback;
use super::std;
use donder_language::dsl::{OperatorDefinition, Value, compile_effects, compile_operators};
use donder_language::values::SampleTime;
use std::prelude::rust_2024::*;

#[test]
fn fusion_preserves_source_order_errors_and_conditional_queries() {
    use super::evaluation::{OperatorEvaluation, context};
    use crate::dsl::{BatchWorkspace, RuntimeError};
    use crate::tests::evaluation::SignalSampler;
    use donder_language::dsl::bytecode::SignalPixel;
    use donder_language::execution::SpatialContext;
    use donder_language::values::Color;
    struct Observe {
        calls: Vec<(usize, u32)>,
        fail: Option<usize>,
    }
    impl SignalSampler for Observe {
        fn sample_signal(
            &mut self,
            input: usize,
            time: SampleTime,
            _: SignalPixel<i32>,
            _: Option<usize>,
        ) -> Result<Color, RuntimeError> {
            self.calls.push((input, time.as_ticks()));
            if self.fail == Some(input) {
                Err(RuntimeError {
                    message: "source failed".into(),
                })
            } else {
                Ok(Color::BLACK)
            }
        }
    }
    let definitions = compile_operators(
        "operator Inner { input Signal first; input Signal second;
        color sample() {
            color value = first.at(seconds());
            if (pixel_index() == 0) { return value; }
            return value + second.at(seconds() + 0.25);
        }
    }
    operator Outer { input Signal other; input Signal source; param float at = 1.0;
        color sample() {
            if (pixel_index() == -1) { return #000000; }
            color value = other.at(0.25);
            value = value + source.at(at);
            return value + other.at(0.5);
        }
    }",
    )
    .unwrap();
    let source = definitions[0].bind([]).unwrap();
    for (seconds, valid) in [(1.0, true), (-1.0, false), (f32::NAN, false), (8.0, false)] {
        let caller = OperatorDefinition::new(definitions[1].program().clone())
            .bind(vec![Value::Float(seconds)])
            .unwrap();
        let fused = caller.fuse_input(1, &source).unwrap();
        for pixel in [0, 1] {
            for fail in [None, Some(0), Some(1), Some(2)] {
                let mut expected = vec![(0, 250_000)];
                if valid {
                    expected.push((1, 1_000_000));
                    if pixel == 1 {
                        expected.push((2, 1_250_000));
                    }
                }
                expected.push((0, 500_000));
                // A failed query is reported after the program finishes.
                let error = expected.iter().position(|(input, _)| Some(*input) == fail);
                let mut sampler = Observe {
                    calls: Vec::new(),
                    fail,
                };
                let result = fused.evaluate(
                    &context(2, pixel, 0),
                    &SpatialContext {
                        position: [0.0; 2],
                        min: [0.0; 2],
                        max: [0.0; 2],
                    },
                    &mut sampler,
                    &mut BatchWorkspace::default(),
                );
                assert_eq!(result.is_err(), error.is_some());
                assert_eq!(
                    sampler.calls, expected,
                    "seconds={seconds} pixel={pixel} fail={fail:?}"
                );
            }
        }
    }
}

#[test]
fn fusion_removes_loop_storage_when_the_source_makes_a_branch_unreachable() {
    let definitions = compile_operators(
        "operator Black { input Signal source; color sample() { return #000000; } }
    operator Conditional { input Signal source; color sample() {
        color value = source.at(seconds());
        if (value != #000000) {
            for (int i = 0; i < 3; i = i + 1) { value = value + #010203; }
        }
        return value;
    } }",
    )
    .unwrap();
    let fused = definitions[1]
        .bind([])
        .unwrap()
        .fuse_input(0, &definitions[0].bind([]).unwrap())
        .unwrap();
    assert_eq!(fused.program().bytecode().loop_count, 0);
    let source = compile_effects("effect Bright { color sample() { return #ffffff; } }")
        .unwrap()
        .remove(0)
        .bind([])
        .unwrap();
    assert!(
        playback::chain(9, &source, 1, &[fused])
            .into_playback()
            .evaluate(SampleTime::from_ticks(0))
            .colors()
            .iter()
            .all(|c| *c == donder_language::values::Color::BLACK)
    );
}

#[test]
fn fusion_preserves_quantized_source_clocks_and_invalid_query_black() {
    let effect = compile_effects(
        "effect Source { color sample() {
        return rgb(pixel_fraction(), seconds() - floor(seconds()), progress());
    } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let sources = compile_operators("operator Clock { input Signal source;
        color sample() { return max(source.at(seconds() + 0.0000003), rgb(progress(), seconds() - floor(seconds()), 0.1)); }
    }
    operator Constant { input Signal source; color sample() { return #123456; } }").unwrap();
    let caller = compile_operators(
        "operator Query { input Signal source;
        param float at = 0.0;
        color sample() { return source.at(at); }
    }",
    )
    .unwrap()
    .remove(0);
    for source in sources {
        let source = source.bind([]).unwrap();
        for seconds in [
            -1.0,
            -0.0000003,
            -0.0,
            0.0,
            0.0000003,
            0.0000006,
            3.0000002,
            7.9999995,
            8.0,
            3456.789,
            4000.0,
            4294.968,
            f32::NAN,
            f32::INFINITY,
        ] {
            let caller = OperatorDefinition::new(caller.program().clone())
                .bind(vec![Value::Float(seconds)])
                .unwrap();
            let fused = caller.fuse_input(0, &source).expect("one compatible query");
            for duration in [8_000_000, 4_000_000_001] {
                for count in [1, 7, 8, 9, 17] {
                    let build = |fuse| {
                        playback::build(count, playback::timing(duration), |b, target| {
                            let effect = b.sample(&effect, b.whole_sequence(), target);
                            let layer = b.layer(true, [effect]);
                            let output = if fuse {
                                b.operator(&fused, |_| layer)
                            } else {
                                let upstream = b.operator(&source, |_| layer);
                                b.operator(&caller, |_| upstream)
                            };
                            b.output([output])
                        })
                    };
                    let mut original = build(false).into_playback();
                    let mut composed = build(true).into_playback();
                    for ticks in [0, 1, 3_000_001, duration - 1, 1] {
                        let time = SampleTime::from_ticks(ticks);
                        assert_eq!(
                            original.evaluate(time).colors(),
                            composed.evaluate(time).colors(),
                            "seconds={seconds} duration={duration} count={count} ticks={ticks}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fusion_remaps_parameters_inputs_loops_and_early_returns() {
    let definitions = compile_operators(
        "operator Inner { input Signal a; input Signal b;
        param float gain = 0.7; param bool first = true; param color tint = #314159;
        color sample() {
            if (pixel_index() % 3 == 0) { return tint; }
            color value = #000000;
            for (int i = 0; i < 3; i = i + 1) {
                if (first) { value = max(value, a.at(seconds()) * gain); }
                else { value = max(value, b.at(seconds()) * gain); }
            }
            return value;
        }
    }
    operator Outer { input Signal other; input Signal source;
        param float gain = 0.4;
        color sample() {
            if (pixel_index() == -1) { return #000000; }
            return max(source.at(seconds()) * gain, other.at(seconds()));
        }
    }",
    )
    .unwrap();
    let outer = definitions[1].bind([]).unwrap();
    let colors = ["#e03040", "#2080c0", "#010203"].map(|color| {
        compile_effects(&format!(
            "effect Color {{ color sample() {{ return {color}; }} }}"
        ))
        .unwrap()
        .remove(0)
        .bind([])
        .unwrap()
    });
    for first in [false, true] {
        let inner = OperatorDefinition::new(definitions[0].program().clone())
            .bind(vec![
                Value::Float(0.7),
                Value::Bool(first),
                Value::Color(donder_language::values::Color {
                    red: 49,
                    green: 65,
                    blue: 89,
                }),
            ])
            .unwrap();
        let fused = outer
            .fuse_input(1, &inner)
            .expect("numeric source with uniform query times");
        assert_eq!(fused.program().input_count(), 3);
        let build = |fuse| {
            playback::build(17, playback::timing(8_000_000), |b, target| {
                let layers = colors.each_ref().map(|effect| {
                    let sample = b.sample(effect, b.whole_sequence(), target);
                    b.layer(true, [sample])
                });
                let output = if fuse {
                    b.operator(&fused, |input| [layers[2], layers[0], layers[1]][input])
                } else {
                    let inner = b.operator(&inner, |input| layers[input]);
                    b.operator(&outer, |input| [layers[2], inner][input])
                };
                b.output([output])
            })
        };
        let mut original = build(false).into_playback();
        let mut composed = build(true).into_playback();
        for ticks in [0, 1, 3_000_001, 7_999_999, 0] {
            let time = SampleTime::from_ticks(ticks);
            assert_eq!(
                original.evaluate(time).colors(),
                composed.evaluate(time).colors()
            );
        }
    }
}

#[test]
fn fusion_preserves_resource_banks_and_array_snapshots() {
    let definitions = compile_operators(
        "operator Inner { input Signal source;
        param array<float> levels = [0.2, 0.8]; param curve shape;
        param gradient colors; param enum mode { first, second } = second;
        fixed param marks beats;
        color sample() {
            array<float> values = [levels[0], value_or(shape[progress()], 0.3), levels[1]];
            array<float> saved = values; values = [0.0];
            float gain = saved[pixel_index() % 3];
            if (mode == first) { gain = gain * 0.5; }
            for (int i in beats) { gain = gain + mark_at(beats, i) * 0.01; }
            return max(source.at(seconds()) * gain, colors[progress()]);
        }
    }
    operator Outer { input Signal source;
        param array<float> unused = [0.4]; param curve unused_shape;
        param gradient unused_colors; param enum unused_mode { first, second } = first;
        fixed param marks unused_beats;
        color sample() { return source.at(seconds()) * 0.7; }
    }",
    )
    .unwrap();
    let base = compile_effects(
        "effect Source { color sample() { return rgb(0.8, pixel_fraction(), progress()); } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    use donder_language::dsl::Identifier;
    use donder_language::values::{
        Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration,
    };
    let inner_values = [
        (
            Identifier::new("shape".into()).unwrap(),
            Value::Curve(
                Curve {
                    points: vec![
                        CurvePoint {
                            position: 0.0,
                            value: 0.1,
                        },
                        CurvePoint {
                            position: 1.0,
                            value: 0.9,
                        },
                    ],
                }
                .into(),
            ),
        ),
        (
            Identifier::new("colors".into()).unwrap(),
            Value::Gradient(
                Gradient {
                    stops: vec![
                        GradientStop {
                            position: 0.0,
                            color: Color {
                                red: 4,
                                green: 8,
                                blue: 12,
                            },
                        },
                        GradientStop {
                            position: 1.0,
                            color: Color {
                                red: 4,
                                green: 16,
                                blue: 2,
                            },
                        },
                    ],
                }
                .into(),
            ),
        ),
        (
            Identifier::new("beats".into()).unwrap(),
            Value::Marks(
                Marks::new(vec![
                    SampleDuration::from_ticks(1_000_000),
                    SampleDuration::from_ticks(2_000_000),
                ])
                .into(),
            ),
        ),
    ];
    let outer_values = [
        (
            Identifier::new("unused_shape".into()).unwrap(),
            Value::Curve(Curve { points: vec![] }.into()),
        ),
        (
            Identifier::new("unused_colors".into()).unwrap(),
            Value::Gradient(Gradient { stops: vec![] }.into()),
        ),
        (
            Identifier::new("unused_beats".into()).unwrap(),
            Value::Marks(Marks::EMPTY.into()),
        ),
    ];
    let inner = definitions[0]
        .bind(inner_values.iter().map(|(k, v)| (k, v)))
        .unwrap();
    let outer = definitions[1]
        .bind(outer_values.iter().map(|(k, v)| (k, v)))
        .unwrap();
    let fused = outer
        .fuse_input(0, &inner)
        .expect("fusion preserves array snapshots");
    let mut original = playback::chain(17, &base, 1, &[inner, outer]).into_playback();
    let mut composed = playback::chain(17, &base, 1, &[fused]).into_playback();
    for ticks in [0, 1, 3_000_001, 7_999_999, 1] {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            original.evaluate(time).colors(),
            composed.evaluate(time).colors()
        );
    }
}

#[test]
fn fusion_keeps_caller_automation_and_preserves_source_automation_boundaries() {
    use donder_language::automation::AutomationMapping;
    use donder_language::execution::PreparedAutomation;
    use donder_language::values::{Curve, CurvePoint, SampleDuration};
    let definitions = compile_operators(
        "operator Gain { input Signal source; param float gain = 0.7;
        color sample() { return source.at(seconds()) * gain; }
    }
    operator Delayed { input Signal source; param float gain = 0.4;
        color sample() { return source.at(seconds() - 0.125) * gain; }
    }",
    )
    .unwrap();
    let automation = vec![PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        param_index: 0,
        curve: Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 1.0,
                },
            ],
        }
        .into(),
        mapping: AutomationMapping::Float { min: 0.2, max: 0.9 },
    }]
    .into_boxed_slice();
    let inner = definitions[0].bind([]).unwrap();
    let outer = definitions[1]
        .bind([])
        .unwrap()
        .with_automation(automation.clone())
        .unwrap();
    assert!(
        outer
            .fuse_input(0, &inner.clone().with_automation(automation).unwrap())
            .is_none()
    );
    let fused = outer.fuse_input(0, &inner).unwrap();
    let base = compile_effects(
        "effect Source { color sample() { return rgb(0.8, pixel_fraction(), progress()); } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let mut original = playback::chain(17, &base, 1, &[inner, outer]).into_playback();
    let mut composed = playback::chain(17, &base, 1, &[fused]).into_playback();
    for ticks in [0, 124_999, 125_000, 125_001, 3_000_001, 7_999_999, 125_000] {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            original.evaluate(time).colors(),
            composed.evaluate(time).colors()
        );
    }
}
