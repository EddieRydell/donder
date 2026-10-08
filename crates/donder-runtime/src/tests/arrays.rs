//! Array literals and array parameters: indexing, measuring and conversions.
use super::evaluation::{
    SampleEvaluation, bind, compile_effect, effect, one_pixel, runtime_effect,
};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_runtime_types::SpatialContext = donder_runtime_types::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use crate::dsl::StripWorkspace;
use donder_runtime_types::Color;
use donder_runtime_types::Value;

fn red(red: u8) -> Color {
    Color {
        red,
        green: 0,
        blue: 0,
    }
}

#[test]
fn selecting_array_items_keeps_integer_to_float_conversion() {
    let array = compile_effect(
        "effect Array { param index: int in 0..1 = 0; sample {
            let values = [progress, pixel.index + 1];
            rgb(values[1], values[index], [pixel.index + 1, 2][0] * 1.0)
        } }",
    );
    let scalar = compile_effect(
        "effect Scalar { param index: int in 0..1 = 0; sample {
            let selected = if index == 1 { pixel.index + 1.0 } else { progress };
            rgb(pixel.index + 1, selected, pixel.index + 1)
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for index in [0, 1] {
        let params = [("index", Value::Int(index))];
        let expected = playback::lower_sample(&bind(&scalar, &params));
        for actual in [
            playback::lower_sample(&bind(&array, &params)),
            runtime_effect(&array, &[], &params),
        ] {
            for progress in [0.0, 0.25, 0.75, 1.0] {
                let context = one_pixel(progress);
                assert_eq!(
                    actual.evaluate(&context, &SPATIAL, &mut workspace),
                    expected.evaluate(&context, &SPATIAL, &mut workspace)
                );
            }
        }
    }
}

#[test]
fn curve_items_sample_at_integer_and_float_positions() {
    use donder_runtime_types::{Curve, CurvePoint};
    let effect = compile_effect(
        "effect Indexed {
            param shapes: array<curve>;
            param integer: int in -2147483648..2147483647 = 0;
            param fraction: float in -1.0..1.0 = 0.0;
            sample {
                let shape = shapes[0];
                rgb(shape[integer], shape[fraction], 0.0)
            }
        }",
    );
    let curve = Curve {
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
    };
    let shapes = Value::Array(vec![Value::Curve(curve.clone().into())].into());
    let mut workspace = StripWorkspace::default();
    for integer in [i32::MIN, -1, 0, 1, i32::MAX] {
        for fraction in [
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            0.25,
            1.0,
            f32::INFINITY,
            f32::NAN,
        ] {
            let sampled = runtime_effect(
                &effect,
                &[("shapes", shapes.clone())],
                &[
                    ("integer", Value::Int(integer)),
                    ("fraction", Value::Float(fraction)),
                ],
            )
            .evaluate(&one_pixel(0.0), &SPATIAL, &mut workspace);
            let channel = |position| {
                (donder_runtime_types::sampling::sample_curve(&curve, position) * 255.0).round()
                    as u8
            };
            assert_eq!(
                sampled,
                if fraction.is_nan() {
                    Color::BLACK
                } else {
                    Color {
                        red: channel(integer as f32),
                        green: channel(fraction),
                        blue: 0,
                    }
                },
                "integer={integer} fraction={fraction}"
            );
        }
    }
}

#[test]
fn lets_keep_the_items_they_bound() {
    let fixed = effect(
        "effect Fixed { sample {
            let values = [progress, progress + 0.25];
            let saved = values;
            let values = [0.0];
            rgb(saved[0], saved[1], len(saved) * 0.25 + len(values) * 0.0)
        } }",
    );
    let mut workspace = StripWorkspace::default();
    for progress in [0.25, 0.0, 0.25] {
        assert_eq!(
            fixed.evaluate(&one_pixel(progress), &SPATIAL, &mut workspace),
            crate::sampling::rgb(progress, progress + 0.25, 0.5)
        );
    }
    for (body, expected) in [
        (
            "let value = progress; let saved = [value]; let value = 0.75; rgb(saved[0], value, 0.0)",
            [64, 191],
        ),
        (
            "let saved = if progress > 0.5 { [0.1][0] } else { [progress][0] }; rgb(saved, 0.0, 0.0)",
            [64, 26],
        ),
        (
            "let items = [progress, 0.9]; rgb(items[if progress > 0.5 { 1 } else { 0 }], 0.0, 0.0)",
            [64, 230],
        ),
        (
            "rgb(first for i in 0..3 { let current = [progress + i * 0.1]; guard i == 0; current[0] } else { 0.0 }, 0.0, 0.0)",
            [64, 191],
        ),
    ] {
        let effect = effect(&format!("effect Snapshot {{ sample {{ {body} }} }}"));
        for (progress, expected) in [
            (0.25, expected[0]),
            (0.75, expected[1]),
            (0.25, expected[0]),
        ] {
            assert_eq!(
                effect
                    .evaluate(&one_pixel(progress), &SPATIAL, &mut workspace)
                    .red,
                expected,
                "{body}"
            );
        }
    }
}

#[test]
fn indices_clamp_and_empty_arrays_produce_defaults() {
    let mut workspace = StripWorkspace::default();
    for (index, expected_red) in [
        ("pixel.index", 64),
        ("-1", 64),
        ("2", 191),
        ("pixel.index - 5", 64),
        ("pixel.index + 5", 191),
    ] {
        let effect = effect(&format!(
            "effect Dynamic {{ sample {{ rgb([progress, 0.75][{index}], 0.0, 0.0) }} }}"
        ));
        assert_eq!(
            effect
                .evaluate(&one_pixel(0.25), &SPATIAL, &mut workspace)
                .red,
            expected_red,
            "{index}"
        );
    }
    let empty = compile_effect(
        "effect Empty {
            param values: array<float>;
            param colors: array<color>;
            param flags: array<bool>;
            sample {
                guard !flags[pixel.index] else #ffffff;
                max(rgb(values[pixel.index] + len(values), 0.0, 0.0), colors[pixel.index])
            }
        }",
    );
    let invocation = playback::lower_sample(&bind(
        &empty,
        &[
            ("values", Value::Array(vec![].into())),
            ("colors", Value::Array(vec![].into())),
            ("flags", Value::Array(vec![].into())),
        ],
    ));
    assert_eq!(
        invocation.evaluate(&one_pixel(0.25), &SPATIAL, &mut workspace),
        Color::BLACK
    );
}

#[test]
fn dynamic_selection_preserves_typed_values() {
    for (params, body) in [
        (
            "",
            "let values = [0.25, progress, 0.75]; let saved = values; let values = [0.0];
             rgb(saved[pixel.index], values[0], 0.0)",
        ),
        (
            "",
            "[rgb(0.25, 0.0, 0.0), rgb(progress, 0.0, 0.0), rgb(0.75, 0.0, 0.0)][pixel.index]",
        ),
        (
            "param offsets: array<float>;",
            "rgb([0.25, progress, 0.75][pixel.index] + offsets[pixel.index] - 0.5, 0.0, 0.0)",
        ),
    ] {
        let effect = compile_effect(&format!("effect Select {{ {params} sample {{ {body} }} }}"));
        let values: Vec<(&str, Value)> = if params.is_empty() {
            vec![]
        } else {
            vec![("offsets", Value::Array(vec![Value::Float(0.5); 3].into()))]
        };
        let effect = playback::lower_sample(&bind(&effect, &values));
        let mut workspace = StripWorkspace::default();
        for progress in [0.0, 0.5, 1.0, 0.0] {
            for (pixel, expected) in [64, (progress * 255.0_f32).round() as u8, 191]
                .into_iter()
                .enumerate()
            {
                let mut ctx = one_pixel(progress);
                ctx.index = pixel as i32;
                ctx.run.pixel_count = 3;
                assert_eq!(
                    effect.evaluate(&ctx, &SPATIAL, &mut workspace),
                    red(expected),
                    "{body}"
                );
            }
        }
    }
}
