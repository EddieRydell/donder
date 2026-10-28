use super::evaluation::{OperatorEvaluation, PixelContext, bind};
use super::playback;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_runtime_types::SpatialContext = donder_runtime_types::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
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
use donder_runtime_types::bytecode::{STATE_INPUT, SignalPixel};

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color::from_bytes([red, green, blue])
}

/// Each input's color; `previous` is input [`STATE_INPUT`].
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
    frames_elapsed: u32,
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
                    progress: 0.0,
                    time: SampleDuration::from_ticks(0),
                    duration: SampleDuration::from_ticks(10_000_000),
                    frame_rate: 40.0,
                    frames_elapsed,
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
            sample(&operators, name, 1, &params, inputs).to_bytes(),
            expected.to_bytes(),
            "{name}"
        );
    }
}

#[test]
fn afterglow_fades_its_previous_frame_by_the_frames_elapsed() {
    let operators = library();
    // A tenth per frame at 40 frames per second.
    let params = [("seconds", Value::Float(0.025))];
    for (frames, input, expected) in [
        (1, rgb(0, 0, 0), rgb(20, 10, 0)),
        (2, rgb(0, 0, 0), rgb(2, 1, 0)),
        (1, rgb(30, 0, 40), rgb(30, 10, 40)),
    ] {
        let color = sample(&operators, "Afterglow", frames, &params, |index| {
            if index == usize::from(STATE_INPUT) {
                rgb(200, 100, 0)
            } else {
                input
            }
        });
        assert_eq!(color.to_bytes(), expected.to_bytes(), "frames={frames}");
    }
}
