use dawn_runtime::patch::{PixelEncoding, PreparedPatch, PreparedPixelRoute};
extern crate alloc;

use alloc::vec;
use dawn_runtime::dsl::{BoundParams, RunContext, bytecode::BytecodeProgram};
use dawn_runtime::sequence::PreparedSequence;
use dawn_runtime::signal::*;
use dawn_runtime::values::{SampleDuration, SampleTime};

pub const COUNTS: [usize; 4] = [200, 400, 800, 1600];
pub const FRAMES: usize = 32;
pub const GAMMA_CASE: usize = 5; // PixelRamp in the shared fixture list.
pub const OPERATOR_DEPTHS: [usize; 3] = [2, 4, 8];
#[allow(dead_code)] // PC profiler and host golden generation only.
pub const CHASE_PULSE_CASES: [(&str, usize); 3] =
    [("ChasePulse1", 1), ("ChasePulse4", 4), ("ChasePulse16", 16)];
#[allow(dead_code)]
pub const MARK_CASES: [(&str, bool, f32); 3] = [
    ("MarkPulse200", true, 0.0),
    ("MarkChase200", false, 0.0),
    ("MarkPulseEdge200", true, 1.0),
];

// Profiling fixture only: varied, overlapping native chases and pulses. This
// contains no evaluator shortcuts; desktop and device use the same setup.
#[allow(dead_code)] // Normal timing binary uses a different workload subset.
pub fn chase_pulse_show(count: usize, layers: usize, program: BytecodeProgram) -> PreparedSequence {
    use dawn_runtime::dsl::{Identifier, ParamDecl, Type, Value};
    use dawn_runtime::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
    let mut show = layered_show(count, program, BoundParams::default(), layers);
    show.signals.programs = vec![].into();
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
    for (index, effect) in show.signals.effects.iter_mut().enumerate() {
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
        let builtin = if index % 2 == 0 {
            dawn_runtime::BuiltinEffect::Chase
        } else {
            dawn_runtime::BuiltinEffect::Pulse
        };
        let values = if builtin == dawn_runtime::BuiltinEffect::Chase {
            vec![
                (Type::Gradient, gradient),
                (
                    Type::Enum(vec![Identifier::new("across_items".into()).unwrap()]),
                    Value::Enum(Identifier::new("across_items".into()).unwrap()),
                ),
                (Type::Float, Value::Float(12.0 + index as f32)),
                (Type::Int, Value::Int(3 + index as i32 % 4)),
                (Type::Curve, position.clone()),
                (Type::Bool, Value::Bool(index % 4 == 0)),
                (Type::Bool, Value::Bool(true)),
                (Type::Bool, Value::Bool(true)),
                (Type::Curve, shape.clone()),
            ]
        } else {
            vec![(Type::Gradient, gradient), (Type::Curve, shape.clone())]
        };
        let declarations = values
            .into_iter()
            .enumerate()
            .map(|(slot, (ty, value))| ParamDecl {
                fixed: false,
                name: Identifier::new(alloc::format!("p{slot}")).unwrap(),
                ty,
                default: Some(value),
            })
            .collect::<vec::Vec<_>>();
        let params = BoundParams::bind_pairs(&declarations, &[]).unwrap();
        effect.implementation = PreparedEffectImplementation::Native {
            sample: dawn_runtime::native_effect::prepare_sample(builtin, &params).unwrap(),
            params: None,
        };
        effect.start_time = SampleTime::from_ticks(index as u32 * 43_000);
        effect.duration = SampleDuration::from_ticks(4_000_000 + index as u32 * 97_000);
    }
    show
}

// Extend the single-operator fixture into a chain, sharing its bytecode.
pub fn nest_operator(show: &mut PreparedSequence, depth: usize) {
    assert!(depth > 0);
    let graph = &mut show.signals.plan;
    let mut nodes = core::mem::take(&mut graph.nodes).into_vec();
    assert_eq!(nodes.len(), 2);
    for input in 1..depth {
        let mut node = nodes[1].clone();
        let PreparedSignalKind::Operator {
            inputs, vm_slot, ..
        } = &mut node.kind
        else {
            panic!("nested fixture requires a DSL operator");
        };
        inputs[0] = input;
        *vm_slot = input as u16;
        nodes.push(node);
    }
    graph.nodes = nodes.into();
    graph.output_index = depth;
    graph.vm_workspace_count = depth;
    graph.frame_nodes = vec![depth].into();
    graph.frame_slots = vec![0; depth + 1].into();
}

#[allow(dead_code)] // Compiled on the host, not the device.
pub const IDENTITY_SOURCE: &str =
    "operator Identity { input Signal source; color sample() { return source.at(seconds()); } }";

pub fn insert_native_invert(show: &mut PreparedSequence) {
    nest_operator(show, 2);
    let graph = &mut show.signals.plan;
    let mut nodes = core::mem::take(&mut graph.nodes).into_vec();
    nodes.insert(
        2,
        PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot: 0,
                    implementation: PreparedOperator::Native(dawn_runtime::BuiltinOperator::Invert),
                    params: Default::default(),
                },
                inputs: vec![1].into(),
                automation: vec![].into(),
                vm_slot: 0,
            },
        },
    );
    let PreparedSignalKind::Operator { inputs, .. } = &mut nodes[3].kind else {
        unreachable!()
    };
    inputs[0] = 2;
    graph.nodes = nodes.into();
    graph.output_index = 3;
    graph.frame_nodes = vec![3].into();
    graph.frame_slots = vec![0; 4].into();
}

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

pub fn apply_native_automation(show: &mut PreparedSequence, empty: bool) {
    use dawn_runtime::dsl::{Identifier, ParamDecl, Type, Value};
    use dawn_runtime::values::{Color, Curve, CurvePoint, Gradient, GradientStop};
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
            "ramp",
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
        ("shape", Type::Curve, Value::Curve(curve.clone().into())),
    ];
    let declarations = values.map(|(name, ty, value)| ParamDecl {
        fixed: false,
        name: Identifier::new(name.into()).unwrap(),
        ty,
        default: Some(value),
    });
    let params = BoundParams::bind_pairs(&declarations, &[]).unwrap();
    show.signals.effects[0].implementation = PreparedEffectImplementation::Native {
        sample: dawn_runtime::native_effect::prepare_sample(
            dawn_runtime::BuiltinEffect::Pulse,
            &params,
        )
        .unwrap(),
        params: Some((dawn_runtime::BuiltinEffect::Pulse, params)),
    };
    show.signals.effects[0].automation = Some(alloc::boxed::Box::new(PreparedEffectAutomation {
        workspace_slot: 0,
        bindings: vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            curve: curve.into(),
            mapping: dawn_runtime::automation::AutomationMapping::Curve {
                min: if empty { 0.5 } else { 0.0 },
                max: 1.0,
            },
            param_index: 1,
        }]
        .into(),
    }));
}

#[allow(dead_code)] // Compiled on the host; firmware receives only bytecode.
pub const OPERATOR_SOURCE: &str = "operator Wave { input Signal source;
    color sample() { return source.at(seconds()) * (sin(seconds() * 7.0) * 0.5 + 0.5); }
}";

pub fn apply_operator(show: &mut PreparedSequence, mut program: BytecodeProgram, reuse: bool) {
    if !reuse {
        program.pixel_entry = 0;
    }
    let sequence = &mut show.signals;
    let mut programs = core::mem::take(&mut sequence.programs).into_vec();
    let program_index = programs.len() as u32;
    programs.push(program);
    sequence.programs = programs.into();
    sequence.plan = SignalPlan {
        output_index: 1,
        target: 0,
        vm_workspace_count: 1,
        nodes: vec![
            PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            },
            PreparedSignalNode {
                kind: PreparedSignalKind::Operator {
                    operator: PreparedOperatorNode {
                        automation_slot: 0,
                        implementation: PreparedOperator::Dsl(program_index),
                        params: BoundParams::default(),
                    },
                    inputs: vec![0].into(),
                    automation: vec![].into(),
                    vm_slot: 0,
                },
            },
        ]
        .into(),
        frame_nodes: vec![1].into(),
        frame_slots: vec![0, 0].into(),
        frame_buffer_count: 1,
    };
}

// The firmware receives this host-prepared lookup as data.
#[cfg(not(target_arch = "xtensa"))]
pub fn gamma_lookup() -> [u8; 256] {
    core::array::from_fn(|value| ((value as f32 / 255.0).powf(2.2) * 255.0).round() as u8)
}

pub fn apply_gamma(show: &mut PreparedSequence, lookup: [u8; 256]) {
    show.patch.lookups = vec![lookup].into_boxed_slice();
    for route in &mut show.patch.routes {
        route.lookup = Some(0);
    }
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
        pixel_fraction: pixel as f32 / (count - 1) as f32,
    }
}

pub fn checksum(bytes: &[u8]) -> u32 {
    bytes.iter().fold(0x811c9dc5_u32, |hash, byte| {
        (hash ^ u32::from(*byte)).wrapping_mul(0x01000193)
    })
}

// A deliberately small prepared workload: one DSL effect on one RGB fixture,
// one layer/output signal graph, and the production RGB-to-GRB patch path.
// Construction is measured separately; the runtime evaluator is not duplicated.
pub fn show(count: usize, program: BytecodeProgram, params: BoundParams) -> PreparedSequence {
    PreparedSequence {
        workspace_key: 1,
        signals: PreparedSignalGraph {
            parameter_environments: vec![].into_boxed_slice(),
            workspace_key: 1,
            frame_rate: 120,
            frame_count: 960,
            duration: SampleDuration::from_ticks(8_000_000),
            fixtures: vec![PreparedFixture {
                id: 0,
                pixel_count: count,
            }]
            .into_boxed_slice(),
            fixture_pixel_offsets: vec![0].into_boxed_slice(),
            pixel_count: count,
            effects: vec![PreparedEffect {
                start_time: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                target: 0,
                implementation: PreparedEffectImplementation::Dsl {
                    program: 0,
                    bound_params: params,
                },
                automation: None,
            }]
            .into_boxed_slice(),
            programs: vec![program].into_boxed_slice(),
            targets: vec![PreparedTarget {
                pixels: 0..count as u32,
                sample_count: 0,
            }]
            .into_boxed_slice(),
            target_pixels: (0..count)
                .map(|pixel| PreparedPixel {
                    fixture_index: 0,
                    fixture_pixel_index: pixel as u16,
                    pixel_index: pixel as u32,
                    pixel_count: count as u32,
                    pixel_fraction: pixel as f32 / (count - 1) as f32,
                })
                .collect(),
            effects_by_layer: vec![vec![0].into_boxed_slice()].into_boxed_slice(),
            layers: vec![PreparedLayer { enabled: true }].into_boxed_slice(),
            plan: SignalPlan {
                output_index: 1,
                target: 0,
                nodes: vec![
                    PreparedSignalNode {
                        kind: PreparedSignalKind::Layer { layer_index: 0 },
                    },
                    PreparedSignalNode {
                        kind: PreparedSignalKind::Output {
                            inputs: vec![0].into_boxed_slice(),
                        },
                    },
                ]
                .into_boxed_slice(),
                vm_workspace_count: 0,
                frame_nodes: vec![0, 1].into_boxed_slice(),
                frame_slots: vec![0, 1].into_boxed_slice(),
                frame_buffer_count: 2,
            },
        },
        patch: PreparedPatch {
            routes: vec![PreparedPixelRoute {
                pixels: 0..count as u32,
                frame: 0,
                start_slot: 0,
                encoding: PixelEncoding::Rgb { order: [1, 0, 2] },
                lookup: None,
            }]
            .into_boxed_slice(),
            lookups: vec![].into_boxed_slice(),
        },
        output_widths: vec![count as u32 * 3].into_boxed_slice(),
    }
}

// Identical overlapping inputs have the same max-composited golden output.
// This isolates coverage/layer cost without changing effect math or geometry.
pub fn layered_show(
    count: usize,
    program: BytecodeProgram,
    params: BoundParams,
    layers: usize,
) -> PreparedSequence {
    assert!(layers > 0);
    let mut show = show(count, program, params);
    let sequence = &mut show.signals;
    sequence.effects = vec![sequence.effects[0].clone(); layers].into();
    sequence.layers = vec![PreparedLayer { enabled: true }; layers].into();
    sequence.effects_by_layer = (0..layers).map(|i| vec![i].into()).collect();
    let graph = &mut sequence.plan;
    graph.nodes = (0..layers)
        .map(|i| PreparedSignalNode {
            kind: PreparedSignalKind::Layer { layer_index: i },
        })
        .chain(core::iter::once(PreparedSignalNode {
            kind: PreparedSignalKind::Output {
                inputs: (0..layers).collect(),
            },
        }))
        .collect();
    graph.output_index = layers;
    // Match elaboration's single-input alias; multiple inputs need composition.
    graph.frame_nodes = (0..if layers == 1 { 1 } else { layers + 1 }).collect();
    graph.frame_slots = (0..=layers)
        .map(|i| if layers == 1 { 0 } else { i as u16 })
        .collect();
    graph.frame_buffer_count = if layers == 1 { 1 } else { (layers + 1) as u16 };
    show
}
