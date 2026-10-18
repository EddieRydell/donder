//! Operator fusion (`Instance::fuse_input`) and black inputs
//! (`Instance::with_black_input`) keep what playback produces.
use super::evaluation::{
    OperatorEvaluation, SignalSampler, bind, build, chain, compile_operator, context, effect,
};
use super::evaluation::{RuntimeError, StripWorkspace};
use super::playback;
use super::std;
use donder_language::compiler::{CompiledOperator, Instance, ProgramConstants, compile_operators};
use donder_runtime_types::bytecode::{Instruction, SignalPixel};
use donder_runtime_types::{Color, SampleTime};
use donder_runtime_types::{OperatorInvocation, SampleInvocation, Value};
use donder_runtime_types::{PreparedAutomation, SpatialContext};
use std::prelude::rust_2024::*;

fn instance(operator: &CompiledOperator, values: &[(&str, Value)]) -> Instance {
    bind(operator, values).instance(ProgramConstants::default())
}

fn operators(source: &str) -> Vec<CompiledOperator> {
    compile_operators(source).unwrap()
}

const TICKS: [u32; 7] = [0, 1, 124_999, 125_000, 3_000_001, 7_999_999, 1];

/// Playback of `source` through `chain`, and through `fused` alone.
fn assert_fused_playback(
    count: usize,
    source: &SampleInvocation,
    operators: &[OperatorInvocation],
    fused: &OperatorInvocation,
    what: &str,
) {
    let mut original = chain(count, source, 1, operators).into_playback();
    let mut composed = chain(count, source, 1, std::slice::from_ref(fused)).into_playback();
    for ticks in TICKS {
        let time = SampleTime::from_ticks(ticks);
        playback::assert_frames_close(
            original.evaluate(time).colors(),
            composed.evaluate(time).colors(),
            &format!("{what} ticks={ticks}"),
        );
    }
}

fn gradient_source() -> SampleInvocation {
    effect("effect Source { sample { rgb(0.8, pixel.fraction, progress) } }")
}

#[test]
fn fusion_samples_conditionally() {
    struct Observe {
        calls: Vec<(usize, SignalPixel<i32>)>,
        fail: Option<usize>,
    }
    impl SignalSampler for Observe {
        fn sample_signal(
            &mut self,
            input: usize,
            pixel: SignalPixel<i32>,
        ) -> Result<Color, RuntimeError> {
            self.calls.push((input, pixel));
            if self.fail == Some(input) {
                Err(RuntimeError {
                    message: "source failed".into(),
                })
            } else {
                Ok(Color::BLACK)
            }
        }
    }
    let definitions = operators(
        "operator Inner { input first; input second; sample {
            let value = first;
            if pixel.index == 0 { value } else { value + second }
        } }
        operator Outer { input other; input source; sample {
            guard pixel.index != -1;
            other.at(0) + source + other.at(1)
        } }",
    );
    let source = instance(&definitions[0], &[]);
    let caller = instance(&definitions[1], &[]);
    let fused = caller.fuse_input(1, &source).unwrap();
    assert_eq!(fused.inputs(), 3);
    let fused = fused.operator();
    for pixel in [0, 1] {
        for fail in [None, Some(0), Some(1), Some(2)] {
            let mut expected = vec![
                (0, SignalPixel::Local(0)),
                (0, SignalPixel::Local(1)),
                (1, SignalPixel::Current),
            ];
            if pixel == 1 {
                expected.push((2, SignalPixel::Current));
            }
            // A failed query is reported after the program finishes.
            let error = expected.iter().any(|(input, _)| Some(*input) == fail);
            let mut sampler = Observe {
                calls: Vec::new(),
                fail,
            };
            let result = fused.evaluate(
                &context(2, pixel, 0),
                &SpatialContext {
                    position: [0.0; 2],
                    min: [0.0; 2],
                    max: [0.0; 2],
                },
                &mut sampler,
                &mut StripWorkspace::default(),
            );
            assert_eq!(result.is_err(), error);
            sampler
                .calls
                .sort_by_key(|&(input, pixel)| (input, pixel.index().copied()));
            assert_eq!(sampler.calls, expected, "pixel={pixel} fail={fail:?}");
        }
    }
}

#[test]
fn fusing_a_black_source_folds_its_consumer() {
    let definitions = operators(
        "operator Black { input source; sample { #000000 } }
        operator Conditional { input source; sample {
            let value = source;
            if value != #000000 { value + sum for i in 0..3 { [#010203, #020304, #030405][i] } } else { value }
        } }",
    );
    let black = instance(&definitions[0], &[]);
    let consumer = instance(&definitions[1], &[]);
    assert_eq!(black.constant_color(), Some(Color::BLACK));
    assert_eq!(consumer.constant_color(), None);
    let fused = consumer.fuse_input(0, &black).unwrap();
    let substituted = consumer.with_black_input(0);
    for folded in [&fused, &substituted] {
        assert_eq!(folded.constant_color(), Some(Color::BLACK));
        let operator = folded.operator();
        let bytecode = operator.program().bytecode();
        assert!(
            !bytecode
                .instructions()
                .any(|instruction| matches!(instruction, Instruction::Reduce { .. })),
            "{}",
            donder_runtime_types::bytecode::listing(bytecode)
        );
    }
    assert_eq!(fused.inputs(), 1);
    assert_eq!(substituted.inputs(), 0);
    let bright = effect("effect Bright { sample { #ffffff } }");
    assert!(
        chain(9, &bright, 1, &[fused.operator()])
            .into_playback()
            .evaluate(SampleTime::from_ticks(0))
            .colors()
            .iter()
            .all(|c| *c == Color::BLACK)
    );
}

#[test]
fn black_inputs_fold_samples_and_shift_later_inputs() {
    let library = compile_operators(include_str!(
        "../../../../examples/starter/operators/standard.donder"
    ))
    .unwrap();
    let find = |name: &str| {
        library
            .iter()
            .find(|operator| operator.name().as_str() == name)
            .unwrap()
    };
    let identity = playback::operator(&compile_operator(playback::IDENTITY_SOURCE));
    let source = gradient_source();
    // The remaining input behaves like the operator's other input alone.
    for (name, black) in [("Max", 0), ("Max", 1), ("Add", 0), ("Add", 1)] {
        let substituted = instance(find(name), &[]).with_black_input(black);
        assert_eq!(substituted.inputs(), 1, "{name}");
        assert_eq!(substituted.constant_color(), None, "{name}");
        assert_fused_playback(
            9,
            &source,
            std::slice::from_ref(&identity),
            &substituted.operator(),
            &format!("{name} without input {black}"),
        );
    }
    // Black under color max, add and scale is black, and so is any color
    // multiplied by black, scaled by zero intensity or given zero value.
    for (name, input) in [
        ("Dim", 0),
        ("Multiply", 0),
        ("Multiply", 1),
        ("IntensityModulate", 0),
        ("IntensityModulate", 1),
        ("Colorize", 0),
        ("HueShift", 0),
    ] {
        let substituted = instance(find(name), &[]).with_black_input(input);
        assert_eq!(substituted.constant_color(), Some(Color::BLACK), "{name}");
    }
    let both = instance(find("Max"), &[])
        .with_black_input(0)
        .with_black_input(0);
    assert_eq!(
        (both.inputs(), both.constant_color()),
        (0, Some(Color::BLACK))
    );
    // Inverting black is white, an ordinary color afterwards.
    assert_eq!(
        instance(find("Invert"), &[])
            .with_black_input(0)
            .constant_color(),
        Some(Color::from_bytes([255, 255, 255]))
    );
}

#[test]
fn fusion_remaps_parameters_inputs_reductions_and_guards() {
    let definitions = operators(
        "operator Inner { input a; input b;
            param gain: float in 0.0..1.0 = 0.7; param first: bool = true; param tint: color = #314159;
            sample {
                guard pixel.index % 3 != 0 else tint;
                max for i in 0..3 { (if first { a } else { b }) * (gain - i * 0.2) }
            }
        }
        operator Outer { input other; input source;
            param gain: float in 0.0..1.0 = 0.4;
            sample {
                guard pixel.index != -1;
                max(source * gain, other)
            }
        }",
    );
    let outer = instance(&definitions[1], &[]);
    let colors = ["#e03040", "#2080c0", "#010203"]
        .map(|color| effect(&format!("effect Color {{ sample {{ {color} }} }}")));
    for first in [false, true] {
        let inner = instance(
            &definitions[0],
            &[
                ("first", Value::Bool(first)),
                ("tint", Value::Color(Color::from_bytes([49, 65, 89]))),
            ],
        );
        let fused = outer
            .fuse_input(1, &inner)
            .expect("one sample of the current pixel");
        assert_eq!(fused.inputs(), 3);
        let (fused, inner, outer) = (fused.operator(), inner.operator(), outer.operator());
        let show = |fuse| {
            build(17, 8_000_000, |b, target| {
                let layers = colors.each_ref().map(|effect| {
                    let sample = b.sample(effect, b.whole_sequence(), target);
                    b.layer(true, [sample])
                });
                let output = if fuse {
                    b.operator(&fused, |input| [layers[2], layers[0], layers[1]][input])
                } else {
                    let inner = b.operator(&inner, |input| layers[input]);
                    b.operator(&outer, |input| [layers[2], inner][input])
                };
                b.output(Some(output))
            })
        };
        let mut original = show(false).into_playback();
        let mut composed = show(true).into_playback();
        for ticks in TICKS {
            let time = SampleTime::from_ticks(ticks);
            playback::assert_frames_close(
                original.evaluate(time).colors(),
                composed.evaluate(time).colors(),
                &format!("first={first} ticks={ticks}"),
            );
        }
    }
}

#[test]
fn fusion_preserves_resource_parameters() {
    use donder_runtime_types::{Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration};
    let definitions = operators(
        "operator Inner { input source;
            param levels: array<float>; param shape: curve in 0.0..1.0;
            param colors: gradient; param mode: enum { First, Second } = Second;
            param beats: marks;
            sample {
                let values = [levels[0], value_or(shape[progress], 0.3), levels[1]];
                let gain = values[pixel.index % 3];
                let gain = if mode == First { gain * 0.5 } else { gain };
                let gain = gain + sum for i in 0..len(beats) { mark_at(beats, i) * 0.01 };
                max(source * gain, colors[progress])
            }
        }
        operator Outer { input source;
            param unused: array<float>; param unused_shape: curve in 0.0..1.0;
            param unused_colors: gradient; param unused_mode: enum { First, Second } = First;
            param unused_beats: marks;
            sample { source * 0.7 }
        }",
    );
    let inner = instance(
        &definitions[0],
        &[
            (
                "levels",
                Value::Array(vec![Value::Float(0.2), Value::Float(0.8)].into()),
            ),
            (
                "shape",
                Value::Curve(
                    Curve {
                        points: vec![
                            CurvePoint {
                                position: 0.0,
                                value: 0.1,
                            },
                            CurvePoint {
                                position: 1.0,
                                value: 0.9,
                            },
                        ],
                    }
                    .into(),
                ),
            ),
            (
                "colors",
                Value::Gradient(
                    Gradient {
                        stops: vec![
                            GradientStop {
                                position: 0.0,
                                color: Color::from_bytes([4, 8, 12]),
                            },
                            GradientStop {
                                position: 1.0,
                                color: Color::from_bytes([4, 16, 2]),
                            },
                        ],
                    }
                    .into(),
                ),
            ),
            (
                "beats",
                Value::Marks(
                    Marks::new([1_000_000, 2_000_000].map(SampleDuration::from_ticks)).into(),
                ),
            ),
        ],
    );
    let outer = instance(
        &definitions[1],
        &[
            ("unused", Value::Array(vec![Value::Float(0.4)].into())),
            (
                "unused_shape",
                Value::Curve(Curve { points: vec![] }.into()),
            ),
            (
                "unused_colors",
                Value::Gradient(Gradient { stops: vec![] }.into()),
            ),
            ("unused_beats", Value::Marks(Marks::empty().into())),
        ],
    );
    let fused = outer
        .fuse_input(0, &inner)
        .expect("one sample of the current pixel");
    assert_fused_playback(
        17,
        &gradient_source(),
        &[inner.operator(), outer.operator()],
        &fused.operator(),
        "resources",
    );
}

#[test]
fn automated_sources_and_consumers_fuse_with_their_automation() {
    use donder_runtime_types::AutomationMapping;
    use donder_runtime_types::{Curve, CurvePoint, SampleDuration};
    let definitions = operators(
        "operator Gain { input source; param gain: float in 0.0..1.0 = 0.7;
            sample { source * gain }
        }
        operator Scaled { input source; param gain: float in 0.0..1.0 = 0.4;
            sample { source * (gain * 0.5) }
        }",
    );
    let automation = || {
        Box::new([PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            param_index: 0,
            curve: Curve {
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
            }
            .into(),
            mapping: AutomationMapping::Float { min: 0.2, max: 0.9 },
            quantity: donder_runtime_types::AutomatedQuantity::Value,
        }]) as Box<[PreparedAutomation]>
    };
    let automated = |operator: &CompiledOperator| {
        operator
            .invoke(vec![Value::Float(0.5)], automation())
            .unwrap()
            .instance(ProgramConstants::default())
    };
    let gain = instance(&definitions[0], &[]);
    let scaled = automated(&definitions[1]);
    let source = gradient_source();
    // The consumer's own automation is kept.
    let fused = scaled.fuse_input(0, &gain).unwrap();
    assert_fused_playback(
        17,
        &source,
        &[gain.operator(), scaled.operator()],
        &fused.operator(),
        "consumer automation",
    );
    // The source's automation is kept too.
    let consumer = instance(&definitions[0], &[("gain", Value::Float(0.4))]);
    let upstream = automated(&definitions[0]);
    let fused = consumer.fuse_input(0, &upstream).unwrap();
    assert_eq!(fused.operator().automation().len(), 1);
    assert_fused_playback(
        17,
        &source,
        &[upstream.operator(), consumer.operator()],
        &fused.operator(),
        "source automation",
    );
}

#[test]
fn fusion_keeps_the_boundary_for_shared_or_addressed_samples() {
    let gain = instance(
        &compile_operator("operator Gain { input source; sample { source * 0.5 } }"),
        &[],
    );
    for consumer in [
        "max(source, source.at(pixel.index + 1))",
        "source.at(pixel.index + 1)",
        "source.at(pixel.index)",
        "max(source, previous)",
    ] {
        let consumer = instance(
            &compile_operator(&format!(
                "operator Consumer {{ input source; sample {{ {consumer} }} }}"
            )),
            &[],
        );
        assert!(consumer.fuse_input(0, &gain).is_none());
    }
    // One sample site inside a reduction runs the source per iteration.
    let consumer = instance(
        &compile_operator(
            "operator Steps { input source; sample {
                max for i in 0..3 { guard pixel.index >= i; source * (1.0 - i * 0.25) }
            } }",
        ),
        &[],
    );
    let fused = consumer.fuse_input(0, &gain).unwrap();
    assert_fused_playback(
        17,
        &gradient_source(),
        &[gain.operator(), consumer.operator()],
        &fused.operator(),
        "reduction",
    );
}
