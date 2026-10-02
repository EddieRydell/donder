#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;

#[allow(dead_code)]
#[path = "../benches/fixtures/mod.rs"]
mod fixtures;

use donder_language::dsl::{VmWorkspace, compile_effects};
use indexmap::IndexMap;

#[test]
fn host_mark_fixtures_preserve_children_and_rendered_frames() {
    use donder_runtime::values::SampleTime;
    let actual = [true, false].map(|pulse| {
        let show = mark_workload::mark_show(200, pulse);
        let mut workspace = show.workspace().unwrap();
        let mut output = [vec![0u8; 600]];
        let mut checksum = 0xcbf2_9ce4_8422_2325u64;
        for ticks in [
            1_999_999, 2_000_000, 2_050_000, 2_375_000, 3_125_000, 4_750_000, 2_375_000,
        ] {
            show.evaluate(SampleTime::from_ticks(ticks), &mut output, &mut workspace)
                .unwrap();
            for byte in &output[0] {
                checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        (show.signals().effects.len(), checksum)
    });
    // Captured from the raw generator path before migrating these host fixtures
    // to the specialization path used by project preparation.
    assert_eq!(
        actual,
        [
            (96, 4_276_337_823_300_249_221),
            (32, 2_010_629_835_981_686_248)
        ]
    );
}

#[test]
fn effect_automation_slots_skip_unautomated_effects() {
    use donder_runtime::automation::AutomationMapping;
    use donder_runtime::signal::{PreparedAutomation, PreparedEffectAutomation};
    use donder_runtime::values::{SampleDuration, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    let show = workload::show(
        200,
        effect.sample_program().unwrap().clone(),
        params.clone(),
    );
    let mut signals = show.signals().clone();
    signals.effects = vec![signals.effects[0].clone(); 4].into();
    signals.effects_by_layer[0] = vec![1, 3, 0, 2].into();
    for index in [0, 2] {
        signals.effects[index].start_time = SampleTime::from_ticks(7_999_999);
        signals.effects[index].duration = SampleDuration::from_ticks(1);
    }
    for (slot, index) in [1, 3].into_iter().enumerate() {
        signals.effects[index].automation = Some(Box::new(PreparedEffectAutomation {
            workspace_slot: slot,
            bindings: vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                curve: params.curve(0).unwrap(),
                param_index: 0,
                mapping: AutomationMapping::Curve {
                    min: slot as f32 * 0.8,
                    max: 0.2 + slot as f32 * 0.8,
                },
            }]
            .into(),
        }));
    }
    let singles = [1, 3].map(|index| {
        let mut single = workload::show(
            200,
            effect.sample_program().unwrap().clone(),
            params.clone(),
        )
        .signals()
        .clone();
        single.effects[0] = signals.effects[index].clone();
        single.effects[0]
            .automation
            .as_mut()
            .unwrap()
            .workspace_slot = 0;
        workload::rgb_output(single)
    });
    let show = workload::rgb_output(signals);
    let mut workspace = show.workspace().unwrap();
    let mut actual = [vec![0; 600]];
    let mut expected = vec![0; 600];
    let mut component = [vec![0; 600]];
    for frame in [0, 31, 4, 0] {
        expected.fill(0);
        for single in &singles {
            single
                .evaluate(
                    workload::time(frame),
                    &mut component,
                    &mut single.workspace().unwrap(),
                )
                .unwrap();
            for (expected, component) in expected.iter_mut().zip(&component[0]) {
                *expected = (*expected).max(*component);
            }
        }
        show.evaluate(workload::time(frame), &mut actual, &mut workspace)
            .unwrap();
        assert_eq!(actual[0], expected);
    }
}

#[test]
fn uniform_resource_samples_are_hoisted_without_retaining_references() {
    use donder_runtime::dsl::bytecode::Instruction;
    let (effect, params) = fixtures::uniform_resources();
    let prefix = &effect.sample_program().unwrap().instructions
        [..effect.sample_program().unwrap().pixel_entry as usize];
    assert!(
        prefix
            .iter()
            .any(|op| matches!(op, Instruction::CurveParamSample { .. }))
    );
    assert!(
        prefix
            .iter()
            .any(|op| matches!(op, Instruction::GradientParamSample { .. }))
    );
    let show = workload::show(
        200,
        effect.sample_program().unwrap().clone(),
        params.clone(),
    );
    let mut workspace = show.workspace().unwrap();
    let mut output = [vec![0; 600]];
    let mut vm = VmWorkspace::default();
    for frame in [0, 31, 4, 0] {
        show.evaluate(workload::time(frame), &mut output, &mut workspace)
            .unwrap();
        for pixel in 0..200 {
            let color = effect
                .sample_bound(&params, &workload::context(200, pixel, frame), &mut vm)
                .unwrap();
            assert_eq!(
                &output[0][pixel * 3..pixel * 3 + 3],
                &[color.green, color.red, color.blue]
            );
        }
    }
}

#[test]
fn resource_hoisting_preserves_branches_and_empty_gradient_defaults() {
    use donder_language::dsl::{Identifier, Value};
    use donder_runtime::dsl::bytecode::Instruction;
    use donder_runtime::values::Gradient;
    for (source, returns_black) in [
        (
            "effect Guarded { param gradient colors; color sample() { if (pixel_index() < 0) { return colors[progress()]; } return rgb(pixel_fraction(), progress(), 0.25); } }",
            false,
        ),
        (
            "effect Guarded { param gradient colors; color sample() { array<float> values = []; float value = values[pixel_index()]; return colors[progress()] * value; } }",
            true,
        ),
    ] {
        let effect = compile_effects(source).unwrap().remove(0).effect;
        assert!(
            !effect.sample_program().unwrap().instructions
                [..effect.sample_program().unwrap().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::GradientParamSample { .. }
                        | Instruction::GradientParamColorScaled { .. }
                ))
        );
        let params = effect
            .bind_params(&IndexMap::from([(
                Identifier::new("colors".into()).unwrap(),
                Value::Gradient(Gradient { stops: vec![] }.into()),
            )]))
            .unwrap();
        let result = effect.sample_bound(
            &params,
            &workload::context(200, 0, 0),
            &mut VmWorkspace::default(),
        );
        if returns_black {
            assert_eq!(result.unwrap(), donder_runtime::values::Color::BLACK);
        } else {
            result.unwrap();
        }
    }
}

#[test]
fn recursive_operator_automation_matches_frame_sampling_after_seeks_and_edits() {
    use donder_language::dsl::compile_operators;
    use donder_runtime::automation::AutomationMapping;
    use donder_runtime::signal::{
        PreparedAutomation, PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
        PreparedSignalNode,
    };
    use donder_runtime::values::{Curve, CurvePoint, SampleDuration, SampleTime};
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let gain = compile_operators("operator Gain { input Signal source; param float gain = 0.5; color sample() { return source.at(seconds()) * gain; } }").unwrap().remove(0);
    let mut show = workload::show(
        200,
        effect.sample_program().unwrap().clone(),
        effect.bind_params(&IndexMap::new()).unwrap(),
    );
    workload::apply_operator(&mut show, gain.bytecode.clone(), true);
    let mut signals = show.signals().clone();
    let PreparedSignalKind::Operator {
        operator,
        automation,
        ..
    } = &mut signals.plan.nodes[1].kind
    else {
        panic!("operator")
    };
    operator.params = gain.bind_params(&IndexMap::new()).unwrap();
    *automation = vec![PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        curve: Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 1.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        }
        .into(),
        mapping: AutomationMapping::Float { min: 0.0, max: 1.0 },
        param_index: 0,
    }]
    .into();
    let reference = signals.clone();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for source in [
        workload::IDENTITY_SOURCE,
        "operator Mix { input Signal source; color sample() { return max(source.at(seconds()), source.at(seconds() * 0.5)); } }",
    ] {
        let outer = compile_operators(source).unwrap().remove(0);
        signals = reference.clone();
        let mut programs = signals.programs.to_vec();
        programs.push(outer.bytecode.clone());
        signals.programs = programs.into();
        let mut nodes = signals.plan.nodes.to_vec();
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot: 0,
                    implementation: PreparedOperator::Dsl(2),
                    params: outer.bind_params(&IndexMap::new()).unwrap(),
                },
                inputs: vec![1].into(),
                automation: vec![].into(),
                vm_slot: 1,
            },
        });
        signals.plan.nodes = nodes.into();
        signals.plan.output_index = 2;
        signals.plan.frame_slots = vec![0; 3].into();
        signals.plan.frame_nodes = vec![2].into();
        signals.plan.vm_workspace_count = 2;
        let mut direct_signals = reference.clone();
        for min in [0.0, 0.4] {
            for sequence in [&mut signals, &mut direct_signals] {
                let PreparedSignalKind::Operator { automation, .. } =
                    &mut sequence.plan.nodes[1].kind
                else {
                    panic!("operator")
                };
                automation[0].mapping = AutomationMapping::Float { min, max: 1.0 };
            }
            let show = workload::rgb_output(signals.clone());
            let direct = workload::rgb_output(direct_signals.clone());
            let mut workspace = show.workspace().unwrap();
            for ticks in [3_000_000, 6_000_000, 1_000_000, 3_000_000] {
                let time = SampleTime::from_ticks(ticks);
                show.evaluate(time, &mut actual, &mut workspace).unwrap();
                direct
                    .evaluate(time, &mut expected, &mut direct.workspace().unwrap())
                    .unwrap();
                if source != workload::IDENTITY_SOURCE {
                    let mut past = [vec![0; 600]];
                    direct
                        .evaluate(
                            SampleTime::from_ticks(ticks / 2),
                            &mut past,
                            &mut direct.workspace().unwrap(),
                        )
                        .unwrap();
                    for (now, past) in expected[0].iter_mut().zip(&past[0]) {
                        *now = (*now).max(*past);
                    }
                }
                assert_eq!(actual, expected, "ticks={ticks} min={min}");
                assert!(actual[0].iter().any(|&byte| byte != 0));
            }
        }
    }
}

#[test]
fn upstream_prefix_reuse_matches_full_execution_across_effects_and_times() {
    use donder_language::dsl::compile_operators;
    use donder_runtime::values::{SampleDuration, SampleTime};
    let effect = compile_effects("effect Source { color sample() { float gain = sin(seconds() * 7.0) * 0.5 + 0.5; return rgb(pixel_fraction(), progress() * gain, gain); } }").unwrap().remove(0).effect;
    assert!(effect.sample_program().unwrap().pixel_entry > 0);
    let operators = [
        workload::IDENTITY_SOURCE,
        "operator Mix { input Signal source; color sample() { return max(source.at(seconds()), source.at(seconds() * 0.5)); } }",
    ];
    for (source, count) in operators
        .into_iter()
        .flat_map(|source| [1, 2].map(|count| (source, count)))
    {
        let operator = compile_operators(source).unwrap().remove(0);
        let mut show = workload::show(
            200,
            effect.sample_program().unwrap().clone(),
            effect.bind_params(&IndexMap::new()).unwrap(),
        );
        let mut signals = show.signals().clone();
        if count == 2 {
            let mut second = signals.effects[0].clone();
            second.start_time = SampleTime::from_ticks(500_000);
            second.duration = SampleDuration::from_ticks(3_000_000);
            signals.effects = vec![signals.effects[0].clone(), second].into();
            signals.effects_by_layer[0] = vec![0, 1].into();
        }
        show = workload::rgb_output(signals);
        workload::apply_operator(&mut show, operator.bytecode.clone(), true);
        let mut full = show.signals().clone();
        for program in &mut full.programs {
            workload::disable_uniform_reuse(program);
        }
        let full = workload::rgb_output(full);
        let mut workspace = show.workspace().unwrap();
        let mut full_workspace = full.workspace().unwrap();
        let mut actual = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            show.evaluate(workload::time(frame), &mut actual, &mut workspace)
                .unwrap();
            full.evaluate(workload::time(frame), &mut expected, &mut full_workspace)
                .unwrap();
            assert_eq!(actual, expected, "effects={count} frame={frame}");
            assert!(actual[0].iter().any(|&byte| byte != 0));
        }
    }
}

#[test]
fn operator_uniform_reuse_matches_full_evaluation_with_nested_signals() {
    use donder_language::dsl::compile_operators;
    use donder_runtime::signal::{
        PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
    };
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    for source in [
        "operator Wave { input Signal source; color sample() {
            float gain = sin(seconds() * 7.0) * 0.5 + 0.5;
            return source.at(seconds()) * gain;
        } }",
        "operator Wave { input Signal source; color sample() {
            float gain = sin(seconds() * 7.0) * 0.5 + 0.5;
            if (pixel_index() % 2 == 0) { gain = gain * pixel_fraction(); }
            return source.at(seconds() * 0.5) * gain;
        } }",
    ] {
        let operator = compile_operators(source).unwrap().remove(0);
        assert!(operator.bytecode.pixel_entry > 0);
        for depth in [1, 2, 8] {
            let show = workload::show(
                200,
                effect.sample_program().unwrap().clone(),
                effect.bind_params(&IndexMap::new()).unwrap(),
            );
            let mut signals = show.signals().clone();
            signals.programs = vec![
                effect.sample_program().unwrap().clone(),
                operator.bytecode.clone(),
            ]
            .into();
            let graph = &mut signals.plan;
            graph.nodes = core::iter::once(PreparedSignalNode {
                kind: PreparedSignalKind::Layer { layer_index: 0 },
            })
            .chain((0..depth).map(|input| PreparedSignalNode {
                kind: PreparedSignalKind::Operator {
                    operator: PreparedOperatorNode {
                        automation_slot: 0,
                        implementation: PreparedOperator::Dsl(1),
                        params: operator.bind_params(&IndexMap::new()).unwrap(),
                    },
                    inputs: vec![input].into(),
                    automation: vec![].into(),
                    vm_slot: input,
                },
            }))
            .collect();
            graph.output_index = depth;
            graph.vm_workspace_count = depth;
            graph.frame_nodes = vec![depth].into();
            graph.frame_slots = vec![0; depth + 1].into();
            graph.frame_buffer_count = 1;
            let mut full = signals.clone();
            workload::disable_uniform_reuse(&mut full.programs[1]);
            let show = workload::rgb_output(signals);
            let full = workload::rgb_output(full);
            let mut workspace = show.workspace().unwrap();
            let mut full_workspace = full.workspace().unwrap();
            let mut actual = [vec![0; 600]];
            let mut expected = [vec![0; 600]];
            for frame in [0, 31, 4, 0] {
                show.evaluate(workload::time(frame), &mut actual, &mut workspace)
                    .unwrap();
                full.evaluate(workload::time(frame), &mut expected, &mut full_workspace)
                    .unwrap();
                assert_eq!(actual, expected, "depth={depth} frame={frame} {source}");
                // Repeated dimming can quantize the deeper chain to black.
                if depth <= 2 {
                    assert!(actual[0].iter().any(|&value| value != 0));
                }
            }
        }
    }
}

#[test]
fn nested_prefix_reuse_tracks_sibling_parameters_and_temporal_revisits() {
    use donder_language::dsl::{Value, compile_operators};
    use donder_runtime::automation::AutomationMapping;
    use donder_runtime::signal::{
        PreparedAutomation, PreparedOperator, PreparedOperatorNode, PreparedSignalKind,
        PreparedSignalNode,
    };
    use donder_runtime::values::{Curve, CurvePoint, SampleDuration, SampleTime};
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let gain = compile_operators("operator Gain { input Signal source; param float gain = 0.5; color sample() { return source.at(seconds()) * (gain * progress()); } }").unwrap().remove(0);
    let mix = compile_operators(
        "operator Mix { input Signal a; input Signal b; color sample() {
        float now = seconds(); float past = now * 0.5;
        return max(max(a.at(now), b.at(now)), max(a.at(past), a.at(now)));
    } }",
    )
    .unwrap()
    .remove(0);
    assert!(gain.bytecode.pixel_entry > 0);
    let show = workload::show(
        200,
        effect.sample_program().unwrap().clone(),
        effect.bind_params(&IndexMap::new()).unwrap(),
    );
    let mut signals = show.signals().clone();
    signals.programs = vec![
        effect.sample_program().unwrap().clone(),
        gain.bytecode.clone(),
        mix.bytecode.clone(),
    ]
    .into();
    let mut nodes = vec![PreparedSignalNode {
        kind: PreparedSignalKind::Layer { layer_index: 0 },
    }];
    for value in [0.2, 0.9] {
        nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot: nodes.len() - 1,
                    implementation: PreparedOperator::Dsl(1),
                    params: gain
                        .bind_params(&IndexMap::from([(
                            donder_language::dsl::Identifier::new("gain".into()).unwrap(),
                            Value::Float(value),
                        )]))
                        .unwrap(),
                },
                inputs: vec![0].into(),
                automation: vec![PreparedAutomation {
                    start: SampleTime::from_ticks(0),
                    duration: SampleDuration::from_ticks(8_000_000),
                    curve: Curve {
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
                    mapping: AutomationMapping::Float {
                        min: value,
                        max: value * 0.5,
                    },
                    param_index: 0,
                }]
                .into(),
                vm_slot: 0,
            },
        });
    }
    nodes.push(PreparedSignalNode {
        kind: PreparedSignalKind::Operator {
            operator: PreparedOperatorNode {
                automation_slot: 0,
                implementation: PreparedOperator::Dsl(2),
                params: mix.bind_params(&IndexMap::new()).unwrap(),
            },
            inputs: vec![1, 2].into(),
            automation: vec![].into(),
            vm_slot: 1,
        },
    });
    let graph = &mut signals.plan;
    graph.nodes = nodes.into();
    graph.output_index = 3;
    graph.vm_workspace_count = 2;
    graph.frame_nodes = vec![3].into();
    graph.frame_slots = vec![0; 4].into();
    graph.frame_buffer_count = 1;
    let mut full = signals.clone();
    for program in &mut full.programs {
        workload::disable_uniform_reuse(program);
    }
    let show = workload::rgb_output(signals);
    let full = workload::rgb_output(full);
    let mut workspace = show.workspace().unwrap();
    let mut full_workspace = full.workspace().unwrap();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for frame in [0, 31, 4, 0] {
        show.evaluate(workload::time(frame), &mut actual, &mut workspace)
            .unwrap();
        full.evaluate(workload::time(frame), &mut expected, &mut full_workspace)
            .unwrap();
        assert_eq!(actual, expected, "frame={frame}");
        assert!(actual[0].iter().any(|&value| value != 0));
    }
}

#[test]
fn uniform_frames_match_individual_samples_when_seeking() {
    let effect = compile_effects(
        "effect Uniform {
        color sample() { return rgb(progress(), sin(seconds()) * 0.5 + 0.5, 0.25); }
    }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert!(!effect.sample_program().unwrap().uses_pixel_context);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let identity = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    for (layers, wrapped) in [1, 4, 16]
        .into_iter()
        .flat_map(|layers| [false, true].map(|wrapped| (layers, wrapped)))
    {
        let mut show = workload::layered_show(
            200,
            effect.sample_program().unwrap().clone(),
            params.clone(),
            layers,
        );
        if wrapped {
            workload::apply_operator(&mut show, identity.bytecode.clone(), true);
        }
        let mut workspace = show.workspace().unwrap();
        let mut buffers = [vec![0; 600]];
        let mut vm = VmWorkspace::default();
        for frame in [0, 31, 4, 0] {
            show.evaluate(workload::time(frame), &mut buffers, &mut workspace)
                .unwrap();
            for pixel in 0..200 {
                let color = effect
                    .sample_bound(&params, &workload::context(200, pixel, frame), &mut vm)
                    .unwrap();
                assert_eq!(
                    &buffers[0][pixel * 3..pixel * 3 + 3],
                    &[color.green, color.red, color.blue]
                );
            }
        }
    }
}

#[test]
fn uniform_empty_gradient_samples_black_for_empty_and_nonempty_targets() {
    use donder_language::dsl::{Identifier, Value};
    use donder_runtime::values::Gradient;
    let effect = compile_effects(
        "effect Uniform { param gradient colors; color sample() { return colors[progress()]; } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert!(!effect.sample_program().unwrap().uses_pixel_context);
    let params = effect
        .bind_params(&IndexMap::from([(
            Identifier::new("colors".into()).unwrap(),
            Value::Gradient(Gradient { stops: vec![] }.into()),
        )]))
        .unwrap();
    let mut show = workload::show(200, effect.sample_program().unwrap().clone(), params);
    let mut output = [vec![0; 600]];
    show.evaluate(
        workload::time(0),
        &mut output,
        &mut show.workspace().unwrap(),
    )
    .unwrap();
    assert!(output[0].iter().all(|&byte| byte == 0));
    let mut signals = show.signals().clone();
    signals.targets = vec![
        signals.targets[0].clone(),
        donder_runtime::signal::PreparedTarget {
            pixels: 0..0,
            sample_count: 0,
        },
    ]
    .into();
    signals.effects[0].target = 1;
    show = workload::rgb_output(signals);
    output[0].fill(255);
    show.evaluate(
        workload::time(0),
        &mut output,
        &mut show.workspace().unwrap(),
    )
    .unwrap();
    assert!(output[0].iter().all(|&byte| byte == 0));
}

#[test]
fn identical_target_routing_matches_address_search() {
    let effect = compile_effects(
        "effect Ramp { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let operator = donder_language::dsl::compile_operators(workload::OPERATOR_SOURCE)
        .unwrap()
        .remove(0);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut direct = workload::show(
        200,
        effect.sample_program().unwrap().clone(),
        params.clone(),
    );
    workload::apply_operator(&mut direct, operator.bytecode, true);
    let mut searched = direct.signals().clone();
    // Duplicate the target under another ID to exercise the general route with
    // exactly the same contexts. Production elaboration interns this duplicate.
    searched.targets = vec![direct.signals().targets[0].clone(); 2].into();
    searched.effects[0].target = 1;
    let searched = workload::rgb_output(searched);
    let mut direct_workspace = direct.workspace().unwrap();
    let mut searched_workspace = searched.workspace().unwrap();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for frame in [0, 31, 4, 0] {
        direct
            .evaluate(workload::time(frame), &mut actual, &mut direct_workspace)
            .unwrap();
        searched
            .evaluate(
                workload::time(frame),
                &mut expected,
                &mut searched_workspace,
            )
            .unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn mixed_pixel_and_time_expressions_match_scalar_sampling() {
    use donder_runtime::dsl::bytecode::{FloatUnary, Instruction};
    for source in [
        "effect Mixed { color sample() {
            float phase = sin(seconds()) * 0.5 + 0.5;
            color tint = mix(hsv(phase, 0.8, 1.0), rgb(phase, progress(), 0.4), 0.3);
            return tint * pixel_fraction();
        } }",
        "effect Mixed { color sample() {
            float phase = sin(seconds()) * 0.5 + 0.5;
            color tint = hsv(phase, 0.8, 1.0);
            if (pixel_index() % 2 == 0) { tint = rgb(progress(), phase, 0.25); }
            return tint * pixel_fraction();
        } }",
        "effect Mixed { color sample() {
            float x = pixel_fraction();
            float phase = sin(seconds() * 7.0) * 0.5 + 0.5;
            return rgb(x * phase, progress(), phase);
        } }",
        "effect Mixed { param float gain = 0.7; color sample() {
            float phase = sin(seconds()) * 0.5 + 0.5;
            gain = gain * progress();
            if (pixel_index() % 2 == 0) { gain = gain * pixel_fraction(); }
            return rgb(gain, phase, pixel_fraction());
        } }",
        "effect Mixed { color sample() {
            float phase = sin(seconds()) * 0.5 + 0.5;
            float level = progress();
            for (int i = 0; i < 3; i = i + 1) { level = level * pixel_fraction(); }
            return rgb(level, phase, progress());
        } }",
        "effect Mixed { color sample() {
            float phase = sin(seconds()) * 0.5 + 0.5;
            if (pixel_index() < 0) { int bad = 1 % 0; return rgb(bad, bad, bad); }
            array<float> values = [phase, pixel_fraction(), progress()];
            return rgb(values[pixel_index() % 3], phase, progress());
        } }",
    ] {
        let effect = compile_effects(source).unwrap().remove(0).effect;
        assert!(effect.sample_program().unwrap().uses_pixel_context);
        assert!(
            effect.sample_program().unwrap().instructions
                [..effect.sample_program().unwrap().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::FloatUnary {
                        op: FloatUnary::Sin,
                        ..
                    }
                ))
        );
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        for layers in [1, 4, 16] {
            let show = workload::layered_show(
                200,
                effect.sample_program().unwrap().clone(),
                params.clone(),
                layers,
            );
            let mut workspace = show.workspace().unwrap();
            let mut output = [vec![0; 600]];
            let mut vm = VmWorkspace::default();
            for frame in [0, 31, 4, 0] {
                show.evaluate(workload::time(frame), &mut output, &mut workspace)
                    .unwrap();
                for pixel in 0..200 {
                    let color = effect
                        .sample_bound(&params, &workload::context(200, pixel, frame), &mut vm)
                        .unwrap();
                    assert_eq!(
                        &output[0][pixel * 3..pixel * 3 + 3],
                        &[color.green, color.red, color.blue]
                    );
                }
            }
        }
    }
}
