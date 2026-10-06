use super::evaluation::{PixelContext, SampleEvaluation, compile_effect, curve, marks};
use super::playback;
use super::std;
use crate::dsl::RunContext;
use crate::dsl::StripWorkspace;
use donder_language::dsl::Value;
use donder_language::execution::SpatialContext;
use donder_language::values::{Color, Gradient, GradientStop, SampleDuration};
use std::prelude::rust_2024::*;

/// One parameter: its name, its declared type and range, and its value.
type Param<'a> = (&'a str, &'a str, Value);

/// `body` with `params`, evaluated two ways. When every value lies inside its
/// declared range, the parameters are fixed and preparation computes what it
/// can. Then every parameter is selected from a one-item array by the pixel
/// index, so the VM computes everything; arrays have no declared range.
fn sample(params: &[Param], body: &str) -> Color {
    let context = PixelContext {
        run: RunContext {
            progress: 0.75,
            time: SampleDuration::from_ticks(750_000),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_count: 1,
        },
        index: 0,
        fraction: 0.0,
    };
    let spatial = SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };
    let declarations: String = params
        .iter()
        .map(|(name, ty, _)| format!("param {name}: {ty};"))
        .collect();
    let fixed = compile_effect(&format!(
        "effect Query {{ {declarations} sample {{ {body} }} }}"
    ));
    let evaluate = |invocation: &donder_language::dsl::Invocation| {
        playback::lower_sample(invocation).evaluate(
            &context,
            &spatial,
            &mut StripWorkspace::default(),
        )
    };
    let values: Vec<Value> = params.iter().map(|(_, _, value)| value.clone()).collect();
    // Positional values, checked against their declared ranges.
    let prepared = fixed
        .invoke(values.clone(), Box::new([]))
        .ok()
        .map(|invocation| evaluate(&invocation));
    let selected_declarations: String = params
        .iter()
        .map(|(name, ty, _)| {
            let ty = ty.split(" in ").next().unwrap();
            format!("param {name}_items: array<{ty}>;")
        })
        .collect();
    let selections: String = params
        .iter()
        .map(|(name, _, _)| format!("let {name} = {name}_items[pixel.index];"))
        .collect();
    let selected = compile_effect(&format!(
        "effect Query {{ {selected_declarations} sample {{ {selections} {body} }} }}"
    ));
    let items = values
        .into_iter()
        .map(|value| Value::Array(vec![value].into()))
        .collect();
    let runtime = evaluate(&selected.invoke(items, Box::new([])).unwrap());
    if let Some(prepared) = prepared {
        assert_eq!(prepared, runtime, "prepared and runtime differ: {body}");
    }
    runtime
}

fn assert_query(params: &[Param], predicate: &str) {
    assert_eq!(
        sample(
            params,
            &format!("if {predicate} {{ #ffffff }} else {{ #000000 }}")
        ),
        Color {
            red: 255,
            green: 255,
            blue: 255
        },
        "{predicate}"
    );
}

#[test]
fn crossing_queries_distinguish_first_and_latest() {
    let shape = curve(&[(0.125, 0.0), (0.375, 1.0), (0.625, 0.0), (0.875, 1.0)]);
    for predicate in [
        "curve_first_crossing(shape, 0.5) == 0.25",
        "is_nan(curve_last_crossing(shape, 0.5, 0.125))",
        "curve_last_crossing(shape, 0.5, 0.25) == 0.25",
        "curve_last_crossing(shape, 0.5, 0.375) == 0.25",
        "curve_last_crossing(shape, 0.5, 0.5) == 0.5",
        "curve_last_crossing(shape, 0.5, 0.75) == 0.75",
        "curve_last_crossing(shape, 0.5, 2.0) == 0.75",
        "curve_last_crossing(shape, 0.5, progress) == 0.75",
        "is_nan(curve_first_crossing(shape, 2.0))",
        "is_nan(curve_last_crossing(shape, 2.0, 2.0))",
        "is_nan(curve_last_crossing(shape, 0.5, 0.0 / 0.0))",
        "is_nan(curve_first_crossing(shape, 0.0 / 0.0))",
        "curve_last_crossing(shape, 0.0, 0.125) == 0.125",
        "is_nan(curve_last_crossing(shape, 0.0, 0.0))",
    ] {
        assert_query(&[("shape", "curve in 0.0..1.0", shape.clone())], predicate);
    }
}

#[test]
fn plateau_arrival_and_exact_touches_are_events_but_endpoint_hold_is_not() {
    let shape = curve(&[
        (0.125, 0.0),
        (0.25, 1.0),
        (0.5, 1.0),
        (0.75, 0.0),
        (1.0, 1.0),
    ]);
    for predicate in [
        "curve_first_crossing(shape, 1.0) == 0.25",
        "curve_last_crossing(shape, 1.0, 0.5) == 0.25",
        "curve_last_crossing(shape, 1.0, 0.75) == 0.25",
        "curve_last_crossing(shape, 1.0, 1.0) == 1.0",
        "curve_last_crossing(shape, 1.0, 2.0) == 1.0",
        "curve_last_crossing(shape, 0.0, 0.75) == 0.75",
    ] {
        assert_query(&[("shape", "curve in 0.0..1.0", shape.clone())], predicate);
    }
    assert_query(
        &[(
            "shape",
            "curve in 0.0..1.0",
            curve(&[(0.25, 1.0), (0.75, 1.0)]),
        )],
        "curve_last_crossing(shape, 1.0, 2.0) == 0.25",
    );
}

#[test]
fn mark_queries_have_explicit_clocks_inclusive_boundaries_and_missing_values() {
    let beats = marks(&[250_000, 500_000, 500_000]);
    for predicate in [
        "mark_count(beats) == 3",
        "len(beats) == 3",
        "is_nan(mark_last(beats, 0.0))",
        "mark_last_index(beats, 0.0) == -1",
        "mark_last(beats, 0.25) == 0.25",
        "mark_last_index(beats, 0.25) == 0",
        "mark_last(beats, time) == 0.5",
        "mark_last_index(beats, 0.5) == 2",
        "mark_at(beats, 2) == 0.5",
        "is_nan(mark_at(beats, -1))",
        "is_nan(mark_at(beats, 3))",
        "is_nan(mark_last(beats, 0.0 / 0.0))",
        "mark_last_index(beats, 0.0 / 0.0) == -1",
        "mark_last(beats, 1.0 / 0.0) == 0.5",
        "is_nan(mark_last(beats, -1.0 / 0.0))",
        "value_or(mark_at(beats, -1), 0.125) == 0.125",
    ] {
        assert_query(&[("beats", "marks", beats.clone())], predicate);
    }
    for predicate in [
        "mark_count(beats) == 0",
        "is_nan(mark_last(beats, 1.0))",
        "mark_last_index(beats, 1.0) == -1",
        "is_nan(mark_at(beats, 0))",
    ] {
        assert_query(&[("beats", "marks", marks(&[]))], predicate);
    }
    // Construction sorts, so indices follow time rather than authoring order.
    let unsorted = marks(&[500_000, 250_000, 500_000, 125_000]);
    for predicate in [
        "mark_last(beats, 0.75) == 0.5",
        "mark_last_index(beats, 0.75) == 3",
        "mark_last(beats, 0.375) == 0.25",
        "mark_last_index(beats, 0.375) == 1",
        "mark_at(beats, 0) == 0.125",
        "mark_at(beats, 3) == 0.5",
    ] {
        assert_query(&[("beats", "marks", unsorted.clone())], predicate);
    }
}

#[test]
fn scalar_missingness_survives_arithmetic_sampling_and_builtins_until_explicit_default() {
    let nan = || ("x", "float in 0.0..1.0", Value::Float(f32::NAN));
    for expression in [
        "x * 0.0",
        "x + 0.0",
        "x * 1.0",
        "x - x",
        "x / x",
        "-x",
        "sin(x)",
        "cos(x)",
        "abs(x)",
        "floor(x)",
        "sqrt(x)",
        "atan2(x, 1.0)",
        "pow(x, 2)",
        "min(x, 0.0)",
        "min(0.0, x)",
        "max(x, 0.0)",
        "max(0.0, x)",
        "clamp(x, 0.0, 1.0)",
        "clamp(0.5, x, 1.0)",
        "clamp(0.5, 0.0, x)",
        "clamp(0.5, 1.0, 0.0)",
        "rand(x)",
        "mix(0.0, 1.0, x)",
        "smoothstep(0.0, 1.0, x)",
    ] {
        let predicate = format!("is_nan({expression})");
        assert_query(&[nan()], &predicate);
        // A missing value computed from literals.
        assert_query(&[], &format!("{{ let x = 0.0 / 0.0; {predicate} }}"));
    }
    // Every NaN comparison is false.
    for predicate in [
        "!(x < 0.5)",
        "!(x >= 0.5)",
        "!(x == x)",
        "x != x",
        "!(x == 0.0 / 0.0)",
    ] {
        assert_query(&[nan()], predicate);
    }
    let x = |value| ("x", "float in 0.0..1.0", Value::Float(value));
    for (value, expected) in [
        (f32::NAN, "0.25"),
        (0.0, "0.0"),
        (f32::INFINITY, "1.0 / 0.0"),
        (f32::NEG_INFINITY, "-1.0 / 0.0"),
    ] {
        assert_query(&[x(value)], &format!("value_or(x, 0.25) == {expected}"));
    }
    for predicate in [
        "value_or(0.25, x) == 0.25",
        "value_or(0.0, x) == 0.0",
        "is_nan(value_or(x, x))",
        "value_or(0.0 / 0.0, 0.75) == 0.75",
        "(if is_nan(x) { 0.5 } else { x }) == 0.5",
    ] {
        assert_query(&[nan()], predicate);
    }
    for shape in [
        curve(&[]),
        curve(&[(0.5, 0.75)]),
        curve(&[(0.0, 0.25), (1.0, 0.75)]),
    ] {
        for predicate in [
            "is_nan(shape[x])",
            "is_nan(curve_clamped(shape, x, 0.5, 1.0))",
            "is_nan(curve_first_crossing(shape, x))",
        ] {
            assert_query(
                &[("shape", "curve in 0.0..1.0", shape.clone()), nan()],
                predicate,
            );
        }
    }
    for predicate in [
        "is_nan(shape[0.5])",
        "is_nan(shape[progress])",
        "is_nan(curve_first_crossing(shape, 0.0))",
        "is_nan(curve_last_crossing(shape, 0.0, 1.0))",
        "value_or(curve_first_crossing(shape, 0.0), 0.25) == 0.25",
    ] {
        assert_query(&[("shape", "curve in 0.0..1.0", curve(&[]))], predicate);
    }
    // Sampling outside the authored positions holds the end values.
    for position in [f32::NEG_INFINITY, -1.0, 2.0, f32::INFINITY] {
        assert_query(
            &[
                (
                    "shape",
                    "curve in 0.0..1.0",
                    curve(&[(0.0, 0.25), (1.0, 0.75)]),
                ),
                x(position),
            ],
            if position < 0.0 {
                "shape[x] == 0.25"
            } else {
                "shape[x] == 0.75"
            },
        );
    }
}

#[test]
fn latest_crossing_handles_finite_extreme_values_without_intermediate_overflow() {
    for points in [
        [(0.0, -f32::MAX), (1.0, f32::MAX)],
        [(0.0, f32::MAX), (1.0, -f32::MAX)],
    ] {
        let shape = curve(&points);
        for predicate in [
            "curve_first_crossing(shape, 0.0) == 0.5",
            "is_nan(curve_last_crossing(shape, 0.0, 0.25))",
            "curve_last_crossing(shape, 0.0, 0.5) == 0.5",
            "curve_last_crossing(shape, 0.0, 1.0) == 0.5",
        ] {
            assert_query(&[("shape", "curve in 0.0..1.0", shape.clone())], predicate);
        }
    }
}

#[test]
fn color_boundaries_consume_nan_as_whole_black() {
    let white = Gradient {
        stops: vec![GradientStop {
            position: 0.0,
            color: Color {
                red: 255,
                green: 255,
                blue: 255,
            },
        }],
    };
    for expression in [
        "rgb(x, 1.0, 0.0)",
        "rgb(1.0, x, 0.0)",
        "rgb(1.0, 0.0, x)",
        "hsv(x, 1.0, 1.0)",
        "hsv(0.0, x, 1.0)",
        "hsv(0.0, 1.0, x)",
        "mix(#ff0000, #00ff00, x)",
        "#ffffff * x",
        "x * #ffffff",
        "gradient[x]",
        "gradient_color_scaled(gradient, 0.0, x)",
        "gradient_color_scaled(gradient, x, 1.0)",
    ] {
        assert_eq!(
            sample(
                &[
                    ("x", "float in 0.0..1.0", Value::Float(f32::NAN)),
                    (
                        "gradient",
                        "gradient",
                        Value::Gradient(white.clone().into())
                    ),
                ],
                expression,
            ),
            Color::BLACK,
            "{expression}"
        );
    }
    // That black is an ordinary color afterwards.
    assert_query(
        &[("x", "float in 0.0..1.0", Value::Float(f32::NAN))],
        "invert(rgb(x, 0.0, 0.0)) == #ffffff",
    );
    // Gradients with no stops are black.
    for expression in [
        "gradient[0.5]",
        "gradient[progress]",
        "gradient_color_scaled(gradient, progress, 1.0)",
    ] {
        assert_eq!(
            sample(
                &[(
                    "gradient",
                    "gradient",
                    Value::Gradient(Gradient { stops: vec![] }.into())
                )],
                expression,
            ),
            Color::BLACK,
            "{expression}"
        );
    }
}
