#[allow(dead_code)]
#[path = "../../donder-language/benches/fixtures/mod.rs"]
mod fixtures;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

use donder_runtime::BuiltinOperator;
use donder_runtime::dsl::{BoundParams, Value};
use donder_runtime::signal::{
    PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
};

#[test]
fn native_temporal_frames_match_scalar_sampling_through_nested_operators() {
    let (effect, _) = fixtures::uniform_resources();
    let bases = [
        workload::chase_pulse_show(200, 4, effect.bytecode.clone()),
        mark_workload::mark_show(200, true, 0.0, effect.bytecode.clone()),
        mark_workload::mark_show(200, true, 1.0, effect.bytecode.clone()),
        mark_workload::mark_show(200, false, 0.0, effect.bytecode),
    ];
    for (base, builtin) in bases.iter().flat_map(|base| {
        [BuiltinOperator::Delay, BuiltinOperator::Echo].map(|builtin| (base, builtin))
    }) {
        let mut graph = base.signals.clone();
        let declaration = donder_language::operator::builtin_operator_definition(builtin);
        let overrides = declaration
            .params
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
        let temporal_params = BoundParams::bind_pairs(&declaration.params, &overrides).unwrap();
        let node = |builtin, params, inputs: Vec<usize>| PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    implementation: PreparedOperator::Native(builtin),
                    params,
                    automation_slot: 0,
                },
                inputs: inputs.into(),
                automation: Box::new([]),
                vm_slot: 0,
            },
        };
        graph.plan.nodes = vec![
            PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            },
            node(BuiltinOperator::Invert, BoundParams::default(), vec![0]),
            node(builtin, temporal_params.clone(), vec![1]),
            node(
                BuiltinOperator::Multiply,
                BoundParams::default(),
                vec![2, 0],
            ),
            node(builtin, temporal_params, vec![3]),
        ]
        .into();
        graph.plan.output_index = 4;
        graph.plan.frame_nodes = vec![4].into();
        graph.plan.frame_slots = vec![u16::MAX, u16::MAX, u16::MAX, u16::MAX, 0].into();
        graph.plan.frame_buffer_count = 1;

        let mut scalar = base.signals.clone();
        let mut identity = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
            .unwrap()
            .remove(0)
            .bytecode;
        for instruction in &mut identity.instructions {
            if let donder_runtime::dsl::bytecode::Instruction::SignalSample {
                frame_cache, ..
            } = instruction
            {
                *frame_cache = u32::MAX;
            }
        }
        scalar.programs = vec![identity].into();
        let mut nodes = graph.plan.nodes.to_vec();
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    implementation: PreparedOperator::Dsl(0),
                    params: BoundParams::default(),
                    automation_slot: 0,
                },
                inputs: vec![4].into(),
                automation: Box::new([]),
                vm_slot: 0,
            },
        });
        scalar.plan.nodes = nodes.into();
        scalar.plan.output_index = 5;
        scalar.plan.vm_workspace_count = 1;
        scalar.plan.frame_nodes = vec![5].into();
        scalar.plan.frame_slots = vec![u16::MAX, u16::MAX, u16::MAX, u16::MAX, u16::MAX, 0].into();
        scalar.plan.frame_buffer_count = 1;
        let mut workspace = graph.workspace();
        let mut scalar_workspace = scalar.workspace();
        for frame in [0, 1, 4, 31, 12, 4, 0, 31] {
            let actual = graph
                .evaluate(workload::time(frame), &mut workspace)
                .unwrap();
            let expected = scalar
                .evaluate(workload::time(frame), &mut scalar_workspace)
                .unwrap();
            assert_eq!(actual, expected, "{builtin:?} frame {frame}");
        }
    }
}
