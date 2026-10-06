use super::evaluation::{SampleEvaluation, context};
use super::std;
use std::prelude::rust_2024::*;

use crate::dsl::{BatchWorkspace, BoundParams, DslBindCache, sample_once};
use donder_language::dsl::bytecode::Instruction;
use donder_language::dsl::{ProgramConstants, SampleDefinition, Value, compile_effects};
use donder_language::execution::SpatialContext;

use super::playback;

fn uncached(
    operator: &donder_language::dsl::OperatorInvocation,
) -> donder_language::dsl::OperatorInvocation {
    use donder_language::dsl::bytecode::{ContextRead, FloatSlot, NumberSlot};
    use donder_language::dsl::{OperatorDefinition, OperatorProgram};
    let (mut bytecode, inputs, parameters) = operator.program().as_ref().clone().into_parts();
    let mut code = bytecode.instructions.into_vec();
    // An unreachable geometry read preserves executed instructions and branch
    // targets; without frame caches every query samples its run upstream.
    code.push(Instruction::ContextRead {
        dst: NumberSlot::Float(FloatSlot(bytecode.layout.floats)),
        read: ContextRead::PixelFraction,
    });
    for op in &mut code {
        if let Instruction::SignalSample { frame_cache, .. } = op {
            *frame_cache = u32::MAX;
        }
    }
    bytecode.layout.floats += 1;
    bytecode.instructions = code.into();
    bytecode.uses_pixel_context = true;
    let reference_operator = OperatorProgram::admit(bytecode, inputs, parameters).unwrap();
    OperatorDefinition::new(reference_operator)
        .bind(operator.params().iter_values().collect())
        .unwrap()
        .with_automation(operator.automation().into())
        .unwrap()
}

#[test]
fn frame_caches_match_run_sampling_for_temporal_loops_tails_seeks_and_nested_graphs() {
    use donder_language::dsl::compile_operators;
    let effect = compile_effects(
        "effect Source {
        color sample() { return rgb(pixel_fraction(), seconds() * 0.1, progress()); }
    }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let operator = compile_operators(
        "operator Temporal {
        input Signal source;
        color sample() {
            color result = rgb(0.1, 0.2, 0.3);
            for (int i = 0; i < 3; i = i + 1) {
                float time = seconds() - i * 0.04;
                if (time >= 0.0) {
                    result = mix(invert(result), source.at(time) * (1.0 - i * 0.2), 0.25);
                }
            }
            return result;
        }
    }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let reference_operator = uncached(&operator);
    for count in [1, 7, 8, 9, 17] {
        let mut blocked = playback::chain(count, &effect, 2, &[operator.clone(), operator.clone()])
            .into_playback();
        let mut reference = playback::chain(
            count,
            &effect,
            2,
            &[reference_operator.clone(), reference_operator.clone()],
        )
        .into_playback();
        for ticks in [0, 1, 39_999, 40_000, 90_000, 3_083_333, 0, 7_999_999] {
            let time = donder_language::values::SampleTime::from_ticks(ticks);
            assert_eq!(
                blocked.evaluate(time).colors(),
                reference.evaluate(time).colors(),
                "count={count} ticks={ticks}"
            );
        }
    }
}

#[test]
fn frame_caches_preserve_control_arrays_loops_and_seeks() {
    let effect = compile_effects(
        "effect Source { color sample() {
        return hsv(pixel_fraction() + seconds() * 0.03, 0.7, 0.8);
    } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let outer = donder_language::dsl::compile_operators(playback::IDENTITY_SOURCE)
        .unwrap()
        .remove(0)
        .bind([])
        .unwrap();
    for source in [
        "operator P { input Signal source; color sample() {
            color c = source.at(seconds() - 0.04);
            return hsv(hue(c) + pixel_fraction(), saturation(c), intensity(c));
        } }",
        "operator P { input Signal source; color sample() {
            if (pixel_index() % 3 == 0) { return #123456; }
            color c = source.at(seconds() + 0.04);
            if (intensity(c) > 0.5) { return invert(c); } return c;
        } }",
        "operator P { input Signal source; color sample() {
            color result = #000000;
            for (int i in range(pixel_index() % 4)) {
                color c = source.at(seconds());
                array<float> channels = [hue(c), saturation(c), intensity(c)];
                result = max(result, hsv(channels[0] + i * 0.1, channels[1], channels[2]));
            } return result;
        } }",
    ] {
        let operator = donder_language::dsl::compile_operators(source)
            .unwrap()
            .remove(0)
            .bind([])
            .unwrap();
        let reference_operator = uncached(&operator);
        for count in [1, 7, 8, 9, 17] {
            let mut blocked = playback::chain(
                count,
                &effect,
                2,
                &[operator.clone(), operator.clone(), outer.clone()],
            )
            .into_playback();
            let mut reference = playback::chain(
                count,
                &effect,
                2,
                &[
                    reference_operator.clone(),
                    reference_operator.clone(),
                    outer.clone(),
                ],
            )
            .into_playback();
            for ticks in [
                0, 1, 39_999, 40_000, 3_083_333, 0, 7_959_999, 7_999_999, 8_000_000,
            ] {
                let time = donder_language::values::SampleTime::from_ticks(ticks);
                assert_eq!(
                    blocked.evaluate(time).colors(),
                    reference.evaluate(time).colors(),
                    "count={count} ticks={ticks} source={source}"
                );
            }
        }
    }
}

#[test]
fn batches_preserve_local_global_and_subset_target_addressing() {
    use donder_language::dsl::compile_operators;
    use donder_language::execution::{FixtureGeometry, OutputEncoding, RgbOrder, TargetScope};
    let source = compile_effects(
        "effect Source { color sample() {
        return rgb(pixel_fraction(), seconds() * 0.2, 0.5);
    } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let operator = compile_operators(
        "operator Address { input Signal source; color sample() {
        return max(source.at(seconds() + 0.1),
            mix(source.at(seconds() * 0.5, 1), source.at_global(seconds(), 4), 0.3));
    } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let reference_operator = uncached(&operator);
    let build = |op: &donder_language::dsl::OperatorInvocation| {
        crate::PreparedSequence::build(playback::timing(4_000_000), |builder| {
            let left = builder.fixture(
                0,
                FixtureGeometry::admit((0..3).map(|i| [i as f32, 0.0]).collect()).unwrap(),
            );
            let right = builder.fixture(
                1,
                FixtureGeometry::admit((0..7).map(|i| [i as f32, 1.0]).collect()).unwrap(),
            );
            let full = builder.target([left, right], TargetScope::PerFixture);
            let subset = builder.target([right], TargetScope::WholeTarget);
            let port = builder.port(0, 0);
            builder.route(port, full, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            let effect = builder.sample(&source, builder.whole_sequence(), subset);
            let layer = builder.layer(true, [effect]);
            let signal = builder.operator(op, |_| layer);
            builder.output([signal])
        })
        .into_playback()
    };
    let mut blocked = build(&operator);
    let mut reference = build(&reference_operator);
    for ticks in [0, 1, 333_333, 3_900_000, 3_999_999, 4_000_000, 0] {
        let time = donder_language::values::SampleTime::from_ticks(ticks);
        assert_eq!(
            blocked.evaluate(time).colors(),
            reference.evaluate(time).colors(),
            "ticks={ticks}"
        );
    }
}

#[test]
fn constant_colors_use_the_same_quantization_as_runtime_expressions() {
    let dynamic = compile_effects(
        "effect Color { param float amount in -1.0..2.0 = 0.0; color sample() {
        return mix(invert(rgb(0.25, -0.2, 1.5)), hsv(0.4, 0.7, 0.8), amount) * amount;
    } }",
    )
    .unwrap()
    .remove(0);
    for (literal, amount) in [
        ("-0.2", -0.2),
        ("0.3", 0.3),
        ("1.5", 1.5),
        ("0.0 / 0.0", f32::NAN),
    ] {
        let source = format!(
            "effect Color {{ color sample() {{
            return mix(invert(rgb(0.25, -0.2, 1.5)), hsv(0.4, 0.7, 0.8), {literal}) * ({literal});
        }} }}"
        );
        let folded = compile_effects(&source).unwrap().remove(0);
        assert!(matches!(
            folded.sample_program().bytecode().instructions.as_ref(),
            [
                Instruction::LoadColorConst { .. },
                Instruction::ReturnColor(_)
            ]
        ));
        let runtime = SampleDefinition::new(dynamic.sample_program().clone())
            .bind(vec![Value::Float(amount)])
            .unwrap();
        assert_eq!(
            folded.bind([]).unwrap().evaluate(
                &context(8, 3, 0),
                &SPATIAL,
                &mut BatchWorkspace::default()
            ),
            runtime.evaluate(&context(8, 3, 0), &SPATIAL, &mut BatchWorkspace::default())
        );
    }
}

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [1.0; 2],
};

#[test]
fn prepared_binding_arithmetic_keeps_shared_code_and_dynamic_inputs() {
    let effect = compile_effects(
        "effect Prepared {
            param float width in -1.0..2.0 = 0.3;
            param float gain in 0.0..1.0 = 0.5;
            color sample() {
                float scale = 1.0 / max(abs(width), 0.01);
                return rgb(pixel_fraction() * scale * gain, clamp(width, 0.0, 1.0), seconds() * 0.1);
            }
        }",
    ).unwrap().remove(0);
    let mut previous = None;
    for width in [-0.4, 0.0, 0.2, 2.0] {
        let params = effect
            .sample_program()
            .bind(vec![Value::Float(width), Value::Float(0.5)])
            .unwrap();
        let (program, prepared) = effect
            .sample_program()
            .prepare_bindings(&params, |index| index == 1);
        let (again, again_params) = program.prepare_bindings(&prepared, |index| index == 1);
        assert_eq!(again, program, "repeated preparation must not grow code");
        assert_eq!(again_params.types(), prepared.types());
        assert_eq!(again_params.values(), prepared.values());
        if let Some(previous) = previous {
            assert_eq!(
                program, previous,
                "ordinary settings must retain program sharing"
            );
        }
        previous = Some(program.clone());
        for gain in [0.0, 0.25, 1.3] {
            let original = SampleDefinition::new(effect.sample_program().clone())
                .bind(vec![Value::Float(width), Value::Float(gain)])
                .unwrap();
            let mut values = prepared.iter_values().collect::<Vec<_>>();
            values[1] = Value::Float(gain);
            let optimized = SampleDefinition::new(program.clone()).bind(values).unwrap();
            for pixel in [0, 1, 4, 31] {
                for frame in [0, 1, 17] {
                    let context = context(32, pixel, frame);
                    assert_eq!(
                        original.evaluate(&context, &SPATIAL, &mut BatchWorkspace::default()),
                        optimized.evaluate(&context, &SPATIAL, &mut BatchWorkspace::default()),
                        "width={width} gain={gain} pixel={pixel} frame={frame}"
                    );
                }
            }
        }
    }
}

#[test]
fn prepared_divisors_share_code_and_preserve_special_values() {
    let effect = compile_effects(
        "effect Divide { param float divisor in -10.0..10.0 = 3.0; color sample() {
            float value = 0.0;
            for (int i in range(pixel_index() % 4)) {
                value = value + pixel_fraction() / divisor;
            }
            if (pixel_index() % 2 == 0) { return rgb(value_or(value, 0.7), 0.0, 0.0); }
            return rgb(value_or(pixel_fraction() / divisor, 0.7), 0.0, 0.0);
        } }",
    )
    .unwrap()
    .remove(0);
    let mut previous = None;
    for divisor in [
        3.0,
        -7.0,
        0.13,
        f32::MAX,
        0.0,
        -0.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::from_bits(1),
    ] {
        let original = SampleDefinition::new(effect.sample_program().clone())
            .bind(vec![Value::Float(divisor)])
            .unwrap();
        let (program, params) = effect
            .sample_program()
            .prepare_bindings(original.params(), |_| false);
        let safe = divisor.is_finite() && divisor != 0.0 && divisor.recip().is_finite();
        assert_eq!(
            program
                .bytecode()
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FloatDivide { .. })),
            !safe
        );
        if safe {
            if let Some(previous) = &previous {
                assert_eq!(&program, previous);
            }
            previous = Some(program.clone());
            let (again, again_params) = program.prepare_bindings(&params, |_| false);
            assert_eq!(again, program);
            assert_eq!(again_params.values(), params.values());
            assert_eq!(
                params.types().len(),
                1,
                "reuse the now-unused divisor binding: {:#?}",
                program.bytecode()
            );
        }
        let prepared = SampleDefinition::new(program)
            .bind(params.iter_values().collect())
            .unwrap();
        for count in [1, 7, 8, 9, 33] {
            let mut actual = playback::show(count, &prepared, 1).into_playback();
            let mut expected = playback::show(count, &original, 1).into_playback();
            for ticks in [0, 1, 3_000_000, 7_999_999, 0] {
                let time = donder_language::values::SampleTime::from_ticks(ticks);
                let actual = actual.evaluate(time);
                let expected = expected.evaluate(time);
                for (pixel, (a, b)) in actual.colors().iter().zip(expected.colors()).enumerate() {
                    assert!(
                        a.red.abs_diff(b.red) <= u8::from(safe),
                        "divisor={divisor} count={count} pixel={pixel}: {a:?} != {b:?}"
                    );
                    assert_eq!((a.green, a.blue), (b.green, b.blue));
                }
            }
        }
    }
}

#[test]
fn prepared_divisors_preserve_dynamic_and_pixel_mutated_bindings() {
    for source in [
        "effect Divide { param float divisor in -10.0..10.0 = 3.0; color sample() {
            return rgb(value_or(pixel_fraction() / divisor, 0.7), 0.0, 0.0);
        } }",
        "effect Divide { param float divisor in -10.0..10.0 = 3.0; color sample() {
            float current = divisor;
            if (pixel_index() % 2 == 0) { current = pixel_fraction(); }
            return rgb(value_or(pixel_fraction() / current, 0.7), 0.0, 0.0);
        } }",
    ] {
        let effect = compile_effects(source).unwrap().remove(0);
        let initial = effect.bind([]).unwrap();
        let (program, _) = effect
            .sample_program()
            .prepare_bindings(initial.params(), |_| true);
        for divisor in [0.0, 0.3, -2.0, f32::NAN] {
            let original = SampleDefinition::new(effect.sample_program().clone())
                .bind(vec![Value::Float(divisor)])
                .unwrap();
            let prepared = SampleDefinition::new(program.clone())
                .bind(vec![Value::Float(divisor)])
                .unwrap();
            for pixel in 0..17 {
                let ctx = context(17, pixel, 0);
                assert_eq!(
                    original.evaluate(&ctx, &SPATIAL, &mut BatchWorkspace::default()),
                    prepared.evaluate(&ctx, &SPATIAL, &mut BatchWorkspace::default())
                );
            }
        }
    }
}

#[test]
fn prepared_binding_arithmetic_preserves_branches_loops_and_temporal_operators() {
    use donder_language::dsl::{OperatorDefinition, compile_operators};
    let operator = compile_operators(
        "operator Prepared {
            input Signal source;
            param float delay in 0.0..1.0 = 0.08;
            param float gain in 0.0..1.0 = 0.4;
            color sample() {
                float offset = max(delay, 0.0) * 0.5;
                color result = rgb(0.0, 0.0, 0.0);
                for (int i = 0; i < 3; i = i + 1) {
                    float time = seconds() - i * offset;
                    if (time >= 0.0) { result = result + source.at(time) * clamp(gain, 0.0, 1.0); }
                }
                return result;
            }
        }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let (program, params) = operator
        .program()
        .prepare_bindings(operator.params(), |_| false);
    assert!(
        program.bytecode().instructions.len() < operator.program().bytecode().instructions.len()
    );
    let optimized = OperatorDefinition::new(program)
        .bind(params.iter_values().collect())
        .unwrap();
    let source = compile_effects("effect Source { color sample() { return rgb(pixel_fraction(), seconds() * 0.1, progress()); } }")
        .unwrap().remove(0).bind([]).unwrap();
    for count in [1, 7, 8, 9, 17] {
        let mut original =
            playback::chain(count, &source, 2, std::slice::from_ref(&operator)).into_playback();
        let mut prepared =
            playback::chain(count, &source, 2, std::slice::from_ref(&optimized)).into_playback();
        for ticks in [0, 1, 39_999, 40_000, 90_000, 3_083_333, 0, 7_999_999] {
            let time = donder_language::values::SampleTime::from_ticks(ticks);
            assert_eq!(
                original.evaluate(time).colors(),
                prepared.evaluate(time).colors(),
                "count={count} ticks={ticks}"
            );
        }
    }
}

#[test]
fn numeric_control_specializes_while_ordinary_settings_keep_program_sharing() {
    let effect = compile_effects(
        "effect Choice {
        param float mode in -1.0..1.0 = 1.0;
        param float gain in 0.0..1.0 = 0.5;
        color sample() {
            if (mode < 0.0) { return rgb(gain, 0.0, 0.0); }
            return rgb(0.0, gain * pixel_fraction(), 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0);
    let prepare = |mode, gain| {
        let params = effect
            .sample_program()
            .bind(vec![Value::Float(mode), Value::Float(gain)])
            .unwrap();
        effect
            .sample_program()
            .specialize(&params, |_| false, ProgramConstants::default())
    };
    assert_eq!(prepare(1.0, 0.2), prepare(1.0, 0.8));
    assert_ne!(prepare(1.0, 0.2), prepare(-1.0, 0.2));
    assert!(!prepare(-1.0, 0.2).bytecode().uses_pixel_context);
}

#[test]
fn specialization_of_fixed_branches_keeps_live_parameters() {
    let effect = compile_effects(
        "effect Bound {
        param bool reverse = true;
        param float gain in 0.0..1.0 = 0.5;
        color sample() {
            float position = pixel_fraction();
            if (reverse) { position = 1.0 - position; }
            return rgb(position * gain, clamp(gain, 0.0, 1.0), 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0);
    let bound = effect.bind([]).unwrap();
    let specialized = effect.sample_program().specialize(
        bound.params(),
        |index| index == 1,
        ProgramConstants::default(),
    );
    for gain in [-0.25, 0.0, 0.2, 0.75, 1.5] {
        let values = vec![Value::Bool(true), Value::Float(gain)];
        let original = SampleDefinition::new(effect.sample_program().clone())
            .bind(values.clone())
            .unwrap();
        let prepared = SampleDefinition::new(specialized.clone())
            .bind(values)
            .unwrap();
        for pixel in [0, 1, 7, 31] {
            assert_eq!(
                original.evaluate(
                    &context(32, pixel, 0),
                    &SPATIAL,
                    &mut BatchWorkspace::default()
                ),
                prepared.evaluate(
                    &context(32, pixel, 0),
                    &SPATIAL,
                    &mut BatchWorkspace::default()
                )
            );
        }
    }
}

#[test]
fn uniform_samples_split_varying_clamps_and_scales_without_changing_missing_values() {
    use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let effect = compile_effects(
        "effect Samples {
        param curve shape in -1.0..2.0;
        param gradient colors;
        color sample() {
            float lower = pixel_fraction() * 3.0 - 1.0;
            float value = curve_clamped(shape, progress(), lower, 1.0);
            if (pixel_index() % 2 == 0) {
                return gradient_color_scaled(colors, progress(), value * 3.0);
            }
            return rgb(value_or(value, 0.25), 0.0, 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0);
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: -0.5,
            },
            CurvePoint {
                position: 1.0,
                value: 2.0,
            },
        ],
    };
    let gradient = Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color {
                    red: 40,
                    green: 100,
                    blue: 200,
                },
            },
            GradientStop {
                position: 1.0,
                color: Color {
                    red: 120,
                    green: 30,
                    blue: 90,
                },
            },
        ],
    };
    for shape in [&curve, &Curve { points: vec![] }] {
        for colors in [&gradient, &Gradient { stops: vec![] }] {
            let invocation = SampleDefinition::new(effect.sample_program().clone())
                .bind(vec![
                    Value::Curve(shape.clone().into()),
                    Value::Gradient(colors.clone().into()),
                ])
                .unwrap();
            for count in [1, 7, 8, 9, 17] {
                let mut playback = playback::show(count, &invocation, 1).into_playback();
                for ticks in [0, 1, 3_000_000, 7_999_999, 0] {
                    let output =
                        playback.evaluate(donder_language::values::SampleTime::from_ticks(ticks));
                    for (pixel, &actual) in output.colors().iter().enumerate() {
                        let progress = ticks as f32 / 8_000_000.0;
                        let fraction = pixel as f32 / (count - 1).max(1) as f32;
                        let value = crate::sampling::clamp_float(
                            crate::sampling::sample_curve(shape, progress),
                            fraction * 3.0 - 1.0,
                            1.0,
                        );
                        let expected = if pixel % 2 == 0 {
                            crate::sampling::scale_color(
                                crate::sampling::sample_gradient(colors, progress),
                                (value * 3.0).clamp(0.0, 1.0),
                            )
                        } else {
                            crate::sampling::rgb(
                                if value.is_nan() { 0.25 } else { value },
                                0.0,
                                0.0,
                            )
                        };
                        assert_eq!(
                            actual, expected,
                            "count={count} pixel={pixel} ticks={ticks}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn uniform_parameter_samples_refresh_when_target_changes() {
    use donder_language::values::{Color, Gradient, GradientStop};
    let effect = compile_effects(
        "effect Target { param gradient colors;
        color sample() {
            if (pixel_index() >= 0) { return colors[target_max_x()]; }
            return #000000;
        } }",
    )
    .unwrap()
    .remove(0);
    let gradient = Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color::BLACK,
            },
            GradientStop {
                position: 1.0,
                color: Color {
                    red: 50,
                    green: 100,
                    blue: 200,
                },
            },
        ],
    };
    let invocation = SampleDefinition::new(effect.sample_program().clone())
        .bind(vec![Value::Gradient(gradient.clone().into())])
        .unwrap();
    let program = effect.sample_program();
    let params = BoundParams::from_validated(invocation.params(), &mut DslBindCache::default());
    let mut workspace = BatchWorkspace::default();
    for (index, max_x) in [0.0, 0.25, 1.0, 0.25].into_iter().enumerate() {
        let context = context(9, index, 0);
        let spatial = SpatialContext {
            max: [max_x, 1.0],
            ..SPATIAL
        };
        let actual = sample_once(program, &params, &context, &spatial, &mut workspace);
        assert_eq!(actual, crate::sampling::sample_gradient(&gradient, max_x));
    }
}

#[test]
fn section_reciprocal_preserves_boundaries_and_missing_values() {
    let effect = compile_effects(
        "effect Sections {
        param float width in 0.0..20.0 = 4.0;
        color sample() { return rgb(value_or(section_position(width), 0.75), 0.0, 0.0); }
    }",
    )
    .unwrap()
    .remove(0);
    for width in [f32::NAN, -1.0, 0.0, 1.0, 4.0, 7.5, 19.0] {
        let bound = SampleDefinition::new(effect.sample_program().clone())
            .bind(vec![Value::Float(width)])
            .unwrap();
        for pixel in [0, 1, 3, 4, 7, 8, 14, 15, 19, 20, 57] {
            let normalized = width.max(1.0);
            let position = if width.is_nan() {
                0.75
            } else {
                (pixel as f32 - libm::floorf(pixel as f32 / normalized) * normalized) / normalized
            };
            let expected = (position.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            assert_eq!(
                bound
                    .evaluate(
                        &context(64, pixel, 0),
                        &SPATIAL,
                        &mut BatchWorkspace::default()
                    )
                    .red,
                expected,
                "width={width} pixel={pixel}"
            );
        }
    }
}
