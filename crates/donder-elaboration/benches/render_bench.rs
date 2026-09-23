use camino::Utf8PathBuf;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use donder_elaboration::{
    PreparedSequenceOutput, PreparedSignalGraph, RenderedFrame, elaborate_sequence,
};
use donder_language::values::{Color, sample_time_from_frame};
use donder_project_io::load_package;
use std::hint::black_box;
use std::time::Duration;

#[allow(dead_code)]
#[path = "../../donder-language/benches/fixtures/mod.rs"]
mod effect_fixtures;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/generator_workload.rs"]
mod generator_workload;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/mark_workload.rs"]
mod mark_workload;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

const BENCHMARK_SEQUENCE_DOCUMENT: &str = "sequences/layer_test.sequence.donder";
const BENCHMARK_SEQUENCE_OBJECT: &str = "layer_test";
const PLAYBACK_START_FRAME: u32 = 8420;
const PLAYBACK_FRAME_COUNT: u32 = 60;

const SCENARIOS: [RenderScenario; 7] = [
    RenderScenario {
        frame: 8398,
        checksum: 0x8bb5_7d05_87a6_9ae8,
        active_effect_count: 15,
    },
    RenderScenario {
        frame: 8450,
        checksum: 0x5bee_7460_eba9_0468,
        active_effect_count: 30,
    },
    RenderScenario {
        frame: 8494,
        checksum: 0xadc5_9683_e46e_175f,
        active_effect_count: 32,
    },
    RenderScenario {
        frame: 8530,
        checksum: 0xfe52_aa76_b372_c103,
        active_effect_count: 3,
    },
    RenderScenario {
        frame: 9270,
        checksum: 0x520a_4dfc_5977_d97a,
        active_effect_count: 2,
    },
    RenderScenario {
        frame: 9504,
        checksum: 0x9dca_f48a_50e4_f8df,
        active_effect_count: 1,
    },
    RenderScenario {
        frame: 9650,
        checksum: 0x2319_7d72_88c3_6a09,
        active_effect_count: 2,
    },
];

#[derive(Clone, Copy)]
struct RenderScenario {
    frame: u32,
    checksum: u64,
    active_effect_count: usize,
}

fn bench_render(c: &mut Criterion) {
    pin_benchmark_thread();
    let session = load_package(&project_path())
        .expect("benchmark project should load")
        .session;
    let setup_id = &session.project.root.setup;
    let sequence_id = session
        .project
        .root
        .sequences
        .iter()
        .find(|id| {
            id.0.document().as_str() == BENCHMARK_SEQUENCE_DOCUMENT
                && id.0.object() == BENCHMARK_SEQUENCE_OBJECT
        })
        .expect("benchmark project should include the layer_test sequence");
    let renderer = elaborate_sequence(&session.project, setup_id, sequence_id)
        .expect("benchmark project should prepare");
    let output = PreparedSequenceOutput::prepare(&session.project, setup_id, sequence_id)
        .expect("benchmark controller output should prepare");
    assert_scenarios(&renderer);

    c.bench_function("prepare_starter", |b| {
        b.iter(|| {
            black_box(
                elaborate_sequence(
                    black_box(&session.project),
                    black_box(setup_id),
                    black_box(sequence_id),
                )
                .expect("benchmark project should prepare"),
            )
        });
    });

    let mut scenario_workspace = renderer.workspace();
    c.bench_function("render_representative_frames", |b| {
        b.iter(|| {
            for scenario in SCENARIOS {
                black_box(
                    renderer
                        .evaluate_frame_with_workspace(
                            black_box(scenario.frame),
                            &mut scenario_workspace,
                        )
                        .expect("benchmark frame should render"),
                );
            }
        });
    });

    let mut playback_workspace = renderer.workspace();
    c.bench_function("render_playback_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                black_box(
                    renderer
                        .evaluate_frame_with_workspace(black_box(frame), &mut playback_workspace)
                        .expect("benchmark playback frame should render"),
                );
            }
        });
    });

    c.bench_function("render_playback_dense_cold_60_frames", |b| {
        b.iter_batched(
            || renderer.workspace(),
            |mut workspace| {
                for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                    black_box(
                        renderer
                            .evaluate_frame_with_workspace(black_box(frame), &mut workspace)
                            .expect("benchmark cold dense playback frame should render"),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    let mut output_workspace = output.workspace();
    c.bench_function("controller_output_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                let sample_time = sample_time_from_frame(frame, output.frame_rate())
                    .expect("benchmark frame should fit the controller clock");
                black_box(
                    output
                        .sample_into(black_box(sample_time), &mut output_workspace)
                        .expect("benchmark controller frame should render"),
                );
            }
        });
    });
}

fn bench_mark_playback(c: &mut Criterion) {
    use donder_language::dsl::Identifier;
    use donder_language::effect::{BuiltinEffect, CurveSource, EffectParamValue, EffectRef};
    use donder_language::sequence::{MarkCollection, MarkCollectionKey};
    use donder_language::values::{Curve, CurvePoint, DonderDuration, DonderTime};
    use donder_runtime::native_effect::NativeSample;
    use donder_runtime::signal::PreparedEffectImplementation;
    use donder_runtime::values::SampleTime;
    pin_benchmark_thread();
    let source_project = load_package(&project_path()).unwrap().session.project;
    for (name, pulse) in [("pulse", true), ("chase", false)] {
        let mut project = source_project.clone();
        let id = project
            .root
            .sequences
            .iter()
            .find(|id| id.0.object() == "layer_test")
            .unwrap()
            .clone();
        let source = project.sequences.get_mut(&id).unwrap();
        let mut generator = source.effects[0].clone();
        let gradient = generator.param_overrides.get("gradient").unwrap().clone();
        let mark_key = MarkCollectionKey {
            name: "profile_beats".into(),
        };
        source.mark_collections = vec![MarkCollection {
            key: mark_key.clone(),
            name: "Profile beats".into(),
            display_color: source.layers[0].color,
            marks: (0..32)
                .map(|i| DonderTime(Duration::from_millis(2000 + i * 50)))
                .collect(),
        }];
        generator.definition = EffectRef::Builtin(if pulse {
            BuiltinEffect::MarkPulse
        } else {
            BuiltinEffect::MarkChase
        });
        generator.start = DonderTime(Duration::ZERO);
        generator.duration = DonderDuration(Duration::from_secs(8));
        generator.layer_id = source.layers[0].id.clone();
        generator.param_overrides.clear();
        let ramp = EffectParamValue::Curve(CurveSource::Inline(Curve {
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
        }));
        let mut values = vec![
            ("beats", EffectParamValue::Marks(mark_key)),
            ("hue", ramp.clone()),
        ];
        if pulse {
            values.extend([
                ("accent", gradient),
                ("decay_seconds", EffectParamValue::Float(1.2)),
            ]);
        } else {
            values.extend([
                ("gradients", EffectParamValue::Array(vec![gradient])),
                (
                    "chase_positions",
                    EffectParamValue::Array(vec![ramp.clone()]),
                ),
                ("pulse_shape", ramp),
                ("chase_seconds", EffectParamValue::Float(1.2)),
            ]);
        }
        generator.param_overrides.extend(
            values
                .into_iter()
                .map(|(name, value)| (Identifier::new(name.into()).unwrap(), value)),
        );
        source.effects = vec![generator];
        source.automation_clips.clear();
        let sequence = elaborate_sequence(&project, &project.root.setup, &id).unwrap();
        let effect = sequence.effects.iter().find(|effect| matches!(
            &effect.implementation,
            PreparedEffectImplementation::Native { sample, .. }
                if matches!((sample, pulse), (NativeSample::MarkPulseChild(_), true) | (NativeSample::MarkChaseChild(_), false))
        )).expect("constructed fixture must contain the intended generated mark children");
        assert!(effect.duration.as_ticks() > 0);
        assert!(
            sequence.effects.len() >= 32,
            "marks must expand into actual children"
        );
        let start = 3_000_000;
        let mut workspace = sequence.workspace();
        let black = Color {
            red: 0,
            green: 0,
            blue: 0,
        };
        for frame in [0, 15, 3, 0] {
            let time = SampleTime::from_ticks(start + frame * 8333);
            let colors = sequence.evaluate(time, &mut workspace).unwrap();
            let mut fresh = sequence.workspace();
            let expected = sequence.evaluate(time, &mut fresh).unwrap();
            assert_eq!(colors, expected);
            assert!(
                colors.iter().any(|&color| color != black),
                "mark window must produce light"
            );
        }
        let mut frame = 0;
        c.bench_function(&format!("prepared_marks/{name}"), |b| {
            b.iter(|| {
                frame = (frame + 1) % 16;
                let colors = sequence
                    .evaluate(
                        black_box(SampleTime::from_ticks(start + frame * 8333)),
                        &mut workspace,
                    )
                    .unwrap();
                black_box(colors);
            })
        });
    }
}

fn bench_chase_pulse(c: &mut Criterion) {
    pin_benchmark_thread();
    let (name, source, params) = effect_fixtures::layer_cases().into_iter().nth(1).unwrap();
    let (effect, _) = effect_fixtures::prepared_effect(name, source, params);
    let cases = [1, 4, 16]
        .into_iter()
        .map(|layers| {
            (
                format!("prepared_chase_pulse/{layers}"),
                workload::chase_pulse_show(200, layers, effect.bytecode.clone()),
            )
        })
        .chain([
            (
                "prepared_device_marks/pulse".into(),
                mark_workload::mark_show(200, true, 0.0, effect.bytecode.clone()),
            ),
            (
                "prepared_device_marks/chase".into(),
                mark_workload::mark_show(200, false, 0.0, effect.bytecode.clone()),
            ),
            (
                "prepared_device_marks/pulse_edge".into(),
                mark_workload::mark_show(200, true, 1.0, effect.bytecode.clone()),
            ),
        ]);
    for (name, show) in cases {
        let mut workspace = show.workspace();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            show.evaluate(workload::time(frame), &mut output, &mut workspace)
                .unwrap();
            show.evaluate(workload::time(frame), &mut expected, &mut show.workspace())
                .unwrap();
            assert_eq!(output, expected);
            assert!(output[0].iter().any(|&byte| byte != 0));
        }
        let mut frame = 0;
        c.bench_function(&name, |b| {
            b.iter(|| {
                frame = (frame + 1) % workload::FRAMES;
                show.evaluate(
                    black_box(workload::time(frame)),
                    &mut output,
                    &mut workspace,
                )
                .unwrap();
                black_box(&output);
            })
        });
    }
}

fn bench_generator_bindings(c: &mut Criterion) {
    pin_benchmark_thread();
    for (case, name) in generator_workload::CASES {
        for count in [200, 800] {
            for (mode, generator, automated) in [
                ("ordinary", false, true),
                ("live", true, true),
                ("constant", true, false),
            ] {
                let show = generator_workload::show(count, case, generator, automated);
                let reference = generator_workload::show(count, case, false, automated);
                let mut workspace = show.workspace();
                let mut reference_workspace = reference.workspace();
                let mut output = [vec![0u8; count * 3]];
                let mut expected = output.clone();
                for frame in 0..workload::FRAMES {
                    show.evaluate(workload::time(frame), &mut output, &mut workspace)
                        .unwrap();
                    reference
                        .evaluate(
                            workload::time(frame),
                            &mut expected,
                            &mut reference_workspace,
                        )
                        .unwrap();
                    assert_eq!(output, expected, "{name}/{mode}/{count}/{frame}");
                }
                let mut frame = 0;
                c.bench_function(&format!("generator_bindings/{name}/{mode}/{count}"), |b| {
                    b.iter(|| {
                        frame = (frame + 1) % workload::FRAMES;
                        show.evaluate(
                            black_box(workload::time(frame)),
                            &mut output,
                            &mut workspace,
                        )
                        .unwrap();
                        black_box(&output);
                    })
                });
            }
        }
    }
}

fn bench_layers(c: &mut Criterion) {
    pin_benchmark_thread();
    for (name, source, params) in effect_fixtures::layer_cases() {
        let (effect, bound) = effect_fixtures::prepared_effect(name, source, params);
        for layers in [1, 4, 16] {
            let show = workload::layered_show(200, effect.bytecode.clone(), bound.clone(), layers);
            let mut workspace = show.workspace();
            let mut output = [vec![0; 600]];
            let mut frame = 0;
            c.bench_function(&format!("prepared_layers/{name}/{layers}"), |b| {
                b.iter(|| {
                    frame = (frame + 1) % workload::FRAMES;
                    show.evaluate(
                        black_box(workload::time(frame)),
                        &mut output,
                        &mut workspace,
                    )
                    .unwrap();
                    black_box(&output);
                })
            });
        }
    }
}

fn bench_operators(c: &mut Criterion) {
    pin_benchmark_thread();
    let (name, source, params) = effect_fixtures::layer_cases().into_iter().nth(1).unwrap();
    let (effect, bound) = effect_fixtures::prepared_effect(name, source, params);
    for (group, modes) in [
        (
            "prepared_operator",
            [
                ("full", workload::OPERATOR_SOURCE, false),
                ("reuse", workload::OPERATOR_SOURCE, true),
            ],
        ),
        (
            "prepared_temporal",
            [
                ("grouped", workload::GROUPED_SOURCE, true),
                ("alternating", workload::ALTERNATING_SOURCE, true),
            ],
        ),
    ] {
        for count in workload::COUNTS {
            let mut expected = None;
            for (mode, source, reuse) in modes {
                let operator = donder_language::dsl::compile_operators(source)
                    .unwrap()
                    .remove(0);
                let mut show = workload::show(count, effect.bytecode.clone(), bound.clone());
                workload::apply_operator(&mut show, operator.bytecode.clone(), reuse);
                let mut workspace = show.workspace();
                let mut output = [vec![0; count * 3]];
                let mut checksums = Vec::new();
                for frame in 0..workload::FRAMES {
                    show.evaluate(workload::time(frame), &mut output, &mut workspace)
                        .unwrap();
                    checksums.push(workload::checksum(&output[0]));
                }
                if let Some(expected) = &expected {
                    assert_eq!(&checksums, expected);
                } else {
                    expected = Some(checksums);
                }
                let mut frame = 0;
                c.bench_function(&format!("{group}/{mode}/{count}"), |b| {
                    b.iter(|| {
                        frame = (frame + 1) % workload::FRAMES;
                        show.evaluate(
                            black_box(workload::time(frame)),
                            &mut output,
                            &mut workspace,
                        )
                        .unwrap();
                        black_box(&output);
                    })
                });
            }
        }
    }
}

fn bench_uniform_resources(c: &mut Criterion) {
    pin_benchmark_thread();
    let (effect, params) = effect_fixtures::uniform_resources();
    let mut expected = None;
    for (name, reuse) in [("full", false), ("reuse", true)] {
        let mut program = effect.bytecode.clone();
        if !reuse {
            program.pixel_entry = 0;
        }
        let show = workload::show(200, program, params.clone());
        let mut workspace = show.workspace();
        let mut output = [vec![0; 600]];
        let checksums = (0..workload::FRAMES)
            .map(|frame| {
                show.evaluate(workload::time(frame), &mut output, &mut workspace)
                    .unwrap();
                assert!(output[0].iter().any(|&byte| byte != 0));
                workload::checksum(&output[0])
            })
            .collect::<Vec<_>>();
        if let Some(expected) = &expected {
            assert_eq!(&checksums, expected);
        } else {
            expected = Some(checksums);
        }
        let mut frame = 0;
        c.bench_function(&format!("prepared_uniform_resources/{name}"), |b| {
            b.iter(|| {
                frame = (frame + 1) % workload::FRAMES;
                show.evaluate(
                    black_box(workload::time(frame)),
                    &mut output,
                    &mut workspace,
                )
                .unwrap();
                black_box(&output);
            })
        });
    }
}

fn bench_uniform_upstream(c: &mut Criterion) {
    pin_benchmark_thread();
    let (name, source, params) = effect_fixtures::layer_cases().into_iter().next().unwrap();
    let (effect, bound) = effect_fixtures::prepared_effect(name, source, params);
    assert!(!effect.bytecode.uses_pixel_context);
    let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    for count in [200, 1600] {
        let mut expected = None;
        for reuse in [false, true] {
            let mut program = effect.bytecode.clone();
            // Conservative dependency metadata forces recomputation of the same
            // bytecode, giving an exact-output control for uniform-result reuse.
            program.uses_pixel_context = !reuse;
            let mut show = workload::show(count, program, bound.clone());
            workload::apply_operator(&mut show, operator.bytecode.clone(), true);
            let mut workspace = show.workspace();
            let mut output = [vec![0; count * 3]];
            let mut checksums = Vec::new();
            for frame in 0..workload::FRAMES {
                show.evaluate(workload::time(frame), &mut output, &mut workspace)
                    .unwrap();
                checksums.push(workload::checksum(&output[0]));
            }
            if let Some(expected) = &expected {
                assert_eq!(&checksums, expected);
            } else {
                expected = Some(checksums);
            }
            let mode = if reuse { "reuse" } else { "full" };
            let mut frame = 0;
            c.bench_function(&format!("prepared_uniform_upstream/{mode}/{count}"), |b| {
                b.iter(|| {
                    frame = (frame + 1) % workload::FRAMES;
                    show.evaluate(
                        black_box(workload::time(frame)),
                        &mut output,
                        &mut workspace,
                    )
                    .unwrap();
                    black_box(&output);
                })
            });
        }
    }
}

fn bench_gamma(c: &mut Criterion) {
    pin_benchmark_thread();
    let (name, source, params) = effect_fixtures::layer_cases().into_iter().nth(1).unwrap();
    let (effect, bound) = effect_fixtures::prepared_effect(name, source, params);
    for count in workload::COUNTS {
        let mut show = workload::layered_show(count, effect.bytecode.clone(), bound.clone(), 1);
        workload::apply_gamma(&mut show, workload::gamma_lookup());
        let mut workspace = show.workspace();
        let mut output = [vec![0; count * 3]];
        let mut frame = 0;
        c.bench_function(&format!("prepared_gamma/lookup/{count}"), |b| {
            b.iter(|| {
                frame = (frame + 1) % workload::FRAMES;
                show.evaluate(
                    black_box(workload::time(frame)),
                    &mut output,
                    &mut workspace,
                )
                .unwrap();
                black_box(&output);
            })
        });
    }
}

#[cfg(windows)]
fn pin_benchmark_thread() {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadAffinityMask(thread: *mut c_void, affinity_mask: usize) -> usize;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    // Logical CPU 0 commonly handles extra OS work. A fixed nonzero CPU also prevents
    // migrations between unlike cores; smaller systems fall back to their final CPU.
    let cpu = std::thread::available_parallelism()
        .map(|count| 2.min(count.get().saturating_sub(1)))
        .unwrap_or(0);
    let thread = unsafe { GetCurrentThread() };
    let previous = unsafe { SetThreadAffinityMask(thread, 1usize << cpu) };
    assert_ne!(previous, 0, "benchmark thread affinity should be set");
    assert_ne!(
        unsafe { SetThreadPriority(thread, 2) },
        0,
        "benchmark thread priority should be raised"
    );
}

#[cfg(not(windows))]
fn pin_benchmark_thread() {}

fn assert_scenarios(renderer: &PreparedSignalGraph) {
    for scenario in SCENARIOS {
        let rendered = renderer
            .evaluate_frame(scenario.frame)
            .expect("benchmark frame should render");
        assert_eq!(checksum_frame(&rendered), scenario.checksum);
        assert_eq!(
            renderer.active_effect_count_at_frame(scenario.frame),
            scenario.active_effect_count
        );
    }
}

fn project_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter")
}

fn checksum_frame(frame: &RenderedFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, u64::from(frame.frame_index));
    for element in &frame.fixtures {
        hash = checksum_u32(hash, element.fixture_id);
        hash = checksum_colors_with_seed(hash, &element.pixels);
    }
    hash
}

fn checksum_colors_with_seed(hash: u64, colors: &[Color]) -> u64 {
    colors
        .iter()
        .fold(hash, |hash, color| checksum_color(hash, *color))
}

fn checksum_color(hash: u64, color: Color) -> u64 {
    [color.red, color.green, color.blue]
        .into_iter()
        .fold(hash, checksum_u8)
}

fn checksum_u64(hash: u64, value: u64) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u32(hash: u64, value: u32) -> u64 {
    value.to_le_bytes().into_iter().fold(hash, checksum_u8)
}

fn checksum_u8(hash: u64, value: u8) -> u64 {
    (hash ^ u64::from(value)).wrapping_mul(0x0000_0100_0000_01b3)
}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(5))
        .noise_threshold(0.05)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_render, bench_layers, bench_gamma, bench_operators, bench_chase_pulse, bench_mark_playback, bench_uniform_resources, bench_uniform_upstream, bench_generator_bindings
}
criterion_main!(benches);
