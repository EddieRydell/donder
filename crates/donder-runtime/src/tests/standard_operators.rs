use super::{evaluation::OperatorEvaluation, std};
use std::prelude::rust_2024::*;
const SPATIAL: donder_language::execution::SpatialContext =
    donder_language::execution::SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };

use super::evaluation::SignalSampler;
use crate::dsl::BatchWorkspace;
use crate::dsl::RunContext;
use crate::dsl::RuntimeError;
use donder_language::dsl::CompiledOperator;
use donder_language::dsl::Identifier;
use donder_language::dsl::Value;
use donder_language::dsl::bytecode::SignalPixel;
use donder_language::dsl::compile_operators;
use donder_language::values::Color;
use donder_language::values::SampleDuration;
use donder_language::values::SampleTime;

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color { red, green, blue }
}

struct Inputs<F> {
    sample: F,
    times: Vec<u32>,
}

impl<F: Fn(usize, u32) -> Color> SignalSampler for Inputs<F> {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        self.times.push(time.as_ticks());
        Ok((self.sample)(input, time.as_ticks()))
    }
}

fn library() -> Vec<CompiledOperator> {
    compile_operators(include_str!(
        "../../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
}

fn sample(
    operators: &[CompiledOperator],
    name: &str,
    time: u32,
    overrides: &[(&str, Value)],
    source: impl Fn(usize, u32) -> Color,
) -> (Color, Vec<u32>) {
    let operator = operators
        .iter()
        .find(|operator| operator.name().as_str() == name)
        .unwrap();
    let overrides = overrides
        .iter()
        .map(|(name, value)| (Identifier::new((*name).into()).unwrap(), value.clone()))
        .collect::<Vec<_>>();
    let invocation = operator
        .bind(overrides.iter().map(|(name, value)| (name, value)))
        .unwrap();
    let mut inputs = Inputs {
        sample: source,
        times: Vec::new(),
    };
    let color = invocation
        .evaluate(
            &RunContext {
                progress: time as f32 / 10_000_000.0,
                time: SampleDuration::from_ticks(time),
                duration: SampleDuration::from_ticks(10_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &SPATIAL,
            &mut inputs,
            &mut BatchWorkspace::default(),
        )
        .unwrap();
    (color, inputs.times)
}

#[test]
fn standard_color_operators_match_expected_channels() {
    let operators = library();
    let inputs = |index, _| [rgb(200, 100, 50), rgb(128, 255, 0)][index];
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
            sample(&operators, name, 0, &params, inputs).0,
            expected,
            "{name}"
        );
    }
}

#[test]
fn delay_rejects_samples_before_zero() {
    let operators = library();
    let color = rgb(123, 45, 67);
    for (time, delay, expected_time) in [
        (0, 0.25, None),
        (249_999, 0.25, None),
        (250_000, 0.25, Some(0)),
        (500_000, 0.25, Some(250_000)),
        (0, 0.0, Some(0)),
        (500_000, 0.0, Some(500_000)),
    ] {
        let (actual, times) = sample(
            &operators,
            "Delay",
            time,
            &[("seconds", Value::Float(delay))],
            |_, _| color,
        );
        assert_eq!(actual, expected_time.map_or(Color::BLACK, |_| color));
        assert_eq!(times, expected_time.into_iter().collect::<Vec<_>>());
    }
}

#[test]
fn echo_boundaries_and_decay_match_expected_output() {
    let operators = library();
    for (time, repeats, decay, expected, expected_times) in [
        (0, 3, 0.5, rgb(200, 100, 50), vec![0]),
        (249_999, 3, 0.5, Color::BLACK, vec![249_999]),
        (250_000, 3, 0.5, rgb(100, 50, 25), vec![250_000, 0]),
        (500_000, 3, 0.5, rgb(50, 25, 13), vec![500_000, 250_000, 0]),
    ] {
        let params = [
            ("seconds", Value::Float(0.25)),
            ("repeats", Value::Int(repeats)),
            ("decay", Value::Float(decay)),
        ];
        let (actual, times) = sample(&operators, "Echo", time, &params, |_, time| {
            if time == 0 {
                rgb(200, 100, 50)
            } else {
                Color::BLACK
            }
        });
        assert_eq!(
            actual, expected,
            "time={time} repeats={repeats} decay={decay}"
        );
        assert_eq!(times, expected_times);
    }
    {
        let params = [
            ("seconds", Value::Float(0.25)),
            ("repeats", Value::Int(32)),
            ("decay", Value::Float(1.0)),
        ];
        let (color, times) = sample(&operators, "Echo", 8_000_000, &params, |_, time| {
            if time == 0 {
                rgb(200, 100, 50)
            } else {
                Color::BLACK
            }
        });
        assert_eq!(color, rgb(200, 100, 50));
        assert_eq!(
            times,
            (0..=32)
                .rev()
                .map(|index| index * 250_000)
                .collect::<Vec<_>>()
        );
    }
    {
        let params = [
            ("seconds", Value::Float(0.0)),
            ("repeats", Value::Int(3)),
            ("decay", Value::Float(0.5)),
        ];
        let (color, times) = sample(&operators, "Echo", 500_000, &params, |_, _| {
            rgb(200, 100, 50)
        });
        assert_eq!(color, rgb(200, 100, 50));
        assert_eq!(times, vec![500_000; 4]);
    }
}
