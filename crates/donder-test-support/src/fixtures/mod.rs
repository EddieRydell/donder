//! The starter effect fixtures of the playback benchmarks.
pub mod layered;

use donder_language::dsl::{CompiledEffect, Identifier, Invocation, Value, compile_effects};
use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
use indexmap::IndexMap;
use std::sync::Arc;

const SCAN_SWEEP_SOURCE: &str =
    include_str!("../../../../examples/starter/effects/scan-sweep.donder");
const IMPACT_BURST_SOURCE: &str =
    include_str!("../../../../examples/starter/effects/impact-burst.donder");
const SPARKLE_COMET_SOURCE: &str =
    include_str!("../../../../examples/starter/effects/sparkle-comet.donder");
const SHIMMER_FIELD_SOURCE: &str =
    include_str!("../../../../examples/starter/effects/shimmer-field.donder");

pub fn cases() -> [(&'static str, &'static str, IndexMap<Identifier, Value>); 4] {
    [
        ("ScanSweep", SCAN_SWEEP_SOURCE, scan_sweep_params()),
        ("ImpactBurst", IMPACT_BURST_SOURCE, impact_burst_params()),
        ("SparkleComet", SPARKLE_COMET_SOURCE, sparkle_comet_params()),
        ("ShimmerField", SHIMMER_FIELD_SOURCE, shimmer_field_params()),
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
    sample_effect(effect_name, source)
        .bind(&params)
        .expect("valid params")
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

fn sample_effect(effect_name: &str, source: &str) -> CompiledEffect {
    compile_effects(source)
        .expect("effect source should compile")
        .into_iter()
        .find(|effect| effect.name().as_str() == effect_name)
        .expect("compiled effect should exist")
}

fn scan_sweep_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("intensity", Value::Curve(curve())),
        ("direction", enum_value("CenterOut")),
        ("color_mode", enum_value("ScanHead")),
        ("repeats", Value::Float(3.0)),
        ("width", Value::Float(0.18)),
        ("edge_width", Value::Float(0.08)),
        ("background_level", Value::Float(0.05)),
        ("section_width_pixels", Value::Float(6.0)),
    ])
}

fn impact_burst_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("intensity", Value::Curve(curve())),
        ("direction", enum_value("Outward")),
        ("color_mode", enum_value("FromEdge")),
        ("center_position", Value::Float(0.5)),
        ("start_radius", Value::Float(0.0)),
        ("end_radius", Value::Float(0.75)),
        ("edge_width", Value::Float(0.2)),
        ("glow_width", Value::Float(0.25)),
        ("glow_level", Value::Float(0.35)),
        ("mirror", Value::Bool(false)),
    ])
}

fn sparkle_comet_params() -> IndexMap<Identifier, Value> {
    params([
        ("gradient", Value::Gradient(gradient())),
        ("position", Value::Curve(curve())),
        ("intensity", Value::Curve(curve())),
        ("color_mode", enum_value("RainbowHue")),
        (
            "head_color",
            Value::Color(Color {
                red: 255,
                green: 255,
                blue: 255,
            }),
        ),
        ("reverse", Value::Bool(false)),
        ("wrap", Value::Bool(true)),
        ("tail_width", Value::Float(0.22)),
        ("head_width", Value::Float(0.04)),
        ("sparkle_density", Value::Float(0.45)),
        ("sparkle_speed", Value::Float(18.0)),
        ("sparkle_gain", Value::Float(0.8)),
        ("seed", Value::Float(13.0)),
    ])
}

fn shimmer_field_params() -> IndexMap<Identifier, Value> {
    params([
        (
            "base",
            Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
        ),
        ("palette", Value::Gradient(gradient())),
        ("color_mode", enum_value("FromPalette")),
        (
            "sparkle_color",
            Value::Color(Color {
                red: 255,
                green: 255,
                blue: 255,
            }),
        ),
        ("density", Value::Float(0.35)),
        ("shimmer_level", Value::Float(0.4)),
        ("sparkle_level", Value::Float(1.0)),
        ("wave_speed", Value::Float(5.0)),
        ("wave_scale", Value::Float(21.0)),
        ("sparkle_speed", Value::Float(10.0)),
        ("sparkle_hold", Value::Float(0.35)),
        ("section_width_pixels", Value::Float(5.0)),
        ("seed", Value::Float(17.0)),
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

fn gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
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
        color: Color { red, green, blue },
    }
}
