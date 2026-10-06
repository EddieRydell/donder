//! Rewrites of the compiler pipeline (folding, choices, hoisting, early exits,
//! prepared reciprocals), checked by what programs compute and sample.
use super::evaluation::{
    OperatorEvaluation, SampleEvaluation, bind, compile_effect, compile_operator, context, effect,
    operator, runtime_effect,
};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;

use super::evaluation::SignalSampler;
use crate::dsl::{RuntimeError, StripWorkspace};
use donder_language::dsl::bytecode::{FloatUnary, Instruction, SignalPixel};
use donder_language::dsl::{Color, ProgramConstants, Value};
use donder_language::execution::SpatialContext;
use donder_language::values::SampleTime;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[test]
fn else_if_chains_choose_per_pixel() {
    let effect = effect(
        "effect Booleans { sample {
            let first = pixel.index == 0;
            let second = pixel.index == 1;
            if first { rgb(1.0, 0.0, 0.0) } else if second { rgb(0.0, 1.0, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for (pixel, expected) in [
        (0, crate::sampling::rgb(1.0, 0.0, 0.0)),
        (1, crate::sampling::rgb(0.0, 1.0, 0.0)),
        (2, crate::sampling::rgb(0.0, 0.0, 1.0)),
        (0, crate::sampling::rgb(1.0, 0.0, 0.0)),
    ] {
        assert_eq!(
            effect.evaluate(&context(3, pixel, 0), &SPATIAL, &mut workspace),
            expected
        );
    }
}

/// Fails every sample, so a program's result shows whether it sampled.
struct FailOnSample {
    calls: Vec<u32>,
}

impl SignalSampler for FailOnSample {
    fn sample_signal(
        &mut self,
        _: usize,
        time: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.calls.push(time.as_ticks());
        Err(RuntimeError {
            message: "observed sample".into(),
        })
    }
}

/// Answers black and records each sample's time.
#[derive(Default)]
struct Times(Vec<u32>);

impl SignalSampler for Times {
    fn sample_signal(
        &mut self,
        _: usize,
        time: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.0.push(time.as_ticks());
        Ok(Color::BLACK)
    }
}

#[test]
fn conditions_sample_only_when_they_decide() {
    for (condition, should_sample) in [
        ("pixel.index == 0 || source.at(0.0) == #ffffff", false),
        ("pixel.index != 0 && source.at(0.0) == #ffffff", false),
        ("!(pixel.index == 0) || source.at(0.0) == #ffffff", true),
        (
            "pixel.index == 0 && (target.count == 1 || source.at(0.0) == #ffffff)",
            false,
        ),
        (
            "pixel.index == 0 && (target.count != 1 || source.at(0.0) == #ffffff)",
            true,
        ),
    ] {
        let operator = operator(&format!(
            "operator Branch {{ input source; sample {{ if {condition} {{ #ffffff }} else {{ #000000 }} }} }}"
        ));
        let mut sampler = FailOnSample { calls: Vec::new() };
        let result = operator.evaluate(
            &context(1, 0, 0),
            &SPATIAL,
            &mut sampler,
            &mut StripWorkspace::default(),
        );
        assert_eq!(result.is_err(), should_sample, "{condition}");
        assert_eq!(
            sampler.calls.len(),
            usize::from(should_sample),
            "{condition}"
        );
    }
}

#[test]
fn unused_samples_are_never_taken() {
    let operator = compile_operator(
        "operator Dead { input source; sample { let unused = source.at(0.0); #000000 } }",
    );
    let instance = bind(&operator, &[]).instance(ProgramConstants::default());
    assert_eq!(instance.constant_color(), Some(Color::BLACK));
    let mut sampler = FailOnSample { calls: Vec::new() };
    assert_eq!(
        instance.operator().evaluate(
            &context(1, 0, 0),
            &SPATIAL,
            &mut sampler,
            &mut StripWorkspace::default()
        ),
        Ok(Color::BLACK)
    );
    assert!(sampler.calls.is_empty());
}

#[test]
fn reductions_sample_only_contributing_iterations_and_exit_early() {
    for (body, calls) in [
        (
            "max for i in 0..10 { guard i < 3; source.at(i * 0.1) }",
            vec![0, 100_000, 200_000],
        ),
        (
            "first for i in 0..10 { guard i >= 2; source.at(i * 0.1) }",
            vec![200_000],
        ),
        (
            "last for i in 0..10 { guard i < 7; source.at(i * 0.1) }",
            vec![600_000],
        ),
        (
            "if any for i in 0..10 { source.at(i * 0.1) == #000000 } { #ffffff } else { #000000 }",
            vec![0],
        ),
        (
            "if all for i in 0..10 { source.at(i * 0.1) != #000000 } { #ffffff } else { #000000 }",
            vec![0],
        ),
        (
            "if all for i in 0..10 { guard i > 4; source.at(i * 0.1) != #000000 } { #ffffff } else { #000000 }",
            vec![500_000],
        ),
    ] {
        let operator = operator(&format!(
            "operator Reduce {{ input source; sample {{ {body} }} }}"
        ));
        let mut sampler = Times::default();
        operator
            .evaluate(
                &context(1, 0, 0),
                &SPATIAL,
                &mut sampler,
                &mut StripWorkspace::default(),
            )
            .unwrap();
        assert_eq!(sampler.0, calls, "{body}");
    }
}

#[test]
fn reductions_of_index_free_bodies_fold() {
    let effect = compile_effect(
        "effect Folded { param n: int in 0..8 = 3; sample {
            let channels = rgb(max for i in 0..n { progress }, first for i in 0..n { 0.375 } else { 0.0 }, min for i in 0..n { pixel.fraction });
            channels + sum for i in 0..n { #000000 }
        } }",
    );
    for n in [0, 3] {
        for invocation in [
            playback::lower_sample(&bind(&effect, &[("n", Value::Int(n))])),
            runtime_effect(&effect, &[], &[("n", Value::Int(n))]),
        ] {
            assert!(
                !invocation
                    .program()
                    .bytecode()
                    .code
                    .iter()
                    .any(|op| matches!(op, Instruction::Reduce { .. })),
                "n={n}"
            );
            let context = context(3, 1, 0);
            let expected = if n == 0 {
                crate::sampling::rgb(f32::NEG_INFINITY, 0.0, f32::INFINITY)
            } else {
                crate::sampling::rgb(context.progress, 0.375, context.fraction)
            };
            assert_eq!(
                invocation.evaluate(&context, &SPATIAL, &mut StripWorkspace::default()),
                expected,
                "n={n}"
            );
        }
    }
}

#[test]
fn fixed_definitions_fold_to_constant_colors() {
    for (source, expected) in [
        (
            "effect Constant { sample { rgb(0.5, 0.25, 1.0) } }",
            Some(crate::sampling::rgb(0.5, 0.25, 1.0)),
        ),
        (
            "effect Empty { param n: int in 0..4 = 0; sample { sum for i in 0..n { #ffffff } } }",
            Some(Color::BLACK),
        ),
        (
            "effect Choice { param on: bool = true; sample { if on { #ff0000 } else { rgb(pixel.fraction, 0.0, 0.0) } } }",
            Some(Color {
                red: 255,
                green: 0,
                blue: 0,
            }),
        ),
        (
            "effect Varying { sample { rgb(progress, 0.0, 0.0) } }",
            None,
        ),
        ("effect Wave { sample { rgb(sin(1.0), 0.0, 0.0) } }", None),
    ] {
        let instance = bind(&compile_effect(source), &[]).instance(ProgramConstants::default());
        assert_eq!(instance.constant_color(), expected, "{source}");
        let lowered = instance.sample();
        let color = lowered.evaluate(&context(4, 1, 0), &SPATIAL, &mut StripWorkspace::default());
        if let Some(expected) = expected {
            assert_eq!(color, expected, "{source}");
            assert!(
                !lowered.program().bytecode().uses_pixel_context(),
                "{source}"
            );
        }
    }
}

#[test]
fn preparation_computes_fixed_values_into_parameter_slots() {
    let effect = compile_effect(
        "effect Tint { param tint: color = #204060; param level: float in 0.0..1.0 = 0.5;
          sample { tint * level } }",
    );
    let expected = crate::sampling::scale_color(
        Color {
            red: 0x20,
            green: 0x40,
            blue: 0x60,
        },
        0.5,
    );
    let lowered = playback::lower_sample(&bind(&effect, &[]));
    assert_eq!(lowered.params().values(), [Value::Color(expected)]);
    // The program only reads the computed parameter.
    let bytecode = lowered.program().bytecode();
    assert_eq!(
        *bytecode.code,
        [Instruction::ColorParam {
            dst: bytecode.result,
            bank: 0
        }]
    );
    assert_eq!(
        lowered.evaluate(&context(4, 1, 0), &SPATIAL, &mut StripWorkspace::default()),
        expected
    );
}

#[test]
fn invariant_divisors_divide_correctly_with_either_sign() {
    for denominator in ["max(scale, 0.01)", "-max(scale, 0.01)"] {
        let effect = compile_effect(&format!(
            "effect Divide {{ param scale: float in 0.0..10.0 = 2.0;
            sample {{
                let d = {denominator};
                rgb(sum for i in 0..4 {{ abs(i / d) }} / 10.0, 0.0, 0.0)
            }}
        }}"
        ));
        for invocation in [
            playback::lower_sample(&bind(&effect, &[])),
            runtime_effect(&effect, &[], &[("scale", Value::Float(2.0))]),
        ] {
            assert_eq!(
                invocation
                    .evaluate(&context(1, 0, 0), &SPATIAL, &mut StripWorkspace::default())
                    .red,
                77,
                "{denominator}"
            );
        }
    }
}

#[test]
fn smoothstep_with_bounded_edges_preserves_boundaries() {
    for (edge0, edge1) in [("0.0", "max(width, 0.01)"), ("max(width, 0.01)", "0.0")] {
        let effect = compile_effect(&format!(
            "effect Smooth {{ param width: float in 0.0..1.0 = 0.75;
            sample {{ rgb(smoothstep({edge0}, {edge1}, pixel.fraction), 0.0, 0.0) }} }}"
        ));
        let fixed = playback::lower_sample(&bind(&effect, &[]));
        let runtime = runtime_effect(&effect, &[], &[("width", Value::Float(0.75))]);
        let mut workspace = StripWorkspace::default();
        for pixel in 0..65 {
            let context = context(65, pixel, 0);
            let (left, right) = if edge0 == "0.0" {
                (0.0, 0.75)
            } else {
                (0.75, 0.0)
            };
            let t = ((context.fraction - left) / (right - left)).clamp(0.0, 1.0);
            let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.0, 0.0);
            for invocation in [&fixed, &runtime] {
                let actual = invocation.evaluate(&context, &SPATIAL, &mut workspace);
                assert!(
                    actual.red.abs_diff(expected.red) <= 1,
                    "pixel {pixel}: {actual:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn smoothstep_keeps_varying_and_degenerate_edge_semantics() {
    let effect = effect(
        "effect Smooth { sample {
            let edge = pixel.fraction;
            rgb(smoothstep(edge, 0.5, 0.25),
                value_or(smoothstep(edge, edge, edge), 0.25),
                smoothstep(0.0, 1.0, 0.5))
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for pixel in 0..65 {
        let context = context(65, pixel, 0);
        let edge = context.fraction;
        let t = ((0.25 - edge) / (0.5 - edge)).clamp(0.0, 1.0);
        let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.25, 0.5);
        assert_eq!(
            effect.evaluate(&context, &SPATIAL, &mut workspace),
            expected
        );
    }
}

#[test]
fn uniform_smoothstep_runs_in_the_query_block() {
    let effect = compile_effect(
        "effect Smooth { param width: float in 0.0..1.0 = 0.75;
        sample { rgb(smoothstep(0.0, width, time), 0.0, 0.0) } }",
    );
    for invocation in [
        playback::lower_sample(&bind(&effect, &[])),
        runtime_effect(&effect, &[], &[("width", Value::Float(0.75))]),
    ] {
        let bytecode = invocation.program().bytecode();
        assert!(!bytecode.uses_pixel_context());
        let (query, _) = bytecode.prefix();
        assert!(query.iter().any(|op| matches!(
            op,
            Instruction::FloatUnary {
                op: FloatUnary::Smoothstep,
                ..
            }
        )));
        let mut workspace = StripWorkspace::default();
        for frame in [0, 1, 15, 30, 60, 0] {
            let context = context(8, 0, frame);
            let t =
                (crate::values::sample_duration_seconds_f32(context.time) / 0.75).clamp(0.0, 1.0);
            let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.0, 0.0);
            let actual = invocation.evaluate(&context, &SPATIAL, &mut workspace);
            assert!(
                actual.red.abs_diff(expected.red) <= 1,
                "frame {frame}: {actual:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn hoisted_reduction_work_keeps_empty_and_filtered_results() {
    let effect = compile_effect(
        "effect Zero { param count: int in -1..4 = 0; sample {
            let d = max(target.count + 1.0, 1.0);
            let last = last for i in 0..count { guard pixel.index == 0; 1.0 / d } else { 0.25 };
            let result = sum for i in 0..count { i / d };
            rgb(last, result / 10.0, 0.0)
        } }",
    );
    for count in [-1, 0, 3, 4] {
        let values = [("count", Value::Int(count))];
        for invocation in [
            playback::lower_sample(&bind(&effect, &values)),
            runtime_effect(&effect, &[], &values),
        ] {
            let mut workspace = StripWorkspace::default();
            for pixel in [0, 1, 0] {
                let color = invocation.evaluate(&context(2, pixel, 0), &SPATIAL, &mut workspace);
                assert_eq!(
                    color.red,
                    if count > 0 && pixel == 0 { 85 } else { 64 },
                    "count={count} pixel={pixel}"
                );
                assert_eq!(
                    color.green,
                    match count {
                        3 => 26,
                        4 => 51,
                        _ => 0,
                    },
                    "count={count} pixel={pixel}"
                );
            }
        }
    }
}

#[test]
fn reciprocals_of_missing_and_infinite_divisors_divide_exactly() {
    let effect = compile_effect(
        "effect Missing { param divisor: float in 0.0..10.0 = 1.0;
        sample {
            let d = max(divisor, 0.01);
            rgb(value_or(sum for i in 0..4 { i / d }, 0.25), 0.0, 0.0)
        }
    }",
    );
    for (value, expected) in [(f32::NAN, 64), (2.0, 255), (f32::INFINITY, 0)] {
        let invocation = runtime_effect(&effect, &[], &[("divisor", Value::Float(value))]);
        assert_eq!(
            invocation
                .evaluate(&context(1, 0, 0), &SPATIAL, &mut StripWorkspace::default())
                .red,
            expected,
            "{value}"
        );
    }
}
