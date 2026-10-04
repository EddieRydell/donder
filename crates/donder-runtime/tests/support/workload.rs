extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec};
use core::num::NonZeroU32;
use donder_language::dsl::bytecode::{BytecodeProgram, Instruction};
use donder_language::dsl::{
    BoundParams, OperatorDefinition, OperatorInvocation, OperatorProgram, SampleDefinition,
    SampleProgram,
};
use donder_language::execution::{
    FixtureGeometry, OutputEncoding, PreparedAutomation, RgbOrder, SequenceTiming, SequenceWindow,
    TargetScope,
};
use donder_language::values::{SampleDuration, SampleTime};
use donder_runtime::{PreparedSequence, SequenceBuilder, SequenceRoot, SignalHandle};

#[derive(Clone)]
pub(crate) struct SampleFixture {
    pub(crate) program: SampleProgram,
    pub(crate) params: BoundParams,
    pub(crate) start: SampleTime,
    pub(crate) duration: SampleDuration,
    pub(crate) automation: Vec<PreparedAutomation>,
}

/// Typed benchmark inputs. The runtime builder owns all graph and storage addresses.
#[derive(Clone)]
pub(crate) struct Workload {
    count: usize,
    layers: Vec<Vec<SampleFixture>>,
    operators: Vec<OperatorInvocation>,
    lookup: Option<[u8; 256]>,
}

impl Workload {
    pub(crate) fn samples(count: usize, layers: Vec<Vec<SampleFixture>>) -> Self {
        Self {
            count,
            layers,
            operators: vec![],
            lookup: None,
        }
    }

    pub(crate) fn prepare(self) -> PreparedSequence {
        self.prepare_with(|builder, _layers, signal| builder.output([signal]))
    }

    /// Extend the fixture using its layers and composed signal after its operator chain.
    /// Fixture geometry, timing and the GRB output route remain builder-owned.
    pub(crate) fn prepare_with(
        self,
        build: impl for<'id> FnOnce(
            &mut SequenceBuilder<'id>,
            &[SignalHandle<'id>],
            SignalHandle<'id>,
        ) -> SequenceRoot<'id>,
    ) -> PreparedSequence {
        let windows = self
            .layers
            .iter()
            .flatten()
            .map(|effect| SequenceWindow {
                start: effect.start,
                duration: NonZeroU32::new(effect.duration.as_ticks()).unwrap(),
            })
            .collect();
        let timing = SequenceTiming::admit(
            NonZeroU32::new(120).unwrap(),
            NonZeroU32::new(960).unwrap(),
            NonZeroU32::new(8_000_000).unwrap(),
            windows,
        )
        .unwrap();
        PreparedSequence::build(timing, |builder| {
            let fixture = builder.fixture(
                0,
                FixtureGeometry::admit((0..self.count).map(|pixel| [pixel as f32, 0.0]).collect())
                    .unwrap(),
            );
            let target = builder.target([fixture], TargetScope::PerFixture);
            let windows: Vec<_> = builder.windows().collect();
            let mut windows = windows.into_iter();
            let mut definitions: Vec<(SampleProgram, SampleDefinition)> = Vec::new();
            let mut layers = Vec::new();
            for layer in &self.layers {
                let effects: Vec<_> = layer
                    .iter()
                    .map(|effect| {
                        let definition = match definitions
                            .iter()
                            .find(|(program, _)| program == &effect.program)
                        {
                            Some((_, definition)) => definition.clone(),
                            None => {
                                let definition = SampleDefinition::new(effect.program.clone());
                                definitions.push((effect.program.clone(), definition.clone()));
                                definition
                            }
                        };
                        let invocation = definition
                            .bind(effect.params.iter_values().collect())
                            .unwrap()
                            .with_automation(effect.automation.clone().into())
                            .unwrap();
                        builder.sample(&invocation, windows.next().unwrap(), target)
                    })
                    .collect();
                layers.push(builder.layer(true, effects));
            }
            // Operator fixtures measure Layer 0 -> operator chain, including
            // the four-layer Chase/Pulse source used by the Echo cases.
            // Preserve that input selection; layer-only cases mix every layer.
            let mut signal = if layers.len() == 1 || !self.operators.is_empty() {
                layers[0]
            } else {
                builder.mix(layers.iter().copied())
            };
            for operator in &self.operators {
                signal = builder.operator(operator, |input| {
                    assert_eq!(input, 0);
                    signal
                });
            }
            let output = builder.port(0, 0);
            let lookup = self.lookup.map(|table| builder.lookup(table));
            builder.route(output, target, OutputEncoding::Rgb(RgbOrder::Grb), lookup);
            build(builder, &layers, signal)
        })
    }
}

pub(crate) const COUNTS: [usize; 4] = [200, 400, 800, 1600];
pub(crate) const FRAMES: usize = 32;
pub(crate) const GAMMA_CASE: usize = 5; // PixelRamp in the shared fixture list.
pub(crate) const OPERATOR_DEPTHS: [usize; 3] = [2, 4, 8];
#[allow(dead_code)] // PC profiler and host golden generation only.
pub(crate) const CHASE_PULSE_CASES: [(&str, usize); 3] =
    [("ChasePulse1", 1), ("ChasePulse4", 4), ("ChasePulse16", 16)];
#[allow(dead_code)]
pub(crate) const MARK_CASES: [(&str, bool); 2] = [("MarkPulse200", true), ("MarkChase200", false)];

// Profiling fixture: varied, overlapping chases and pulses compiled from the
// same editable effect document included in new projects.
#[allow(dead_code)] // Normal timing binary uses a different workload subset.
pub(crate) fn chase_pulse_show(count: usize, layers: usize) -> Workload {
    use donder_language::dsl::{Identifier, Value, bind_params, compile_effects};
    use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let definitions = compile_effects(include_str!(
        "../../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap();
    let chase = definitions
        .iter()
        .find(|definition| definition.name().as_str() == "Chase")
        .unwrap();
    let pulse = definitions
        .iter()
        .find(|definition| definition.name().as_str() == "Pulse")
        .unwrap();
    let mut effects = vec::Vec::new();
    let shape: Value = Value::Curve(
        Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 0.25,
                    value: 1.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        }
        .into(),
    );
    let position: Value = Value::Curve(
        Curve {
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
    );
    for index in 0..layers {
        let gradient = Value::Gradient(
            Gradient {
                stops: vec![GradientStop {
                    position: 0.0,
                    color: Color {
                        red: (47 + index * 31) as u8,
                        green: (193 + index * 17) as u8,
                        blue: (89 + index * 43) as u8,
                    },
                }],
            }
            .into(),
        );
        let chasing = index % 2 == 0;
        let values = if chasing {
            vec![
                ("gradient", gradient),
                (
                    "gradient_mode",
                    Value::Enum(Identifier::new("across_items".into()).unwrap()),
                ),
                ("pulse_overlap", Value::Float(12.0 + index as f32)),
                ("section_width_pixels", Value::Int(3 + index as i32 % 4)),
                ("chase_position", position.clone()),
                ("reverse", Value::Bool(index % 4 == 0)),
                ("extend_to_start", Value::Bool(true)),
                ("extend_to_end", Value::Bool(true)),
                ("pulse_shape", shape.clone()),
            ]
        } else {
            vec![("gradient", gradient), ("pulse_shape", shape.clone())]
        };
        let overrides = values
            .into_iter()
            .map(|(name, value)| (Identifier::new(name.into()).unwrap(), value))
            .collect::<vec::Vec<_>>();
        let params = bind_params(
            if chasing {
                chase.params()
            } else {
                pulse.params()
            },
            overrides.iter().map(|(name, value)| (name, value)),
        )
        .unwrap();
        effects.push(SampleFixture {
            program: if chasing { chase } else { pulse }.sample_program().clone(),
            params,
            start: SampleTime::from_ticks(index as u32 * 43_000),
            duration: SampleDuration::from_ticks(4_000_000 + index as u32 * 97_000),
            automation: vec![],
        });
    }
    Workload::samples(
        count,
        effects.into_iter().map(|effect| vec![effect]).collect(),
    )
}

// Extend the single-operator fixture into a chain, sharing its admitted definition.
pub(crate) fn nest_operator(show: &mut Workload, depth: usize) {
    assert!(depth > 0);
    assert_eq!(show.operators.len(), 1);
    show.operators.resize(depth, show.operators[0].clone());
}

pub(crate) fn insert_invert(show: &mut Workload, program: BytecodeProgram) {
    nest_operator(show, 2);
    show.operators.insert(1, operator_invocation(program));
}

fn operator_invocation(program: BytecodeProgram) -> OperatorInvocation {
    let operator = OperatorProgram::admit(program, 1, Box::new([])).unwrap();
    OperatorDefinition::new(operator).bind(vec![]).unwrap()
}

pub(crate) fn apply_compiled_operator(
    show: &mut Workload,
    operator: donder_language::dsl::CompiledOperator,
) {
    show.operators = vec![operator.bind(core::iter::empty()).unwrap()];
}

pub(crate) fn apply_operator(show: &mut Workload, mut program: BytecodeProgram, reuse: bool) {
    if !reuse {
        unstaged(&mut program);
    }
    show.operators = vec![operator_invocation(program)];
}

pub(crate) fn set_uniform_upstream(show: &mut Workload, reuse: bool) {
    if reuse {
        return;
    }
    let effect = &mut show.layers[0][0];
    assert!(!effect.program.bytecode().uses_pixel_context);
    let (mut program, types) = effect.program.clone().into_parts();
    // A real, unused pixel read disables whole-result reuse without changing
    // colors. Dependency metadata must agree with the admitted instructions;
    // flipping its flag alone would make this benchmark fixture invalid.
    let dst = donder_language::dsl::bytecode::FloatSlot(program.layout.floats);
    program.layout.floats += 1;
    let mut instructions = program.instructions.into_vec();
    let return_color = instructions.pop().unwrap();
    assert!(matches!(return_color, Instruction::ReturnColor(_)));
    instructions.push(Instruction::ContextRead {
        dst: donder_language::dsl::bytecode::NumberSlot::Float(dst),
        read: donder_language::dsl::bytecode::ContextRead::PixelFraction,
    });
    instructions.push(return_color);
    program.instructions = instructions.into_boxed_slice();
    program.uses_pixel_context = true;
    effect.program = SampleProgram::admit(program, types).unwrap();
}

#[allow(dead_code)] // Compiled on the host, not the device.
pub(crate) const IDENTITY_SOURCE: &str =
    "operator Identity { input Signal source; color sample() { return source.at(seconds()); } }";

#[allow(dead_code)] // Compiled on the host, not the device.
pub(crate) const GROUPED_SOURCE: &str = "operator Times { input Signal source; color sample() {
    float now = seconds(); float past = now - 0.1;
    color a = source.at(now); color b = source.at(now);
    color c = source.at(past); color d = source.at(past);
    return max(max(a, b), max(c, d));
} }";
#[allow(dead_code)]
pub(crate) const ALTERNATING_SOURCE: &str = "operator Times { input Signal source; color sample() {
    float now = seconds(); float past = now - 0.1;
    color a = source.at(now); color b = source.at(past);
    color c = source.at(now); color d = source.at(past);
    return max(max(a, b), max(c, d));
} }";

pub(crate) fn apply_pulse_automation(show: &mut Workload, program: SampleProgram, empty: bool) {
    use donder_language::dsl::{Type, Value};
    use donder_language::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let mut curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.4,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    };
    if empty {
        curve.points.clear();
    }
    let values = [
        (
            Type::Gradient,
            Value::Gradient(
                Gradient {
                    stops: vec![GradientStop {
                        position: 0.0,
                        color: Color {
                            red: 255,
                            green: 128,
                            blue: 64,
                        },
                    }],
                }
                .into(),
            ),
        ),
        (Type::Curve, Value::Curve(curve.clone().into())),
    ];
    let params = BoundParams::bind_values(
        &values.iter().map(|(ty, _)| ty.clone()).collect::<Vec<_>>(),
        values.into_iter().map(|(_, value)| value).collect(),
    )
    .unwrap();
    show.layers[0][0] = SampleFixture {
        program,
        params,
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        automation: vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            curve: curve.into(),
            mapping: donder_language::automation::AutomationMapping::Curve {
                min: if empty { 0.5 } else { 0.0 },
                max: 1.0,
            },
            param_index: 1,
        }],
    };
}

#[allow(dead_code)] // Compiled on the host; firmware receives only bytecode.
pub(crate) const OPERATOR_SOURCE: &str = "operator Wave { input Signal source;
    color sample() { return source.at(seconds()) * (sin(seconds() * 7.0) * 0.5 + 0.5); }
}";

pub(crate) fn unstaged(program: &mut BytecodeProgram) {
    program.pixel_entry = 0;
    for instruction in &mut program.instructions {
        if let Instruction::SignalSample { frame_cache, .. } = instruction {
            *frame_cache = u32::MAX;
        }
    }
}

// The firmware receives this host-prepared lookup as data.
pub(crate) fn gamma_lookup() -> [u8; 256] {
    core::array::from_fn(|value| ((value as f32 / 255.0).powf(2.2) * 255.0).round() as u8)
}

pub(crate) fn apply_gamma(show: &mut Workload, lookup: [u8; 256]) {
    show.lookup = Some(lookup);
}

pub(crate) fn time(frame: usize) -> SampleTime {
    SampleTime::from_ticks(3_000_000 + frame as u32 * 8_333)
}

pub(crate) fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

pub(crate) fn show(count: usize, program: SampleProgram, params: BoundParams) -> Workload {
    layered_show(count, program, params, 1)
}

// Identical overlapping inputs preserve the max-composited golden output.
pub(crate) fn layered_show(
    count: usize,
    program: SampleProgram,
    params: BoundParams,
    layers: usize,
) -> Workload {
    assert!(layers > 0);
    let effect = SampleFixture {
        program,
        params,
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        automation: vec![],
    };
    Workload::samples(count, vec![vec![effect]; layers])
}
