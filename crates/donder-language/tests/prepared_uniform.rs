const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;

#[allow(dead_code)]
#[path = "../benches/fixtures/mod.rs"]
mod fixtures;

#[allow(dead_code)]
#[path = "support/playback.rs"]
mod playback;

use donder_language::dsl::{VmWorkspace, compile_effects};
use indexmap::IndexMap;

#[test]
fn host_mark_fixtures_preserve_children_and_rendered_frames() {
    use donder_runtime::SampleTime;
    let actual = [true, false].map(|pulse| {
        let show = mark_workload::mark_show(200, pulse);
        let mut workspace = show.clone().prepare().into_playback();
        let mut output = [vec![0u8; 600]];
        let mut checksum = 0xcbf2_9ce4_8422_2325u64;
        for ticks in [
            1_999_999, 2_000_000, 2_050_000, 2_375_000, 3_125_000, 4_750_000, 2_375_000,
        ] {
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(SampleTime::from_ticks(ticks)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for byte in &output[0] {
                checksum = (checksum ^ u64::from(*byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        (show.clone().prepare().effect_count(), checksum)
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
    use donder_runtime::{
        AutomationMapping, PreparedAutomation, SampleDuration, SampleTime, SequenceTiming,
        SequenceWindow,
    };
    use std::num::NonZeroU32;
    let (effect, params) = fixtures::uniform_resources();
    let sample = playback::sample(&effect, &params);
    let automated = [0, 1].map(|slot| {
        sample
            .clone()
            .with_automation(
                vec![PreparedAutomation {
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
            )
            .unwrap()
    });
    let timing = SequenceTiming::admit(
        NonZeroU32::new(120).unwrap(),
        NonZeroU32::new(960).unwrap(),
        NonZeroU32::new(8_000_000).unwrap(),
        vec![SequenceWindow {
            start: SampleTime::from_ticks(7_999_999),
            duration: NonZeroU32::new(1).unwrap(),
        }]
        .into(),
    )
    .unwrap();
    let show = playback::build(200, timing, |builder, target| {
        let late = builder.windows().next().unwrap();
        let whole = builder.whole_sequence();
        let effects = [
            builder.sample(&sample, late, target),
            builder.sample(&automated[0], whole, target),
            builder.sample(&sample, late, target),
            builder.sample(&automated[1], whole, target),
        ];
        let layer = builder.layer(true, [effects[1], effects[3], effects[0], effects[2]]);
        builder.output([layer])
    });
    let singles = automated;
    let mut workspace = show.into_playback();
    let mut actual = [vec![0; 600]];
    let mut expected = vec![0; 600];
    let mut component = [vec![0; 600]];
    for frame in [0, 31, 4, 0] {
        expected.fill(0);
        for single in &singles {
            for (snapshot, output) in component.iter_mut().zip(
                playback::show(200, single, 1)
                    .into_playback()
                    .evaluate(workload::time(frame))
                    .outputs(),
            ) {
                snapshot.copy_from_slice(output.bytes);
            }
            for (expected, component) in expected.iter_mut().zip(&component[0]) {
                *expected = (*expected).max(*component);
            }
        }
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        assert_eq!(actual[0], expected);
    }
}

#[test]
fn uniform_resource_samples_are_hoisted_without_retaining_references() {
    use donder_runtime::Instruction;
    let (effect, params) = fixtures::uniform_resources();
    let prefix = &effect.sample_program().unwrap().bytecode().instructions
        [..effect.sample_program().unwrap().bytecode().pixel_entry as usize];
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
    let mut workspace = show.clone().prepare().into_playback();
    let mut output = [vec![0; 600]];
    let invocation = effect
        .sample_program()
        .unwrap()
        .bind(
            params.iter_values().collect(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let mut vm = VmWorkspace::default();
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in output
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        for pixel in 0..200 {
            let color =
                invocation.evaluate(&workload::context(200, pixel, frame), &SPATIAL, &mut vm);
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
    use donder_runtime::Gradient;
    use donder_runtime::Instruction;
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
            !effect.sample_program().unwrap().bytecode().instructions
                [..effect.sample_program().unwrap().bytecode().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::GradientParamSample { .. }
                        | Instruction::GradientParamColorScaled { .. }
                ))
        );
        let params = donder_runtime::BoundParams::bind(
            effect.params(),
            &IndexMap::from([(
                Identifier::new("colors".into()).unwrap(),
                Value::Gradient(Gradient { stops: vec![] }.into()),
            )]),
        )
        .unwrap();
        let result = effect
            .sample_program()
            .unwrap()
            .bind(
                params.iter_values().collect(),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap()
            .evaluate(
                &workload::context(200, 0, 0),
                &SPATIAL,
                &mut VmWorkspace::default(),
            );
        if returns_black {
            assert_eq!(result, donder_runtime::Color::BLACK);
        }
    }
}

#[test]
fn recursive_operator_automation_matches_frame_sampling_after_seeks_and_edits() {
    use donder_language::dsl::compile_operators;
    use donder_runtime::{
        AutomationMapping, BoundParams, Curve, CurvePoint, PreparedAutomation, SampleDuration,
        SampleTime,
    };
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let gain = compile_operators("operator Gain { input Signal source; param float gain = 0.5; color sample() { return source.at(seconds()) * gain; } }").unwrap().remove(0);
    let sample = playback::sample(
        &effect,
        &BoundParams::bind(effect.params(), &IndexMap::new()).unwrap(),
    );
    let gain_params = BoundParams::bind(gain.params(), &IndexMap::new()).unwrap();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for source in [
        workload::IDENTITY_SOURCE,
        "operator Mix { input Signal source; color sample() { return max(source.at(seconds()), source.at(seconds() * 0.5)); } }",
    ] {
        let outer = compile_operators(source).unwrap().remove(0);
        let outer = playback::operator(
            &outer,
            &BoundParams::bind(outer.params(), &IndexMap::new()).unwrap(),
        );
        for min in [0.0, 0.4] {
            let gain = playback::operator(&gain, &gain_params)
                .with_automation(
                    vec![PreparedAutomation {
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
                        mapping: AutomationMapping::Float { min, max: 1.0 },
                        param_index: 0,
                    }]
                    .into(),
                )
                .unwrap();
            let direct = || playback::chain(200, &sample, 1, core::slice::from_ref(&gain));
            let mut workspace =
                playback::chain(200, &sample, 1, &[gain.clone(), outer.clone()]).into_playback();
            for ticks in [3_000_000, 6_000_000, 1_000_000, 3_000_000] {
                let time = SampleTime::from_ticks(ticks);
                for (snapshot, output) in actual.iter_mut().zip(workspace.evaluate(time).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                for (snapshot, output) in expected
                    .iter_mut()
                    .zip(direct().into_playback().evaluate(time).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                if source != workload::IDENTITY_SOURCE {
                    let mut past = [vec![0; 600]];
                    for (snapshot, output) in past.iter_mut().zip(
                        direct()
                            .into_playback()
                            .evaluate(SampleTime::from_ticks(ticks / 2))
                            .outputs(),
                    ) {
                        snapshot.copy_from_slice(output.bytes);
                    }
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
    use donder_runtime::{SampleDuration, SampleTime};
    let effect = compile_effects("effect Source { color sample() { float gain = sin(seconds() * 7.0) * 0.5 + 0.5; return rgb(pixel_fraction(), progress() * gain, gain); } }").unwrap().remove(0).effect;
    assert!(effect.sample_program().unwrap().bytecode().pixel_entry > 0);
    let operators = [
        workload::IDENTITY_SOURCE,
        "operator Mix { input Signal source; color sample() { return max(source.at(seconds()), source.at(seconds() * 0.5)); } }",
    ];
    for (source, count) in operators
        .into_iter()
        .flat_map(|source| [1, 2].map(|count| (source, count)))
    {
        let operator = compile_operators(source).unwrap().remove(0);
        let prepare = |reuse| {
            let (mut bytecode, types) = effect.sample_program().unwrap().clone().into_parts();
            let operator = if reuse {
                operator.clone()
            } else {
                workload::disable_uniform_reuse(&mut bytecode);
                playback::map_operator_bytecode(&operator, workload::disable_uniform_reuse)
            };
            let sample = workload::SampleFixture {
                program: donder_runtime::SampleProgram::admit(bytecode, types).unwrap(),
                params: donder_runtime::BoundParams::bind(effect.params(), &IndexMap::new())
                    .unwrap(),
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                automation: vec![],
            };
            let mut effects = vec![sample.clone()];
            if count == 2 {
                effects.push(workload::SampleFixture {
                    start: SampleTime::from_ticks(500_000),
                    duration: SampleDuration::from_ticks(3_000_000),
                    ..sample
                });
            }
            let mut show = workload::Workload::samples(200, vec![effects]);
            workload::apply_compiled_operator(&mut show, operator);
            show.prepare()
        };
        let mut workspace = prepare(true).into_playback();
        let mut full_workspace = prepare(false).into_playback();
        let mut actual = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in actual
                .iter_mut()
                .zip(workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for (snapshot, output) in expected
                .iter_mut()
                .zip(full_workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            assert_eq!(actual, expected, "effects={count} frame={frame}");
            assert!(actual[0].iter().any(|&byte| byte != 0));
        }
    }
}

#[test]
fn operator_uniform_reuse_matches_full_evaluation_with_nested_signals() {
    use donder_language::dsl::compile_operators;
    use donder_runtime::BoundParams;
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
        assert!(operator.bytecode().pixel_entry > 0);
        for depth in [1, 2, 8] {
            let sample = playback::sample(
                &effect,
                &BoundParams::bind(effect.params(), &IndexMap::new()).unwrap(),
            );
            let prepare = |reuse| {
                let operator = if reuse {
                    operator.clone()
                } else {
                    playback::map_operator_bytecode(&operator, workload::disable_uniform_reuse)
                };
                let params = BoundParams::bind(operator.params(), &IndexMap::new()).unwrap();
                let invocation = playback::operator(&operator, &params);
                playback::chain(200, &sample, 1, &vec![invocation; depth])
            };
            let mut workspace = prepare(true).into_playback();
            let mut full_workspace = prepare(false).into_playback();
            let mut actual = [vec![0; 600]];
            let mut expected = [vec![0; 600]];
            for frame in [0, 31, 4, 0] {
                for (snapshot, output) in actual
                    .iter_mut()
                    .zip(workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                for (snapshot, output) in expected
                    .iter_mut()
                    .zip(full_workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
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
    use donder_runtime::AutomationMapping;
    use donder_runtime::{
        BoundParams, DslBindCache, OperatorDefinition, PreparedAutomation, SampleDefinition,
        SampleProgram,
    };
    use donder_runtime::{Curve, CurvePoint, SampleDuration, SampleTime};
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
    assert!(gain.bytecode().pixel_entry > 0);
    let prepare = |reuse| {
        let (mut bytecode, types) = effect.sample_program().unwrap().clone().into_parts();
        let (gain, mix) = if reuse {
            (gain.clone(), mix.clone())
        } else {
            workload::disable_uniform_reuse(&mut bytecode);
            (
                playback::map_operator_bytecode(&gain, workload::disable_uniform_reuse),
                playback::map_operator_bytecode(&mix, workload::disable_uniform_reuse),
            )
        };
        let sample = SampleDefinition::new(SampleProgram::admit(bytecode, types).unwrap())
            .bind(vec![], &mut DslBindCache::default())
            .unwrap();
        let gain = OperatorDefinition::new(gain);
        let siblings = [0.2, 0.9].map(|value| {
            gain.bind(vec![Value::Float(value)], &mut DslBindCache::default())
                .unwrap()
                .with_automation(
                    vec![PreparedAutomation {
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
                )
                .unwrap()
        });
        let mix = playback::operator(&mix, &BoundParams::default());
        playback::build(200, playback::timing(8_000_000), |builder, target| {
            let effect = builder.sample(&sample, builder.whole_sequence(), target);
            let layer = builder.layer(true, [effect]);
            let siblings = siblings.map(|invocation| builder.operator(&invocation, |_| layer));
            let mixed = builder.operator(&mix, |input| siblings[input]);
            builder.output([mixed])
        })
    };
    let mut workspace = prepare(true).into_playback();
    let mut full_workspace = prepare(false).into_playback();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        for (snapshot, output) in expected
            .iter_mut()
            .zip(full_workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
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
    assert!(
        !effect
            .sample_program()
            .unwrap()
            .bytecode()
            .uses_pixel_context
    );
    let params = donder_runtime::BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
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
            workload::apply_operator(&mut show, identity.program().clone().into_parts().0, true);
        }
        let mut workspace = show.clone().prepare().into_playback();
        let mut buffers = [vec![0; 600]];
        let invocation = effect
            .sample_program()
            .unwrap()
            .bind(
                params.iter_values().collect(),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        let mut vm = VmWorkspace::default();
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in buffers
                .iter_mut()
                .zip(workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for pixel in 0..200 {
                let color =
                    invocation.evaluate(&workload::context(200, pixel, frame), &SPATIAL, &mut vm);
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
    use donder_runtime::Gradient;
    let effect = compile_effects(
        "effect Uniform { param gradient colors; color sample() { return colors[progress()]; } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert!(
        !effect
            .sample_program()
            .unwrap()
            .bytecode()
            .uses_pixel_context
    );
    let params = donder_runtime::BoundParams::bind(
        effect.params(),
        &IndexMap::from([(
            Identifier::new("colors".into()).unwrap(),
            Value::Gradient(Gradient { stops: vec![] }.into()),
        )]),
    )
    .unwrap();
    let invocation = playback::sample(&effect, &params);
    let mut output = [vec![0; 600]];
    for empty in [false, true] {
        let show = playback::build(200, playback::timing(8_000_000), |builder, target| {
            let effect_target = if empty {
                builder.target([], donder_runtime::TargetScope::WholeTarget)
            } else {
                target
            };
            let effect = builder.sample(&invocation, builder.whole_sequence(), effect_target);
            let layer = builder.layer(true, [effect]);
            builder.output([layer])
        });
        output[0].fill(255);
        for (snapshot, output) in output
            .iter_mut()
            .zip(show.into_playback().evaluate(workload::time(0)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        assert!(output[0].iter().all(|&byte| byte == 0));
    }
}

#[test]
fn mixed_pixel_and_time_expressions_match_scalar_sampling() {
    use donder_runtime::{FloatUnary, Instruction};
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
        assert!(
            effect
                .sample_program()
                .unwrap()
                .bytecode()
                .uses_pixel_context
        );
        assert!(
            effect.sample_program().unwrap().bytecode().instructions
                [..effect.sample_program().unwrap().bytecode().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::FloatUnary {
                        op: FloatUnary::Sin,
                        ..
                    }
                ))
        );
        let params = donder_runtime::BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
        for layers in [1, 4, 16] {
            let show = workload::layered_show(
                200,
                effect.sample_program().unwrap().clone(),
                params.clone(),
                layers,
            );
            let mut workspace = show.clone().prepare().into_playback();
            let mut output = [vec![0; 600]];
            let invocation = effect
                .sample_program()
                .unwrap()
                .bind(
                    params.iter_values().collect(),
                    &mut donder_runtime::DslBindCache::default(),
                )
                .unwrap();
            let mut vm = VmWorkspace::default();
            for frame in [0, 31, 4, 0] {
                for (snapshot, output) in output
                    .iter_mut()
                    .zip(workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                for pixel in 0..200 {
                    let color = invocation.evaluate(
                        &workload::context(200, pixel, frame),
                        &SPATIAL,
                        &mut vm,
                    );
                    assert_eq!(
                        &output[0][pixel * 3..pixel * 3 + 3],
                        &[color.green, color.red, color.blue]
                    );
                }
            }
        }
    }
}
