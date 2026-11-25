use super::evaluation::{OperatorEvaluation, PixelContext, bind};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_runtime_types::SpatialContext = donder_runtime_types::SpatialContext {
    position: [0.0; 3],
    min: [0.0; 3],
    max: [0.0; 3],
};

use super::evaluation::RuntimeError;
use super::evaluation::SignalSampler;
use super::evaluation::StripWorkspace;
use crate::dsl::RunContext;
use donder_language::compiler::CompiledDefinition;
use donder_language::compiler::compile_script;
use donder_runtime_types::Color;
use donder_runtime_types::SampleDuration;
use donder_runtime_types::Value;
use donder_runtime_types::bytecode::SignalPixel;

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::from_bytes([red, green, blue])
}

/// Each input's color.
struct Inputs<F>(F);

impl<F: Fn(usize) -> Color> SignalSampler for Inputs<F> {
    fn sample_signal(&mut self, input: usize, _: SignalPixel<i32>) -> Result<Color, RuntimeError> {
        Ok((self.0)(input))
    }
}

fn library() -> Vec<CompiledDefinition> {
    compile_script(include_str!(
        "../../../../examples/starter/operators/standard.donder"
    ))
    .unwrap()
}

fn sample(
    operators: &[CompiledDefinition],
    name: &str,
    overrides: &[(&str, Value)],
    source: impl Fn(usize) -> Color,
) -> Color {
    let operator = operators
        .iter()
        .find(|operator| operator.name().as_str() == name)
        .unwrap();
    let invocation = playback::lower(&bind(operator, overrides));
    invocation
        .evaluate_signals(
            &PixelContext {
                run: RunContext {
                    origin: donder_runtime_types::SampleTime::from_ticks(0),
                    time: SampleDuration::from_ticks(0),
                    duration: SampleDuration::from_ticks(10_000_000),
                    frame_rate: 40.0,
                    pixel_count: 1,
                },
                index: 0,
                fraction: 0.0,
            },
            &SPATIAL,
            &mut Inputs(source),
            &mut StripWorkspace::default(),
        )
        .unwrap()
}

#[test]
fn standard_color_operators_match_expected_channels() {
    let operators = library();
    let inputs = |index: usize| [rgb(200, 100, 50), rgb(128, 255, 0)][index];
    for (name, params, expected) in [
        ("Max", vec![], rgb(200, 255, 50)),
        ("Add", vec![], rgb(255, 255, 50)),
        ("Multiply", vec![], rgb(100, 100, 0)),
        ("IntensityModulate", vec![], rgb(200, 100, 50)),
        ("Dim", vec![("amount", Value::Float(0.5))], rgb(100, 50, 25)),
        ("Invert", vec![], rgb(55, 155, 205)),
        (
            "Colorize",
            vec![("tint", Value::Color(rgb(255, 128, 0)))],
            rgb(200, 100, 0),
        ),
    ] {
        assert_eq!(
            sample(&operators, name, &params, inputs).to_bytes(),
            expected.to_bytes(),
            "{name}"
        );
    }
}
