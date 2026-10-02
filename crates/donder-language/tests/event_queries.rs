use donder_language::dsl::{Identifier, RunContext, Value, VmWorkspace, compile_effects};
use donder_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration,
};
use donder_runtime::{DslBindCache, SpatialContext};
use indexmap::IndexMap;

fn sample(parameters: &str, body: &str, values: &[(&str, Value)]) -> Color {
    let source = format!("effect Query {{ {parameters} color sample() {{ {body} }} }}");
    let effect = compile_effects(&source).unwrap().remove(0).effect;
    let values = values
        .iter()
        .map(|(name, value)| (Identifier::new((*name).into()).unwrap(), value.clone()))
        .collect::<IndexMap<_, _>>();
    let invocation = effect.bind(&values, &mut DslBindCache::default()).unwrap();
    invocation.evaluate(
        &RunContext {
            progress: 0.75,
            time: SampleDuration::from_ticks(750_000),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        },
        &SpatialContext {
            position: [0.0; 2],
            min: [0.0; 2],
            max: [0.0; 2],
        },
        &mut VmWorkspace::default(),
    )
}

fn assert_query(parameters: &str, predicate: &str, values: &[(&str, Value)]) {
    assert_eq!(
        sample(
            parameters,
            &format!("if ({predicate}) {{ return #ffffff; }} return #000000;"),
            values
        ),
        Color {
            red: 255,
            green: 255,
            blue: 255
        },
        "{predicate}"
    );
}

fn curve(points: &[(f32, f32)]) -> Value {
    Value::Curve(
        Curve {
            points: points
                .iter()
                .map(|&(position, value)| CurvePoint { position, value })
                .collect(),
        }
        .into(),
    )
}

#[test]
fn crossing_queries_distinguish_first_and_latest_for_parameter_and_selected_curves() {
    let shape = curve(&[(0.125, 0.0), (0.375, 1.0), (0.625, 0.0), (0.875, 1.0)]);
    for (parameters, expression, value) in [
        ("param curve shape;", "shape", shape.clone()),
        (
            "param array<curve> shape;",
            "shape[pixel_index()]",
            Value::Array(vec![shape].into()),
        ),
    ] {
        for predicate in [
            "curve_first_crossing(c, 0.5) == 0.25",
            "is_nan(curve_last_crossing(c, 0.5, 0.125))",
            "curve_last_crossing(c, 0.5, 0.25) == 0.25",
            "curve_last_crossing(c, 0.5, 0.375) == 0.25",
            "curve_last_crossing(c, 0.5, 0.5) == 0.5",
            "curve_last_crossing(c, 0.5, 0.75) == 0.75",
            "curve_last_crossing(c, 0.5, 2.0) == 0.75",
            "is_nan(curve_first_crossing(c, 2.0))",
            "is_nan(curve_last_crossing(c, 2.0, 2.0))",
            "is_nan(curve_last_crossing(c, 0.5, 0.0 / 0.0))",
            "is_nan(curve_first_crossing(c, 0.0 / 0.0))",
            "curve_last_crossing(c, 0.0, 0.125) == 0.125",
            "is_nan(curve_last_crossing(c, 0.0, 0.0))",
        ] {
            assert_query(
                parameters,
                &predicate.replace("c,", &format!("{expression},")),
                &[("shape", value.clone())],
            );
        }
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
        assert_query("param curve shape;", predicate, &[("shape", shape.clone())]);
    }
    let shape = curve(&[(0.25, 1.0), (0.75, 1.0)]);
    assert_query(
        "param curve shape;",
        "curve_last_crossing(shape, 1.0, 2.0) == 0.25",
        &[("shape", shape)],
    );
}

#[test]
fn mark_queries_have_explicit_clocks_inclusive_boundaries_and_missing_values() {
    let beats = Value::Marks(
        Marks {
            marks: [250_000, 500_000, 500_000]
                .map(SampleDuration::from_ticks)
                .to_vec(),
        }
        .into(),
    );
    for predicate in [
        "mark_count(beats) == 3",
        "is_nan(mark_last(beats, 0.0))",
        "mark_last_index(beats, 0.0) == -1",
        "mark_last(beats, 0.25) == 0.25",
        "mark_last_index(beats, 0.25) == 0",
        "mark_last(beats, seconds()) == 0.5",
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
        assert_query("param marks beats;", predicate, &[("beats", beats.clone())]);
    }
    for predicate in [
        "mark_count(beats) == 0",
        "is_nan(mark_last(beats, 1.0))",
        "mark_last_index(beats, 1.0) == -1",
        "is_nan(mark_at(beats, 0))",
    ] {
        assert_query(
            "param marks beats;",
            predicate,
            &[("beats", Value::Marks(Marks { marks: vec![] }.into()))],
        );
    }
    let unsorted = Value::Marks(
        Marks {
            marks: [500_000, 250_000, 500_000, 125_000]
                .map(SampleDuration::from_ticks)
                .to_vec(),
        }
        .into(),
    );
    for predicate in [
        "mark_last(beats, 0.75) == 0.5",
        "mark_last_index(beats, 0.75) == 2",
        "mark_last(beats, 0.375) == 0.25",
        "mark_last_index(beats, 0.375) == 1",
        "mark_at(beats, 3) == 0.125",
    ] {
        assert_query(
            "param marks beats;",
            predicate,
            &[("beats", unsorted.clone())],
        );
    }
}

#[test]
fn scalar_missingness_survives_arithmetic_sampling_and_builtins_until_explicit_default() {
    for expression in [
        "x * 0.0",
        "x - x",
        "x / x",
        "-x",
        "sin(x)",
        "cos(x)",
        "abs(x)",
        "floor(x)",
        "min(x, 0.0)",
        "min(0.0, x)",
        "max(x, 0.0)",
        "max(0.0, x)",
        "clamp(x, 0.0, 1.0)",
        "clamp(0.5, x, 1.0)",
        "clamp(0.5, 0.0, x)",
        "rand(x)",
        "rand(1.0, x, 2.0)",
        "srand(x)",
        "mix(0.0, 1.0, x)",
    ] {
        for (parameters, declaration, values) in [
            ("param float x;", "", vec![("x", Value::Float(f32::NAN))]),
            ("", "float x = 0.0 / 0.0;", vec![]),
        ] {
            assert_eq!(
                sample(
                    parameters,
                    &format!(
                        "{declaration} if (is_nan({expression})) {{ return #ffffff; }} return #000000;"
                    ),
                    &values
                ),
                Color {
                    red: 255,
                    green: 255,
                    blue: 255
                },
                "{expression}"
            );
        }
    }
    for (value, expected) in [
        (f32::NAN, 0.25),
        (0.0, 0.0),
        (f32::INFINITY, f32::INFINITY),
        (f32::NEG_INFINITY, f32::NEG_INFINITY),
    ] {
        assert_query(
            "param float x; param float expected;",
            "value_or(x, 0.25) == expected",
            &[
                ("x", Value::Float(value)),
                ("expected", Value::Float(expected)),
            ],
        );
    }
    for predicate in [
        "value_or(0.25, x) == 0.25",
        "value_or(0.0, x) == 0.0",
        "is_nan(value_or(x, x))",
        "value_or(0.0 / 0.0, 0.75) == 0.75",
    ] {
        assert_query(
            "param float x;",
            predicate,
            &[("x", Value::Float(f32::NAN))],
        );
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
                "param curve shape; param float x;",
                predicate,
                &[("shape", shape.clone()), ("x", Value::Float(f32::NAN))],
            );
        }
    }
    for predicate in [
        "is_nan(shape[0.5])",
        "is_nan(curve_first_crossing(shape, 0.0))",
        "is_nan(curve_last_crossing(shape, 0.0, 1.0))",
        "value_or(curve_first_crossing(shape, 0.0), 0.25) == 0.25",
    ] {
        assert_query("param curve shape;", predicate, &[("shape", curve(&[]))]);
    }
    for position in [f32::NEG_INFINITY, -1.0, 2.0, f32::INFINITY] {
        assert_query(
            "param curve shape; param float x;",
            if position < 0.0 {
                "shape[x] == 0.25"
            } else {
                "shape[x] == 0.75"
            },
            &[
                ("shape", curve(&[(0.0, 0.25), (1.0, 0.75)])),
                ("x", Value::Float(position)),
            ],
        );
    }
}

#[test]
fn latest_crossing_handles_finite_extreme_values_without_intermediate_overflow() {
    for points in [
        vec![(0.0, -f32::MAX), (1.0, f32::MAX)],
        vec![(0.0, f32::MAX), (1.0, -f32::MAX)],
    ] {
        let shape = curve(&points);
        for (parameters, expression, value) in [
            ("param curve shape;", "shape", shape.clone()),
            (
                "param array<curve> shape;",
                "shape[pixel_index()]",
                Value::Array(vec![shape].into()),
            ),
        ] {
            for predicate in [
                "curve_first_crossing(c, 0.0) == 0.5",
                "is_nan(curve_last_crossing(c, 0.0, 0.25))",
                "curve_last_crossing(c, 0.0, 0.5) == 0.5",
                "curve_last_crossing(c, 0.0, 1.0) == 0.5",
            ] {
                assert_query(
                    parameters,
                    &predicate.replace("c,", &format!("{expression},")),
                    &[("shape", value.clone())],
                );
            }
        }
    }
}

#[test]
fn color_boundaries_consume_nan_as_whole_black() {
    for expression in [
        "rgb(x, 1.0, 0.0)",
        "rgb(1.0, x, 0.0)",
        "rgb(1.0, 0.0, x)",
        "hsv(x, 1.0, 1.0)",
        "hsv(0.0, x, 1.0)",
        "hsv(0.0, 1.0, x)",
        "mix(#ff0000, #00ff00, x)",
        "#ffffff * x",
        "gradient[x]",
        "gradient_color_scaled(gradient, 0.0, x)",
    ] {
        assert_eq!(
            sample(
                "param float x; param gradient gradient;",
                &format!("return {expression};"),
                &[
                    ("x", Value::Float(f32::NAN)),
                    (
                        "gradient",
                        Value::Gradient(
                            Gradient {
                                stops: vec![GradientStop {
                                    position: 0.0,
                                    color: Color {
                                        red: 255,
                                        green: 255,
                                        blue: 255
                                    }
                                }]
                            }
                            .into()
                        )
                    ),
                ]
            ),
            Color::BLACK,
            "{expression}"
        );
    }
}
