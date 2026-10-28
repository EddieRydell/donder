//! Typed playback workloads: layers of effect fixtures and operator chains on
//! one fixture, prepared by the runtime builder.
use crate::playback::{bind, invoke, row, whole};
use core::num::NonZeroU32;
use donder_language::compiler::{CompiledDefinition, Invocation, ProgramConstants};
use donder_runtime::PreparedShow;
use donder_runtime_types::bytecode::{Bank, BytecodeProgram, Instruction, Slot};
use donder_runtime_types::{
    BoundProgram, InstructionWord, OutputEncoding, Program, RgbOrder, SampleDuration, SampleTime,
    SequenceTiming, TargetScope,
};

/// An effect of a workload: its program and when it plays.
#[derive(Clone)]
pub struct Clip {
    pub program: BoundProgram,
    pub start: SampleTime,
    pub duration: SampleDuration,
}

/// Typed benchmark inputs. The runtime builder owns all graph and storage addresses.
#[derive(Clone)]
pub struct Workload {
    pub count: usize,
    pub layers: Vec<Vec<Clip>>,
    pub operators: Vec<BoundProgram>,
    pub lookup: Option<[u8; 256]>,
}

impl Workload {
    pub fn new(count: usize, layers: Vec<Vec<Clip>>) -> Self {
        Self {
            count,
            layers,
            operators: vec![],
            lookup: None,
        }
    }

    /// The layers on one fixture of an eight-second sequence, then the
    /// operator chain, routed GRB to one port.
    pub fn prepare(self) -> PreparedShow {
        let timing = SequenceTiming::new(
            NonZeroU32::new(120).unwrap(),
            NonZeroU32::new(960).unwrap(),
            NonZeroU32::new(8_000_000).unwrap(),
        );
        PreparedShow::build(timing, |builder| {
            let geometry = row(self.count);
            builder.fixture(0, geometry.clone());
            let target = whole(builder, &geometry, TargetScope::PerFixture);
            let layers: Vec<_> = self
                .layers
                .iter()
                .map(|layer| {
                    let effects: Vec<_> = layer
                        .iter()
                        .map(|clip| {
                            builder.clip(None, &clip.program, clip.start, clip.duration, target)
                        })
                        .collect();
                    builder.layer(true, effects)
                })
                .collect();
            // Operator fixtures measure Layer 0 -> operator chain, including
            // the four-layer Chase/Pulse source used by the Afterglow cases.
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
            builder.route(
                output,
                target,
                OutputEncoding::Rgb(RgbOrder::Grb),
                self.lookup,
            );
            Some(signal)
        })
    }
}

pub const COUNTS: [usize; 4] = [200, 400, 800, 1600];
pub const FRAMES: usize = 32;

// Profiling fixture: varied, overlapping chases and pulses compiled from the
// same editable effect document included in new projects.
pub fn chase_pulse_show(count: usize, layers: usize) -> Workload {
    use donder_runtime_types::Value;
    use donder_runtime_types::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let source = include_str!("../../../examples/starter/effects/standard.donder");
    let chase = crate::playback::definition(source, "Chase");
    let pulse = crate::playback::definition(source, "Pulse");
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
                    color: Color::from_bytes([
                        (47 + index * 31) as u8,
                        (193 + index * 17) as u8,
                        (89 + index * 43) as u8,
                    ]),
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
                    Value::Enum(
                        donder_runtime_types::Identifier::new("AcrossItems".into()).unwrap(),
                    ),
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
        let invocation = invoke(if chasing { &chase } else { &pulse }, &values);
        effects.push(sample_fixture(
            &invocation,
            count,
            SampleTime::from_ticks(index as u32 * 43_000),
            SampleDuration::from_ticks(4_000_000 + index as u32 * 97_000),
        ));
    }
    Workload::new(
        count,
        effects.into_iter().map(|effect| vec![effect]).collect(),
    )
}

/// Run the single operator again after `invert`, sharing its admitted definition.
pub fn insert_invert(show: &mut Workload, invert: &CompiledDefinition) {
    assert_eq!(show.operators.len(), 1);
    let operator = show.operators[0].clone();
    show.operators.extend([operator_program(invert), operator]);
}

/// An operator with its defaults, lowered for the eight-second fixture sequence.
pub fn operator_program(operator: &CompiledDefinition) -> BoundProgram {
    bind(&invoke(operator, &[]).instance(ProgramConstants {
        pixel_count: None,
        duration_seconds: Some(8.0),
    }))
}

/// Use `operator`; without `reuse`, its initialization runs for every strip.
pub fn apply_operator(show: &mut Workload, operator: &CompiledDefinition, reuse: bool) {
    let program = operator_program(operator);
    show.operators = vec![if reuse {
        program
    } else {
        edit(&program, unstaged)
    }];
}

/// `program` with its lowered bytecode edited and readmitted, keeping its
/// parameter values and automation.
pub fn edit(program: &BoundProgram, edit: impl FnOnce(&mut BytecodeProgram)) -> BoundProgram {
    let mut edited = Program::clone(program.program());
    edit(&mut edited.bytecode);
    BoundProgram::bind(
        edited,
        program.params().values().to_vec(),
        program.automation().into(),
    )
    .unwrap()
}

/// Edit the first effect's lowered bytecode.
pub fn edit_effect(show: &mut Workload, change: impl FnOnce(&mut BytecodeProgram)) {
    let clip = &mut show.layers[0][0];
    clip.program = edit(&clip.program, change);
}

/// Without `reuse`, make the first effect's uniform result a row, which
/// disables whole-result reuse without changing colors.
pub fn set_uniform_upstream(show: &mut Workload, reuse: bool) {
    if reuse {
        return;
    }
    edit_effect(show, |program| {
        assert!(!program.uses_pixel_context());
        let row = Slot::row(program.rows.floats);
        program.rows.floats += 3;
        let mut code = core::mem::take(&mut program.code).into_vec();
        let copy = Instruction::Move {
            bank: Bank::Color,
            dst: row,
            src: program.result,
        };
        InstructionWord::encode(&copy, &mut code);
        program.code = code.into();
        program.result = row;
    });
}

pub const OPERATOR_SOURCE: &str = "operator Wave { input source;
    sample { source * (sin(time * 7.0) * 0.5 + 0.5) }
}";

/// Run the query and target blocks with the body, for every strip.
pub fn unstaged(program: &mut BytecodeProgram) {
    program.query_end = 0;
    program.target_end = 0;
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

/// An effect lowered as preparation would lower it on a `count`-pixel fixture.
pub fn sample_fixture(
    invocation: &Invocation,
    count: usize,
    start: SampleTime,
    duration: SampleDuration,
) -> Clip {
    Clip {
        program: bind(&invocation.instance(ProgramConstants {
            pixel_count: Some(count as i32),
            duration_seconds: Some(donder_runtime_types::sample_duration_seconds_f32(duration)),
        })),
        start,
        duration,
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
    Workload::new(count, vec![vec![effect]; layers])
}
