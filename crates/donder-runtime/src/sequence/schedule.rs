//! Mechanical frame-buffer and VM-workspace scheduling for an assembled graph.
use crate::dsl::AutomationPlan;
use crate::signal::{PreparedSignalKind, PreparedSignalNode, SignalPlan};
use alloc::{vec, vec::Vec};

pub(super) fn finish_plan(
    mut nodes: Vec<PreparedSignalNode<AutomationPlan>>,
    output_index: usize,
    target: usize,
) -> SignalPlan<AutomationPlan> {
    let mut depths = Vec::with_capacity(nodes.len());
    let mut vm_workspace_count = 0;
    for node in &mut nodes {
        let depth = match &mut node.kind {
            PreparedSignalKind::Layer { .. } => 0,
            PreparedSignalKind::Output { inputs } => inputs
                .iter()
                .fold(0, |depth, &input| depth.max(depths[input])),
            PreparedSignalKind::Operator {
                inputs, vm_slot, ..
            } => {
                *vm_slot = inputs
                    .iter()
                    .fold(0, |depth, &input| depth.max(depths[input]));
                *vm_slot + 1
            }
        };
        vm_workspace_count = vm_workspace_count.max(depth);
        depths.push(depth);
    }
    let mut required = vec![false; nodes.len()];
    required[output_index] = true;
    for index in (0..nodes.len()).rev() {
        if required[index] {
            for &input in frame_inputs(&nodes[index]) {
                required[input] = true;
            }
        }
    }
    let mut consumers = vec![0usize; nodes.len()];
    for (index, node) in nodes.iter().enumerate() {
        if required[index] {
            for &input in frame_inputs(node) {
                consumers[input] += 1;
            }
        }
    }
    let mut frame_nodes = Vec::new();
    let mut frame_slots = vec![u32::MAX as usize; nodes.len()];
    let mut available = Vec::new();
    let mut frame_buffer_count = 0;
    for (index, node) in nodes.iter().enumerate() {
        if !required[index] {
            continue;
        }
        if index == output_index
            && let PreparedSignalKind::Output { inputs } = &node.kind
            && let [input] = inputs.as_ref()
        {
            frame_slots[index] = frame_slots[*input];
            continue;
        }
        frame_slots[index] = match available.pop() {
            Some(slot) => slot,
            None => {
                let slot = frame_buffer_count;
                frame_buffer_count += 1;
                slot
            }
        };
        frame_nodes.push(index);
        for &input in frame_inputs(node) {
            consumers[input] -= 1;
            if consumers[input] == 0 {
                available.push(frame_slots[input]);
            }
        }
    }
    SignalPlan {
        output_index,
        target,
        nodes: nodes.into(),
        vm_workspace_count,
        frame_nodes: frame_nodes.into(),
        frame_slots: frame_slots.into(),
        frame_buffer_count,
    }
}

fn frame_inputs(node: &PreparedSignalNode<AutomationPlan>) -> &[usize] {
    match &node.kind {
        PreparedSignalKind::Output { inputs } => inputs,
        PreparedSignalKind::Layer { .. } | PreparedSignalKind::Operator { .. } => &[],
    }
}

#[cfg(test)]
mod tests {
    mod sections;
    mod storage;

    use super::*;
    use crate::dsl::bytecode::{
        BytecodeProgram, ColorSlot, ContextRead, FloatSlot, Instruction, IntSlot, NumberSlot,
        SignalPixel, SlotLayout,
    };
    use crate::sequence::PreparedSequence;
    use crate::values::SampleTime;
    use alloc::boxed::Box;
    use core::num::NonZeroU32;
    use donder_language::dsl::{
        OperatorDefinition, OperatorProgram, SampleDefinition, SampleProgram,
    };
    use donder_language::execution::{
        FixtureGeometry, OutputEncoding, RgbOrder, SequenceTiming, SequenceWindow, TargetScope,
    };

    fn program(
        instructions: Vec<Instruction>,
        layout: SlotLayout,
        pixel_entry: u32,
    ) -> BytecodeProgram {
        BytecodeProgram {
            instructions: instructions.into(),
            array_constants: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            curves: Box::new([]),
            gradients: Box::new([]),
            value_operands: Box::new([]),
            array_types: Box::new([]),
            layout,
            uses_pixel_context: true,
            pixel_entry,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        }
    }

    fn sequence(selected: bool, query: SignalPixel<i32>) -> PreparedSequence {
        let sample = SampleProgram::admit(
            program(
                vec![
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(0)),
                        read: ContextRead::Seconds,
                    },
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(1)),
                        read: ContextRead::PixelFraction,
                    },
                    Instruction::ContextRead {
                        dst: NumberSlot::Float(FloatSlot(2)),
                        read: ContextRead::PixelX,
                    },
                    Instruction::Rgb {
                        dst: ColorSlot(0),
                        red: FloatSlot(1),
                        green: FloatSlot(2),
                        blue: FloatSlot(0),
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 3,
                    colors: 1,
                    ..SlotLayout::default()
                },
                1,
            ),
            Box::new([]),
        )
        .unwrap();
        let query_index = match query {
            SignalPixel::Current => 0,
            SignalPixel::Local(index) | SignalPixel::Global(index) => index,
        };
        let operator = OperatorProgram::admit(
            program(
                vec![
                    Instruction::LoadFloatConst {
                        dst: FloatSlot(0),
                        bits: 0.3f32.to_bits(),
                    },
                    Instruction::LoadIntConst {
                        dst: IntSlot(0),
                        value: query_index,
                    },
                    Instruction::SignalSample {
                        capability: (),
                        dst: ColorSlot(0),
                        input: 0,
                        seconds: FloatSlot(0),
                        pixel: query.map(|_| IntSlot(0)),
                        frame_cache: 0,
                    },
                    Instruction::ReturnColor(ColorSlot(0)),
                ],
                SlotLayout {
                    floats: 1,
                    ints: 1,
                    colors: 1,
                    ..SlotLayout::default()
                },
                2,
            ),
            1,
            Box::new([]),
        )
        .unwrap();
        let sample = SampleDefinition::new(sample).bind(vec![]).unwrap();
        let operator = OperatorDefinition::new(operator).bind(vec![]).unwrap();
        let timing = SequenceTiming::admit(
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(60).unwrap(),
            NonZeroU32::new(1_000_000).unwrap(),
            vec![SequenceWindow {
                start: SampleTime::from_ticks(0),
                duration: NonZeroU32::new(800_000).unwrap(),
            }]
            .into(),
        )
        .unwrap();
        PreparedSequence::build(timing, |builder| {
            // Retention is supplied by preparation. Exercise playback of the
            // already selected cells, not the host's dependency analysis.
            let a = FixtureGeometry::admit((0..8).map(|cell| [cell as f32 / 12.0, 0.0]).collect())
                .unwrap();
            let b = FixtureGeometry::admit((8..12).map(|cell| [cell as f32 / 12.0, 0.0]).collect())
                .unwrap();
            let a = builder.fixture(
                10,
                if selected && matches!(query, SignalPixel::Current) {
                    a.select(|cell| matches!(cell, 2 | 6))
                } else {
                    a
                },
            );
            let b = builder.fixture(
                20,
                if selected && !matches!(query, SignalPixel::Global(_)) {
                    b.select(|_| false)
                } else {
                    b
                },
            );
            let target = builder.target([b, a], TargetScope::WholeTarget);
            let window = builder.windows().next().unwrap();
            let effect = builder.sample(&sample, window, target);
            builder.clip(7, effect);
            if !selected {
                builder.layer(false, [effect]);
            }
            let layer = builder.layer(true, [effect]);
            let inner = builder.operator(&operator, |_| layer);
            let outer = builder.operator(&operator, |_| inner);
            let output = builder.port(0, 1);
            builder.padding(output, 2);
            let ranges = if selected && matches!(query, SignalPixel::Current) {
                [0..1, 1..2]
            } else {
                [2..3, 6..7]
            };
            for range in ranges {
                let span = builder.target_slice(target, range);
                builder.route(output, span, OutputEncoding::Rgb(RgbOrder::Rgb), None);
            }
            builder.padding(output, 1);
            builder.output([outer])
        })
    }

    #[test]
    fn selected_spans_match_full_playback_across_query_domains_and_seeks() {
        for (query, count) in [
            (SignalPixel::Current, 2),
            (SignalPixel::Local(6), 8),
            (SignalPixel::Global(9), 12),
        ] {
            let full = sequence(false, query);
            let compacted = sequence(true, query);
            assert_eq!(full.pixel_count(), 12);
            assert_eq!(compacted.pixel_count(), count);
            // Firmware memory: operators request their inputs at the query
            // time, so neither plan eagerly renders upstream layers, and the
            // terminal output aliases its input's frame buffer.
            assert_eq!(full.archive_data().signals.plan.frame_buffer_count, 1);
            assert_eq!(compacted.archive_data().signals.plan.frame_buffer_count, 1);
            let bytes = crate::archive::encode_sequence(&compacted).unwrap();
            let decoded =
                crate::archive::decode_sequence(&bytes, crate::archive::LoadLimits::default())
                    .unwrap();
            let mut full = full.into_playback();
            let mut compacted = compacted.into_playback();
            let mut decoded = decoded.into_playback();
            for ticks in [900_000, 0, 500_000, 250_000, 1_000_000] {
                let time = SampleTime::from_ticks(ticks);
                let expected = full.evaluate(time);
                let expected = expected.outputs().next().unwrap().bytes;
                assert_eq!(
                    compacted.evaluate(time).outputs().next().unwrap().bytes,
                    expected
                );
                assert_eq!(
                    decoded.evaluate(time).outputs().next().unwrap().bytes,
                    expected
                );
            }
        }
    }
}
