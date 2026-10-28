//! Playback of prepared effects and operators whose work is staged: query
//! and target blocks run once before the per-strip body, and a uniform result
//! is sampled once for its whole target. Playback must match each pixel
//! sampled alone and the same programs run unstaged.
use donder_runtime::{PreparedShow, ShowPlayback};
use donder_runtime_types::AutomationMapping;
use donder_runtime_types::bytecode::{
    BytecodeProgram, ContextRead, FloatUnary, Instruction, STRIP, instructions,
};
use donder_runtime_types::{BoundProgram, Value};
use donder_runtime_types::{
    Color, Curve, CurvePoint, Gradient, GradientStop, SampleDuration, SampleTime,
};
use donder_runtime_types::{PreparedAutomation, SequenceTiming, TargetGeometry, TargetScope};
use donder_test_support::marks as mark_workload;
use donder_test_support::playback::compile;
use donder_test_support::{fixtures, playback, workload};
use std::num::NonZeroU32;

/// The sequence's first output, as bytes.
fn frame(playback: &mut ShowPlayback, time: SampleTime) -> Vec<u8> {
    playback
        .evaluate(time)
        .outputs()
        .next()
        .unwrap()
        .bytes
        .to_vec()
}

fn ramp(from: f32, to: f32) -> Curve {
    Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: from,
            },
            CurvePoint {
                position: 1.0,
                value: to,
            },
        ],
    }
}

fn automation(
    param_index: u16,
    curve: Curve,
    mapping: AutomationMapping,
) -> Box<[PreparedAutomation]> {
    Box::new([PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        curve: curve.into(),
        param_index,
        mapping,
        quantity: donder_runtime_types::AutomatedQuantity::Value,
    }])
}

/// Whether the query block, which runs once per query, holds an instruction.
fn in_query_block(bytecode: &BytecodeProgram, instruction: impl Fn(&Instruction) -> bool) -> bool {
    instructions(bytecode.query()).any(|op| instruction(&op))
}

fn is_sin(instruction: &Instruction) -> bool {
    matches!(
        instruction,
        Instruction::FloatUnary {
            op: FloatUnary::Sin,
            ..
        }
    )
}

/// `program` with its query and target blocks rerun for every strip and no
/// frame caches.
fn unstaged(program: &BoundProgram) -> BoundProgram {
    workload::edit(program, workload::unstaged)
}

/// Every pixel of a `count`-pixel fixture sampled alone: `invocation` on a
/// one-pixel slice of the fixture's target for each pixel.
fn single_pixels(count: usize, invocation: &BoundProgram) -> PreparedShow {
    playback::build(count, playback::timing(8_000_000), |builder, _| {
        let window = SampleDuration::from_ticks(8_000_000);
        let domain = TargetGeometry::new([(0, &playback::row(count))], TargetScope::WholeTarget);
        let effects: Vec<_> = (0..count)
            .map(|pixel| {
                let slice = builder.target(&domain.slice(pixel..pixel + 1));
                builder.clip(None, invocation, SampleTime::from_ticks(0), window, slice)
            })
            .collect();
        let layer = builder.layer(true, effects);
        Some(layer)
    })
}

/// Playback of `show` matches each of its `count` pixels sampled alone by
/// `invocation`.
fn assert_matches_single_pixels(
    show: workload::Workload,
    count: usize,
    invocation: &BoundProgram,
    what: &str,
) {
    let mut playback = show.prepare().into_playback();
    let mut single = single_pixels(count, invocation).into_playback();
    for frame_index in [0, 31, 4, 0] {
        let time = playback::time(frame_index);
        assert_eq!(
            playback.evaluate(time).colors(),
            single.evaluate(time).colors(),
            "{what} frame={frame_index}"
        );
    }
}

#[test]
fn host_mark_fixtures_are_single_samples_and_seek_stably() {
    for pulse in [true, false] {
        let show = mark_workload::mark_show(200, pulse);
        assert_eq!(show.clone().prepare().clip_count(), 1);
        let mut playback = show.prepare().into_playback();
        let mut repeated = None;
        let mut illuminated = false;
        for ticks in [
            1_999_999, 2_000_000, 2_050_000, 2_375_000, 3_125_000, 4_750_000, 2_375_000,
        ] {
            let output = frame(&mut playback, SampleTime::from_ticks(ticks));
            illuminated |= output.iter().any(|&byte| byte != 0);
            if ticks == 1_999_999 {
                assert!(output.iter().all(|&byte| byte == 0));
            }
            if ticks == 2_375_000 {
                if let Some(previous) = &repeated {
                    assert_eq!(previous, &output);
                } else {
                    repeated = Some(output);
                }
            }
        }
        assert!(illuminated);
    }
}

const UNIFORM_RESOURCES: &str = "effect UniformResources {
    param shape: curve in 0.0..1.0;
    param colors: gradient;
    sample { colors[shape[progress]] * pixel.fraction }
}";

fn uniform_gradient() -> Gradient {
    Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color::from_bytes([255, 64, 16]),
            },
            GradientStop {
                position: 1.0,
                color: Color::from_bytes([16, 96, 255]),
            },
        ],
    }
}

#[test]
fn effect_automation_slots_skip_unautomated_effects() {
    let effect = compile(UNIFORM_RESOURCES);
    let curve = ramp(0.0, 1.0);
    let values = vec![
        Value::Curve(curve.clone().into()),
        Value::Gradient(uniform_gradient().into()),
    ];
    let sample = playback::lower(&effect.invoke(values.clone(), Box::new([])).unwrap());
    let automated = [0, 1].map(|slot| {
        let mapping = AutomationMapping::Curve {
            min: slot as f32 * 0.8,
            max: 0.2 + slot as f32 * 0.8,
        };
        playback::lower(
            &effect
                .invoke(values.clone(), automation(0, curve.clone(), mapping))
                .unwrap(),
        )
    });
    let timing = SequenceTiming::new(
        NonZeroU32::new(120).unwrap(),
        NonZeroU32::new(960).unwrap(),
        NonZeroU32::new(8_000_000).unwrap(),
    );
    let show = playback::build(200, timing, |builder, target| {
        let (late, last) = (
            SampleTime::from_ticks(7_999_999),
            SampleDuration::from_ticks(1),
        );
        let (start, whole) = (
            SampleTime::from_ticks(0),
            SampleDuration::from_ticks(8_000_000),
        );
        let effects = [
            builder.clip(None, &sample, late, last, target),
            builder.clip(None, &automated[0], start, whole, target),
            builder.clip(None, &sample, late, last, target),
            builder.clip(None, &automated[1], start, whole, target),
        ];
        let layer = builder.layer(true, [effects[1], effects[3], effects[0], effects[2]]);
        Some(layer)
    });
    let mut playback = show.into_playback();
    for frame_index in [0, 31, 4, 0] {
        let time = playback::time(frame_index);
        let mut expected = vec![0; 600];
        for single in &automated {
            let component = frame(
                &mut playback::show(200, single, 1, &[]).into_playback(),
                time,
            );
            for (expected, component) in expected.iter_mut().zip(&component) {
                *expected = (*expected).max(*component);
            }
        }
        assert_eq!(frame(&mut playback, time), expected);
    }
}

#[test]
fn uniform_resource_samples_match_single_pixels() {
    let invocation = fixtures::uniform_resources();
    assert_matches_single_pixels(
        workload::layered_show(200, &invocation, 1),
        200,
        &playback::lower(&invocation),
        "uniform resources",
    );
}

#[test]
fn recursive_operator_automation_matches_frame_sampling_after_seeks_and_edits() {
    let sample = playback::program(&compile(
        "effect Source { sample { rgb(pixel.fraction, progress, 0.25) } }",
    ));
    let gain = compile(
        "operator Gain { input source; param gain: float in 0.0..1.0 = 0.5; sample { source * gain } }",
    );
    {
        let outer = playback::program(&compile(playback::IDENTITY_SOURCE));
        for min in [0.0, 0.4] {
            let gain = playback::lower(
                &gain
                    .invoke(
                        vec![Value::Float(0.5)],
                        automation(
                            0,
                            ramp(1.0, 0.0),
                            AutomationMapping::Float { min, max: 1.0 },
                        ),
                    )
                    .unwrap(),
            );
            let direct = || playback::show(200, &sample, 1, core::slice::from_ref(&gain));
            let mut playback =
                playback::show(200, &sample, 1, &[gain.clone(), outer.clone()]).into_playback();
            for ticks in [3_000_000, 6_000_000, 1_000_000, 3_000_000] {
                let actual = frame(&mut playback, SampleTime::from_ticks(ticks));
                let expected = frame(&mut direct().into_playback(), SampleTime::from_ticks(ticks));
                assert_eq!(actual, expected, "ticks={ticks} min={min}");
                assert!(actual.iter().any(|&byte| byte != 0));
            }
        }
    }
}

fn assert_same_frames(staged: PreparedShow, unstaged: PreparedShow, what: &str) {
    let mut staged = staged.into_playback();
    let mut unstaged = unstaged.into_playback();
    for frame_index in [0, 31, 4, 0] {
        let time = playback::time(frame_index);
        let actual = frame(&mut staged, time);
        assert_eq!(
            actual,
            frame(&mut unstaged, time),
            "{what} frame={frame_index}"
        );
        assert!(actual.iter().any(|&byte| byte != 0));
    }
}

#[test]
fn staged_upstream_matches_unstaged_execution_across_effects() {
    let effect = playback::invoke(
        &compile(
            "effect Source { sample {
            let gain = sin(time * 7.0) * 0.5 + 0.5;
            rgb(pixel.fraction, progress * gain, gain)
        } }",
        ),
        &[],
    );
    let sample = workload::sample_fixture(
        &effect,
        200,
        SampleTime::from_ticks(0),
        SampleDuration::from_ticks(8_000_000),
    );
    assert!(in_query_block(&sample.program.program().bytecode, is_sin));
    for (source, count) in [
        playback::IDENTITY_SOURCE,
        "operator Mix { input source; sample { max(source, source.at(pixel.index + 1)) } }",
    ]
    .into_iter()
    .flat_map(|source| [1, 2].map(|count| (source, count)))
    {
        let operator = workload::operator_program(&compile(source));
        let prepare = |staged| {
            let (sample, operator) = if staged {
                (sample.clone(), operator.clone())
            } else {
                (
                    workload::Clip {
                        program: unstaged(&sample.program),
                        ..sample.clone()
                    },
                    unstaged(&operator),
                )
            };
            let mut effects = vec![sample.clone()];
            if count == 2 {
                effects.push(workload::Clip {
                    start: SampleTime::from_ticks(500_000),
                    duration: SampleDuration::from_ticks(3_000_000),
                    ..sample
                });
            }
            workload::Workload {
                operators: vec![operator],
                ..workload::Workload::new(200, vec![effects])
            }
            .prepare()
        };
        assert_same_frames(prepare(true), prepare(false), &format!("effects={count}"));
    }
}

#[test]
fn staged_operators_match_unstaged_evaluation_with_nested_signals() {
    let sample = playback::program(&compile(
        "effect Source { sample { rgb(pixel.fraction, progress, 0.25) } }",
    ));
    for source in [
        "operator Wave { input source; sample {
            let gain = sin(time * 7.0) * 0.5 + 0.5;
            source * gain
        } }",
        "operator Wave { input source; sample {
            let gain = sin(time * 7.0) * 0.5 + 0.5;
            let gain = if pixel.index % 2 == 0 { gain * pixel.fraction } else { gain };
            source.at(pixel.index + 1) * gain
        } }",
    ] {
        let operator = workload::operator_program(&compile(source));
        assert!(in_query_block(&operator.program().bytecode, is_sin));
        for depth in [1, 2] {
            let prepare = |staged| {
                let operator = if staged {
                    operator.clone()
                } else {
                    unstaged(&operator)
                };
                playback::show(200, &sample, 1, &vec![operator; depth])
            };
            assert_same_frames(
                prepare(true),
                prepare(false),
                &format!("depth={depth} {source}"),
            );
        }
    }
}

#[test]
fn staged_nested_operators_track_sibling_parameters_and_shifted_reads() {
    let effect = playback::invoke(
        &compile("effect Source { sample { rgb(pixel.fraction, progress, 0.25) } }"),
        &[],
    );
    let gain = compile(
        "operator Gain { input source; param gain: float in 0.0..1.0 = 0.5;
            sample { source * (gain * progress) } }",
    );
    let mix = workload::operator_program(&compile(
        "operator Mix { input a; input b; sample {
            max(max(a, b), max(a.at(pixel.index + 1), a))
        } }",
    ));
    // The parameter's product with the query's progress runs once per query.
    let gain_program = workload::operator_program(&gain);
    assert!(in_query_block(&gain_program.program().bytecode, |op| {
        matches!(
            op,
            Instruction::Context {
                read: ContextRead::Progress,
                ..
            }
        )
    }));
    let siblings = [0.2, 0.9].map(|value| {
        playback::lower(
            &gain
                .invoke(
                    vec![Value::Float(value)],
                    automation(
                        0,
                        ramp(0.0, 1.0),
                        AutomationMapping::Float {
                            min: value,
                            max: value * 0.5,
                        },
                    ),
                )
                .unwrap(),
        )
    });
    let prepare = |staged| {
        let sample = playback::lower(&effect);
        let (sample, siblings, mix) = if staged {
            (sample, siblings.clone(), mix.clone())
        } else {
            (
                unstaged(&sample),
                siblings.each_ref().map(unstaged),
                unstaged(&mix),
            )
        };
        playback::build(200, playback::timing(8_000_000), |builder, target| {
            let effect = builder.clip(
                None,
                &sample,
                SampleTime::from_ticks(0),
                SampleDuration::from_ticks(8_000_000),
                target,
            );
            let layer = builder.layer(true, [effect]);
            let siblings = siblings
                .each_ref()
                .map(|invocation| builder.operator(invocation, |_| layer));
            let mixed = builder.operator(&mix, |input| siblings[input]);
            Some(mixed)
        })
    };
    assert_same_frames(prepare(true), prepare(false), "siblings");
}

#[test]
fn uniform_frames_match_single_pixels_when_seeking() {
    let effect = playback::invoke(
        &compile("effect Uniform { sample { rgb(progress, sin(time) * 0.5 + 0.5, 0.25) } }"),
        &[],
    );
    let invocation = playback::lower(&effect);
    assert!(!invocation.program().bytecode.uses_pixel_context());
    let identity = compile(playback::IDENTITY_SOURCE);
    for (layers, wrapped) in [1, 16]
        .into_iter()
        .flat_map(|layers| [false, true].map(|wrapped| (layers, wrapped)))
    {
        let mut show = workload::layered_show(200, &effect, layers);
        if wrapped {
            workload::apply_operator(&mut show, &identity, true);
        }
        assert_matches_single_pixels(
            show,
            200,
            &invocation,
            &format!("layers={layers} wrapped={wrapped}"),
        );
    }
}

#[test]
fn uniform_results_made_rows_keep_their_colors() {
    let effect = playback::invoke(
        &compile("effect Uniform { sample { rgb(progress, sin(time) * 0.5 + 0.5, 0.25) } }"),
        &[],
    );
    let identity = compile(playback::IDENTITY_SOURCE);
    for count in [1, STRIP - 1, STRIP, STRIP + 1, 2 * STRIP + 1] {
        for wrapped in [false, true] {
            let show = |reuse| {
                let mut show = workload::layered_show(count, &effect, 2);
                workload::set_uniform_upstream(&mut show, reuse);
                if wrapped {
                    workload::apply_operator(&mut show, &identity, reuse);
                }
                show.prepare()
            };
            let (mut uniform, mut rows) = (show(true).into_playback(), show(false).into_playback());
            for frame_index in [0, 31, 4, 0] {
                let time = playback::time(frame_index);
                let expected = uniform.evaluate(time).colors()[0];
                assert!(
                    uniform
                        .evaluate(time)
                        .colors()
                        .iter()
                        .all(|&color| color == expected)
                );
                assert_eq!(
                    rows.evaluate(time).colors(),
                    uniform.evaluate(time).colors(),
                    "count={count} wrapped={wrapped} frame={frame_index}"
                );
            }
        }
    }
}

#[test]
fn uniform_empty_gradient_samples_black_for_empty_and_nonempty_targets() {
    let effect = compile("effect Uniform { param colors: gradient; sample { colors[progress] } }");
    let invocation = playback::lower(
        &effect
            .invoke(
                vec![Value::Gradient(Gradient { stops: vec![] }.into())],
                Box::new([]),
            )
            .unwrap(),
    );
    assert!(!invocation.program().bytecode.uses_pixel_context());
    for empty in [false, true] {
        let show = playback::build(200, playback::timing(8_000_000), |builder, target| {
            let clip_target = if empty {
                builder.target(&TargetGeometry::new([], TargetScope::WholeTarget))
            } else {
                target
            };
            let effect = builder.clip(
                None,
                &invocation,
                SampleTime::from_ticks(0),
                SampleDuration::from_ticks(8_000_000),
                clip_target,
            );
            let layer = builder.layer(true, [effect]);
            Some(layer)
        });
        let output = frame(&mut show.into_playback(), playback::time(0));
        assert!(output.iter().all(|&byte| byte == 0));
    }
}

#[test]
fn mixed_pixel_and_time_expressions_match_single_pixels() {
    for source in [
        "effect Mixed { sample {
            let phase = sin(time) * 0.5 + 0.5;
            let tint = mix(hsv(phase, 0.8, 1.0), rgb(phase, progress, 0.4), 0.3);
            tint * pixel.fraction
        } }",
        "effect Mixed { sample {
            let phase = sin(time) * 0.5 + 0.5;
            let tint = if pixel.index % 2 == 0 { rgb(progress, phase, 0.25) } else { hsv(phase, 0.8, 1.0) };
            tint * pixel.fraction
        } }",
        "effect Mixed { sample {
            let x = pixel.fraction;
            let phase = sin(time * 7.0) * 0.5 + 0.5;
            rgb(x * phase, progress, phase)
        } }",
        "effect Mixed { param gain: float in 0.0..1.0 = 0.7; sample {
            let phase = sin(time) * 0.5 + 0.5;
            let gain = gain * progress;
            let gain = if pixel.index % 2 == 0 { gain * pixel.fraction } else { gain };
            rgb(gain, phase, pixel.fraction)
        } }",
        "effect Mixed { sample {
            let phase = sin(time) * 0.5 + 0.5;
            rgb(progress * pow(pixel.fraction, 3), phase, progress)
        } }",
        "effect Mixed { sample {
            let phase = sin(time) * 0.5 + 0.5;
            guard pixel.index >= 0 else rgb(1 % 0, 1 % 0, 1 % 0);
            let values = [phase, pixel.fraction, progress];
            rgb(values[pixel.index % 3], phase, progress)
        } }",
        "effect Mixed { sample {
            let phase = sin(time) * 0.5 + 0.5;
            rgb(max for i in 0..pixel.index % 5 { phase * i * 0.25 }, phase, progress)
        } }",
    ] {
        let effect = playback::invoke(&compile(source), &[]);
        let invocation = playback::lower(&effect);
        let bytecode = &invocation.program().bytecode;
        assert!(bytecode.uses_pixel_context());
        // The pixel-independent work runs once per query, before the body.
        assert!(in_query_block(bytecode, is_sin), "{source}");
        // Pixel counts straddle the strip boundaries.
        for (layers, count) in [1, 16]
            .into_iter()
            .flat_map(|layers| [1, STRIP - 1, STRIP, STRIP + 1, 2 * STRIP + 1].map(|count| (layers, count)))
        {
            assert_matches_single_pixels(
                workload::layered_show(count, &effect, layers),
                count,
                &invocation,
                &format!("layers={layers} count={count} {source}"),
            );
        }
    }
}

#[test]
fn early_exit_reductions_match_single_pixels() {
    // Pixels decide on different iterations, so later iterations run on
    // fragmented selections that a decision can split further.
    for source in [
        "effect Early { sample {
            let hit = any for j in 0..6 { rand(pixel.index * 7 + j + floor(time * 4.0) * 1000.0) < 0.3 };
            if hit { rgb(1.0, progress, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
        "effect Early { sample {
            let held = all for j in 0..6 { rand(pixel.index * 7 + j + floor(time * 4.0) * 1000.0) < 0.8 };
            if held { rgb(1.0, progress, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
        "effect Early { sample {
            let hit = any for j in 0..pixel.index % 7 { rand(pixel.index * 7 + j) < 0.3 };
            if hit { rgb(1.0, progress, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
        "effect Early { sample {
            let hit = any for j in 0..6 {
                guard rand(pixel.index * 13 + j) < 0.7;
                rand(pixel.index * 7 + j) < 0.3
            };
            if hit { rgb(1.0, progress, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
        "effect Early { sample {
            let hit = any for k in 0..3 {
                let key = rand(pixel.index * 3 + k);
                any for j in 0..int(key * 4.0) { rand(key + 1.0 + j) < 0.5 }
            };
            if hit { rgb(1.0, progress, 0.0) } else { rgb(0.0, 0.0, 1.0) }
        } }",
    ] {
        let effect = playback::invoke(&compile(source), &[]);
        let invocation = playback::lower(&effect);
        for count in [STRIP, 2 * STRIP + 1] {
            assert_matches_single_pixels(
                workload::layered_show(count, &effect, 1),
                count,
                &invocation,
                &format!("count={count} {source}"),
            );
        }
    }
}
