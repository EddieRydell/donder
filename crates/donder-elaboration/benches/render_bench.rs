use camino::Utf8PathBuf;
use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::model::{DonderProject, ProjectEdit};
use donder_language::values::{Color, sample_time_from_frame};
use donder_project_io::load_project;
use donder_runtime::{PreparedSequence, SequenceFrame};
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
        checksum: 0x07ec_1fb5_19f7_8b83,
        active_effect_count: 3,
    },
    RenderScenario {
        frame: 9270,
        checksum: 0xac57_56bd_f3ff_27f8,
        active_effect_count: 2,
    },
    RenderScenario {
        frame: 9504,
        checksum: 0x0ab2_eca9_b6ce_8207,
        active_effect_count: 1,
    },
    RenderScenario {
        frame: 9650,
        checksum: 0xfaf7_582c_96c9_730f,
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
    let session = load_project(&project_path()).expect("benchmark project should load");
    let sequence_id = session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| {
            id.0.document().as_str() == BENCHMARK_SEQUENCE_DOCUMENT
                && id.0.root_source().object() == BENCHMARK_SEQUENCE_OBJECT
        })
        .expect("benchmark project should include the layer_test sequence");
    let output = prepare(&session.project, sequence_id, PrepareOutputs::All)
        .expect("benchmark controller output should prepare");
    let logical_project = render_only_project(&session.project);
    let renderer = prepare(&logical_project, sequence_id, PrepareOutputs::All)
        .expect("benchmark logical fixtures should prepare");
    assert!(renderer.outputs().is_empty());
    assert!(
        renderer
            .fixtures()
            .iter()
            .map(|fixture| (fixture.id, fixture.pixel_count))
            .eq(output
                .fixtures()
                .iter()
                .map(|fixture| (fixture.id, fixture.pixel_count))),
        "render-only preparation must retain every logical fixture"
    );
    assert_scenarios(&renderer);
    let frame_rate = output.frame_rate();

    c.bench_function("prepare_starter", |b| {
        b.iter(|| {
            black_box(
                prepare(
                    black_box(&session.project),
                    black_box(sequence_id),
                    PrepareOutputs::All,
                )
                .expect("benchmark project should prepare"),
            )
        });
    });

    let mut scenario_workspace = renderer.clone().into_playback();
    c.bench_function("render_representative_frames", |b| {
        b.iter(|| {
            for scenario in SCENARIOS {
                black_box(scenario_workspace.evaluate(
                    sample_time_from_frame(black_box(scenario.frame), frame_rate).unwrap(),
                ));
            }
        });
    });

    let mut playback_workspace = renderer.clone().into_playback();
    c.bench_function("render_playback_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                black_box(
                    playback_workspace
                        .evaluate(sample_time_from_frame(black_box(frame), frame_rate).unwrap()),
                );
            }
        });
    });

    c.bench_function("render_playback_dense_cold_60_frames", |b| {
        b.iter_batched(
            || renderer.clone().into_playback(),
            |mut workspace| {
                for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                    black_box(
                        workspace.evaluate(
                            sample_time_from_frame(black_box(frame), frame_rate).unwrap(),
                        ),
                    );
                }
            },
            BatchSize::SmallInput,
        );
    });

    let mut output_workspace = output.into_playback();
    c.bench_function("controller_output_dense_60_frames", |b| {
        b.iter(|| {
            for frame in PLAYBACK_START_FRAME..PLAYBACK_START_FRAME + PLAYBACK_FRAME_COUNT {
                let sample_time = sample_time_from_frame(frame, frame_rate)
                    .expect("benchmark frame should fit the controller clock");
                black_box(output_workspace.evaluate(black_box(sample_time)));
            }
        });
    });
}

fn bench_mark_playback(c: &mut Criterion) {
    use donder_language::dsl::Identifier;
    use donder_language::effect::{CurveSource, EffectParamValue, EffectRef};
    use donder_language::sequence::{MarkCollection, MarkCollectionKey};
    use donder_language::values::{Curve, CurvePoint, DonderDuration, DonderTime};
    use donder_runtime::SampleTime;
    pin_benchmark_thread();
    let source_project = render_only_project(&load_project(&project_path()).unwrap().project);
    for (name, pulse) in [("pulse", true), ("chase", false)] {
        let mut project = source_project.clone();
        let id = project
            .root()
            .sequences
            .iter()
            .map(|source| source.id())
            .find(|id| id.0.root_source().object() == "layer_test")
            .unwrap()
            .clone();
        let mut source = project.sequence(&id).unwrap().clone();
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
        let effect_name = if pulse { "MarkPulse" } else { "MarkChase" };
        let definition_id = project
            .definitions()
            .effects
            .definitions
            .iter()
            .find(|(_, definition)| definition.source_name == effect_name)
            .expect("standard mark effect must be imported")
            .0
            .clone();
        generator.definition = EffectRef::Custom(definition_id);
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
        let mut values = vec![("beats", EffectParamValue::Marks(mark_key))];
        let falloff = EffectParamValue::Curve(CurveSource::Inline(Curve {
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
        }));
        if pulse {
            values.extend([
                ("accent", gradient),
                ("pulse_shape", falloff),
                ("decay_seconds", EffectParamValue::Float(1.2)),
            ]);
        } else {
            values.extend([
                ("gradients", EffectParamValue::Array(vec![gradient])),
                (
                    "chase_positions",
                    EffectParamValue::Array(vec![ramp.clone()]),
                ),
                ("pulse_shape", falloff),
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
        project.replace_sequence(&id, source).unwrap();
        let prepared = prepare(&project, &id, PrepareOutputs::All).unwrap();
        assert!(prepared.outputs().is_empty());
        let (_, duration) = prepared
            .effect_windows()
            .next()
            .expect("constructed fixture must contain DSL mark children");
        assert!(duration.as_ticks() > 0);
        assert!(
            prepared.effect_count() >= 32,
            "marks must expand into actual children"
        );
        let start = 3_000_000;
        let mut workspace = prepared.clone().into_playback();
        let black = Color {
            red: 0,
            green: 0,
            blue: 0,
        };
        for frame in [0, 15, 3, 0] {
            let time = SampleTime::from_ticks(start + frame * 8333);
            let colors = workspace.evaluate(time).colors();
            let mut fresh = prepared.clone().into_playback();
            let expected = fresh.evaluate(time).colors();
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
                let colors = workspace
                    .evaluate(black_box(SampleTime::from_ticks(start + frame * 8333)))
                    .colors();
                black_box(colors);
            })
        });
    }
}

fn bench_chase_pulse(c: &mut Criterion) {
    pin_benchmark_thread();
    let cases = [1, 4, 16]
        .into_iter()
        .map(|layers| {
            (
                format!("prepared_chase_pulse/{layers}"),
                workload::chase_pulse_show(200, layers),
            )
        })
        .chain([
            (
                "prepared_device_marks/pulse".into(),
                mark_workload::mark_show(200, true),
            ),
            (
                "prepared_device_marks/chase".into(),
                mark_workload::mark_show(200, false),
            ),
        ]);
    for (name, show) in cases {
        let mut workspace = show.clone().prepare().into_playback();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            for (snapshot, output) in expected.iter_mut().zip(
                show.clone()
                    .prepare()
                    .into_playback()
                    .evaluate(workload::time(frame))
                    .outputs(),
            ) {
                snapshot.copy_from_slice(output.bytes);
            }
            assert_eq!(output, expected);
            assert!(output[0].iter().any(|&byte| byte != 0));
        }
        let mut frame = 0;
        c.bench_function(&name, |b| {
            b.iter(|| {
                frame = (frame + 1) % workload::FRAMES;
                black_box(workspace.evaluate(black_box(workload::time(frame))));
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
                let mut workspace = show.clone().prepare().into_playback();
                let mut reference_workspace = reference.clone().prepare().into_playback();
                let mut output = [vec![0u8; count * 3]];
                let mut expected = output.clone();
                for frame in 0..workload::FRAMES {
                    for (snapshot, output) in output
                        .iter_mut()
                        .zip(workspace.evaluate(workload::time(frame)).outputs())
                    {
                        snapshot.copy_from_slice(output.bytes);
                    }
                    for (snapshot, output) in expected.iter_mut().zip(
                        reference_workspace
                            .evaluate(workload::time(frame))
                            .outputs(),
                    ) {
                        snapshot.copy_from_slice(output.bytes);
                    }
                    assert_eq!(output, expected, "{name}/{mode}/{count}/{frame}");
                }
                let mut frame = 0;
                c.bench_function(&format!("generator_bindings/{name}/{mode}/{count}"), |b| {
                    b.iter(|| {
                        frame = (frame + 1) % workload::FRAMES;
                        black_box(workspace.evaluate(black_box(workload::time(frame))));
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
            let show = workload::layered_show(
                200,
                effect.sample_program().unwrap().clone(),
                bound.clone(),
                layers,
            );
            let mut workspace = show.clone().prepare().into_playback();
            let mut frame = 0;
            c.bench_function(&format!("prepared_layers/{name}/{layers}"), |b| {
                b.iter(|| {
                    frame = (frame + 1) % workload::FRAMES;
                    black_box(workspace.evaluate(black_box(workload::time(frame))));
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
                let mut show = workload::show(
                    count,
                    effect.sample_program().unwrap().clone(),
                    bound.clone(),
                );
                workload::apply_operator(
                    &mut show,
                    operator.program().clone().into_parts().0,
                    reuse,
                );
                let mut workspace = show.clone().prepare().into_playback();
                let mut output = [vec![0; count * 3]];
                let mut checksums = Vec::new();
                for frame in 0..workload::FRAMES {
                    for (snapshot, output) in output
                        .iter_mut()
                        .zip(workspace.evaluate(workload::time(frame)).outputs())
                    {
                        snapshot.copy_from_slice(output.bytes);
                    }
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
                        black_box(workspace.evaluate(black_box(workload::time(frame))));
                    })
                });
            }
        }
    }

    let standard = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap();
    let echo = standard
        .iter()
        .find(|operator| operator.name().as_str() == "Echo")
        .unwrap();
    let invert = standard
        .iter()
        .find(|operator| operator.name().as_str() == "Invert")
        .unwrap();
    for count in workload::COUNTS {
        for (name, nested) in [
            ("standard_echo", false),
            ("standard_echo_nested_invert", true),
        ] {
            let mut show = workload::chase_pulse_show(count, 4);
            workload::apply_compiled_operator(&mut show, echo.clone());
            if nested {
                // Layer -> Echo -> Invert -> Echo mixes scalar temporal requests
                // with a uniform upstream query that must not promote back to frames.
                workload::insert_invert(&mut show, invert.program().clone().into_parts().0);
            }
            let mut workspace = show.clone().prepare().into_playback();
            let mut output = [vec![0; count * 3]];
            let mut fresh_output = [vec![0; count * 3]];
            let mut any_lit = false;
            for frame in (0..workload::FRAMES).chain([12, 0, workload::FRAMES - 1]) {
                let time = workload::time(frame);
                for (snapshot, output) in output.iter_mut().zip(workspace.evaluate(time).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
                for (snapshot, output) in fresh_output.iter_mut().zip(
                    show.clone()
                        .prepare()
                        .into_playback()
                        .evaluate(time)
                        .outputs(),
                ) {
                    snapshot.copy_from_slice(output.bytes);
                }
                assert_eq!(
                    workload::checksum(&output[0]),
                    workload::checksum(&fresh_output[0]),
                    "{name}/{count} frame {frame}: reused workspace differs from fresh"
                );
                any_lit |= output[0].iter().any(|&byte| byte != 0);
            }
            assert!(any_lit, "{name}/{count} produced only black frames");
            let mut frame = 0;
            c.bench_function(&format!("prepared_temporal/{name}/{count}"), |b| {
                b.iter(|| {
                    frame = (frame + 1) % workload::FRAMES;
                    black_box(workspace.evaluate(black_box(workload::time(frame))));
                })
            });
        }
    }
}

fn bench_uniform_resources(c: &mut Criterion) {
    pin_benchmark_thread();
    let (effect, params) = effect_fixtures::uniform_resources();
    let mut expected = None;
    for (name, reuse) in [("full", false), ("reuse", true)] {
        let (mut program, types) = effect.sample_program().unwrap().clone().into_parts();
        if !reuse {
            program.pixel_entry = 0;
        }
        let program = donder_runtime::SampleProgram::admit(program, types).unwrap();
        let show = workload::show(200, program, params.clone());
        let mut workspace = show.clone().prepare().into_playback();
        let mut output = [vec![0; 600]];
        let checksums = (0..workload::FRAMES)
            .map(|frame| {
                for (snapshot, output) in output
                    .iter_mut()
                    .zip(workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
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
                black_box(workspace.evaluate(black_box(workload::time(frame))));
            })
        });
    }
}

fn bench_uniform_upstream(c: &mut Criterion) {
    pin_benchmark_thread();
    let (name, source, params) = effect_fixtures::layer_cases().into_iter().next().unwrap();
    let (effect, bound) = effect_fixtures::prepared_effect(name, source, params);
    assert!(
        !effect
            .sample_program()
            .unwrap()
            .bytecode()
            .uses_pixel_context
    );
    let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    for count in [200, 1600] {
        let mut expected = None;
        for reuse in [false, true] {
            let mut show = workload::show(
                count,
                effect.sample_program().unwrap().clone(),
                bound.clone(),
            );
            workload::set_uniform_upstream(&mut show, reuse);
            workload::apply_operator(&mut show, operator.program().clone().into_parts().0, true);
            let mut workspace = show.clone().prepare().into_playback();
            let mut output = [vec![0; count * 3]];
            let mut checksums = Vec::new();
            for frame in 0..workload::FRAMES {
                for (snapshot, output) in output
                    .iter_mut()
                    .zip(workspace.evaluate(workload::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
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
                    black_box(workspace.evaluate(black_box(workload::time(frame))));
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
        let mut show = workload::layered_show(
            count,
            effect.sample_program().unwrap().clone(),
            bound.clone(),
            1,
        );
        workload::apply_gamma(&mut show, workload::gamma_lookup());
        let mut workspace = show.clone().prepare().into_playback();
        let mut frame = 0;
        c.bench_function(&format!("prepared_gamma/lookup/{count}"), |b| {
            b.iter(|| {
                frame = (frame + 1) % workload::FRAMES;
                black_box(workspace.evaluate(black_box(workload::time(frame))));
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

fn assert_scenarios(sequence: &PreparedSequence) {
    let mut playback = sequence.clone().into_playback();
    for scenario in SCENARIOS {
        let time = sample_time_from_frame(scenario.frame, sequence.frame_rate()).unwrap();
        let rendered = playback.evaluate(time);
        assert_eq!(checksum_frame(scenario.frame, &rendered), scenario.checksum);
        assert_eq!(
            sequence.active_effect_count(time),
            scenario.active_effect_count
        );
    }
}

// All logical fixtures remain selected while normal playback has no bytes to pack.
fn render_only_project(source: &DonderProject) -> DonderProject {
    let mut project = source.clone();
    let mut setup = project.setup(project.root().setup.id()).unwrap().clone();
    let mut patch = project.patch(setup.patch.id()).unwrap().clone();
    patch.routes.clear();
    setup.controllers.clear();
    project
        .apply_edits([
            ProjectEdit::ReplacePatch {
                id: patch.id.clone(),
                value: patch,
            },
            ProjectEdit::ReplaceSetup {
                id: setup.id.clone(),
                value: setup,
            },
        ])
        .expect("render-only fixture should retain a valid typed project");
    project
}

fn project_path() -> Utf8PathBuf {
    Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter")
}

fn checksum_frame(frame_index: u32, frame: &SequenceFrame<'_>) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    hash = checksum_u64(hash, u64::from(frame_index));
    for element in frame.fixtures() {
        hash = checksum_u32(hash, element.fixture_id);
        hash = checksum_colors_with_seed(hash, element.pixels);
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
