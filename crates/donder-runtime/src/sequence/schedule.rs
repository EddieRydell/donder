//! Mechanical VM-workspace scheduling for an assembled graph.
use crate::dsl::AutomationPlan;
use crate::signal::{PreparedSignalKind, PreparedSignalNode, SignalPlan};
use alloc::vec::Vec;

pub(super) fn finish_plan(
    mut nodes: Vec<PreparedSignalNode<AutomationPlan>>,
    output: Option<usize>,
    target: usize,
) -> SignalPlan<AutomationPlan> {
    let mut depths = Vec::with_capacity(nodes.len());
    let mut vm_workspace_count = 0;
    for node in &mut nodes {
        let depth = match &mut node.kind {
            PreparedSignalKind::Layer { .. } => 0,
            PreparedSignalKind::Mix { inputs } => inputs
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
    SignalPlan {
        output,
        target,
        nodes: nodes.into(),
        vm_workspace_count,
    }
}

#[cfg(test)]
mod tests {
    mod sections;
    mod storage;

    use super::*;
    use crate::dsl::bytecode::{
        Banks, BytecodeProgram, ContextRead, Input, Instruction, SignalPixel, Slot,
    };
    use crate::sequence::PreparedSequence;
    use crate::sequence::tests::program;
    use alloc::boxed::Box;
    use alloc::vec;
    use core::num::NonZeroU32;
    use donder_runtime_types::SampleTime;
    use donder_runtime_types::{
        FixtureGeometry, OutputEncoding, RgbOrder, SequenceTiming, SequenceWindow, TargetScope,
    };
    use donder_runtime_types::{
        OperatorInvocation, OperatorProgram, SampleInvocation, SampleProgram,
    };

    /// Slot counts of floats, ints and colors.
    fn banks(floats: u16, ints: u16, colors: u16) -> Banks {
        Banks {
            floats,
            ints,
            colors,
            ..Banks::default()
        }
    }

    fn sequence(selected: bool, query: SignalPixel<i32>) -> PreparedSequence {
        let sample = SampleProgram::admit(program(
            vec![
                Instruction::Context {
                    dst: Slot::scalar(0),
                    read: ContextRead::Seconds,
                },
                Instruction::Rgb {
                    dst: Slot::row(0),
                    red: Slot::input(Input::PixelFraction),
                    green: Slot::input(Input::PixelX),
                    blue: Slot::scalar(0),
                },
            ],
            1,
            Slot::row(0),
            banks(1, 0, 0),
            banks(0, 0, 1),
        ))
        .unwrap();
        let query_index = match query {
            SignalPixel::Current => 0,
            SignalPixel::Local(index)
            | SignalPixel::Global(index)
            | SignalPixel::Shifted(index, _) => index,
        };
        let operator = OperatorProgram::admit(
            BytecodeProgram {
                frame_caches: 1,
                ..program(
                    vec![
                        Instruction::FloatConst {
                            dst: Slot::scalar(0),
                            bits: 0.3f32.to_bits(),
                        },
                        Instruction::IntConst {
                            dst: Slot::scalar(0),
                            value: query_index,
                        },
                        Instruction::Sample {
                            dst: Slot::row(0),
                            input: 0,
                            seconds: Slot::scalar(0),
                            pixel: query.map(|_| Slot::scalar(0)),
                            frame_cache: 0,
                        },
                    ],
                    2,
                    Slot::row(0),
                    banks(1, 1, 0),
                    banks(0, 0, 1),
                )
            },
            1,
        )
        .unwrap();
        let sample = SampleInvocation::bind(sample, vec![]).unwrap();
        let operator = OperatorInvocation::bind(operator, vec![]).unwrap();
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
            builder.output(Some(outer))
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
