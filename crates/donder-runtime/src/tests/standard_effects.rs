use super::evaluation::{PixelContext, SampleEvaluation, bind};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_language::execution::SpatialContext =
    donder_language::execution::SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };

use crate::dsl::RunContext;
use crate::dsl::StripWorkspace;
use donder_language::dsl::Value;
use donder_language::dsl::compile_effects;
use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop, SampleDuration};

#[test]
fn standard_pulse_obeys_linear_falloff_without_an_extra_envelope() {
    let pulse = compile_effects(include_str!(
        "../../../../examples/starter/effects/standard.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|effect| effect.name().as_str() == "Pulse")
    .unwrap();
    let invocation = playback::lower_sample(&bind(
        &pulse,
        &[
            (
                "gradient",
                Value::Gradient(
                    Gradient {
                        stops: vec![GradientStop {
                            position: 0.0,
                            color: Color {
                                red: 255,
                                green: 255,
                                blue: 255,
                            },
                        }],
                    }
                    .into(),
                ),
            ),
            (
                "pulse_shape",
                Value::Curve(
                    Curve {
                        points: vec![
                            CurvePoint {
                                position: 0.0,
                                value: 1.0,
                            },
                            CurvePoint {
                                position: 1.0,
                                value: 0.0,
                            },
                        ],
                    }
                    .into(),
                ),
            ),
        ],
    ));
    for (progress, brightness) in [(0.0, 255), (0.25, 191), (0.5, 128), (0.75, 64), (1.0, 0)] {
        let color = invocation.evaluate(
            &PixelContext {
                run: RunContext {
                    progress,
                    time: SampleDuration::from_ticks((progress * 1_000_000.0) as u32),
                    duration: SampleDuration::from_ticks(1_000_000),
                    pixel_count: 1,
                },
                index: 0,
                fraction: 0.0,
            },
            &SPATIAL,
            &mut StripWorkspace::default(),
        );
        assert_eq!(color.red, brightness, "progress={progress}");
        assert_eq!(color.green, brightness);
        assert_eq!(color.blue, brightness);
    }
}
