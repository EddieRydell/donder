extern crate alloc;

use alloc::{boxed::Box, vec, vec::Vec};
use core::num::NonZeroU32;
use donder_runtime::{
    BoundParams, BytecodeProgram, DslBindCache, FixtureGeometry, Instruction, OperatorDefinition,
    OperatorInvocation, OperatorProgram, OutputEncoding, PreparedAutomation, PreparedSequence,
    RgbOrder, RunContext, SampleDefinition, SampleDuration, SampleProgram, SampleTime,
    SequenceBuilder, SequenceRoot, SequenceTiming, SequenceWindow, SignalHandle, TargetScope,
};

#[derive(Clone)]
pub struct SampleFixture {
    pub program: SampleProgram,
    pub params: BoundParams,
    pub start: SampleTime,
    pub duration: SampleDuration,
    pub automation: Vec<PreparedAutomation>,
}

/// Typed benchmark inputs. The runtime builder owns all graph and storage addresses.
#[derive(Clone)]
pub struct Workload {
    count: usize,
    layers: Vec<Vec<SampleFixture>>,
    operators: Vec<OperatorInvocation>,
    lookup: Option<[u8; 256]>,
}

impl Workload {
    pub fn samples(count: usize, layers: Vec<Vec<SampleFixture>>) -> Self {
        Self {
            count,
            layers,
            operators: vec![],
            lookup: None,
        }
    }

    pub fn prepare(self) -> PreparedSequence {
        self.prepare_with(|builder, _layers, signal| builder.output([signal]))
    }

    /// Extend the fixture using its layers and composed signal after its operator chain.
    /// Fixture geometry, timing and the GRB output route remain builder-owned.
    pub fn prepare_with(
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
            let mut cache = DslBindCache::default();
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
                            .bind(effect.params.iter_values().collect(), &mut cache)
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

pub const COUNTS: [usize; 4] = [200, 400, 800, 1600];
pub const FRAMES: usize = 32;
pub const GAMMA_CASE: usize = 5; // PixelRamp in the shared fixture list.
pub const OPERATOR_DEPTHS: [usize; 3] = [2, 4, 8];
#[allow(dead_code)] // PC profiler and host golden generation only.
pub const CHASE_PULSE_CASES: [(&str, usize); 3] =
    [("ChasePulse1", 1), ("ChasePulse4", 4), ("ChasePulse16", 16)];
#[allow(dead_code)]
pub const MARK_CASES: [(&str, bool); 2] = [("MarkPulse200", true), ("MarkChase200", false)];

// Profiling fixture: varied, overlapping chases and pulses compiled from the
// same editable effect document included in new projects.
#[cfg(not(target_arch = "xtensa"))]
#[allow(dead_code)] // Normal timing binary uses a different workload subset.
pub fn chase_pulse_show(count: usize, layers: usize) -> Workload {
    use donder_language::dsl::{bind_params, compile_effects};
    use donder_runtime::{Color, Curve, CurvePoint, Gradient, GradientStop};
    use donder_runtime::{Identifier, Value};
    let definitions = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
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
            &mut DslBindCache::default(),
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
pub fn nest_operator(show: &mut Workload, depth: usize) {
    assert!(depth > 0);
    assert_eq!(show.operators.len(), 1);
    show.operators.resize(depth, show.operators[0].clone());
}

pub fn insert_invert(show: &mut Workload, program: BytecodeProgram) {
    nest_operator(show, 2);
    show.operators.insert(1, operator_invocation(program));
}

fn operator_invocation(program: BytecodeProgram) -> OperatorInvocation {
    let operator = OperatorProgram::admit(program, 1, Box::new([])).unwrap();
    OperatorDefinition::new(operator)
        .bind(vec![], &mut DslBindCache::default())
        .unwrap()
}

#[cfg(not(target_arch = "xtensa"))]
pub fn apply_compiled_operator(
    show: &mut Workload,
    operator: donder_language::dsl::CompiledOperator,
) {
    let params = donder_language::dsl::bind_params(
        operator.params(),
        core::iter::empty(),
        &mut DslBindCache::default(),
    )
    .unwrap();
    show.operators = vec![
        OperatorDefinition::new(operator.program().clone())
            .bind(params.iter_values().collect(), &mut DslBindCache::default())
            .unwrap(),
    ];
}

pub fn apply_operator(show: &mut Workload, mut program: BytecodeProgram, reuse: bool) {
    if !reuse {
        disable_uniform_reuse(&mut program);
    }
    show.operators = vec![operator_invocation(program)];
}

pub fn set_uniform_upstream(show: &mut Workload, reuse: bool) {
    if reuse {
        return;
    }
    let effect = &mut show.layers[0][0];
    assert!(!effect.program.bytecode().uses_pixel_context);
    let (mut program, types) = effect.program.clone().into_parts();
    // A real, unused pixel read disables whole-result reuse without changing
    // colors. Dependency metadata must agree with the admitted instructions;
    // flipping its flag alone would make this benchmark fixture invalid.
    let dst = donder_runtime::FloatSlot(program.layout.floats);
    program.layout.floats += 1;
    let mut instructions = program.instructions.into_vec();
    let return_color = instructions.pop().unwrap();
    assert!(matches!(return_color, Instruction::ReturnColor(_)));
    instructions.push(Instruction::ContextRead {
        dst: donder_runtime::NumberSlot::Float(dst),
        read: donder_runtime::ContextRead::PixelFraction,
    });
    instructions.push(return_color);
    program.instructions = instructions.into_boxed_slice();
    program.uses_pixel_context = true;
    effect.program = SampleProgram::admit(program, types).unwrap();
}

#[allow(dead_code)] // Compiled on the host, not the device.
pub const IDENTITY_SOURCE: &str =
    "operator Identity { input Signal source; color sample() { return source.at(seconds()); } }";

#[allow(dead_code)] // Compiled on the host, not the device.
pub const GROUPED_SOURCE: &str = "operator Times { input Signal source; color sample() {
    float now = seconds(); float past = now - 0.1;
    color a = source.at(now); color b = source.at(now);
    color c = source.at(past); color d = source.at(past);
    return max(max(a, b), max(c, d));
} }";
#[allow(dead_code)]
pub const ALTERNATING_SOURCE: &str = "operator Times { input Signal source; color sample() {
    float now = seconds(); float past = now - 0.1;
    color a = source.at(now); color b = source.at(past);
    color c = source.at(now); color d = source.at(past);
    return max(max(a, b), max(c, d));
} }";

pub fn apply_pulse_automation(show: &mut Workload, program: SampleProgram, empty: bool) {
    use donder_runtime::{Color, Curve, CurvePoint, Gradient, GradientStop};
    use donder_runtime::{Type, Value};
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
        &mut DslBindCache::default(),
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
            mapping: donder_runtime::AutomationMapping::Curve {
                min: if empty { 0.5 } else { 0.0 },
                max: 1.0,
            },
            param_index: 1,
        }],
    };
}

#[allow(dead_code)] // Compiled on the host; firmware receives only bytecode.
pub const OPERATOR_SOURCE: &str = "operator Wave { input Signal source;
    color sample() { return source.at(seconds()) * (sin(seconds() * 7.0) * 0.5 + 0.5); }
}";

pub fn disable_uniform_reuse(program: &mut BytecodeProgram) {
    program.pixel_entry = 0;
    for instruction in &mut program.instructions {
        if let Instruction::SignalSample { frame_cache, .. } = instruction {
            *frame_cache = u32::MAX;
        }
    }
}

// The firmware receives this host-prepared lookup as data.
#[cfg(not(target_arch = "xtensa"))]
pub fn gamma_lookup() -> [u8; 256] {
    core::array::from_fn(|value| ((value as f32 / 255.0).powf(2.2) * 255.0).round() as u8)
}

pub fn apply_gamma(show: &mut Workload, lookup: [u8; 256]) {
    show.lookup = Some(lookup);
}

pub fn time(frame: usize) -> SampleTime {
    SampleTime::from_ticks(3_000_000 + frame as u32 * 8_333)
}

pub fn context(count: usize, pixel: usize, frame: usize) -> RunContext {
    RunContext {
        progress: time(frame).as_ticks() as f32 / 8_000_000.0,
        time: SampleDuration::from_ticks(time(frame).as_ticks()),
        duration: SampleDuration::from_ticks(8_000_000),
        pixel_index: pixel as i32,
        pixel_count: count as i32,
        pixel_fraction: pixel as f32 / count.saturating_sub(1).max(1) as f32,
    }
}

pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

pub fn show(count: usize, program: SampleProgram, params: BoundParams) -> Workload {
    layered_show(count, program, params, 1)
}

// Identical overlapping inputs preserve the max-composited golden output.
pub fn layered_show(
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
