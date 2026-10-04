use super::evaluation::BindForTest;
use super::evaluation::SampleEvaluation;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_language::execution::SpatialContext =
    donder_language::execution::SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };

#[allow(dead_code)]
#[path = "../../../../firmware/esp32/src/workload.rs"]
mod workload;

#[allow(dead_code)]
#[path = "../../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;

#[allow(dead_code)]
#[path = "../../benches/fixtures/mod.rs"]
mod fixtures;

use super::playback;

use crate::dsl::BatchWorkspace;
use donder_language::dsl::compile_effects;
use indexmap::IndexMap;

#[test]
fn host_mark_fixtures_are_single_samples_and_seek_stably() {
    use donder_language::values::SampleTime;
    for pulse in [true, false] {
        let show = mark_workload::mark_show(200, pulse);
        let mut workspace = show.clone().prepare().into_playback();
        let mut output = [vec![0u8; 600]];
        assert_eq!(show.clone().prepare().effect_count(), 1);
        let mut repeated = None;
        let mut illuminated = false;
        for ticks in [
            1_999_999, 2_000_000, 2_050_000, 2_375_000, 3_125_000, 4_750_000, 2_375_000,
        ] {
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(SampleTime::from_ticks(ticks)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            illuminated |= output[0].iter().any(|&byte| byte != 0);
            if ticks == 1_999_999 {
                assert!(output[0].iter().all(|&byte| byte == 0));
            }
            if ticks == 2_375_000 {
                if let Some(previous) = &repeated {
                    assert_eq!(previous, &output[0]);
                } else {
                    repeated = Some(output[0].clone());
                }
            }
        }
        assert!(illuminated);
    }
}

#[test]
fn effect_automation_slots_skip_unautomated_effects() {
    use donder_language::automation::AutomationMapping;
    use donder_language::execution::PreparedAutomation;
    use donder_language::execution::SequenceTiming;
    use donder_language::execution::SequenceWindow;
    use donder_language::values::SampleDuration;
    use donder_language::values::SampleTime;
    use std::num::NonZeroU32;
    let (effect, params) = fixtures::uniform_resources();
    let Some(donder_language::dsl::Value::Curve(curve)) = params.iter_values().next() else {
        std::panic!("first uniform resource must be a curve");
    };
    let sample = playback::sample(&effect, &params);
    let automated = [0, 1].map(|slot| {
        sample
            .clone()
            .with_automation(
                vec![PreparedAutomation {
                    start: SampleTime::from_ticks(0),
                    duration: SampleDuration::from_ticks(8_000_000),
                    curve: curve.clone(),
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
    use donder_language::dsl::bytecode::Instruction;
    let (effect, params) = fixtures::uniform_resources();
    let prefix = &effect.sample_program().bytecode().instructions
        [..effect.sample_program().bytecode().pixel_entry as usize];
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
    let show = workload::show(200, effect.sample_program().clone(), params.clone());
    let mut workspace = show.clone().prepare().into_playback();
    let mut output = [vec![0; 600]];
    let invocation = effect
        .sample_program()
        .bind_for_test(params.iter_values().collect())
        .unwrap();
    let mut vm = BatchWorkspace::default();
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in output
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        for pixel in 0..200 {
            let color = invocation.evaluate(
                &super::evaluation::context(200, pixel, frame),
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

#[test]
fn resource_hoisting_preserves_branches_and_empty_gradient_defaults() {
    use donder_language::dsl::Identifier;
    use donder_language::dsl::Value;
    use donder_language::dsl::bytecode::Instruction;
    use donder_language::values::Gradient;
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
        let effect = compile_effects(source).unwrap().remove(0);
        assert!(
            effect.sample_program().bytecode().instructions
                [..effect.sample_program().bytecode().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::GradientParamSample { .. }
                        | Instruction::GradientParamColorScaled { .. }
                ))
        );
        let params = donder_language::dsl::bind_params(
            effect.params(),
            &IndexMap::from([(
                Identifier::new("colors".into()).unwrap(),
                Value::Gradient(Gradient { stops: vec![] }.into()),
            )]),
        )
        .unwrap();
        let result = effect
            .sample_program()
            .bind_for_test(params.iter_values().collect())
            .unwrap()
            .evaluate(
                &super::evaluation::context(200, 0, 0),
                &SPATIAL,
                &mut BatchWorkspace::default(),
            );
        if returns_black {
            assert_eq!(result, donder_language::values::Color::BLACK);
        } else {
            assert_eq!(
                result,
                crate::sampling::rgb(0.0, super::evaluation::context(200, 0, 0).progress, 0.25)
            );
        }
    }
}

#[test]
fn recursive_operator_automation_matches_frame_sampling_after_seeks_and_edits() {
    use donder_language::automation::AutomationMapping;
    use donder_language::dsl::compile_operators;
    use donder_language::execution::PreparedAutomation;
    use donder_language::values::Curve;
    use donder_language::values::CurvePoint;
    use donder_language::values::SampleDuration;
    use donder_language::values::SampleTime;
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0);
    let gain = compile_operators("operator Gain { input Signal source; param float gain = 0.5; color sample() { return source.at(seconds()) * gain; } }").unwrap().remove(0);
    let sample = playback::sample(
        &effect,
        &donder_language::dsl::bind_params(effect.params(), &IndexMap::new()).unwrap(),
    );
    let gain_params = donder_language::dsl::bind_params(gain.params(), &IndexMap::new()).unwrap();
    let mut actual = [vec![0; 600]];
    let mut expected = [vec![0; 600]];
    for source in [
        workload::IDENTITY_SOURCE,
        "operator Mix { input Signal source; color sample() { return max(source.at(seconds()), source.at(seconds() * 0.5)); } }",
    ] {
        let outer = compile_operators(source).unwrap().remove(0);
        let outer = playback::operator(
            &outer,
            &donder_language::dsl::bind_params(outer.params(), &IndexMap::new()).unwrap(),
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
    use donder_language::values::SampleDuration;
    use donder_language::values::SampleTime;
    let effect = compile_effects("effect Source { color sample() { float gain = sin(seconds() * 7.0) * 0.5 + 0.5; return rgb(pixel_fraction(), progress() * gain, gain); } }").unwrap().remove(0);
    assert!(effect.sample_program().bytecode().pixel_entry > 0);
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
            let (mut bytecode, types) = effect.sample_program().clone().into_parts();
            let operator = if reuse {
                operator.program().clone()
            } else {
                workload::disable_uniform_reuse(&mut bytecode);
                playback::map_operator_bytecode(&operator, workload::disable_uniform_reuse)
            };
            let sample = workload::SampleFixture {
                program: donder_language::dsl::SampleProgram::admit(bytecode, types).unwrap(),
                params: donder_language::dsl::bind_params(effect.params(), &IndexMap::new())
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
            let show = workload::Workload::samples(200, vec![effects]);
            let invocation = playback::operator_program(
                &operator,
                &donder_language::dsl::BoundParams::bind_values(&[], vec![]).unwrap(),
            );
            show.prepare_with(|builder, _, signal| {
                let result = builder.operator(&invocation, |_| signal);
                builder.output([result])
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
            assert_eq!(actual, expected, "effects={count} frame={frame}");
            assert!(actual[0].iter().any(|&byte| byte != 0));
        }
    }
}

#[test]
fn operator_uniform_reuse_matches_full_evaluation_with_nested_signals() {
    use donder_language::dsl::compile_operators;
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0);
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
                &donder_language::dsl::bind_params(effect.params(), &IndexMap::new()).unwrap(),
            );
            let prepare = |reuse| {
                let params =
                    donder_language::dsl::bind_params(operator.params(), &IndexMap::new()).unwrap();
                let operator = if reuse {
                    operator.program().clone()
                } else {
                    playback::map_operator_bytecode(&operator, workload::disable_uniform_reuse)
                };
                let invocation = playback::operator_program(&operator, &params);
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
    use donder_language::automation::AutomationMapping;
    use donder_language::dsl::BoundParams;
    use donder_language::dsl::OperatorDefinition;
    use donder_language::dsl::SampleDefinition;
    use donder_language::dsl::SampleProgram;
    use donder_language::dsl::Value;
    use donder_language::dsl::compile_operators;
    use donder_language::execution::PreparedAutomation;
    use donder_language::values::Curve;
    use donder_language::values::CurvePoint;
    use donder_language::values::SampleDuration;
    use donder_language::values::SampleTime;
    let effect = compile_effects(
        "effect Source { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0);
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
        let (mut bytecode, types) = effect.sample_program().clone().into_parts();
        let (gain, mix) = if reuse {
            (gain.program().clone(), mix.program().clone())
        } else {
            workload::disable_uniform_reuse(&mut bytecode);
            (
                playback::map_operator_bytecode(&gain, workload::disable_uniform_reuse),
                playback::map_operator_bytecode(&mix, workload::disable_uniform_reuse),
            )
        };
        let sample = SampleDefinition::new(SampleProgram::admit(bytecode, types).unwrap())
            .bind(vec![])
            .unwrap();
        let gain = OperatorDefinition::new(gain);
        let siblings = [0.2, 0.9].map(|value| {
            gain.bind(vec![Value::Float(value)])
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
        let mix = playback::operator_program(&mix, &BoundParams::bind_values(&[], vec![]).unwrap());
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
    .remove(0);
    assert!(!effect.sample_program().bytecode().uses_pixel_context);
    let params = donder_language::dsl::bind_params(effect.params(), &IndexMap::new()).unwrap();
    let identity = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    for (layers, wrapped) in [1, 4, 16]
        .into_iter()
        .flat_map(|layers| [false, true].map(|wrapped| (layers, wrapped)))
    {
        let mut show =
            workload::layered_show(200, effect.sample_program().clone(), params.clone(), layers);
        if wrapped {
            workload::apply_operator(&mut show, identity.program().clone().into_parts().0, true);
        }
        let mut workspace = show.clone().prepare().into_playback();
        let mut buffers = [vec![0; 600]];
        let invocation = effect
            .sample_program()
            .bind_for_test(params.iter_values().collect())
            .unwrap();
        let mut vm = BatchWorkspace::default();
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in buffers
                .iter_mut()
                .zip(workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for pixel in 0..200 {
                let color = invocation.evaluate(
                    &super::evaluation::context(200, pixel, frame),
                    &SPATIAL,
                    &mut vm,
                );
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
    use donder_language::dsl::Identifier;
    use donder_language::dsl::Value;
    use donder_language::values::Gradient;
    let effect = compile_effects(
        "effect Uniform { param gradient colors; color sample() { return colors[progress()]; } }",
    )
    .unwrap()
    .remove(0);
    assert!(!effect.sample_program().bytecode().uses_pixel_context);
    let params = donder_language::dsl::bind_params(
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
                builder.target([], donder_language::execution::TargetScope::WholeTarget)
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
    use donder_language::dsl::bytecode::FloatUnary;
    use donder_language::dsl::bytecode::Instruction;
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
        let effect = compile_effects(source).unwrap().remove(0);
        assert!(effect.sample_program().bytecode().uses_pixel_context);
        assert!(
            effect.sample_program().bytecode().instructions
                [..effect.sample_program().bytecode().pixel_entry as usize]
                .iter()
                .any(|op| matches!(
                    op,
                    Instruction::FloatUnary {
                        op: FloatUnary::Sin,
                        ..
                    }
                ))
        );
        let params = donder_language::dsl::bind_params(effect.params(), &IndexMap::new()).unwrap();
        for layers in [1, 4, 16] {
            let show = workload::layered_show(
                200,
                effect.sample_program().clone(),
                params.clone(),
                layers,
            );
            let mut workspace = show.clone().prepare().into_playback();
            let mut output = [vec![0; 600]];
            let invocation = effect
                .sample_program()
                .bind_for_test(params.iter_values().collect())
                .unwrap();
            let mut vm = BatchWorkspace::default();
            for frame in [0, 31, 4, 0] {
                for (snapshot, output) in output
                    .iter_mut()
                    .zip(workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                for pixel in 0..200 {
                    let color = invocation.evaluate(
                        &super::evaluation::context(200, pixel, frame),
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
