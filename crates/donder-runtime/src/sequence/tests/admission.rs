//! Raw graph admission and representation-specific execution checks.
use super::*;
use crate::dsl::bytecode::{ColorSlot, Instruction, SignalPixel};
use crate::signal::PreparedOperatorNode;
use crate::wire::LoadError;

fn rejects(data: SequenceData) {
    assert!(matches!(
        PreparedSequence::admit_data(data, None),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_invalid_bytecode_return_and_sample_addresses() {
    let sequence = timed_sequence();
    let mut data = sequence.archive_data();
    data.signals.programs[0].instructions[0] = Instruction::ReturnColor(ColorSlot(u32::MAX));
    rejects(data);

    let mut data = sequence.archive_data();
    data.signals.programs[0].instructions = Box::new([]);
    rejects(data);

    let mut data = sequence.archive_data();
    data.signals.targets[0].sample_count = 1;
    data.signals.target_pixels[0].pixel_index = 1;
    data.signals.target_pixels[0].pixel_count = 2;
    rejects(data);

    let mut data = sequence.archive_data();
    data.signals.plan.output_index = usize::MAX;
    rejects(data);
}

#[test]
fn admission_rejects_missing_frame_input_and_overwritten_output_buffer() {
    let sequence = timed_sequence();
    let mut data = sequence.archive_data();
    data.signals.plan.frame_nodes = vec![1].into();
    rejects(data);

    let mut data = sequence.archive_data();
    data.signals.plan.vm_workspace_count = usize::MAX;
    rejects(data);

    let mut data = sequence.archive_data();
    let plan = &mut data.signals.plan;
    let extra = plan.nodes.len();
    let output_slot = plan.frame_slots[plan.output_index];
    let mut nodes = plan.nodes.to_vec();
    nodes.push(PreparedSignalNode {
        kind: PreparedSignalKind::Layer { layer_index: 0 },
    });
    plan.nodes = nodes.into();
    let mut slots = plan.frame_slots.to_vec();
    slots.push(output_slot);
    plan.frame_slots = slots.into();
    let mut scheduled = plan.frame_nodes.to_vec();
    scheduled.push(extra);
    plan.frame_nodes = scheduled.into();
    rejects(data);
}

#[test]
fn admission_rejects_reused_nested_operator_vm_slot() {
    let mut data = queried_sequence(SignalPixel::Current).archive_data();
    let first = data.signals.plan.nodes.len();
    let second = first + 1;
    let first_slot = data.signals.plan.vm_workspace_count;
    let program = match &data.signals.plan.nodes[1].kind {
        PreparedSignalKind::Operator { operator, .. } => operator.program,
        _ => unreachable!(),
    };
    let node = |input, slot| -> PreparedSignalNode {
        PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot: 0,
                    program,
                    params: crate::dsl::BoundParams::default(),
                },
                inputs: vec![input].into(),
                automation: Box::new([]),
                vm_slot: slot,
            },
        }
    };
    let mut nodes = data.signals.plan.nodes.into_vec();
    nodes.push(node(data.signals.plan.output_index, first_slot));
    nodes.push(node(first, first_slot + 1));
    data.signals.plan.nodes = nodes.into();
    data.signals.plan.output_index = second;
    data.signals.plan.vm_workspace_count += 2;
    data.signals.plan.frame_nodes = vec![second].into();
    data.signals.plan.frame_slots = vec![usize::MAX; second + 1].into();
    data.signals.plan.frame_slots[second] = 0;
    data.signals.plan.frame_buffer_count = 1;
    let sequence = PreparedSequence::admit_data(data, None).unwrap();
    let mut data = sequence.archive_data();
    let PreparedSignalKind::Operator { vm_slot, .. } = &mut data.signals.plan.nodes[second].kind
    else {
        unreachable!()
    };
    *vm_slot = first_slot;
    rejects(data);
}

#[test]
fn identical_target_routing_matches_address_search() {
    use crate::dsl::bytecode::{ContextRead, FloatSlot, NumberSlot};
    let mut data = queried_sequence(SignalPixel::Current).archive_data();
    data.signals.effects = vec![data.signals.effects[0].clone()].into();
    data.signals.effects[0].target = 0;
    data.signals.effects_by_layer[0] = vec![0].into();
    let program = &mut data.signals.programs[0];
    program.layout.floats = 3;
    program.uses_pixel_context = true;
    program.instructions = vec![
        Instruction::ContextRead {
            dst: NumberSlot::Float(FloatSlot(0)),
            read: ContextRead::PixelFraction,
        },
        Instruction::ContextRead {
            dst: NumberSlot::Float(FloatSlot(1)),
            read: ContextRead::Progress,
        },
        Instruction::LoadFloatConst {
            dst: FloatSlot(2),
            bits: 0.25f32.to_bits(),
        },
        Instruction::Rgb {
            dst: ColorSlot(0),
            red: FloatSlot(0),
            green: FloatSlot(1),
            blue: FloatSlot(2),
        },
        Instruction::ReturnColor(ColorSlot(0)),
    ]
    .into();
    data.signals.programs[2].instructions[0] = Instruction::ContextRead {
        dst: NumberSlot::Float(FloatSlot(0)),
        read: ContextRead::Seconds,
    };
    let direct = PreparedSequence::admit_data(data, None).unwrap();
    let mut data = direct.archive_data();
    // A different ID for the same target forces the general address-search path.
    data.signals.targets = vec![data.signals.targets[0].clone(); 2].into();
    data.signals.effects[0].target = 1;
    let searched = PreparedSequence::admit_data(data, None).unwrap();
    let mut direct = direct.into_playback();
    let mut searched = searched.into_playback();
    for ticks in [300_000, 700_000, 400_000, 300_000, 0] {
        let time = SampleTime::from_ticks(ticks);
        let actual = direct.evaluate(time);
        let expected = searched.evaluate(time);
        assert_eq!(actual.colors(), expected.colors());
        assert!(actual.outputs().eq(expected.outputs()));
    }
}
