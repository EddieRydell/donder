//! Language semantics, executed by the VM and, where the values are fixed for
//! an instance, by preparation.
use super::evaluation::{
    OperatorEvaluation, PixelContext, SampleEvaluation, SignalSampler, bind, compile_effect,
    compile_operator, effect, operator, runtime_effect, runtime_operator, try_bind,
};
use super::playback;
use super::std;
use crate::dsl::{RunContext, RuntimeError, StripWorkspace};
use donder_language::dsl::bytecode::SignalPixel;
use donder_language::dsl::{Color, Value, compile_effects, compile_operators};
use donder_language::values::{Marks, SampleDuration, SampleTime};
use std::prelude::rust_2024::*;

const SPATIAL: donder_language::execution::SpatialContext =
    donder_language::execution::SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };

const WHITE: Color = Color {
    red: 255,
    green: 255,
    blue: 255,
};

/// One pixel of a one-pixel target, `progress` through a one-second clip.
fn at(progress: f32) -> PixelContext {
    PixelContext {
        run: RunContext {
            progress,
            time: SampleDuration::from_ticks((progress * 1_000_000.0) as u32),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_count: 1,
        },
        index: 0,
        fraction: 0.0,
    }
}

fn pixel(index: usize, count: usize) -> PixelContext {
    PixelContext {
        run: RunContext {
            pixel_count: count as i32,
            ..at(0.25).run
        },
        index: index as i32,
        fraction: index as f32 / (count - 1).max(1) as f32,
    }
}

fn messages(diagnostics: &[donder_language::dsl::Diagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn effect_error(source: &str) -> String {
    messages(&compile_effects(source).unwrap_err())
}

fn operator_error(source: &str) -> String {
    messages(&compile_operators(source).unwrap_err())
}

/// `predicate` holds for `values` when preparation computes it, and when the
/// VM does because every parameter is left to playback.
fn assert_holds(params: &str, predicate: &str, values: &[(&str, Value)]) {
    let effect = compile_effect(&format!(
        "effect Probe {{ {params} sample {{ if {predicate} {{ #ffffff }} else {{ #000000 }} }} }}"
    ));
    let mut workspace = StripWorkspace::default();
    if let Ok(fixed) = try_bind(&effect, values) {
        assert_eq!(
            playback::lower_sample(&fixed).evaluate(&at(0.25), &SPATIAL, &mut workspace),
            WHITE,
            "prepared: {predicate} with {values:?}"
        );
    }
    assert_eq!(
        runtime_effect(&effect, &[], values).evaluate(&at(0.25), &SPATIAL, &mut workspace),
        WHITE,
        "runtime: {predicate} with {values:?}"
    );
}

#[test]
fn declaration_kinds_are_source_specific() {
    assert!(
        effect_error("operator Gain { input source; sample { source } }")
            .contains("operator declarations belong in operator sources")
    );
    assert!(
        operator_error("effect Solid { sample { #ffffff } }")
            .contains("effect declarations belong in effect sources")
    );
}

#[test]
fn let_refinements_and_choices_do_not_leak_between_evaluations() {
    let effect = effect(
        "effect Refined {
            param amount: float in 0.0..1.0 = 0.25;
            sample {
                let amount = if progress > 0.5 { amount + 0.25 } else { amount };
                let amount = amount + sum for i in 0..3 { i * 0.0625 };
                rgb(amount, 0.0, 0.0)
            }
        }",
    );
    let mut workspace = StripWorkspace::default();
    for (progress, red) in [(0.0, 0.4375), (1.0, 0.6875), (0.0, 0.4375)] {
        assert_eq!(
            effect.evaluate(&at(progress), &SPATIAL, &mut workspace),
            crate::sampling::rgb(red, 0.0, 0.0)
        );
    }
}

#[test]
fn length_bounded_reductions_iterate_marks_and_reject_long_collections() {
    let effect = compile_effect(
        "effect Iterate {
            param beats: marks;
            sample { rgb(sum for mark in 0..len(beats) { max(mark_at(beats, mark) - time, 0.0) } / 10.0, 0.0, 0.0) }
        }",
    );
    let marks = |ticks: &[u32]| {
        Value::Marks(Marks::new(ticks.iter().copied().map(SampleDuration::from_ticks)).into())
    };
    let sample = |beats| {
        playback::lower_sample(&bind(&effect, &[("beats", beats)])).evaluate(
            &at(0.0),
            &SPATIAL,
            &mut StripWorkspace::default(),
        )
    };
    assert_eq!(
        sample(marks(&[1_000_000, 2_000_000])),
        crate::sampling::rgb(0.3, 0.0, 0.0)
    );
    assert_eq!(sample(marks(&[])), Color::BLACK);

    // A bound that depends on a length is checked when values are supplied.
    let many =
        |count: u32| Value::Marks(Marks::new((0..count).map(SampleDuration::from_ticks)).into());
    assert!(try_bind(&effect, &[("beats", many(10_000))]).is_ok());
    let error = try_bind(&effect, &[("beats", many(10_001))]).unwrap_err();
    assert!(
        error.message.contains("a reduction can run 10001 times"),
        "{}",
        error.message
    );
    let doubled = compile_effect(
        "effect Doubled {
            param values: array<float>;
            sample { rgb(sum for i in 0..len(values) * 2 { values[int(i / 2)] }, 0.0, 0.0) }
        }",
    );
    let values = |count| Value::Array(vec![Value::Float(0.0); count].into());
    assert!(doubled.check_values(&[values(5_000)]).is_ok());
    assert!(doubled.check_values(&[values(5_001)]).is_err());
}

#[test]
fn integer_comparisons_do_not_round_through_float() {
    assert_holds(
        "param left: int in 0..16777217 = 16777217;
         param right: int in 0..16777216 = 16777216;",
        "left > 16777216 && left > right && right < left && left != right",
        &[
            ("left", Value::Int(16_777_217)),
            ("right", Value::Int(16_777_216)),
        ],
    );
}

#[test]
fn reduction_bounds_are_proven_from_literals_ranges_lengths_and_enclosing_indices() {
    for source in [
        "sum for i in 0..3 { i }",
        "sum for i in 0..count { i }",
        "sum for i in 0..10000 { i }",
        "sum for i in -5000..=4999 { i }",
        "sum for i in 0..int(min(pixel.index, 8)) { i }",
        "sum for i in 0..int(clamp(pixel.index, 0, 8)) { i }",
        "sum for i in 0..pixel.index % 4 { i }",
        "sum for i in 0..4 { sum for j in 0..i { j } }",
        "sum for i in 0..int(abs(sin(time)) * 10.0) { i }",
        "int(pow(2.0, count))",
    ] {
        assert!(
            compile_effects(&format!(
                "effect Bounded {{ param count: int in -3..4 = 3; sample {{ rgb({source}, 0.0, 0.0) }} }}"
            ))
            .is_ok(),
            "{source}"
        );
    }
    for (source, what) in [
        ("sum for i in 0..pixel.index { i }", "reduction"),
        ("sum for i in 0..10001 { i }", "reduction"),
        ("sum for i in 0..=10000 { i }", "reduction"),
        ("sum for i in 0..count * 10000 { i }", "reduction"),
        ("sum for i in 0..int(time) { i }", "reduction"),
    ] {
        let message = effect_error(&format!(
            "effect Unbounded {{ param count: int in -3..4 = 3; sample {{ rgb({source}, 0.0, 0.0) }} }}"
        ));
        assert!(
            message.contains(&format!(
                "cannot prove this {what} runs at most 10000 times"
            )),
            "{source}: {message}"
        );
    }

    // Ranges that end before they start are empty.
    let effect = compile_effect(
        "effect Count { param count: int in -3..4 = 3; sample {
            rgb(sum for i in 0..count { i } / 10.0, sum for i in count..2 { 1 } / 10.0, 0.0)
        } }",
    );
    for (count, red, green) in [(-3, 0, 5), (0, 0, 2), (3, 3, 0), (4, 6, 0)] {
        let expected = crate::sampling::rgb(red as f32 / 10.0, green as f32 / 10.0, 0.0);
        let runtime = runtime_effect(&effect, &[], &[("count", Value::Int(count))]);
        let fixed = playback::lower_sample(&bind(&effect, &[("count", Value::Int(count))]));
        for invocation in [runtime, fixed] {
            assert_eq!(
                invocation.evaluate(&at(0.0), &SPATIAL, &mut StripWorkspace::default()),
                expected,
                "count={count}"
            );
        }
    }
}

#[test]
fn nested_reductions_restart_their_inner_iterations() {
    let effect = compile_effect(
        "effect Nested { param count: int in 0..3 = 2; sample {
            let total = sum for outer in 0..2 { sum for inner in 0..count { 1 } };
            let triangle = sum for outer in 0..3 { sum for inner in 0..outer + count { inner } };
            if total == 2 * count && triangle == 10 { #ffffff } else { #000000 }
        } }",
    );
    let fixed = playback::lower_sample(&bind(&effect, &[]));
    let runtime = runtime_effect(&effect, &[], &[("count", Value::Int(2))]);
    let mut workspace = StripWorkspace::default();
    for invocation in [&fixed, &runtime, &fixed] {
        assert_eq!(
            invocation.evaluate(&at(0.0), &SPATIAL, &mut workspace),
            WHITE
        );
    }
}

#[test]
fn array_literals_are_indexed_and_measured_through_lets() {
    let effect = effect(
        "effect Arrays { sample {
            let values = [progress, [0.125, 0.25, 0.5][2]];
            let saved = values;
            let values = [0.75];
            rgb(saved[0], saved[1], values[0] * len(saved) / 2.0)
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for _ in 0..3 {
        assert_eq!(
            effect.evaluate(&at(0.25), &SPATIAL, &mut workspace),
            crate::sampling::rgb(0.25, 0.5, 0.75)
        );
    }
    for (body, message) in [
        (
            "let items = [#ffffff, 1.0]; items[0]",
            "expected a float, found a color",
        ),
        (
            "let items = [[1.0], [2.0]]; #ffffff",
            "array literals hold numbers, bools or colors",
        ),
        ("let items = [1.0]; items", "expected a color"),
    ] {
        let error = effect_error(&format!("effect Bad {{ sample {{ {body} }} }}"));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn guards_skip_to_black_or_produce_their_else_value() {
    let effect = effect(
        "effect Guarded { sample {
            guard pixel.index != 0;
            guard pixel.index != 1 else #ff0000;
            let level = if pixel.index == 2 { 0.5 } else { 1.0 };
            guard level < 1.0 else {
                guard pixel.index != 3 else rgb(0.0, 0.0, level);
                if pixel.index == 4 { guard false; #ffffff } else { #00ff00 }
            };
            rgb(0.0, level, 0.0)
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for (index, expected) in [
        (0, Color::BLACK),
        (
            1,
            Color {
                red: 255,
                green: 0,
                blue: 0,
            },
        ),
        (2, crate::sampling::rgb(0.0, 0.5, 0.0)),
        (3, crate::sampling::rgb(0.0, 0.0, 1.0)),
        (4, Color::BLACK),
        (5, crate::sampling::rgb(0.0, 1.0, 0.0)),
        (0, Color::BLACK),
    ] {
        assert_eq!(
            effect.evaluate(&pixel(index, 8), &SPATIAL, &mut workspace),
            expected,
            "pixel {index}"
        );
    }
    let error = effect_error(
        "effect Bad { sample { let x = { guard progress > 0.5; 1.0 }; rgb(x, 0.0, 0.0) } }",
    );
    assert!(
        error.contains("a guard without `else` can only skip a sample block or a reduction body"),
        "{error}"
    );
    let error = effect_error("effect Bad { sample { if progress > 0.5 { #ffffff } } }");
    assert!(error.contains("needs an `else`"), "{error}");
}

#[test]
fn reductions_start_from_their_identity_and_skip_guarded_iterations() {
    let params = "param n: int in 0..8 = 4;";
    for (n, predicate) in [
        (0, "max for i in 0..n { i * 1.0 } == -1.0 / 0.0"),
        (0, "min for i in 0..n { i * 1.0 } == 1.0 / 0.0"),
        (0, "max for i in 0..n { i } == -2147483647 - 1"),
        (0, "min for i in 0..n { i } == 2147483647"),
        (0, "sum for i in 0..n { i * 1.5 } == 0.0"),
        (0, "!(any for i in 0..n { true })"),
        (0, "all for i in 0..n { false }"),
        (0, "first for i in 0..n { i } else { -7 } == -7"),
        (0, "last for i in 0..n { i } else { -7 } == -7"),
        (0, "first for i in 0..n { #ffffff } == #000000"),
        (0, "max for i in 0..n { #ffffff } == #000000"),
        (4, "max for i in 0..n { 2 - i } == 2"),
        (4, "min for i in 0..n { 2.0 - i } == -1.0"),
        (4, "sum for i in 0..n { i } == 6"),
        (4, "sum for i in 0..=n { i } == 10"),
        (4, "max for i in 0..n { guard i != 3; i } == 2"),
        (4, "min for i in 0..n { guard i > 1; i } == 2"),
        (4, "sum for i in 0..n { guard i % 2 == 1; i } == 4"),
        (4, "sum for i in 0..n { guard i % 2 == 1 else 10; i } == 24"),
        (4, "any for i in 0..n { i == 3 }"),
        (4, "!(any for i in 0..n { guard i != 3; i == 3 })"),
        (4, "all for i in 0..n { i < 4 }"),
        (4, "all for i in 0..n { guard i < 2; i < 2 }"),
        (4, "!(all for i in 0..n { i < 3 })"),
        (4, "first for i in 0..n { guard i > 1; i } else { -1 } == 2"),
        (4, "last for i in 0..n { guard i < 2; i } else { -1 } == 1"),
        (4, "last for i in 0..n { guard i > 9; i } else { -1 } == -1"),
        (
            4,
            "first for i in 0..n { guard i > 0; [0.25, 0.5, 0.75][i] } else { 0.0 } == 0.5",
        ),
        (4, "sum for i in 0..n { #404040 } == #ffffff"),
        (
            4,
            "max for i in 0..n { [#ff0000, #00ff00, #0000ff, #000000][i] } == #ffffff",
        ),
        (
            4,
            "last for i in 0..n { guard i < 3; [#ff0000, #00ff00, #0000ff, #000000][i] } == #0000ff",
        ),
        (2, "sum for i in 0..n { sum for j in i..n { 1 } } == 3"),
    ] {
        assert_holds(params, predicate, &[("n", Value::Int(n))]);
    }
    for (body, message) in [
        ("max for i in 0..3 { true }", "`max` cannot combine a bool"),
        (
            "min for i in 0..3 { #ffffff }",
            "`min` cannot combine a color",
        ),
        (
            "first for i in 0..3 { i }",
            "`first` of an int needs an `else` value",
        ),
        ("any for i in 0..3 { i }", "expected a bool, found an int"),
        ("each for i in 0..3 { i }", "`each` is not a reducer"),
    ] {
        let error = effect_error(&format!(
            "effect Bad {{ sample {{ let x = {body}; #000000 }} }}"
        ));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn only_first_and_last_take_an_else_so_a_guard_keeps_its_own() {
    let effect = compile_effect(
        "effect Guarded { param n: int in 0..8 = 4; sample {
            guard all for i in 0..n { i < 3 } else #ff0000;
            guard any for i in 0..n { i == 1 } else #0000ff;
            #00ff00
        } }",
    );
    for (n, expected) in [(4, "#ff0000"), (3, "#00ff00"), (1, "#0000ff")] {
        let values = [("n", Value::Int(n))];
        for invocation in [
            playback::lower_sample(&bind(&effect, &values)),
            runtime_effect(&effect, &[], &values),
        ] {
            assert_eq!(
                invocation.evaluate(&at(0.0), &SPATIAL, &mut StripWorkspace::default()),
                Color::from_hex(expected).unwrap(),
                "n={n}"
            );
        }
    }
}

#[test]
fn powers_take_int_and_float_exponents() {
    let params = "param x: float in -4.0..4.0 = 1.5; param n: int in -2..8 = 3;";
    for (x, n, expected) in [
        (1.5, 3, 3.375),
        (-2.0, 3, -8.0),
        (0.0, 0, 1.0),
        (4.0, 0, 1.0),
        (0.5, 8, 0.00390625),
        // A negative exponent divides.
        (2.0, -2, 0.25),
    ] {
        assert_holds(
            params,
            &format!("pow(x, n) == {expected:?}"),
            &[("x", Value::Float(x)), ("n", Value::Int(n))],
        );
    }
    // Powers of a reduction index.
    assert_holds(
        params,
        "sum for i in 0..=n { pow(x, i) } == 1.0 + 1.5 + 2.25 + 3.375",
        &[("x", Value::Float(1.5)), ("n", Value::Int(3))],
    );
    // Float exponents take the general power function.
    assert_holds(
        params,
        "pow(x, 0.5) == 2.0",
        &[("x", Value::Float(4.0)), ("n", Value::Int(0))],
    );
}

#[test]
fn operator_requires_a_signal_input() {
    let error = operator_error("operator Empty { sample { #000000 } }");
    assert!(
        error.contains("an operator declares at least one `input`"),
        "{error}"
    );
}

#[test]
fn workspaces_are_reused_across_programs_with_different_slot_counts() {
    let large = effect(
        "effect Large { param iterations: int in 0..256 = 4; sample {
            let values = [progress, 0.1, 0.9, progress * 0.5];
            let total = sum for i in 0..iterations { values[i % 4] * 0.01 };
            let nested = max for i in 0..3 { min for j in 0..4 { values[(i + j) % 4] } };
            rgb(values[0], total, nested)
        } }",
    );
    let small = effect("effect Small { sample { rgb(0.0, 0.0, 0.0) } }");
    let mut workspace = StripWorkspace::default();
    for progress in [0.25, 0.5, 0.0] {
        let context = at(progress);
        let expected = large.evaluate(&context, &SPATIAL, &mut StripWorkspace::default());
        for _ in 0..2 {
            assert_eq!(large.evaluate(&context, &SPATIAL, &mut workspace), expected);
            assert_eq!(
                small.evaluate(&context, &SPATIAL, &mut workspace),
                Color::BLACK
            );
        }
    }
}

#[test]
fn enums_compare_by_option_name_across_declarations() {
    for options in ["Alpha, Beta, Gamma", "Gamma, Alpha, Beta"] {
        for predicate in [
            "wide == subset",
            "wide == Beta && subset == Beta",
            "wide != Alpha && subset != Gamma && other != subset",
            "(if pixel.index >= 0 { subset } else { other }) == Beta",
            "(if pixel.index < 0 { subset } else { other }) == Gamma",
        ] {
            assert_holds(
                &format!(
                    "param wide: enum {{ {options} }} = Alpha;
                     param subset: enum {{ Gamma, Beta }} = Gamma;
                     param other: enum {{ Gamma, Beta }} = Beta;"
                ),
                predicate,
                &[
                    ("wide", enum_value("Beta")),
                    ("subset", enum_value("Beta")),
                    ("other", enum_value("Gamma")),
                ],
            );
        }
    }
    let error = effect_error(
        "effect Bad { param mode: enum { A, B } = A; sample { if mode == C { #ffffff } else { #000000 } } }",
    );
    assert!(
        error.contains("`C` is not an option of this enum"),
        "{error}"
    );
}

#[test]
fn choices_hold_one_enum_type() {
    let error = effect_error(
        "effect Bad {
           param wide: enum { Alpha, Beta, Gamma } = Alpha;
           param subset: enum { Gamma, Beta } = Gamma;
           sample { if (if pixel.index >= 0 { subset } else { wide }) == wide { #ffffff } else { #000000 } }
         }",
    );
    assert!(error.contains("the branches produce"), "{error}");
    assert_holds(
        "param subset: enum { Gamma, Beta } = Gamma;
         param other: enum { Gamma, Beta } = Beta;",
        "(if pixel.index >= 0 { subset } else { other }) == Beta",
        &[
            ("subset", enum_value("Beta")),
            ("other", enum_value("Gamma")),
        ],
    );
}

fn enum_value(name: &str) -> Value {
    Value::Enum(donder_language::dsl::Identifier::new(name.into()).unwrap())
}

#[test]
fn enum_params_require_distinct_options() {
    assert!(compile_effects("effect Bad { param mode: enum {}; sample { #000000 } }").is_err());
    let error = effect_error("effect Bad { param mode: enum { A, A } = A; sample { #000000 } }");
    assert!(error.contains("option `A` is listed twice"), "{error}");
}

#[test]
fn signals_are_only_operator_inputs() {
    assert!(
        effect_error("effect Bad { input source; sample { #000000 } }")
            .contains("effects have no signal inputs")
    );
    assert!(
        operator_error("operator Bad { input source; param stored: Signal; sample { source } }")
            .contains("unknown type `Signal`")
    );
    assert!(
        operator_error("operator Bad { input source; sample { let t = time; t.at(0.0) } }")
            .contains("only operator inputs have methods")
    );
    assert!(
        operator_error("operator Bad { input source; sample { source.later(time) } }")
            .contains("signals have no method `later`")
    );
    assert!(
        operator_error("operator Bad { input source; input source; sample { source } }")
            .contains("`source` is declared twice")
    );
}

#[test]
fn signal_sampling_and_color_operations_execute() {
    let operator = operator(
        "operator Colors { input source; sample {
            let sampled = source.at(time);
            max(invert(sampled) * 0.5 + #010203, sampled * #ffffff)
        } }",
    );
    let color = operator
        .evaluate(
            &at(0.25),
            &SPATIAL,
            &mut ConstantSignal(Color {
                red: 10,
                green: 20,
                blue: 30,
            }),
            &mut StripWorkspace::default(),
        )
        .unwrap();
    assert_eq!(
        color,
        Color {
            red: 124,
            green: 120,
            blue: 116
        }
    );
}

#[test]
fn literal_and_type_errors_report_diagnostics() {
    for (body, message) in [
        (
            "let value = 2147483648; #000000",
            "integer literal is out of range",
        ),
        (
            "let value = 999999999999999999999999999999; #000000",
            "invalid number",
        ),
        (
            "let value = 340282350000000000000000000000000000000000.0; #000000",
            "float literal is out of range",
        ),
        (
            "let value: int = 4 / 2; #000000",
            "expected an int, found a float",
        ),
        (
            "let value = 1 + true; #000000",
            "`+` does not apply to an int and a bool",
        ),
        ("let value = 1 < 2 < 3; #000000", ""),
        (
            "let progress = 1.0; #000000",
            "`progress` is a reserved name",
        ),
        ("unknown", "unknown name `unknown`"),
        ("rgb(1.0, 0.0)", "`rgb` takes 3 arguments, found 2"),
        ("frobnicate(1.0)", "unknown function `frobnicate`"),
        ("rgb(pixel.size, 0.0, 0.0)", "`pixel` has no field `size`"),
        ("rgb(pixel, 0.0, 0.0)", "use a field of `pixel`"),
        (
            "if true { #ffffff } else { 1.0 }",
            "the branches produce a color and a float",
        ),
        ("#ff00", "a color literal has six hexadecimal digits"),
    ] {
        let error = effect_error(&format!("effect Bad {{ sample {{ {body} }} }}"));
        assert!(error.contains(message), "{body}: {error}");
    }
}

#[test]
fn required_parameters_have_no_default_and_ranges_reject_values() {
    let effect = compile_effect(
        "effect Required { param amount: float in 0.0..1.0; sample { rgb(amount, 1 % 0, 0.0) } }",
    );
    let missing = try_bind(&effect, &[]).unwrap_err();
    assert!(
        missing
            .message
            .contains("missing required parameter `amount`")
    );
    let outside = try_bind(&effect, &[("amount", Value::Float(1.5))]).unwrap_err();
    assert!(
        outside
            .message
            .contains("parameter `amount` is outside its declared range")
    );
    assert!(try_bind(&effect, &[("amount", Value::Float(f32::NAN))]).is_err());
    let bound = playback::lower_sample(&bind(&effect, &[("amount", Value::Float(1.0))]));
    assert_eq!(
        bound.evaluate(&at(0.0), &SPATIAL, &mut StripWorkspace::default()),
        crate::sampling::rgb(1.0, 0.0, 0.0)
    );
    for (declaration, message) in [
        ("param amount: float;", "`amount` must declare its range"),
        (
            "param on: bool in 0..1;",
            "only int, float and curve parameters take a range",
        ),
        ("param amount: float in 1.0..0.0;", "minimum first"),
        (
            "param amount: float in 0.0..1.0 = 2.0;",
            "the default of `amount` is outside its range",
        ),
        (
            "param count: int in 0..4 = 1.5;",
            "the default of `count` must be an int",
        ),
        (
            "param shape: curve in 0.0..1.0 = 0.5;",
            "a curve has no literal, so `shape` has no default",
        ),
        (
            "param tables: array<array<float>>;",
            "an array's items cannot be arrays",
        ),
    ] {
        let error = effect_error(&format!(
            "effect Bad {{ {declaration} sample {{ #000000 }} }}"
        ));
        assert!(error.contains(message), "{declaration}: {error}");
    }
}

#[test]
fn integer_arithmetic_wraps_and_remainders_are_floored_and_total() {
    let params = "param a: int in -2147483648..2147483647 = 0;
                  param b: int in -2147483648..2147483647 = 0;
                  param x: float in -100.0..100.0 = 0.0;
                  param y: float in -100.0..100.0 = 0.0;";
    for (predicate, a, b) in [
        ("-a == -2147483647 - 1", i32::MIN, 0),
        ("a + b == -2147483647 - 1", i32::MAX, 1),
        ("a - b == 2147483647", i32::MIN, 1),
        ("a * b == -2", i32::MAX, 2),
        ("a % b == 0", 1, 0),
        ("a % b == 0", i32::MIN, -1),
        ("a % b == 2", -7, 3),
        ("a % b == -2", 7, -3),
        ("a % b == 1", 7, 3),
        ("a / b == 3.5", 7, 2),
        ("a < b && !(a > b) && a != b", 16_777_216, 16_777_217),
        ("int(a * 1.0) == a", 1 << 24, 0),
    ] {
        assert_holds(
            params,
            predicate,
            &[("a", Value::Int(a)), ("b", Value::Int(b))],
        );
    }
    for (predicate, x, y) in [
        ("x % y == 0.5", -7.5, 2.0),
        ("x % y == -0.5", 7.5, -2.0),
        ("is_nan(x % y)", 1.0, 0.0),
        ("int(x) == -2 && int(floor(x)) == -3", -2.5, 0.0),
        ("int(x / y) == 2147483647", 100.0, 0.0),
        ("int(x / y) == -2147483647 - 1", -100.0, 0.0),
        ("int(x / y) == 0", 0.0, 0.0),
        ("round_even(x) == 2.0 && round_even(y) == -4.0", 2.5, -3.5),
    ] {
        assert_holds(
            params,
            predicate,
            &[("x", Value::Float(x)), ("y", Value::Float(y))],
        );
    }
}

struct ConstantSignal(Color);

impl SignalSampler for ConstantSignal {
    fn sample_signal(
        &mut self,
        _input: usize,
        _sample_time: SampleTime,
        _pixel: SignalPixel<i32>,
        _frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        Ok(self.0)
    }
}

/// Records every sample and answers with a color derived from it.
#[derive(Default)]
struct Samples(Vec<(usize, u32, SignalPixel<i32>)>);

impl SignalSampler for Samples {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        _frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.0.push((input, time.as_ticks(), pixel));
        Ok(Color {
            red: (time.as_ticks() / 1000).min(255) as u8,
            green: input as u8,
            blue: pixel.index().copied().unwrap_or_default() as u8,
        })
    }
}

impl Samples {
    fn sorted(&self) -> Vec<(usize, u32, SignalPixel<i32>)> {
        let mut samples = self.0.clone();
        samples.sort_by_key(|&(input, time, pixel)| {
            let address = match pixel {
                SignalPixel::Current => (0, 0),
                SignalPixel::Local(index) => (1, index),
                SignalPixel::Global(index) => (2, index),
            };
            (input, time, address)
        });
        samples
    }
}

#[test]
fn signal_sampling_outside_the_portable_clock_returns_black_without_sampling() {
    let operator = compile_operator(
        "operator Query {
            input source;
            param query_seconds: float in -1.0..1.0 = 0.0;
            sample { source.at(query_seconds) }
        }",
    );
    let source_color = Color {
        red: 17,
        green: 34,
        blue: 51,
    };
    for (seconds, expected) in [
        (0.0, Some(source_color)),
        (0.5, Some(source_color)),
        (-1.0, None),
        (f32::NAN, None),
        (f32::INFINITY, None),
        (f32::NEG_INFINITY, None),
        (f32::MAX, None),
    ] {
        let invocation =
            runtime_operator(&operator, &[], &[("query_seconds", Value::Float(seconds))]);
        let mut sampler = Samples::default();
        let color = invocation
            .evaluate(
                &at(0.0),
                &SPATIAL,
                &mut sampler,
                &mut StripWorkspace::default(),
            )
            .unwrap();
        match expected {
            Some(_) => assert_eq!(sampler.0.len(), 1, "{seconds}"),
            None => {
                assert!(sampler.0.is_empty(), "{seconds}");
                assert_eq!(color, Color::BLACK, "{seconds}");
            }
        }
        let mut constant = ConstantSignal(source_color);
        let color = invocation
            .evaluate(
                &at(0.0),
                &SPATIAL,
                &mut constant,
                &mut StripWorkspace::default(),
            )
            .unwrap();
        assert_eq!(color, expected.unwrap_or(Color::BLACK), "{seconds}");
    }
}

#[test]
fn spatial_signal_queries_keep_their_coordinate_domains() {
    let operator = operator(
        "operator Spatial {
            input source;
            sample {
                let t = time;
                let p = pixel.index;
                let a = source.at(t, p);
                let b = source.at_global(t, p);
                let c = source.at(t);
                let d = source.at(t, p);
                let p = p + 1;
                max(max(max(a, b), max(c, d)), source.at(t, p))
            }
        }",
    );
    let context = pixel(3, 10);
    let mut samples = Samples::default();
    let result = operator
        .evaluate(
            &context,
            &SPATIAL,
            &mut samples,
            &mut StripWorkspace::default(),
        )
        .unwrap();
    assert_eq!(
        samples.sorted(),
        [
            (0, 250_000, SignalPixel::Current),
            (0, 250_000, SignalPixel::Local(3)),
            (0, 250_000, SignalPixel::Local(4)),
            (0, 250_000, SignalPixel::Global(3)),
        ]
    );
    assert_eq!(result.blue, 4);
    for query in [
        "source.at()",
        "source.at(0.0, 1.0)",
        "source.at(0.0, 1, 2)",
        "source.at_global(0.0)",
        "source.at_global(0.0, true)",
        "source.at(#ffffff)",
    ] {
        assert!(
            compile_operators(&format!(
                "operator Invalid {{ input source; sample {{ {query} }} }}"
            ))
            .is_err(),
            "{query}"
        );
    }
}

#[test]
fn equal_signal_queries_share_one_sample() {
    for (body, calls) in [
        (
            "max(max(source.at(time), other.at(time)), source)",
            vec![(0, 250_000), (1, 250_000)],
        ),
        (
            "let past = time - 0.125;
             max(max(source.at(time), source.at(past)), max(source.at(time), source.at(past)))",
            vec![(0, 125_000), (0, 250_000)],
        ),
        (
            "let a = source.at(time); let t = time + 0.125; max(a, source.at(t))",
            vec![(0, 250_000), (0, 375_000)],
        ),
        (
            "let a = source.at(time);
             let b = if progress > 0.5 { source.at(time) } else { a };
             max(b, source.at(time))",
            vec![(0, 250_000)],
        ),
        (
            "max for i in 0..2 { max(source.at(time), source.at(time)) }",
            vec![(0, 250_000)],
        ),
        (
            "max for i in 0..3 { source.at(time - i * 0.125) }",
            vec![(0, 0), (0, 125_000), (0, 250_000)],
        ),
    ] {
        let operator = operator(&format!(
            "operator Reads {{ input source; input other; sample {{ {body} }} }}"
        ));
        let mut context = at(0.25);
        context.run.progress = 1.0;
        let mut samples = Samples::default();
        operator
            .evaluate(
                &context,
                &SPATIAL,
                &mut samples,
                &mut StripWorkspace::default(),
            )
            .unwrap();
        let calls: Vec<_> = calls
            .into_iter()
            .map(|(input, time)| (input, time, SignalPixel::Current))
            .collect();
        assert_eq!(samples.sorted(), calls, "{body}");
    }
}
