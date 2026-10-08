//! Uniform parameter resources evaluated beside per-pixel work. Playback of
//! prepared uniform effects is covered by the `prepared_uniform` integration
//! tests.
use super::evaluation::{SampleEvaluation, bind, compile_effect, context};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;

use crate::dsl::StripWorkspace;
use donder_runtime_types::SpatialContext;
use donder_runtime_types::Value;
use donder_runtime_types::{Color, Gradient};

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[test]
fn uniform_resource_samples_preserve_branches_and_empty_gradient_defaults() {
    for (source, returns_black) in [
        (
            "effect Guarded { param colors: gradient; param values: array<float>; sample {
                if pixel.index < 0 { colors[progress] } else { rgb(pixel.fraction, progress, values[0]) }
            } }",
            false,
        ),
        (
            "effect Guarded { param colors: gradient; param values: array<float>; sample {
                colors[progress] * values[pixel.index]
            } }",
            true,
        ),
    ] {
        let effect = compile_effect(source);
        let result = playback::lower_sample(&bind(
            &effect,
            &[
                ("colors", Value::Gradient(Gradient { stops: vec![] }.into())),
                ("values", Value::Array(vec![].into())),
            ],
        ))
        .evaluate(&context(200, 0, 0), &SPATIAL, &mut StripWorkspace::default());
        if returns_black {
            assert_eq!(result, Color::BLACK);
        } else {
            assert_eq!(
                result,
                crate::sampling::rgb(0.0, context(200, 0, 0).progress, 0.0)
            );
        }
    }
}
