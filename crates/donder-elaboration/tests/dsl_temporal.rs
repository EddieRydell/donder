#[allow(dead_code)]
#[path = "../../donder-language/benches/fixtures/mod.rs"]
mod fixtures;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

use donder_runtime::dsl::{BoundParams, Value};
use donder_runtime::signal::{
    PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
};

#[test]
fn dsl_effect_temporal_frames_match_scalar_sampling_through_nested_operators() {
    let bases = [
        workload::chase_pulse_show(200, 4),
        mark_workload::mark_show(200, true),
        mark_workload::mark_show(200, false),
    ];
    let operators = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap();
    let compiled = |name| {
        operators
            .iter()
            .find(|operator| operator.name().as_str() == name)
            .unwrap()
    };
    for (base, name) in bases
        .iter()
        .flat_map(|base| ["Delay", "Echo"].map(|name| (base, name)))
    {
        let mut graph = base.signals.clone();
        let declaration = compiled(name);
        let overrides = declaration
            .params()
            .iter()
            .filter_map(|param| {
                let value = match param.name.as_str() {
                    "seconds" => Value::Float(0.025),
                    "repeats" => Value::Int(3),
                    "decay" => Value::Float(0.6),
                    _ => return None,
                };
                Some((param.name.clone(), value))
            })
            .collect::<Vec<_>>();
        let temporal_params = BoundParams::bind_pairs(declaration.params(), &overrides).unwrap();
        let invert = graph.programs.len() as u32;
        let temporal = invert + 1;
        let multiply = invert + 2;
        let mut programs = graph.programs.to_vec();
        programs.extend([
            compiled("Invert").bytecode.clone(),
            declaration.bytecode.clone(),
            compiled("Multiply").bytecode.clone(),
        ]);
        graph.programs = programs.into();
        assert!(graph.programs.iter().any(|program| program.instructions.iter().any(
            |instruction| matches!(instruction, donder_runtime::dsl::bytecode::Instruction::SignalSample { frame_cache, .. } if *frame_cache != u32::MAX)
        )));
        let node = |program, params, inputs: Vec<usize>, vm_slot| PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    implementation: PreparedOperator::Dsl(program),
                    params,
                    automation_slot: 0,
                },
                inputs: inputs.into(),
                automation: Box::new([]),
                vm_slot,
            },
        };
        graph.plan.nodes = vec![
            PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            },
            node(invert, BoundParams::default(), vec![0], 0),
            node(temporal, temporal_params.clone(), vec![1], 1),
            node(multiply, BoundParams::default(), vec![2, 0], 2),
            node(temporal, temporal_params, vec![3], 3),
        ]
        .into();
        graph.plan.output_index = 4;
        graph.plan.frame_nodes = vec![4].into();
        graph.plan.frame_slots = vec![u16::MAX, u16::MAX, u16::MAX, u16::MAX, 0].into();
        graph.plan.frame_buffer_count = 1;
        graph.plan.vm_workspace_count = 4;

        let mut scalar = graph.clone();
        for program in &mut scalar.programs {
            for instruction in &mut program.instructions {
                if let donder_runtime::dsl::bytecode::Instruction::SignalSample {
                    frame_cache,
                    ..
                } = instruction
                {
                    *frame_cache = u32::MAX;
                }
            }
        }
        let mut workspace = graph.workspace();
        let mut scalar_workspace = scalar.workspace();
        for frame in [0, 1, 4, 31, 12, 4, 0, 31] {
            let actual = graph
                .evaluate(workload::time(frame), &mut workspace)
                .unwrap();
            let expected = scalar
                .evaluate(workload::time(frame), &mut scalar_workspace)
                .unwrap();
            assert_eq!(actual, expected, "{name} frame {frame}");
        }
    }
}
