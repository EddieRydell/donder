//! The starter effect fixtures of the playback benchmarks.
pub mod layered;

use donder_language::compiler::Invocation;
use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientStop};
use donder_runtime_types::{Identifier, Value};
use indexmap::IndexMap;
use std::sync::Arc;

const STANDARD_SOURCE: &str = include_str!("../../../../examples/starter/effects/standard.donder");

/// The most used library effects of a converted show.
pub fn cases() -> [(&'static str, &'static str, IndexMap<Identifier, Value>); 4] {
    [
        ("Pulse", STANDARD_SOURCE, pulse_params()),
        ("Chase", STANDARD_SOURCE, chase_params()),
        ("Twinkle", STANDARD_SOURCE, twinkle_params()),
        ("Butterfly", STANDARD_SOURCE, butterfly_params()),
    ]
}

pub fn layer_cases() -> [(&'static str, &'static str, IndexMap<Identifier, Value>); 4] {
    [
        (
            "UniformFade",
            "effect UniformFade { sample { rgb(progress, abs(sin(time)), 0.25) } }",
            IndexMap::new(),
        ),
        (
            "PixelRamp",
            "effect PixelRamp { sample { rgb(pixel.fraction, progress, 0.25) } }",
            IndexMap::new(),
        ),
        (
            "ArrayRamp",
            "effect ArrayRamp { sample {
                let values = [pixel.fraction, progress, 0.25];
                let saved = values;
                let values = [0.0];
                rgb(saved[0], saved[1], saved[2])
            } }",
            IndexMap::new(),
        ),
        (
            "DynamicArray",
            "effect DynamicArray { sample {
                let values = [pixel.fraction, progress, 0.25];
                rgb(values[pixel.index % 3],
                    values[(pixel.index + 1) % 3], values[(pixel.index + 2) % 3])
            } }",
            IndexMap::new(),
        ),
    ]
}

pub fn prepared_effect(
    effect_name: &str,
    source: &str,
    params: IndexMap<Identifier, Value>,
) -> Invocation {
    crate::playback::invoke(
        &crate::playback::definition(source, effect_name),
        &params
            .iter()
            .map(|(name, value)| (name.as_str(), value.clone()))
            .collect::<Vec<_>>(),
    )
}

pub fn uniform_resources() -> Invocation {
    prepared_effect(
        "UniformResources",
        "effect UniformResources { param shape: curve in 0.0..1.0; param colors: gradient; sample {
            let level = shape[progress];
            let tint = colors[level];
            tint * pixel.fraction
        } }",
        params([
            ("shape", Value::Curve(curve())),
            ("colors", Value::Gradient(gradient())),
        ]),
    )
}

fn pulse_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("pulse_shape", Value::Curve(curve())),
    ])
}

fn chase_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("section_width_pixels", Value::Int(6)),
        ("chase_position", Value::Curve(ramp())),
        ("pulse_shape", Value::Curve(curve())),
    ])
}

fn twinkle_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("average_coverage", Value::Float(0.35)),
        ("random_seed", Value::Float(17.0)),
    ])
}

fn butterfly_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("butterfly_type", enum_value("Type3")),
    ])
}

fn params<const N: usize>(items: [(&str, Value); N]) -> IndexMap<Identifier, Value> {
    items
        .into_iter()
        .map(|(key, value)| (identifier(key), value))
        .collect()
}

fn enum_value(value: &str) -> Value {
    Value::Enum(identifier(value))
}

fn identifier(value: &str) -> Identifier {
    Identifier::new(value.to_string()).expect("identifier should be valid")
}

fn curve() -> Arc<Curve> {
    Arc::new(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.12,
                value: 1.0,
            },
            CurvePoint {
                position: 0.45,
                value: 0.35,
            },
            CurvePoint {
                position: 0.78,
                value: 0.8,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    })
}

fn ramp() -> Arc<Curve> {
    Arc::new(Curve {
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
    })
}

fn gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
        space: donder_runtime_types::GradientSpace::LinearRgb,
        stops: vec![
            color_point(0.0, 255, 32, 16),
            color_point(0.28, 255, 184, 24),
            color_point(0.58, 24, 220, 255),
            color_point(1.0, 160, 64, 255),
        ],
    })
}

fn color_point(position: f32, red: u8, green: u8, blue: u8) -> GradientStop {
    GradientStop {
        position,
        color: Color::from_bytes([red, green, blue]),
    }
}
