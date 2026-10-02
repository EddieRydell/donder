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
use donder_runtime::PreparedEffectImplementation;
use donder_runtime::{LoadLimits, decode_sequence, encode_sequence};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::time::Duration;

fn prepare(source: &str, automated: bool) -> PreparedSequence {
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
    assert!(!prepared.to_raw_signals().effects.is_empty());
    assert_eq!(
        prepared.to_raw_signals().effects.len(),
        valid_only.to_raw_signals().effects.len()
    );
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
    let signal = prepared.to_raw_signals();
    assert!(!signal.parameter_environments.is_empty());
    assert!(signal.effects.iter().all(|effect| matches!(
        effect.implementation,
        PreparedEffectImplementation::Bound { .. }
    )));
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
    let mut workspace = donder_runtime::PreparedSequence::admit(
        signal.clone(),
        donder_runtime::PreparedPatch {
            routes: Box::new([]),
            lookups: Box::new([]),
        },
        Box::new([]),
    )
    .unwrap()
    .into_playback();
    let mut decoded_workspace = donder_runtime::PreparedSequence::admit(
        decoded.to_raw_signals(),
        donder_runtime::PreparedPatch {
            routes: Box::new([]),
            lookups: Box::new([]),
        },
        Box::new([]),
    )
    .unwrap()
    .into_playback();
    for tick in [1_500_000, 1_750_000, 2_500_000, 1_500_000, 3_000_000] {
        let time = SampleTime::from_ticks(tick);
        let actual = workspace.evaluate(time).colors().to_vec();
        assert_eq!(
            actual,
            donder_runtime::PreparedSequence::admit(
                signal.clone(),
                donder_runtime::PreparedPatch {
                    routes: Box::new([]),
                    lookups: Box::new([])
                },
                Box::new([])
            )
            .unwrap()
            .into_playback()
            .evaluate(time)
            .colors()
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
fn unchanged_constant_generator_keeps_static_playback_path() {
    let prepared = prepare(
        "effect MarkImpactBurst { param float level = 0.2; void generate() { timeline.emit Leaf { start: 0.0, duration: 1.0, target: target, value: level * 0.5 }; } } effect Leaf { param float value; color sample() { return rgb(value, value, value); } }",
        false,
    );
    assert!(prepared.to_raw_signals().parameter_environments.is_empty());
    assert!(
        prepared
            .to_raw_signals()
            .effects
            .iter()
            .all(|effect| matches!(
                effect.implementation,
                PreparedEffectImplementation::Dsl { .. }
            ))
    );
}

fn query_graph(
    base: &donder_runtime::PreparedSignalGraph,
    expression: &str,
    cached: bool,
) -> donder_runtime::PreparedSignalGraph {
    use donder_runtime::{
        PreparedOperator, PreparedOperatorNode, PreparedSignalKind, PreparedSignalNode,
    };
    let mut graph = base.clone();
    let mut operator = donder_language::dsl::compile_operators(&format!(
        "operator Query {{ input Signal source; color sample() {{ return {expression}; }} }}"
    ))
    .unwrap()
    .remove(0)
    .program()
    .clone()
    .into_parts()
    .0;
    if !cached {
        for instruction in &mut operator.instructions {
            if let donder_runtime::Instruction::SignalSample { frame_cache, .. } = instruction {
                *frame_cache = u32::MAX;
            }
        }
    }
    let program = graph.programs.len();
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
            vm_slot: graph.plan.vm_workspace_count,
        },
    });
    graph.plan.output_index = nodes.len() - 1;
    graph.plan.vm_workspace_count += 1;
    graph.plan.frame_nodes = vec![graph.plan.output_index].into();
    graph.plan.frame_slots = vec![usize::MAX; nodes.len()].into();
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
    let base = &prepared.to_raw_signals();
    for expression in [
        "source.at(seconds() - 0.125)",
        "max(source.at(seconds()), source.at(seconds() - 0.125))",
        "source.at(seconds() - 0.125, pixel_index())",
    ] {
        let framed = query_graph(base, expression, true);
        let scalar = query_graph(base, expression, false);
        let mut frames = donder_runtime::PreparedSequence::admit(
            framed.clone(),
            donder_runtime::PreparedPatch {
                routes: Box::new([]),
                lookups: Box::new([]),
            },
            Box::new([]),
        )
        .unwrap()
        .into_playback();
        let mut pixels = donder_runtime::PreparedSequence::admit(
            scalar.clone(),
            donder_runtime::PreparedPatch {
                routes: Box::new([]),
                lookups: Box::new([]),
            },
            Box::new([]),
        )
        .unwrap()
        .into_playback();
        for tick in [1_250_000, 1_750_000, 2_500_000, 1_250_000] {
            let time = SampleTime::from_ticks(tick);
            let actual = frames.evaluate(time).colors();
            let expected = pixels.evaluate(time).colors();
            for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
                assert_eq!(actual, expected, "{expression} at {tick} pixel {index}");
            }
            let fresh = donder_runtime::PreparedSequence::admit(
                framed.clone(),
                donder_runtime::PreparedPatch {
                    routes: Box::new([]),
                    lookups: Box::new([]),
                },
                Box::new([]),
            )
            .unwrap()
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
                let show = generator_workload::show(count, case, true, automated);
                let reference = generator_workload::show(count, case, false, automated);
                assert_eq!(
                    show.signals().effects.len(),
                    reference.signals().effects.len()
                );
                assert_eq!(show.signals().parameter_environments.is_empty(), !automated);
                let mut workspace = show.clone().prepare().unwrap().into_playback();
                let mut reference_workspace = reference.clone().prepare().unwrap().into_playback();
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
            let graph = query_graph(show.signals(), expression, cached);
            let reference = query_graph(reference.signals(), expression, cached);
            let mut workspace = donder_runtime::PreparedSequence::admit(
                graph.clone(),
                donder_runtime::PreparedPatch {
                    routes: Box::new([]),
                    lookups: Box::new([]),
                },
                Box::new([]),
            )
            .unwrap()
            .into_playback();
            let mut reference_workspace = donder_runtime::PreparedSequence::admit(
                reference.clone(),
                donder_runtime::PreparedPatch {
                    routes: Box::new([]),
                    lookups: Box::new([]),
                },
                Box::new([]),
            )
            .unwrap()
            .into_playback();
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

#[test]
fn admission_rejects_malformed_environment_bindings_before_workspace_creation() {
    use donder_runtime::LoadError;
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
        let show = generator_workload::show(8, generator_workload::Case::Derived, true, true);
        let mut signals = show.signals().clone();
        let index = signals
            .parameter_environments
            .iter()
            .position(|environment| !environment.bindings.is_empty())
            .unwrap();
        let environment = &mut signals.parameter_environments[index];
        match invalid {
            Invalid::SourceEnvironment => environment.bindings[0].source.environment = index,
            Invalid::SourceSlot => environment.bindings[0].source.parameter = u16::MAX,
            Invalid::DestinationSlot => environment.bindings[0].parameter = u16::MAX,
            Invalid::MissingBinding => environment.bindings = Box::new([]),
            Invalid::Duration => environment.duration = SampleDuration::from_ticks(0),
            Invalid::ChildEnvironment => {
                let PreparedEffectImplementation::Bound { environment, .. } =
                    &mut signals.effects[0].implementation
                else {
                    panic!("bound child")
                };
                *environment = usize::MAX;
            }
        }
        let admitted = admit_modified(signals.clone(), show.patch(), show.outputs());
        assert!(matches!(admitted, Err(LoadError::InvalidSequence)));
    }
}

#[test]
fn admission_rejects_automation_that_would_fail_during_playback() {
    use donder_runtime::LoadError;

    for generator in [false, true] {
        for mapping in [
            AutomationMapping::Enum { values: vec![] },
            AutomationMapping::Bool,
            AutomationMapping::Float {
                min: f32::NAN,
                max: 1.0,
            },
        ] {
            let show =
                generator_workload::show(8, generator_workload::Case::Derived, generator, true);
            let mut signals = show.signals().clone();
            let binding = if generator {
                signals
                    .parameter_environments
                    .iter_mut()
                    .flat_map(|environment| environment.automation.iter_mut())
                    .next()
                    .expect("generator automation")
            } else {
                signals
                    .effects
                    .iter_mut()
                    .flat_map(|effect| effect.automation.iter_mut())
                    .flat_map(|automation| automation.bindings.iter_mut())
                    .next()
                    .expect("sample automation")
            };
            binding.mapping = mapping.clone();
            let admitted = admit_modified(signals.clone(), show.patch(), show.outputs());
            assert!(
                matches!(admitted, Err(LoadError::InvalidSequence)),
                "generator={generator} mapping={mapping:?}"
            );
        }
    }
}

#[test]
fn admission_rejects_operator_automation_with_the_wrong_parameter_type() {
    use donder_runtime::BoundParams;
    use donder_runtime::LoadError;
    use donder_runtime::{PreparedAutomation, PreparedSignalKind};
    use std::sync::Arc;

    let show = generator_workload::show(8, generator_workload::Case::Forward, false, false);
    let mut signals = show.signals().clone();
    signals = query_graph(&signals, "source.at(seconds())", true);
    let compiled = donder_language::dsl::compile_operators(
        "operator Query { input Signal source; param float gain = 0.5; color sample() { return source.at(seconds()) * gain; } }",
    )
    .unwrap()
    .remove(0);
    *signals.programs.last_mut().unwrap() = compiled.program().clone().into_parts().0;
    {
        let PreparedSignalKind::Operator {
            operator,
            automation,
            ..
        } = &mut signals.plan.nodes.last_mut().unwrap().kind
        else {
            panic!("query node is an operator")
        };
        operator.params = BoundParams::bind_pairs(compiled.params(), &[]).unwrap();
        *automation = vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            curve: Arc::new(Curve {
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
            }),
            mapping: AutomationMapping::Float { min: 0.0, max: 1.0 },
            param_index: 0,
        }]
        .into();
    }
    assert!(admit_modified(signals.clone(), show.patch(), show.outputs()).is_ok());

    let PreparedSignalKind::Operator { automation, .. } =
        &mut signals.plan.nodes.last_mut().unwrap().kind
    else {
        panic!("query node is an operator")
    };
    automation[0].mapping = AutomationMapping::Bool;
    assert!(matches!(
        admit_modified(signals.clone(), show.patch(), show.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_undersized_retained_array_arena() {
    use donder_runtime::LoadError;

    let prepared = prepare(
        "effect MarkImpactBurst { void generate() { timeline.emit Leaf { start: 0.0, duration: 1.0, target: target, values: [[seconds(), seconds() + 1.0], [2.0, 3.0]] }; } } effect Leaf { param array<array<float>> values; color sample() { return rgb(values[0][0], 0.0, 0.0); } }",
        false,
    );
    let mut signals = prepared.to_raw_signals();
    let environment = signals
        .parameter_environments
        .iter_mut()
        .find(|environment| environment.array_capacity > 1)
        .expect("calculated nested arrays require a retained arena");
    environment.array_capacity -= 1;
    assert!(matches!(
        admit_modified(signals, prepared.patch(), prepared.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_malformed_retained_calculation_bytecode() {
    use donder_runtime::LoadError;
    use donder_runtime::{ColorSlot, Instruction};

    let show = generator_workload::show(8, generator_workload::Case::Derived, true, true);
    let mut signals = show.signals().clone();
    let calculation = signals
        .parameter_environments
        .iter_mut()
        .find_map(|environment| environment.calculation.as_mut())
        .expect("derived generator has a retained calculation");
    calculation.program.instructions[0] = Instruction::ReturnColor(ColorSlot(u32::MAX));

    assert!(matches!(
        admit_modified(signals.clone(), show.patch(), show.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_retained_calculation_with_invalid_parameter_read() {
    use donder_runtime::Instruction;
    use donder_runtime::LoadError;

    let show = generator_workload::show(8, generator_workload::Case::Derived, true, true);
    let mut signals = show.signals().clone();
    let calculation = signals
        .parameter_environments
        .iter_mut()
        .find_map(|environment| environment.calculation.as_mut())
        .expect("derived generator has a retained calculation");
    let param = calculation
        .program
        .instructions
        .iter_mut()
        .find_map(|instruction| match instruction {
            Instruction::LoadFloatParam { param, .. } => Some(param),
            _ => None,
        })
        .expect("calculation reads its parent level");
    *param = usize::MAX;

    assert!(matches!(
        admit_modified(signals.clone(), show.patch(), show.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_retained_calculation_with_mismatched_tuple_type() {
    use donder_language::dsl::Type;
    use donder_runtime::LoadError;

    let show = generator_workload::show(8, generator_workload::Case::Derived, true, true);
    let mut signals = show.signals().clone();
    let calculation = signals
        .parameter_environments
        .iter_mut()
        .find_map(|environment| environment.calculation.as_mut())
        .expect("derived generator has a retained calculation");
    let first = calculation
        .outputs
        .first_mut()
        .expect("calculation has output");
    *first = if *first == Type::Bool {
        Type::Float
    } else {
        Type::Bool
    };

    assert!(matches!(
        admit_modified(signals.clone(), show.patch(), show.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

#[test]
fn admission_rejects_sample_program_with_invalid_parameter_read() {
    use donder_runtime::Instruction;
    use donder_runtime::LoadError;

    let show = generator_workload::show(8, generator_workload::Case::Derived, false, false);
    let mut signals = show.signals().clone();
    let program_index = signals
        .effects
        .iter()
        .find_map(|effect| match &effect.implementation {
            PreparedEffectImplementation::Dsl { program, .. } => Some(*program),
            _ => None,
        })
        .expect("ordinary effect has a sample program");
    let param = signals.programs[program_index]
        .instructions
        .iter_mut()
        .find_map(|instruction| match instruction {
            Instruction::LoadFloatParam { param, .. } => Some(param),
            _ => None,
        })
        .expect("sample reads its level parameter");
    *param = usize::MAX;

    assert!(matches!(
        admit_modified(signals.clone(), show.patch(), show.outputs()),
        Err(LoadError::InvalidSequence)
    ));
}

// Malformed fixtures are rejected before becoming admitted sequences.
fn admit_modified(
    signals: donder_runtime::PreparedSignalGraph,
    patch: &donder_runtime::PreparedPatch,
    outputs: &[donder_runtime::PreparedOutput],
) -> Result<PreparedSequence, donder_runtime::LoadError> {
    PreparedSequence::admit(signals, patch.clone(), outputs.into())
}
