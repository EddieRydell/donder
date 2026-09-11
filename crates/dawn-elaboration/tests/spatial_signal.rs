#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

use dawn_language::dsl::{compile_effects, compile_operators};
use dawn_runtime::dsl::BoundParams;
use dawn_runtime::dsl::bytecode::Instruction;
use dawn_runtime::signal::{
    PreparedFixture, PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
    PreparedSignalNode, PreparedTarget,
};
use dawn_runtime::values::{Color, SampleTime};

#[test]
fn spatial_queries_match_explicit_source_pixels_with_and_without_frame_caches() {
    let effect = compile_effects(
        "effect Positions { color sample() {
        return rgb(pixel_index() / 8.0, seconds() / 8.0, 0.0);
    } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let mut base = workload::show(8, effect.bytecode, BoundParams::default()).signals;
    base.fixtures = vec![
        PreparedFixture {
            id: 0,
            pixel_count: 4,
        },
        PreparedFixture {
            id: 1,
            pixel_count: 4,
        },
    ]
    .into();
    base.fixture_pixel_offsets = vec![0, 4].into();
    let mut pixels = base.target_pixels.to_vec();
    for (index, pixel) in pixels.iter_mut().enumerate() {
        pixel.fixture_index = (index / 4) as u16;
        pixel.fixture_pixel_index = (index % 4) as u16;
    }
    let effect_pixels = pixels.clone();
    for (index, pixel) in pixels.iter_mut().enumerate() {
        pixel.pixel_index = (index % 4) as u32;
        pixel.pixel_count = 4;
        pixel.pixel_fraction = (index % 4) as f32 / 3.0;
    }
    pixels.extend(effect_pixels);
    base.target_pixels = pixels.into();
    base.targets = vec![
        PreparedTarget {
            pixels: 0..8,
            sample_count: 0,
        },
        PreparedTarget {
            pixels: 8..16,
            sample_count: 0,
        },
    ]
    .into();
    base.effects[0].target = 1;

    for (query, indices) in [
        (
            "source.at(seconds(), pixel_count() - 1 - pixel_index())",
            vec![
                Some(3),
                Some(2),
                Some(1),
                Some(0),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        (
            "source.at(seconds(), pixel_index() + 1)",
            vec![
                Some(1),
                Some(2),
                Some(3),
                None,
                Some(5),
                Some(6),
                Some(7),
                None,
            ],
        ),
        (
            "source.at_global(seconds(), 7 - pixel_index())",
            vec![
                Some(7),
                Some(6),
                Some(5),
                Some(4),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        ("source.at(seconds(), -1)", vec![None; 8]),
        ("source.at_global(seconds(), 8)", vec![None; 8]),
        (
            "max(source.at_global(seconds(), 1), source.at_global(seconds(), 6))",
            vec![Some(6); 8],
        ),
    ] {
        for cached in [true, false] {
            let mut graph = base.clone();
            let mut program = compile_operators(&format!(
                "operator Spatial {{ input Signal source; color sample() {{ return {query}; }} }}"
            ))
            .unwrap()
            .remove(0)
            .bytecode;
            if !cached {
                for op in &mut program.instructions {
                    if let Instruction::SignalSample { frame_cache, .. } = op {
                        *frame_cache = u32::MAX;
                    }
                }
            }
            let mut programs = graph.programs.to_vec();
            let program_index = programs.len() as u32;
            programs.push(program);
            graph.programs = programs.into();
            graph.plan.nodes[1] = PreparedSignalNode {
                kind: PreparedSignalKind::Operator {
                    operator: PreparedOperatorNode {
                        implementation: PreparedOperator::Dsl(program_index),
                        params: BoundParams::default(),
                        automation_slot: 0,
                    },
                    inputs: vec![0].into(),
                    automation: Box::new([]),
                    vm_slot: 0,
                },
            };
            graph.plan.vm_workspace_count = 1;
            graph.plan.frame_nodes = vec![1].into();
            graph.plan.frame_slots = vec![u16::MAX, 0].into();
            graph.plan.frame_buffer_count = 1;
            let mut workspace = graph.workspace();
            let mut source_workspace = base.workspace();
            for ticks in [0, 500000, 2000000, 100000, 0] {
                let time = SampleTime::from_ticks(ticks);
                let source = base.evaluate(time, &mut source_workspace).unwrap();
                let expected = indices
                    .iter()
                    .map(|index| {
                        index.map(|index| source[index]).unwrap_or(Color {
                            red: 0,
                            green: 0,
                            blue: 0,
                        })
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    graph.evaluate(time, &mut workspace).unwrap(),
                    expected,
                    "{query}, cached={cached}, time={ticks}"
                );
            }
        }
    }
}
