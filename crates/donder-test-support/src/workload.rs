//! Typed playback workloads: layers of effect fixtures and operator chains on
//! one fixture, prepared by the runtime builder.
use core::num::NonZeroU32;
use donder_language::compiler::{CompiledOperator, Invocation, ProgramConstants};
use donder_runtime::{PreparedSequence, SequenceBuilder, SequenceRoot, SignalHandle};
use donder_runtime_types::bytecode::{Bank, BytecodeProgram, Instruction, NO_FRAME_CACHE, Slot};
use donder_runtime_types::{
    BoundParams, OperatorDefinition, OperatorInvocation, OperatorProgram, SampleDefinition,
    SampleProgram,
};
use donder_runtime_types::{
    FixtureGeometry, OutputEncoding, PreparedAutomation, RgbOrder, SequenceTiming, SequenceWindow,
    TargetScope,
};
use donder_runtime_types::{SampleDuration, SampleTime};

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

pub const COUNTS: [usize; 4] = [200, 400, 800, 1600];
pub const FRAMES: usize = 32;

// Profiling fixture: varied, overlapping chases and pulses compiled from the
// same editable effect document included in new projects.
pub fn chase_pulse_show(count: usize, layers: usize) -> Workload {
    use donder_language::compiler::{bind_params, compile_effects};
    use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientStop};
    use donder_runtime_types::{Identifier, Value};
    let definitions = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.donder"
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
    let mut effects = Vec::new();
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
                    Value::Enum(Identifier::new("AcrossItems".into()).unwrap()),
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
            .collect::<Vec<_>>();
        let params = bind_params(
            if chasing {
                chase.params()
            } else {
                pulse.params()
            },
            overrides.iter().map(|(name, value)| (name, value)),
        )
        .unwrap();
        let invocation = if chasing { chase } else { pulse }
            .invoke(params.iter_values().collect(), Box::new([]))
            .unwrap();
        effects.push(sample_fixture(
            &invocation,
            count,
            SampleTime::from_ticks(index as u32 * 43_000),
            SampleDuration::from_ticks(4_000_000 + index as u32 * 97_000),
        ));
    }
    Workload::samples(
        count,
        effects.into_iter().map(|effect| vec![effect]).collect(),
    )
}

/// Run the single operator again after `invert`, sharing its admitted definition.
pub fn insert_invert(show: &mut Workload, invert: &CompiledOperator) {
    assert_eq!(show.operators.len(), 1);
    let operator = show.operators[0].clone();
    show.operators
        .extend([operator_invocation(invert), operator]);
}

/// An operator with its defaults, lowered for the eight-second fixture sequence.
pub fn operator_invocation(operator: &CompiledOperator) -> OperatorInvocation {
    operator
        .bind(core::iter::empty())
        .unwrap()
        .instance(ProgramConstants {
            pixel_count: None,
            duration_seconds: Some(8.0),
        })
        .operator()
}

pub fn apply_compiled_operator(show: &mut Workload, operator: &CompiledOperator) {
    show.operators = vec![operator_invocation(operator)];
}

/// Use `operator`; without `reuse`, its initialization runs for every strip.
pub fn apply_operator(show: &mut Workload, operator: &CompiledOperator, reuse: bool) {
    let invocation = operator_invocation(operator);
    show.operators = vec![if reuse {
        invocation
    } else {
        edit_operator(&invocation, unstaged)
    }];
}

/// `invocation` with its lowered bytecode edited and readmitted, keeping its
/// parameter values and automation.
pub fn edit_operator(
    invocation: &OperatorInvocation,
    edit: impl FnOnce(&mut BytecodeProgram),
) -> OperatorInvocation {
    let program = (**invocation.program()).clone();
    let (inputs, types) = (program.input_count(), program.parameter_types().into());
    let mut bytecode = program.into_bytecode();
    edit(&mut bytecode);
    OperatorDefinition::new(OperatorProgram::admit(bytecode, inputs, types).unwrap())
        .bind(invocation.params().iter_values().collect())
        .unwrap()
        .with_automation(invocation.automation().into())
        .unwrap()
}

/// Edit the first effect's lowered bytecode.
pub fn edit_effect(show: &mut Workload, edit: impl FnOnce(&mut BytecodeProgram)) {
    let effect = &mut show.layers[0][0];
    let types = effect.program.input_types().into();
    let mut program = effect.program.clone().into_bytecode();
    edit(&mut program);
    effect.program = SampleProgram::admit(program, types).unwrap();
}

/// Without `reuse`, make the first effect's uniform result a row, which
/// disables whole-result reuse without changing colors.
pub fn set_uniform_upstream(show: &mut Workload, reuse: bool) {
    if reuse {
        return;
    }
    edit_effect(show, |program| {
        assert!(!program.uses_pixel_context());
        let row = Slot::row(program.rows.colors);
        program.rows.colors += 1;
        let mut code = core::mem::take(&mut program.code).into_vec();
        code.push(Instruction::Move {
            bank: Bank::Color,
            dst: row,
            src: program.result,
        });
        program.code = code.into();
        program.result = row;
    });
}

pub const GROUPED_SOURCE: &str = "operator Times { input source; sample {
    let past = time - 0.1;
    let a = source.at(time); let b = source.at(time);
    let c = source.at(past); let d = source.at(past);
    max(max(a, b), max(c, d))
} }";
pub const ALTERNATING_SOURCE: &str = "operator Times { input source; sample {
    let past = time - 0.1;
    let a = source.at(time); let b = source.at(past);
    let c = source.at(time); let d = source.at(past);
    max(max(a, b), max(c, d))
} }";

pub const OPERATOR_SOURCE: &str = "operator Wave { input source;
    sample { source * (sin(time * 7.0) * 0.5 + 0.5) }
}";

/// Run the query and target blocks with the body, for every strip, and
/// sample inputs without frame caches.
pub fn unstaged(program: &mut BytecodeProgram) {
    program.query_end = 0;
    program.target_end = 0;
    without_frame_caches(program);
}

/// Sample every input directly instead of through a whole-frame cache.
pub fn without_frame_caches(program: &mut BytecodeProgram) {
    program.frame_caches = 0;
    for instruction in &mut program.code {
        if let Instruction::Sample { frame_cache, .. } = instruction {
            *frame_cache = NO_FRAME_CACHE;
        }
    }
}

/// Input samples that read a whole-frame cache.
pub fn cached_samples(program: &BytecodeProgram) -> usize {
    program
        .code
        .iter()
        .filter(|instruction| {
            matches!(
                instruction,
                Instruction::Sample { frame_cache, .. } if *frame_cache != NO_FRAME_CACHE
            )
        })
        .count()
}

// The firmware receives this host-prepared lookup as data.
pub fn gamma_lookup() -> [u8; 256] {
    core::array::from_fn(|value| ((value as f32 / 255.0).powf(2.2) * 255.0).round() as u8)
}

pub fn apply_gamma(show: &mut Workload, lookup: [u8; 256]) {
    show.lookup = Some(lookup);
}

pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

pub fn show(count: usize, invocation: &Invocation) -> Workload {
    layered_show(count, invocation, 1)
}

/// An effect lowered as preparation would lower it on a `count`-pixel fixture.
pub fn sample_fixture(
    invocation: &Invocation,
    count: usize,
    start: SampleTime,
    duration: SampleDuration,
) -> SampleFixture {
    let lowered = invocation
        .instance(ProgramConstants {
            pixel_count: Some(count as i32),
            duration_seconds: Some(donder_runtime_types::sample_duration_seconds_f32(duration)),
        })
        .sample();
    SampleFixture {
        program: (**lowered.program()).clone(),
        params: lowered.params().clone(),
        start,
        duration,
        automation: lowered.automation().to_vec(),
    }
}

// Identical overlapping inputs preserve the max-composited golden output.
pub fn layered_show(count: usize, invocation: &Invocation, layers: usize) -> Workload {
    assert!(layers > 0);
    let effect = sample_fixture(
        invocation,
        count,
        SampleTime::from_ticks(0),
        SampleDuration::from_ticks(8_000_000),
    );
    Workload::samples(count, vec![vec![effect]; layers])
}
