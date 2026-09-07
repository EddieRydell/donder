use camino::Utf8PathBuf;
use dawn_elaboration::PreparedSequenceOutput;
use dawn_language::dsl::{Identifier, RunContext, VmWorkspace, compile_effects};
use dawn_language::effect::EffectRef;
use dawn_language::sequence::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationMapping, AutomationTarget,
};
use dawn_language::values::{
    Curve, CurvePoint, DawnDuration, DawnTime, SampleDuration, SampleTime,
};
use dawn_runtime::signal::PreparedEffectImplementation;
use dawn_runtime::wire::{LoadLimits, decode_sequence, encode_sequence};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

fn prepare(source: &str, automated: bool) -> PreparedSequenceOutput {
    let root = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter");
    let mut sources = dawn_project_io::project_source_texts(&root).unwrap();
    sources.insert(
        "effects/mark-impact-burst.effect.dawn".into(),
        source.into(),
    );
    let report = dawn_project_io::check_package_with_overrides(&root, &sources);
    assert!(report.diagnostics.is_empty(), "{:?}", report.diagnostics);
    let mut session = report.session.unwrap();
    let generator = session
        .project
        .definitions
        .effects
        .definitions
        .keys()
        .find(|id| id.0.object() == "MarkImpactBurst")
        .unwrap()
        .clone();
    let declarations = session
        .project
        .definitions
        .effects
        .get(&generator)
        .unwrap()
        .params
        .clone();
    let sequence = session
        .project
        .sequences
        .values_mut()
        .find(|sequence| !sequence.effects.is_empty())
        .unwrap();
    sequence.effects.truncate(1);
    sequence.automation_clips.clear();
    let effect = &mut sequence.effects[0];
    effect.definition = EffectRef::Custom(generator);
    effect.param_overrides.clear();
    for param in declarations.iter().filter(|param| param.default.is_none()) {
        use dawn_language::effect::{CurveSource, EffectParamValue, GradientSource};
        let value = match param.ty {
            dawn_language::dsl::Type::Marks => {
                sequence.mark_collections[0].marks = vec![
                    DawnTime(Duration::from_secs(1)),
                    DawnTime(Duration::from_millis(1100)),
                ];
                EffectParamValue::Marks(sequence.mark_collections[0].key.clone())
            }
            dawn_language::dsl::Type::Curve => {
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
            dawn_language::dsl::Type::Gradient => EffectParamValue::Gradient(
                GradientSource::Inline(dawn_language::values::Gradient {
                    stops: vec![dawn_language::values::GradientStop {
                        position: 0.0,
                        color: dawn_runtime::sampling::hsv(0.3, 1.0, 1.0),
                    }],
                }),
            ),
            _ => panic!("unsupported fixture resource"),
        };
        effect.param_overrides.insert(param.name.clone(), value);
    }
    effect.start = DawnTime(Duration::from_secs(1));
    effect.duration = DawnDuration(Duration::from_secs(1));
    if automated {
        sequence.automation_clips.push(AutomationClip {
            id: AutomationClipId(900),
            start: effect.start.clone(),
            duration: effect.duration.clone(),
            anchor_lane_index: 0,
            lane_index: 0,
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
    PreparedSequenceOutput::prepare(&session.project, &session.project.root.setup, &id).unwrap()
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
    let signal = &prepared.sequence.signals;
    assert!(!signal.parameter_environments.is_empty());
    assert!(signal.effects.iter().all(|effect| matches!(
        effect.implementation,
        PreparedEffectImplementation::Bound { .. }
    )));
    let bytes = encode_sequence(&prepared.sequence).unwrap();
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
    let params = reference.bind_params_pairs(&[]).unwrap();
    let mut workspace = signal.workspace();
    let mut decoded_workspace = decoded.signals.workspace();
    for tick in [1_500_000, 1_750_000, 2_500_000, 1_500_000, 3_000_000] {
        let time = SampleTime::from_ticks(tick);
        let actual = signal.evaluate(time, &mut workspace).unwrap().to_vec();
        assert_eq!(
            actual,
            signal.evaluate(time, &mut signal.workspace()).unwrap()
        );
        assert_eq!(
            actual,
            decoded
                .signals
                .evaluate(time, &mut decoded_workspace)
                .unwrap()
        );
        let elapsed = tick - 1_500_000;
        let expected = reference
            .sample_bound(
                &params,
                &RunContext {
                    progress: elapsed as f32 / 2_000_000.0,
                    time: SampleDuration::from_ticks(elapsed),
                    duration: SampleDuration::from_ticks(2_000_000),
                    pixel_index: 0,
                    pixel_count: 1,
                    pixel_fraction: 0.0,
                },
                &mut VmWorkspace::default(),
            )
            .unwrap();
        assert!(
            actual.contains(&expected),
            "time {tick}: expected {expected:?}, first {:?}",
            actual.first()
        );
    }
}

#[test]
fn unchanged_constant_generator_keeps_static_playback_path() {
    let prepared = prepare(
        "effect MarkImpactBurst { param float level = 0.2; void generate() { timeline.emit Leaf { start: 0.0, duration: 1.0, target: target, value: level * 0.5 }; } } effect Leaf { param float value; color sample() { return rgb(value, value, value); } }",
        false,
    );
    assert!(prepared.sequence.signals.parameter_environments.is_empty());
    assert!(
        prepared
            .sequence
            .signals
            .effects
            .iter()
            .all(|effect| matches!(
                effect.implementation,
                PreparedEffectImplementation::Dsl { .. }
            ))
    );
}

fn query_graph(
    base: &dawn_runtime::signal::PreparedSignalGraph,
    expression: &str,
    cached: bool,
) -> dawn_runtime::signal::PreparedSignalGraph {
    use dawn_runtime::signal::{
        PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
    };
    let mut graph = base.clone();
    let mut operator = dawn_language::dsl::compile_operators(&format!(
        "operator Query {{ input Signal source; color sample() {{ return {expression}; }} }}"
    ))
    .unwrap()
    .remove(0)
    .bytecode;
    if !cached {
        for instruction in &mut operator.instructions {
            if let dawn_runtime::dsl::bytecode::Instruction::SignalSample { frame_cache, .. } =
                instruction
            {
                *frame_cache = u32::MAX;
            }
        }
    }
    let program = graph.programs.len() as u32;
    let mut programs = graph.programs.to_vec();
    programs.push(operator);
    graph.programs = programs.into();
    let mut nodes = graph.plan.nodes.to_vec();
    nodes.push(PreparedSignalNode {
        kind: PreparedSignalKind::Operator {
            operator: PreparedOperatorNode {
                implementation: PreparedOperator::Dsl(program),
                params: Default::default(),
                automation_slot: 0,
            },
            inputs: vec![graph.plan.output_index].into(),
            automation: Box::new([]),
            vm_slot: graph.plan.vm_workspace_count as u16,
        },
    });
    graph.plan.output_index = nodes.len() - 1;
    graph.plan.vm_workspace_count += 1;
    graph.plan.frame_nodes = vec![graph.plan.output_index].into();
    graph.plan.frame_slots = vec![u16::MAX; nodes.len()].into();
    graph.plan.frame_slots[graph.plan.output_index] = 0;
    graph.plan.frame_buffer_count = 1;
    graph.plan.nodes = nodes.into();
    graph
}

#[test]
fn temporal_and_spatial_queries_share_live_bindings_in_recursive_frames_and_pixels() {
    let prepared = prepare(
        "effect MarkImpactBurst { param float level = 0.2; void generate() { timeline.emit Leaf { start: 0.0, duration: 3.0, target: target, value: level * 0.5 + seconds() * 0.1 }; } } effect Leaf { param float value; color sample() { return rgb(value, pixel_fraction() * 0.5, progress()); } }",
        true,
    );
    let base = &prepared.sequence.signals;
    for expression in [
        "source.at(seconds() - 0.125)",
        "max(source.at(seconds()), source.at(seconds() - 0.125))",
        "source.at(seconds() - 0.125, pixel_index())",
    ] {
        let framed = query_graph(base, expression, true);
        let scalar = query_graph(base, expression, false);
        let mut frames = framed.workspace();
        let mut pixels = scalar.workspace();
        for tick in [1_250_000, 1_750_000, 2_500_000, 1_250_000] {
            let time = SampleTime::from_ticks(tick);
            let actual = framed.evaluate(time, &mut frames).unwrap();
            let expected = scalar.evaluate(time, &mut pixels).unwrap();
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(actual, expected, "{expression} at {tick} pixel {index}");
            }
            let fresh = framed
                .evaluate(time, &mut framed.workspace())
                .unwrap()
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
                let show = generator_workload::show(count, case, true, automated);
                let reference = generator_workload::show(count, case, false, automated);
                assert_eq!(show.signals.effects.len(), reference.signals.effects.len());
                assert_eq!(show.signals.parameter_environments.is_empty(), !automated);
                let mut workspace = show.workspace();
                let mut reference_workspace = reference.workspace();
                let mut buffers = show
                    .output_widths
                    .iter()
                    .map(|width| vec![0u8; *width as usize])
                    .collect::<Vec<_>>();
                let mut expected = buffers.clone();
                for frame in [0, 1, 31, 12, 0] {
                    let time = workload::time(frame);
                    reference
                        .evaluate(time, &mut expected, &mut reference_workspace)
                        .unwrap();
                    ALLOCATIONS.set(0);
                    COUNTING.set(true);
                    let result = show.evaluate(time, &mut buffers, &mut workspace);
                    COUNTING.set(false);
                    result.unwrap();
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
            let graph = query_graph(&show.signals, expression, cached);
            let reference = query_graph(&reference.signals, expression, cached);
            let mut workspace = graph.workspace();
            let mut reference_workspace = reference.workspace();
            for frame in [0, 31, 12, 0] {
                let time = workload::time(frame);
                let expected = reference.evaluate(time, &mut reference_workspace).unwrap();
                ALLOCATIONS.set(0);
                COUNTING.set(true);
                let actual = graph.evaluate(time, &mut workspace);
                COUNTING.set(false);
                let actual = actual.unwrap();
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

#[test]
fn wire_rejects_malformed_environment_bindings_before_workspace_creation() {
    use dawn_runtime::wire::LoadError;
    enum Invalid {
        SourceEnvironment,
        SourceSlot,
        DestinationSlot,
        MissingBinding,
        ChildEnvironment,
        Duration,
    }
    for invalid in [
        Invalid::SourceEnvironment,
        Invalid::SourceSlot,
        Invalid::DestinationSlot,
        Invalid::MissingBinding,
        Invalid::ChildEnvironment,
        Invalid::Duration,
    ] {
        let mut show = generator_workload::show(8, generator_workload::Case::Derived, true, true);
        let index = show
            .signals
            .parameter_environments
            .iter()
            .position(|environment| !environment.bindings.is_empty())
            .unwrap();
        let environment = &mut show.signals.parameter_environments[index];
        match invalid {
            Invalid::SourceEnvironment => environment.bindings[0].source.environment = index as u32,
            Invalid::SourceSlot => environment.bindings[0].source.parameter = u16::MAX,
            Invalid::DestinationSlot => environment.bindings[0].parameter = u16::MAX,
            Invalid::MissingBinding => environment.bindings = Box::new([]),
            Invalid::Duration => environment.duration = SampleDuration::from_ticks(0),
            Invalid::ChildEnvironment => {
                let PreparedEffectImplementation::Bound { environment, .. } =
                    &mut show.signals.effects[0].implementation
                else {
                    panic!("bound child")
                };
                *environment = u32::MAX;
            }
        }
        let bytes = encode_sequence(&show).unwrap();
        assert!(matches!(
            decode_sequence(&bytes, LoadLimits::default()),
            Err(LoadError::InvalidSequence)
        ));
    }
}

#[test]
fn nested_native_mark_children_resolve_live_colors_resources_and_rendering_parameters() {
    use dawn_runtime::bindings::ParameterWorkspace;
    use dawn_runtime::signal::BoundEffectImplementation;
    for (builtin, fields, mix_slot) in [
        ("mark_pulse", "accent: ramp, decay_seconds: 2.0", 4),
        (
            "mark_chase",
            "gradients: ramps, chase_positions: [shape], pulse_shape: shape, chase_seconds: 2.0",
            5,
        ),
    ] {
        let source = format!(
            "effect MarkImpactBurst {{ fixed param marks beats; param float level = 0.2; param gradient ramp; param curve shape; void generate() {{ array<gradient> ramps = [ramp]; if (level > 0.5) {{ ramps = [ramp, ramp]; }} timeline.emit builtins.{builtin} {{ start: 0.0, duration: 1.0, target: target, beats: beats, base: rgb(level, 0.0, 0.0), hue: shape, hue_mix: level, {fields} }}; }} }}"
        );
        let prepared = prepare(&source, true);
        let graph = &prepared.sequence.signals;
        assert!(!graph.effects.is_empty());
        let PreparedEffectImplementation::Bound {
            environment,
            implementation: BoundEffectImplementation::Native(_),
        } = graph.effects[0].implementation
        else {
            panic!("expected retained native child")
        };
        let mut parameters = ParameterWorkspace::new(&graph.parameter_environments, 2);
        let mut workspace = graph.workspace();
        for tick in [1_250_000, 1_750_000, 2_500_000, 1_250_000] {
            let time = SampleTime::from_ticks(tick);
            let values = parameters
                .resolve(&graph.parameter_environments, environment, time)
                .unwrap();
            assert_eq!(
                values.float(mix_slot).unwrap(),
                ((tick - 1_000_000) as f32 / 1_000_000.0).min(1.0)
            );
            let expected = graph
                .evaluate(time, &mut graph.workspace())
                .unwrap()
                .to_vec();
            ALLOCATIONS.set(0);
            COUNTING.set(true);
            let actual = graph.evaluate(time, &mut workspace);
            COUNTING.set(false);
            assert_eq!(ALLOCATIONS.get(), 0, "{builtin}/{tick}");
            assert!(actual.unwrap().iter().eq(&expected));
        }
    }
}
