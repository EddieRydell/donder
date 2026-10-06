//! Staging and preparation: query and target blocks run before the body,
//! frame caches reuse query-uniform samples, and values fixed for an instance
//! are computed before playback without changing what playback produces.
use super::evaluation::{
    SampleEvaluation, bind, chain, compile_effect, compile_operator, context, effect, operator,
    runtime_effect, runtime_operator,
};
use super::std;
use std::prelude::rust_2024::*;

use crate::dsl::bytecode::{FloatBinary, Instruction};
use crate::dsl::{BoundParams, DslBindCache, NoSignals, STRIP, Strip, StripWorkspace};
use donder_language::dsl::{OperatorDefinition, OperatorInvocation, OperatorProgram};
use donder_language::dsl::{SampleInvocation, Value};
use donder_language::execution::{FixtureGeometry, SpatialContext, TargetScope};
use donder_language::values::SampleTime;
use donder_test_support::workload;

use super::playback;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [1.0; 2],
};

/// Pixel counts around the strip boundaries.
const COUNTS: [usize; 5] = [1, 7, STRIP, STRIP + 1, 2 * STRIP + 1];

/// `operator` with its query and target blocks rerun for every strip and
/// without frame caches: every query samples its run upstream.
fn uncached(operator: &OperatorInvocation) -> OperatorInvocation {
    let program = (**operator.program()).clone();
    let (inputs, types) = (program.input_count(), program.parameter_types().into());
    let mut bytecode = program.into_bytecode();
    workload::unstaged(&mut bytecode);
    OperatorDefinition::new(OperatorProgram::admit(bytecode, inputs, types).unwrap())
        .bind(operator.params().iter_values().collect())
        .unwrap()
        .with_automation(operator.automation().into())
        .unwrap()
}

fn has_frame_cache(operator: &OperatorInvocation) -> bool {
    operator.program().bytecode().frame_cache_count() > 0
}

fn assert_same_playback(
    actual: crate::PreparedSequence,
    expected: crate::PreparedSequence,
    ticks: &[u32],
    what: &str,
) {
    let mut actual = actual.into_playback();
    let mut expected = expected.into_playback();
    for &ticks in ticks {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            actual.evaluate(time).colors(),
            expected.evaluate(time).colors(),
            "{what} ticks={ticks}"
        );
    }
}

#[test]
fn frame_caches_match_run_sampling_for_temporal_reductions_tails_seeks_and_nested_graphs() {
    let effect = effect("effect Source { sample { rgb(pixel.fraction, time * 0.1, progress) } }");
    let operator = operator(
        "operator Temporal {
            input source;
            sample {
                // Query-uniform sample times use frame caches; times that
                // vary with a reduction index sample per iteration.
                let base = mix(rgb(0.1, 0.2, 0.3), source.at(time - 0.04), 0.5);
                max(base, max for i in 0..3 {
                    let t = time - i * 0.04;
                    guard t >= 0.0;
                    mix(invert(base), source.at(t) * (1.0 - i * 0.2), 0.25)
                })
            }
        }",
    );
    assert!(has_frame_cache(&operator));
    let reference_operator = uncached(&operator);
    assert!(!has_frame_cache(&reference_operator));
    for count in COUNTS {
        assert_same_playback(
            chain(count, &effect, 2, &[operator.clone(), operator.clone()]),
            chain(
                count,
                &effect,
                2,
                &[reference_operator.clone(), reference_operator.clone()],
            ),
            &[0, 1, 39_999, 40_000, 90_000, 3_083_333, 0, 7_999_999],
            &format!("count={count}"),
        );
    }
}

#[test]
fn frame_caches_preserve_choices_arrays_reductions_and_seeks() {
    let effect = effect("effect Source { sample { hsv(pixel.fraction + time * 0.03, 0.7, 0.8) } }");
    let outer = operator(playback::IDENTITY_SOURCE);
    for source in [
        "operator P { input source; sample {
            let c = source.at(time - 0.04);
            hsv(hue(c) + pixel.fraction, saturation(c), intensity(c))
        } }",
        "operator P { input source; sample {
            guard pixel.index % 3 != 0 else #123456;
            let c = source.at(time + 0.04);
            if intensity(c) > 0.5 { invert(c) } else { c }
        } }",
        "operator P { input source; sample {
            max for i in 0..pixel.index % 4 {
                let c = source.at(time);
                let channels = [hue(c), saturation(c), intensity(c)];
                hsv(channels[0] + i * 0.1, channels[1], channels[2])
            }
        } }",
    ] {
        let operator = operator(source);
        let reference_operator = uncached(&operator);
        for count in COUNTS {
            assert_same_playback(
                chain(
                    count,
                    &effect,
                    2,
                    &[operator.clone(), operator.clone(), outer.clone()],
                ),
                chain(
                    count,
                    &effect,
                    2,
                    &[
                        reference_operator.clone(),
                        reference_operator.clone(),
                        outer.clone(),
                    ],
                ),
                &[
                    0, 1, 39_999, 40_000, 3_083_333, 0, 7_959_999, 7_999_999, 8_000_000,
                ],
                &format!("count={count} source={source}"),
            );
        }
    }
}

#[test]
fn strips_preserve_local_global_and_subset_target_addressing() {
    use donder_language::execution::{OutputEncoding, RgbOrder};
    let source = effect("effect Source { sample { rgb(pixel.fraction, time * 0.2, 0.5) } }");
    let operator = operator(
        "operator Address { input source; sample {
            max(source.at(time + 0.1), mix(source.at(time * 0.5, 1), source.at_global(time, 4), 0.3))
        } }",
    );
    let reference_operator = uncached(&operator);
    // The second fixture fits one strip, or spans several.
    for right_count in [7, 2 * STRIP + 3] {
        let build = |op: &OperatorInvocation| {
            crate::PreparedSequence::build(playback::timing(4_000_000), |builder| {
                let left = builder.fixture(
                    0,
                    FixtureGeometry::admit((0..3).map(|i| [i as f32, 0.0]).collect()).unwrap(),
                );
                let right = builder.fixture(
                    1,
                    FixtureGeometry::admit((0..right_count).map(|i| [i as f32, 1.0]).collect())
                        .unwrap(),
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
        };
        assert_same_playback(
            build(&operator),
            build(&reference_operator),
            &[0, 1, 333_333, 3_900_000, 3_999_999, 4_000_000, 0],
            &format!("right={right_count}"),
        );
    }
}

#[test]
fn constant_colors_use_the_same_quantization_as_runtime_expressions() {
    let expression = "mix(invert(rgb(0.25, -0.2, 1.5)), hsv(0.4, 0.7, 0.8), amount) * amount";
    let dynamic = compile_effect(&format!(
        "effect Color {{ param amount: float in -1.0..2.0 = 0.0; sample {{ {expression} }} }}"
    ));
    for (literal, amount) in [
        ("-0.2", -0.2),
        ("0.3", 0.3),
        ("1.5", 1.5),
        ("0.0 / 0.0", f32::NAN),
    ] {
        let folded = compile_effect(&format!(
            "effect Color {{ sample {{ let amount = {literal}; {expression} }} }}"
        ));
        let instance = bind(&folded, &[]).instance(Default::default());
        let constant = instance.constant_color().expect("literals fold");
        let runtime = runtime_effect(&dynamic, &[], &[("amount", Value::Float(amount))]);
        assert_eq!(
            runtime.evaluate(&context(8, 3, 0), &SPATIAL, &mut StripWorkspace::default()),
            constant,
            "{literal}"
        );
        assert_eq!(
            instance
                .sample()
                .evaluate(&context(8, 3, 0), &SPATIAL, &mut StripWorkspace::default()),
            constant
        );
        if !amount.is_nan() {
            let prepared =
                playback::lower_sample(&bind(&dynamic, &[("amount", Value::Float(amount))]));
            assert_eq!(
                prepared.evaluate(&context(8, 3, 0), &SPATIAL, &mut StripWorkspace::default()),
                constant,
                "{literal}"
            );
        }
    }
}

fn assert_same_pixels(actual: &SampleInvocation, expected: &SampleInvocation, what: &str) {
    for pixel in [0, 1, 4, 31] {
        for frame in [0, 1, 17] {
            let context = context(32, pixel, frame);
            assert_eq!(
                actual.evaluate(&context, &SPATIAL, &mut StripWorkspace::default()),
                expected.evaluate(&context, &SPATIAL, &mut StripWorkspace::default()),
                "{what} pixel={pixel} frame={frame}"
            );
        }
    }
}

fn divides(invocation: &SampleInvocation) -> bool {
    invocation.program().bytecode().code.iter().any(|op| {
        matches!(
            op,
            Instruction::FloatBinary {
                op: FloatBinary::Divide,
                ..
            }
        )
    })
}

#[test]
fn prepared_values_share_programs_and_keep_runtime_inputs() {
    let effect = compile_effect(
        "effect Prepared {
            param width: float in -1.0..2.0 = 0.3;
            param gain: float in 0.0..1.0 = 0.5;
            sample {
                let scale = 1.0 / max(abs(width), 0.01);
                rgb(pixel.fraction * scale * gain, clamp(width, 0.0, 1.0), time * 0.1)
            }
        }",
    );
    let mut previous: Option<SampleInvocation> = None;
    for width in [-0.4, 0.0, 0.2, 2.0] {
        let fixed = [("width", Value::Float(width))];
        for gain in [0.0, 0.25, 1.3] {
            let gain = [("gain", Value::Float(gain))];
            let prepared = runtime_effect(&effect, &fixed, &gain);
            if let Some(previous) = &previous {
                assert_eq!(
                    prepared.program(),
                    previous.program(),
                    "ordinary settings keep program sharing"
                );
            }
            let reference = runtime_effect(
                &effect,
                &[],
                &[("width", Value::Float(width)), gain[0].clone()],
            );
            assert_same_pixels(&prepared, &reference, &format!("width={width} {gain:?}"));
            previous = Some(prepared);
        }
    }
}

#[test]
fn fixed_divisors_become_prepared_reciprocals_that_share_programs() {
    let effect = compile_effect(
        "effect Divide { param divisor: float in -10.0..10.0 = 3.0; sample {
            let value = sum for i in 0..pixel.index % 4 { pixel.fraction / divisor };
            if pixel.index % 2 == 0 { rgb(value_or(value, 0.7), 0.0, 0.0) }
            else { rgb(value_or(pixel.fraction / divisor, 0.7), 0.0, 0.0) }
        } }",
    );
    let mut previous: Option<SampleInvocation> = None;
    for divisor in [3.0, -7.0, 0.13, 10.0, 1.0e-38, 0.0, -0.0, f32::from_bits(1)] {
        let prepared =
            playback::lower_sample(&bind(&effect, &[("divisor", Value::Float(divisor))]));
        let exact = runtime_effect(&effect, &[], &[("divisor", Value::Float(divisor))]);
        assert!(divides(&exact));
        let safe = divisor != 0.0 && (1.0 / divisor).is_normal();
        assert_eq!(divides(&prepared), !safe, "divisor={divisor}");
        if safe {
            if let Some(previous) = &previous {
                assert_eq!(prepared.program(), previous.program(), "divisor={divisor}");
            }
            previous = Some(prepared.clone());
        }
        for count in COUNTS {
            let mut actual = chain(count, &prepared, 1, &[]).into_playback();
            let mut expected = chain(count, &exact, 1, &[]).into_playback();
            for ticks in [0, 1, 3_000_000, 7_999_999, 0] {
                let time = SampleTime::from_ticks(ticks);
                let actual = actual.evaluate(time);
                let expected = expected.evaluate(time);
                for (pixel, (a, b)) in actual.colors().iter().zip(expected.colors()).enumerate() {
                    // Multiplying by a reciprocal may round differently.
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
fn divisors_left_to_playback_or_varying_per_pixel_keep_division() {
    for source in [
        "effect Divide { param divisor: float in -10.0..10.0 = 3.0; sample {
            rgb(value_or(pixel.fraction / divisor, 0.7), 0.0, 0.0)
        } }",
        "effect Divide { param divisor: float in -10.0..10.0 = 3.0; sample {
            let current = if pixel.index % 2 == 0 { pixel.fraction } else { divisor };
            rgb(value_or(pixel.fraction / current, 0.7), 0.0, 0.0)
        } }",
    ] {
        let effect = compile_effect(source);
        for divisor in [0.0, 0.3, -2.0, f32::NAN, f32::INFINITY] {
            let runtime = runtime_effect(&effect, &[], &[("divisor", Value::Float(divisor))]);
            assert!(divides(&runtime));
            for pixel in 0..17 {
                let ctx = context(17, pixel, 0);
                let fraction = ctx.fraction;
                let current = if source.contains("current") && pixel % 2 == 0 {
                    fraction
                } else {
                    divisor
                };
                let quotient = fraction / current;
                let expected =
                    crate::sampling::rgb(if quotient.is_nan() { 0.7 } else { quotient }, 0.0, 0.0);
                assert_eq!(
                    runtime.evaluate(&ctx, &SPATIAL, &mut StripWorkspace::default()),
                    expected,
                    "divisor={divisor} pixel={pixel}"
                );
                if divisor.is_finite() {
                    let fixed = playback::lower_sample(&bind(
                        &effect,
                        &[("divisor", Value::Float(divisor))],
                    ));
                    if source.contains("current") {
                        assert!(divides(&fixed));
                    }
                    assert!(
                        fixed
                            .evaluate(&ctx, &SPATIAL, &mut StripWorkspace::default())
                            .red
                            .abs_diff(expected.red)
                            <= 1,
                        "divisor={divisor} pixel={pixel}"
                    );
                }
            }
        }
    }
}

#[test]
fn prepared_operators_match_runtime_parameters_across_choices_reductions_and_time() {
    let operator = compile_operator(
        "operator Prepared {
            input source;
            param delay: float in 0.0..1.0 = 0.08;
            param gain: float in 0.0..1.0 = 0.4;
            sample {
                let offset = max(delay, 0.0) * 0.5;
                sum for i in 0..3 {
                    let t = time - i * offset;
                    guard t >= 0.0;
                    source.at(t) * clamp(gain, 0.0, 1.0)
                }
            }
        }",
    );
    let prepared = playback::lower_operator(&bind(&operator, &[]));
    let runtime = runtime_operator(
        &operator,
        &[],
        &[("delay", Value::Float(0.08)), ("gain", Value::Float(0.4))],
    );
    assert!(prepared.program().bytecode().code.len() < runtime.program().bytecode().code.len());
    let source = effect("effect Source { sample { rgb(pixel.fraction, time * 0.1, progress) } }");
    for count in COUNTS {
        assert_same_playback(
            chain(count, &source, 2, std::slice::from_ref(&runtime)),
            chain(count, &source, 2, std::slice::from_ref(&prepared)),
            &[0, 1, 39_999, 40_000, 90_000, 3_083_333, 0, 7_999_999],
            &format!("count={count}"),
        );
    }
}

#[test]
fn fixed_conditions_specialize_while_ordinary_settings_share_programs() {
    let effect = compile_effect(
        "effect Choice {
            param mode: float in -1.0..1.0 = 1.0;
            param gain: float in 0.0..1.0 = 0.5;
            sample {
                if mode < 0.0 { rgb(gain, 0.0, 0.0) } else { rgb(0.0, gain * pixel.fraction, 0.0) }
            }
        }",
    );
    let prepare = |mode, gain| {
        playback::lower_sample(&bind(
            &effect,
            &[("mode", Value::Float(mode)), ("gain", Value::Float(gain))],
        ))
    };
    assert_eq!(prepare(1.0, 0.2).program(), prepare(1.0, 0.8).program());
    assert_ne!(prepare(1.0, 0.2).program(), prepare(-1.0, 0.2).program());
    assert!(prepare(1.0, 0.2).program().bytecode().uses_pixel_context());
    assert!(!prepare(-1.0, 0.2).program().bytecode().uses_pixel_context());
    for (mode, gain) in [(1.0, 0.2), (-1.0, 0.2), (0.0, 0.8)] {
        let reference = runtime_effect(
            &effect,
            &[],
            &[("mode", Value::Float(mode)), ("gain", Value::Float(gain))],
        );
        assert_same_pixels(&prepare(mode, gain), &reference, &format!("mode={mode}"));
    }

    // A choice fixed as a whole is computed, so it does not multiply programs.
    let whole = compile_effect(
        "effect Whole {
            param mode: float in -1.0..1.0 = 1.0;
            param gain: float in 0.0..1.0 = 0.5;
            sample { if mode < 0.0 { rgb(gain, 0.0, 0.0) } else { rgb(0.0, gain, 0.0) } }
        }",
    );
    let programs: Vec<_> = [-1.0, 1.0]
        .map(|mode| {
            playback::lower_sample(&bind(&whole, &[("mode", Value::Float(mode))]))
                .program()
                .clone()
        })
        .into();
    assert_eq!(programs[0], programs[1]);
}

#[test]
fn specialized_branches_keep_runtime_parameters() {
    let effect = compile_effect(
        "effect Bound {
            param reverse: bool = true;
            param gain: float in 0.0..1.0 = 0.5;
            sample {
                let position = if reverse { 1.0 - pixel.fraction } else { pixel.fraction };
                rgb(position * gain, clamp(gain, 0.0, 1.0), 0.0)
            }
        }",
    );
    for gain in [-0.25, 0.0, 0.2, 0.75, 1.5] {
        let gain = ("gain", Value::Float(gain));
        let specialized = runtime_effect(&effect, &[], std::slice::from_ref(&gain));
        let reference = runtime_effect(
            &effect,
            &[],
            &[("reverse", Value::Bool(true)), gain.clone()],
        );
        assert_ne!(specialized.program(), reference.program());
        assert_same_pixels(&specialized, &reference, &format!("{gain:?}"));
    }
}

#[test]
fn uniform_samples_stay_in_the_query_block_beside_varying_clamps_and_scales() {
    use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let effect = compile_effect(
        "effect Samples {
            param shape: curve in -1.0..2.0;
            param colors: gradient;
            sample {
                let lower = pixel.fraction * 3.0 - 1.0;
                let value = curve_clamped(shape, progress, lower, 1.0);
                if pixel.index % 2 == 0 { gradient_color_scaled(colors, progress, value * 3.0) }
                else { rgb(value_or(value, 0.25), 0.0, 0.0) }
            }
        }",
    );
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
            let invocation = playback::lower_sample(&bind(
                &effect,
                &[
                    ("shape", Value::Curve(shape.clone().into())),
                    ("colors", Value::Gradient(colors.clone().into())),
                ],
            ));
            // The samples at the query's progress run once per query; the
            // clamps by pixel bounds run per strip, so nothing is fused.
            let bytecode = invocation.program().bytecode();
            let (query, _) = bytecode.prefix();
            assert!(
                query
                    .iter()
                    .any(|op| matches!(op, Instruction::CurveSample { .. }))
            );
            assert!(
                query
                    .iter()
                    .any(|op| matches!(op, Instruction::GradientSample { .. }))
            );
            assert!(!bytecode.code.iter().any(|op| matches!(
                op,
                Instruction::CurveClamped { .. } | Instruction::GradientScaled { .. }
            )));
            assert!(
                bytecode
                    .body()
                    .iter()
                    .any(|op| matches!(op, Instruction::Clamp { .. }))
            );
            for count in COUNTS {
                let mut playback = chain(count, &invocation, 1, &[]).into_playback();
                for ticks in [0, 1, 3_000_000, 7_999_999, 0] {
                    let output = playback.evaluate(SampleTime::from_ticks(ticks));
                    for (pixel, &actual) in output.colors().iter().enumerate() {
                        let progress = ticks as f32 / 8_000_000.0;
                        let fraction = pixel as f32 / (count - 1).max(1) as f32;
                        let value = crate::sampling::clamp_float(
                            crate::sampling::sample_curve(shape, progress),
                            fraction * 3.0 - 1.0,
                            1.0,
                        );
                        let expected = if pixel % 2 == 0 {
                            crate::sampling::gradient_color_scaled(colors, progress, value * 3.0)
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
fn target_blocks_rerun_when_the_target_shape_changes() {
    use donder_language::values::{Color, Gradient, GradientStop};
    let effect = compile_effect(
        "effect Target { param colors: gradient; sample {
            guard pixel.index >= 0;
            colors[target.max_x * target.count / 9.0]
        } }",
    );
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
    let invocation = playback::lower_sample(&bind(
        &effect,
        &[("colors", Value::Gradient(gradient.clone().into()))],
    ));
    let bytecode = invocation.program().bytecode();
    // The sample reads only the target, so it runs once per target shape;
    // the guard reads the pixel, so the result is a row.
    let (_, target) = bytecode.prefix();
    assert!(
        target
            .iter()
            .any(|op| matches!(op, Instruction::GradientSample { .. }))
    );
    assert!(bytecode.uses_pixel_context());
    let params = BoundParams::from_validated(invocation.params(), &mut DslBindCache::default());
    let mut workspace = StripWorkspace::default();
    workspace.reserve(bytecode);
    let run = context(9, 0, 0).run;
    let mut strip = Strip::new(bytecode, &params, &run, None, &mut workspace);
    for (pixel, pixel_count, max_x) in [
        (0, 9, 0.0),
        (1, 9, 0.25),
        (2, 9, 0.25),
        (3, 9, 1.0),
        (4, 3, 1.0),
        (5, 9, 0.25),
    ] {
        let pixels = strip.pixels();
        pixels.index[0].set(-1);
        pixels.index[1].set(pixel);
        let mut colors = [Color::BLACK; 2];
        strip.run(
            pixel_count,
            [0.0; 2],
            [max_x, 1.0],
            &mut NoSignals,
            &mut colors,
        );
        let position = max_x * pixel_count as f32 / 9.0;
        assert_eq!(
            colors,
            [
                Color::BLACK,
                crate::sampling::sample_gradient(&gradient, position)
            ],
            "count={pixel_count} max_x={max_x}"
        );
    }
}

#[test]
fn section_reciprocal_preserves_boundaries_and_missing_values() {
    let effect = compile_effect(
        "effect Sections {
            param width: float in 0.0..20.0 = 4.0;
            sample { rgb(value_or(section_position(width), 0.75), 0.0, 0.0) }
        }",
    );
    for width in [f32::NAN, -1.0, 0.0, 1.0, 4.0, 7.5, 19.0] {
        let runtime = runtime_effect(&effect, &[], &[("width", Value::Float(width))]);
        let fixed = (0.0..=20.0)
            .contains(&width)
            .then(|| playback::lower_sample(&bind(&effect, &[("width", Value::Float(width))])));
        for pixel in [0, 1, 3, 4, 7, 8, 14, 15, 19, 20, 57] {
            let normalized = width.max(1.0);
            let position = if width.is_nan() {
                0.75
            } else {
                (pixel as f32 - libm::floorf(pixel as f32 / normalized) * normalized) / normalized
            };
            let expected = (position.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            for invocation in [Some(&runtime), fixed.as_ref()].into_iter().flatten() {
                assert_eq!(
                    invocation
                        .evaluate(
                            &context(64, pixel, 0),
                            &SPATIAL,
                            &mut StripWorkspace::default()
                        )
                        .red,
                    expected,
                    "width={width} pixel={pixel}"
                );
            }
        }
    }
}
