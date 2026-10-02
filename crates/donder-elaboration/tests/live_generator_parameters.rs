mod support;

const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, PreparedSequence, prepare as prepare_sequence};
use donder_language::dsl::{Identifier, RunContext, VmWorkspace, compile_effects};
use donder_language::effect::EffectRef;
use donder_language::sequence::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationMapping, AutomationTarget,
};
use donder_language::values::{
    Curve, CurvePoint, DonderDuration, DonderTime, SampleDuration, SampleTime,
};
use donder_runtime::{LoadLimits, decode_sequence, encode_sequence};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

fn prepare(source: &str, automated: bool) -> PreparedSequence {
    prepare_query(source, automated, None)
}

fn prepare_query(source: &str, automated: bool, query: Option<(&str, bool)>) -> PreparedSequence {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut sources = donder_project_io::project_source_texts(&root).unwrap();
    sources.insert(
        "effects/mark-impact-burst.effect.donder".into(),
        source.into(),
    );
    let report = donder_project_io::check_project_with_overrides(&root, &sources);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let mut session = report.session.unwrap();
    let generator = session
        .project
        .definitions()
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .clone();
    let declarations = session
        .project
        .definitions()
        .effects
        .get(&generator)
        .unwrap()
        .params()
        .to_vec();
    let mut sequence = session
        .project
        .sequences()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap()
        .clone();
    sequence.effects.truncate(1);
    sequence.automation_clips.clear();
    let effect = &mut sequence.effects[0];
    effect.definition = EffectRef::Custom(generator);
    effect.param_overrides.clear();
    for param in declarations.iter().filter(|param| param.default.is_none()) {
        use donder_language::effect::{CurveSource, EffectParamValue, GradientSource};
        let value = match param.ty {
            donder_language::dsl::Type::Marks => {
                sequence.mark_collections[0].marks = vec![
                    DonderTime(Duration::from_secs(1)),
                    DonderTime(Duration::from_millis(1100)),
                ];
                EffectParamValue::Marks(sequence.mark_collections[0].key.clone())
            }
            donder_language::dsl::Type::Curve => {
                EffectParamValue::Curve(CurveSource::Inline(Curve {
                    points: vec![
                        CurvePoint {
                            position: 0.0,
                            value: 0.2,
                        },
                        CurvePoint {
                            position: 1.0,
                            value: 0.8,
                        },
                    ],
                }))
            }
            donder_language::dsl::Type::Gradient => EffectParamValue::Gradient(
                GradientSource::Inline(donder_language::values::Gradient {
                    stops: vec![donder_language::values::GradientStop {
                        position: 0.0,
                        color: donder_runtime::hsv(0.3, 1.0, 1.0),
                    }],
                }),
            ),
            _ => panic!("unsupported fixture resource"),
        };
        effect.param_overrides.insert(param.name.clone(), value);
    }
    effect.start = DonderTime(Duration::from_secs(1));
    effect.duration = DonderDuration(Duration::from_secs(1));
    if automated {
        sequence.automation_clips.push(AutomationClip {
            id: AutomationClipId(900),
            start: effect.start.clone(),
            duration: effect.duration.clone(),
            row_target: effect.target.clone(),
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
            },
            bindings: vec![AutomationBinding {
                target: AutomationTarget::EffectParam {
                    effect_id: effect.id.clone(),
                    param: Identifier::new("level".into()).unwrap(),
                },
                mapping: AutomationMapping::Float { min: 0.0, max: 1.0 },
            }],
            detached_bindings: vec![],
        });
    }
    let id = sequence.id.clone();
    session.project.replace_sequence(&id, sequence).unwrap();
    if let Some((expression, cached)) = query {
        support::append_operator(
            &mut session.project,
            &id,
            query_operator(expression, cached),
        );
    }
    prepare_sequence(&session.project, &id, PrepareOutputs::All).unwrap()
}

#[test]
fn generated_children_outside_the_sequence_are_omitted_before_wire_admission() {
    let prepared = prepare(
        "effect MarkImpactBurst { void generate() {
            timeline.emit Leaf { start: 0.0, duration: 1.0, target: target };
            timeline.emit Leaf { start: 1000.0, duration: 1.0, target: target };
        } }
        effect Leaf { color sample() { return #ff0000; } }",
        false,
    );
    let valid_only = prepare(
        "effect MarkImpactBurst { void generate() {
            timeline.emit Leaf { start: 0.0, duration: 1.0, target: target };
        } }
        effect Leaf { color sample() { return #ff0000; } }",
        false,
    );
    assert!(prepared.effect_count() > 0);
    assert_eq!(prepared.effect_count(), valid_only.effect_count());
    let bytes = encode_sequence(&prepared).unwrap();
    assert!(decode_sequence(&bytes, LoadLimits::default()).is_ok());
}

#[test]
fn nested_expressions_keep_parent_clocks_after_parent_lifetimes_and_across_wire_roundtrips() {
    let prepared = prepare(
        r#"
        effect MarkImpactBurst {
            param float level = 0.2;
            void generate() {
                float outer = seconds() / 4.0 + level * 0.25;
                timeline.emit Inner { start: 0.25, duration: 0.5, target: target, value: outer };
            }
        }
        effect Inner {
            param float value;
            void generate() {
                timeline.emit Leaf { start: 0.25, duration: 2.0, target: target, value: value + seconds() / 4.0 };
            }
        }
        effect Leaf {
            param float value;
            color sample() { return rgb(value, seconds() / 4.0, progress()); }
        }
    "#,
        true,
    );
    let bytes = encode_sequence(&prepared).unwrap();
    let decoded = decode_sequence(
        &bytes,
        LoadLimits {
            payload_bytes: bytes.len(),
            workspace_bytes: 32 * 1024 * 1024,
            ..LoadLimits::default()
        },
    )
    .unwrap();
    let reference = compile_effects("effect Reference { color sample() { return rgb((seconds() + 0.5) / 4.0 + min(seconds() + 0.5, 1.0) * 0.25 + (seconds() + 0.25) / 4.0, seconds() / 4.0, progress()); } }").unwrap().remove(0).effect;
    let params = reference
        .bind(
            [].iter().map(|(name, value)| (name, value)),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let mut workspace = prepared.clone().into_playback();
    let mut decoded_workspace = decoded.into_playback();
    for tick in [1_500_000, 1_750_000, 2_500_000, 1_500_000, 3_000_000] {
        let time = SampleTime::from_ticks(tick);
        let actual = workspace.evaluate(time).colors().to_vec();
        assert_eq!(
            actual,
            prepared.clone().into_playback().evaluate(time).colors()
        );
        assert_eq!(actual, decoded_workspace.evaluate(time).colors());
        let elapsed = tick - 1_500_000;
        let expected = params.evaluate(
            &RunContext {
                progress: elapsed as f32 / 2_000_000.0,
                time: SampleDuration::from_ticks(elapsed),
                duration: SampleDuration::from_ticks(2_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &SPATIAL,
            &mut VmWorkspace::default(),
        );
        assert!(
            actual.contains(&expected),
            "time {tick}: expected {expected:?}, first {:?}",
            actual.first()
        );
    }
}

#[test]
fn unchanged_constant_generator_keeps_constant_playback() {
    let prepared = prepare(
        "effect MarkImpactBurst { param float level = 0.2; void generate() { timeline.emit Leaf { start: 0.0, duration: 1.0, target: target, value: level * 0.5 }; } } effect Leaf { param float value; color sample() { return rgb(value, value, value); } }",
        false,
    );
    assert!(prepared.effect_count() > 0);
    let mut playback = prepared.into_playback();
    for tick in [1_000_000, 1_500_000, 1_999_999] {
        assert!(
            playback
                .evaluate(SampleTime::from_ticks(tick))
                .colors()
                .contains(&donder_runtime::Color {
                    red: 26,
                    green: 26,
                    blue: 26
                })
        );
    }
}

fn query_operator(expression: &str, cached: bool) -> donder_runtime::CompiledOperator {
    let compiled = donder_language::dsl::compile_operators(&format!(
        "operator Query {{ input Signal source; color sample() {{ return {expression}; }} }}"
    ))
    .unwrap()
    .remove(0);
    if cached {
        return compiled;
    }
    let mut bytecode = compiled.program().clone().into_parts().0;
    for instruction in &mut bytecode.instructions {
        if let donder_runtime::Instruction::SignalSample { frame_cache, .. } = instruction {
            *frame_cache = u32::MAX;
        }
    }
    donder_runtime::CompiledOperator::admit(
        compiled.name().clone(),
        compiled.inputs().to_vec(),
        compiled.params().to_vec(),
        bytecode,
    )
    .unwrap()
}

#[test]
fn temporal_and_spatial_queries_share_live_bindings_in_recursive_frames_and_pixels() {
    let source = "effect MarkImpactBurst { param float level = 0.2; void generate() { timeline.emit Leaf { start: 0.0, duration: 3.0, target: target, value: level * 0.5 + seconds() * 0.1 }; } } effect Leaf { param float value; color sample() { return rgb(value, pixel_fraction() * 0.5, progress()); } }";
    for expression in [
        "source.at(seconds() - 0.125)",
        "max(source.at(seconds()), source.at(seconds() - 0.125))",
        "source.at(seconds() - 0.125, pixel_index())",
    ] {
        let scalar = prepare_query(source, true, Some((expression, false)));
        let framed = prepare_query(source, true, Some((expression, true)));
        let mut frames = framed.clone().into_playback();
        let mut pixels = scalar.into_playback();
        for tick in [1_250_000, 1_750_000, 2_500_000, 1_250_000] {
            let time = SampleTime::from_ticks(tick);
            let actual = frames.evaluate(time).colors();
            let expected = pixels.evaluate(time).colors();
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(actual, expected, "{expression} at {tick} pixel {index}");
            }
            let fresh = framed
                .clone()
                .into_playback()
                .evaluate(time)
                .colors()
                .to_vec();
            assert!(actual.iter().eq(fresh.iter()));
        }
    }
}

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<isize> = const { Cell::new(0) };
    static PEAK_BYTES: Cell<usize> = const { Cell::new(0) };
}

fn record_memory_change(allocated: usize, freed: usize) {
    // Ignore allocations on other test threads and during thread-local teardown.
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        if allocated != 0 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        let _ = LIVE_BYTES.try_with(|live| {
            let bytes = live.get() + allocated as isize - freed as isize;
            live.set(bytes);
            let _ = PEAK_BYTES.try_with(|peak| peak.set(peak.get().max(bytes.max(0) as usize)));
        });
    }
}

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            record_memory_change(layout.size(), 0);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        record_memory_change(0, layout.size());
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc_zeroed(layout) };
        if !pointer.is_null() {
            record_memory_change(layout.size(), 0);
        }
        pointer
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let pointer = unsafe { System.realloc(pointer, layout, size) };
        if !pointer.is_null() {
            record_memory_change(size, layout.size());
        }
        pointer
    }
}

#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/generator_workload.rs"]
mod generator_workload;
#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

#[test]
fn generator_workloads_match_ordinary_samples_and_allocate_nothing_during_playback() {
    for (case, name) in generator_workload::CASES {
        for automated in [false, true] {
            for count in [8, 200, 800] {
                let show = generator_workload::show(count, case, true, automated).prepare();
                let reference = generator_workload::show(count, case, false, automated).prepare();
                assert_eq!(show.effect_count(), reference.effect_count());
                let mut workspace = show.clone().into_playback();
                let mut reference_workspace = reference.into_playback();
                let mut buffers = show
                    .outputs()
                    .iter()
                    .map(|output| vec![0u8; output.width])
                    .collect::<Vec<_>>();
                let mut expected = buffers.clone();
                for frame in [0, 1, 31, 12, 0] {
                    let time = workload::time(frame);
                    for (snapshot, output) in expected
                        .iter_mut()
                        .zip(reference_workspace.evaluate(time).outputs())
                    {
                        snapshot.copy_from_slice(output.bytes);
                    }
                    ALLOCATIONS.set(0);
                    COUNTING.set(true);
                    for (snapshot, output) in
                        buffers.iter_mut().zip(workspace.evaluate(time).outputs())
                    {
                        snapshot.copy_from_slice(output.bytes);
                    }
                    COUNTING.set(false);
                    assert_eq!(
                        ALLOCATIONS.get(),
                        0,
                        "{name} automated={automated} pixels={count} frame={frame}"
                    );
                    assert_eq!(
                        buffers, expected,
                        "{name} automated={automated} pixels={count} frame={frame}"
                    );
                }
            }
        }
    }
}

#[test]
fn alternating_query_times_keep_forwarded_curves_current_without_allocations() {
    for case in [
        generator_workload::Case::Curve,
        generator_workload::Case::Resources,
        generator_workload::Case::Nested,
    ] {
        let show = generator_workload::show(200, case, true, true);
        let reference = generator_workload::show(200, case, false, true);
        for cached in [true, false] {
            let expression = "max(source.at(seconds()), source.at(seconds() - 0.125))";
            let mut graph = show.clone();
            let mut reference = reference.clone();
            workload::apply_compiled_operator(&mut graph, query_operator(expression, cached));
            workload::apply_compiled_operator(&mut reference, query_operator(expression, cached));
            let mut workspace = graph.prepare().into_playback();
            let mut reference_workspace = reference.prepare().into_playback();
            for frame in [0, 31, 12, 0] {
                let time = workload::time(frame);
                let expected = reference_workspace.evaluate(time).colors();
                ALLOCATIONS.set(0);
                COUNTING.set(true);
                let actual = workspace.evaluate(time).colors();
                COUNTING.set(false);
                assert_eq!(
                    ALLOCATIONS.get(),
                    0,
                    "{case:?} cached={cached} frame={frame}"
                );
                assert!(
                    actual.iter().eq(expected),
                    "{case:?} cached={cached} frame={frame}"
                );
            }
        }
    }
}
