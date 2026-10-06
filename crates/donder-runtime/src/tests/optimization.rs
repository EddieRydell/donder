use super::evaluation::{OperatorEvaluation, SampleEvaluation, context};
use super::std;
use std::prelude::rust_2024::*;

use super::evaluation::SignalSampler;
use crate::dsl::{BatchWorkspace, RuntimeError};
use donder_language::dsl::bytecode::{Instruction, SignalPixel};
use donder_language::dsl::{Color, Value, compile_effects, compile_operators};
use donder_language::execution::SpatialContext;
use donder_language::values::SampleTime;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[test]
fn propagation_merges_branch_values_and_respects_loop_backedges_and_snapshots() {
    let effect = compile_effects(
        "effect Flow {
        color sample() {
            int n = 1;
            if (pixel_index() == 0) { n = 2; } else { n = 4; }
            int total = 0;
            for (int i = 0; i < 3; i = i + 1) {
                int previous = n;
                n = n + 1;
                total = total + previous;
            }
            return rgb(total / 30.0, n / 10.0, 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let mut workspace = BatchWorkspace::default();
    for (pixel, red, green) in [(0, 77, 128), (1, 128, 179), (0, 77, 128)] {
        assert_eq!(
            effect.evaluate(&context(2, pixel, 0), &SPATIAL, &mut workspace),
            Color {
                red,
                green,
                blue: 0
            }
        );
    }
}

#[test]
fn branches_can_reuse_a_previously_materialized_boolean() {
    let effect = compile_effects(
        "effect Booleans { color sample() {
        bool first = pixel_index() == 0;
        bool second = pixel_index() == 1;
        if (first) { return rgb(1.0, 0.0, 0.0); }
        if (second) { return rgb(0.0, 1.0, 0.0); }
        return rgb(0.0, 0.0, 1.0);
    } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    for (pixel, expected) in [
        (
            0,
            Color {
                red: 255,
                green: 0,
                blue: 0,
            },
        ),
        (
            1,
            Color {
                red: 0,
                green: 255,
                blue: 0,
            },
        ),
        (
            2,
            Color {
                red: 0,
                green: 0,
                blue: 255,
            },
        ),
    ] {
        assert_eq!(
            effect.evaluate(
                &context(3, pixel, 0),
                &SPATIAL,
                &mut BatchWorkspace::default()
            ),
            expected
        );
    }
}

struct FailOnSample {
    calls: usize,
}

impl SignalSampler for FailOnSample {
    fn sample_signal(
        &mut self,
        _: usize,
        _: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.calls += 1;
        Err(RuntimeError {
            message: "observed sample".into(),
        })
    }
}

#[test]
fn direct_conditions_preserve_short_circuit_order_and_sampling_errors() {
    for (condition, should_sample) in [
        ("pixel_index() == 0 || source.at(0.0) == #ffffff", false),
        ("pixel_index() != 0 && source.at(0.0) == #ffffff", false),
        ("!(pixel_index() == 0) || source.at(0.0) == #ffffff", true),
        (
            "pixel_index() == 0 && (pixel_count() == 1 || source.at(0.0) == #ffffff)",
            false,
        ),
        (
            "pixel_index() == 0 && (pixel_count() != 1 || source.at(0.0) == #ffffff)",
            true,
        ),
    ] {
        let source = format!(
            "operator Branch {{ input Signal source; color sample() {{ if ({condition}) {{ return #ffffff; }} return #000000; }} }}"
        );
        let operator = compile_operators(&source)
            .unwrap()
            .remove(0)
            .bind([])
            .unwrap();
        let mut sampler = FailOnSample { calls: 0 };
        let result = operator.evaluate(
            &context(1, 0, 0),
            &SPATIAL,
            &mut sampler,
            &mut BatchWorkspace::default(),
        );
        assert_eq!(result.is_err(), should_sample, "{condition}");
        assert_eq!(sampler.calls, usize::from(should_sample), "{condition}");
    }
}

#[test]
fn dead_sample_results_preserve_errors() {
    let operator = compile_operators("operator Dead { input Signal source; color sample() { color unused = source.at(0.0); return #000000; } }").unwrap().remove(0).bind([]).unwrap();
    let mut sampler = FailOnSample { calls: 0 };
    assert!(
        operator
            .evaluate(
                &context(1, 0, 0),
                &SPATIAL,
                &mut sampler,
                &mut BatchWorkspace::default()
            )
            .is_err()
    );
    assert_eq!(sampler.calls, 1);
}

#[test]
fn monotonic_loop_guards_preserve_ascending_descending_and_nested_counts() {
    for (loop_header, predicate, expected) in [
        ("int i = 0; i < 20; i = i + 2", "i < 5.0", 3),
        ("int i = 20; i > 0; i = i - 2", "i > 15.0", 3),
        ("int i = 0; i < 20; i = i + 1", "10.0 - i > 6.0", 4),
        ("int i = 1; i < 512; i = i * 2", "i < 5.0", 3),
        ("int i = -1; i > -512; i = i * 2", "i > -5.0", 3),
    ] {
        let source = format!(
            "effect Limited {{ color sample() {{
            int total = 0;
            for (int outer = 0; outer < 2; outer = outer + 1) {{
                for ({loop_header}) {{ if ({predicate}) {{ total = total + 1; }} }}
            }}
            return rgb(total / 20.0, 0.0, 0.0);
        }} }}"
        );
        let effect = compile_effects(&source).unwrap().remove(0);
        let bound = effect.bind([]).unwrap();
        let mut workspace = BatchWorkspace::default();
        for _ in 0..3 {
            assert_eq!(
                bound
                    .evaluate(&context(1, 0, 0), &SPATIAL, &mut workspace)
                    .red,
                (expected as f32 / 10.0 * 255.0 + 0.5) as u8,
                "{source}"
            );
        }
    }
}

#[test]
fn multiplicative_loop_guards_preserve_wraparound_alternation_and_live_outs() {
    for (header, body, result, expected) in [
        (
            "int i = 1; i > 0; i = i * 2",
            "if (i < 4.0) { total = total + 1; }",
            "total / 10.0",
            51,
        ),
        (
            "int i = 1; i < 100; i = i * -2",
            "if (i < 0.0) { total = total + 1; }",
            "total / 10.0",
            102,
        ),
        (
            "int i = 1; i < 512; i = i * 2",
            "if (i < 4.0) { total = total + 1; } last = i;",
            "last / 256.0",
            255,
        ),
    ] {
        let source = format!(
            "effect Observe {{ color sample() {{
            int total = 0; int last = 0;
            for ({header}) {{ {body} }}
            return rgb({result}, 0.0, 0.0);
        }} }}"
        );
        let effect = compile_effects(&source).unwrap().remove(0);
        assert_eq!(
            effect
                .bind([])
                .unwrap()
                .evaluate(&context(1, 0, 0), &SPATIAL, &mut BatchWorkspace::default())
                .red,
            expected,
            "{source}"
        );
    }
}

#[test]
fn loop_rejection_proofs_preserve_live_values_and_nonmonotonic_conditions() {
    for (body, result, expected) in [
        (
            "if (i < 3.0) { total = total + 1; } last = i;",
            "last / 9.0",
            255,
        ),
        (
            "if (i % 3 == 0) { total = total + 1; }",
            "total / 10.0",
            102,
        ),
        (
            "float x = sin(i); if (x > 0.0) { total = total + 1; }",
            "total / 10.0",
            153,
        ),
        (
            "if (changing > 0.0) { total = total + 1; } changing = -changing;",
            "total / 10.0",
            128,
        ),
    ] {
        let source = format!(
            "effect Observe {{ color sample() {{
            int total = 0; int last = 0; float changing = 1.0;
            for (int i = 0; i < 10; i = i + 1) {{ {body} }}
            return rgb({result}, 0.0, 0.0);
        }} }}"
        );
        let effect = compile_effects(&source).unwrap().remove(0);
        assert_eq!(
            effect
                .bind([])
                .unwrap()
                .evaluate(&context(1, 0, 0), &SPATIAL, &mut BatchWorkspace::default())
                .red,
            expected,
            "{source}"
        );
    }
}

#[test]
fn rejecting_loop_guards_do_not_skip_earlier_sampling_errors() {
    let operator = compile_operators(
        "operator Ordered { input Signal source;
        color sample() { color result = #000000;
            for (int i = 0; i < 10; i = i + 1) {
                color observed = source.at(i);
                if (i < -1.0) { result = observed; }
            }
            return result;
        }
    }",
    )
    .unwrap()
    .remove(0);
    let mut sampler = FailOnSample { calls: 0 };
    assert!(
        operator
            .bind([])
            .unwrap()
            .evaluate(
                &context(1, 0, 0),
                &SPATIAL,
                &mut sampler,
                &mut BatchWorkspace::default()
            )
            .is_err()
    );
    // Every iteration samples; the guard cannot remove observable queries.
    assert_eq!(sampler.calls, 10);
}

#[test]
fn invariant_loop_divisors_divide_correctly_with_either_sign() {
    for denominator in ["max(scale, 0.01)", "-max(scale, 0.01)"] {
        let source = format!(
            "effect Divide {{ param float scale in 0.0..10.0 = 2.0;
            color sample() {{ float total = 0.0;
                for (int i = 0; i < 4; i = i + 1) {{
                    float d = {denominator};
                    total = total + abs(i / d);
                }}
                return rgb(total / 10.0, 0.0, 0.0);
            }}
        }}"
        );
        let effect = compile_effects(&source).unwrap().remove(0);
        assert_eq!(
            effect
                .bind([])
                .unwrap()
                .evaluate(&context(1, 0, 0), &SPATIAL, &mut BatchWorkspace::default())
                .red,
            77
        );
    }
}

#[test]
fn smoothstep_with_bounded_edges_preserves_boundaries() {
    for (edge0, edge1) in [("0.0", "max(width, 0.01)"), ("max(width, 0.01)", "0.0")] {
        let effect = compile_effects(&format!(
            "effect Smooth {{ param float width in 0.0..1.0 = 0.75;
            color sample() {{ return rgb(smoothstep({edge0}, {edge1}, pixel_fraction()), 0.0, 0.0); }} }}"
        ))
        .unwrap()
        .remove(0);
        let effect = effect.bind([]).unwrap();
        let mut workspace = BatchWorkspace::default();
        for pixel in 0..65 {
            let context = context(65, pixel, 0);
            let (left, right) = if edge0 == "0.0" {
                (0.0, 0.75)
            } else {
                (0.75, 0.0)
            };
            let t = ((context.pixel_fraction - left) / (right - left)).clamp(0.0, 1.0);
            let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.0, 0.0);
            let actual = effect.evaluate(&context, &SPATIAL, &mut workspace);
            assert!(
                actual.red.abs_diff(expected.red) <= 1,
                "pixel {pixel}: {actual:?} vs {expected:?}"
            );
        }
    }
}

#[test]
fn smoothstep_keeps_varying_and_degenerate_edge_semantics() {
    let effect = compile_effects(
        "effect Smooth {
        color sample() {
            float edge = pixel_fraction();
            return rgb(smoothstep(edge, 0.5, 0.25),
                       value_or(smoothstep(edge, edge, edge), 0.25),
                       smoothstep(0.0, 1.0, 0.5));
        } }",
    )
    .unwrap()
    .remove(0)
    .bind([])
    .unwrap();
    let mut workspace = BatchWorkspace::default();
    for pixel in 0..65 {
        let context = context(65, pixel, 0);
        let edge = context.pixel_fraction;
        let t = ((0.25 - edge) / (0.5 - edge)).clamp(0.0, 1.0);
        let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.25, 0.5);
        assert_eq!(
            effect.evaluate(&context, &SPATIAL, &mut workspace),
            expected
        );
    }
}

#[test]
fn uniform_smoothstep_is_admitted_in_query_initialization() {
    let effect = compile_effects(
        "effect Smooth { param float width in 0.0..1.0 = 0.75;
        color sample() { return rgb(smoothstep(0.0, width, seconds()), 0.0, 0.0); } }",
    )
    .unwrap()
    .remove(0);
    let bytecode = effect.sample_program().bytecode();
    let (program, inputs) = effect.sample_program().clone().into_parts();
    assert!(donder_language::dsl::SampleProgram::admit(program, inputs).is_some());
    assert!(
        bytecode.instructions[..bytecode.pixel_entry as usize]
            .iter()
            .any(|op| matches!(op, Instruction::Smoothstep { .. }))
    );
    let bound = effect.bind([]).unwrap();
    let mut workspace = BatchWorkspace::default();
    for frame in [0, 1, 15, 30, 60, 0] {
        let context = context(8, 0, frame);
        let t = (crate::values::sample_duration_seconds_f32(context.time) / 0.75).clamp(0.0, 1.0);
        let expected = crate::sampling::rgb(t * t * (3.0 - 2.0 * t), 0.0, 0.0);
        assert_eq!(bound.evaluate(&context, &SPATIAL, &mut workspace), expected);
    }
}

#[test]
fn loop_hoisting_preserves_zero_trip_and_conditional_live_out_values() {
    for count in [-1, 0, 3, 100] {
        let source = "effect Zero { param int count in -1..4; color sample() {
            float last = 0.25; float result = 0.0;
            for (int i in range(count)) {
                float d = max(pixel_count() + 1.0, 1.0);
                if (pixel_index() == 0) { last = 1.0 / d; }
                result = result + i / d;
            }
            return rgb(last, result / 10.0, 0.0);
        } }";
        // Positional binding reaches counts past the declared range; the VM caps them.
        let effect = donder_language::dsl::SampleDefinition::new(
            compile_effects(source)
                .unwrap()
                .remove(0)
                .sample_program()
                .clone(),
        )
        .bind(vec![Value::Int(count)])
        .unwrap();
        let mut workspace = BatchWorkspace::default();
        for pixel in [0, 1, 0] {
            let color = effect.evaluate(&context(2, pixel, 0), &SPATIAL, &mut workspace);
            assert_eq!(
                color.red,
                if count > 0 && pixel == 0 { 85 } else { 64 },
                "{source}"
            );
            assert_eq!(
                color.green,
                match count {
                    3 => 26,
                    100 => 51,
                    _ => 0,
                },
                "{source}"
            );
        }
    }
}

#[test]
fn invariant_reciprocals_preserve_missing_values() {
    let effect = compile_effects(
        "effect Missing { param float divisor in 0.0..10.0;
        color sample() {
            float d = max(divisor, 0.01); float total = 0.0;
            for (int i = 0; i < 4; i = i + 1) { total = total + i / d; }
            return rgb(value_or(total, 0.25), 0.0, 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0);
    for (value, expected) in [(f32::NAN, 64), (2.0, 255), (f32::INFINITY, 0)] {
        // Positional binding: non-finite values lie outside any declared range.
        let bound = donder_language::dsl::SampleDefinition::new(effect.sample_program().clone())
            .bind(vec![Value::Float(value)])
            .unwrap();
        assert_eq!(
            bound
                .evaluate(&context(1, 0, 0), &SPATIAL, &mut BatchWorkspace::default())
                .red,
            expected
        );
    }
}
