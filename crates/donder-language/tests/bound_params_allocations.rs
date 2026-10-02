use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use donder_language::dsl::{BoundParams, DslBindCache, Identifier, ParamDecl, Type, Value};
use donder_language::sequence::{AutomationClip, AutomationClipId, AutomationMapping};
use donder_language::values::{Curve, CurvePoint, DonderDuration, DonderTime};
use donder_runtime::SpatialContext;
use indexmap::IndexMap;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[allow(dead_code)]
#[path = "../../../firmware/esp32/src/workload.rs"]
mod workload;

#[allow(dead_code)]
#[path = "../benches/fixtures/mod.rs"]
mod fixtures;

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

#[test]
fn borrowed_sequence_output_seeks_and_clears_without_allocating() {
    use donder_runtime::{Color, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    let show = workload::show(
        200,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
    );
    let sequence = show.signals();
    let mut playback = show.clone().prepare().unwrap().into_playback();
    let expected = show
        .clone()
        .prepare()
        .unwrap()
        .into_playback()
        .evaluate(workload::time(4))
        .colors()
        .to_vec();
    let black = Color {
        red: 0,
        green: 0,
        blue: 0,
    };
    assert!(expected.iter().any(|&color| color != black));
    for time in [
        workload::time(4),
        SampleTime::from_ticks(sequence.duration.as_ticks()),
        SampleTime::from_ticks(u32::MAX),
        workload::time(4),
    ] {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = playback.evaluate(time);
        COUNTING.set(false);
        let colors = result.colors();
        assert_eq!(colors.len(), 200);
        assert_eq!(ALLOCATIONS.get(), 0);
        if time.as_ticks() >= sequence.duration.as_ticks() {
            assert!(colors.iter().all(|&color| color == black));
        } else {
            assert_eq!(colors, expected);
        }
    }
}

#[test]
fn warmed_curve_enum_automation_and_constant_arrays_do_not_allocate() {
    let declarations = [ParamDecl {
        fixed: false,
        name: Identifier::new("shape".to_string()).expect("valid identifier"),
        ty: Type::Curve,
        default: Some(Value::Curve(Arc::new(Curve { points: Vec::new() }))),
    }];
    let base = BoundParams::bind(&declarations, &IndexMap::new()).expect("curve should bind");
    let mut automated = base.clone();
    let clip = AutomationClip {
        id: AutomationClipId(1),
        start: DonderTime::from_micros(0),
        duration: DonderDuration::from_micros(1_000_000),
        row_target: donder_language::layout::FixtureTarget {
            layout: donder_language::layout::LayoutId(
                donder_language::identity::SourceIdentity::from_document(
                    donder_language::identity::DocumentId::new(
                        uuid::Uuid::nil(),
                        "allocation.layout.donder".into(),
                    ),
                    "layout".into(),
                )
                .into(),
            ),
            fixture: donder_language::layout::FixtureInstanceId(1),
        },
        curve: Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 0.0,
                },
                CurvePoint {
                    position: 0.25,
                    value: 1.0,
                },
                CurvePoint {
                    position: 0.5,
                    value: 0.25,
                },
                CurvePoint {
                    position: 0.75,
                    value: 0.75,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        },
        bindings: Vec::new(),
        detached_bindings: Vec::new(),
    };
    let mapping = AutomationMapping::Curve {
        min: -1.0,
        max: 2.0,
    };
    let samples = [0.0, 0.25, 0.5, 0.75, 1.0];
    for seconds in samples {
        automated
            .apply_automation(0, &clip.curve, &mapping, seconds)
            .expect("warmup automation should apply");
    }

    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = samples
        .into_iter()
        .try_for_each(|seconds| automated.apply_automation(0, &clip.curve, &mapping, seconds));
    COUNTING.set(false);

    result.expect("measured automation should apply");
    assert_eq!(ALLOCATIONS.get(), 0);

    let options =
        ["short", "much_longer_option"].map(|value| Identifier::new(value.into()).unwrap());
    let declarations = [ParamDecl {
        fixed: false,
        name: Identifier::new("mode".into()).unwrap(),
        ty: Type::Enum(options.to_vec()),
        default: Some(Value::Enum(options[0].clone())),
    }];
    let mut bound = BoundParams::bind(&declarations, &IndexMap::new()).unwrap();
    let mapping = AutomationMapping::Enum {
        values: options.to_vec(),
    };
    // Visit the longest option first so subsequent updates must reuse its storage.
    bound
        .apply_automation(0, &clip.curve, &mapping, 0.25)
        .unwrap();
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = samples
        .into_iter()
        .try_for_each(|position| bound.apply_automation(0, &clip.curve, &mapping, position));
    COUNTING.set(false);
    result.unwrap();
    assert_eq!(bound.enum_name(0).unwrap(), options[0].as_str());
    assert_eq!(ALLOCATIONS.get(), 0, "enum automation allocated");

    let effect = donder_language::dsl::compile_effects(
        "effect Constants { color sample() {
            array<array<float>> values = [[0.1, 0.2], [0.3, 0.4]];
            return rgb(values[0][1], values[1][0], values[1][1]);
        } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let bound = effect
        .bind(&IndexMap::new(), &mut DslBindCache::default())
        .unwrap();
    let mut workspace = donder_language::dsl::VmWorkspace::default();
    let context = donder_language::dsl::RunContext {
        progress: 0.0,
        time: donder_language::values::SampleDuration::from_ticks(0),
        duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let expected = bound.evaluate(&context, &SPATIAL, &mut workspace);
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let sampled = bound.evaluate(&context, &SPATIAL, &mut workspace);
    COUNTING.set(false);
    assert_eq!(sampled, expected);
    assert_eq!(ALLOCATIONS.get(), 0, "constant arrays allocated");
}

#[test]
fn calculated_arrays_do_not_allocate_after_warmup() {
    let effect = donder_language::dsl::compile_effects(include_str!(
        "fixtures/array-lifetimes.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .effect;
    let mut workspace = donder_language::dsl::VmWorkspace::default();
    let mut counts = [0; 3];
    let mut peaks = [0; 3];
    for (case, iterations) in [2, 64, 9_999].into_iter().enumerate() {
        let bound = effect
            .bind(
                &IndexMap::from([(
                    Identifier::new("iterations".into()).unwrap(),
                    Value::Int(iterations),
                )]),
                &mut DslBindCache::default(),
            )
            .unwrap();
        let context = donder_language::dsl::RunContext {
            progress: 0.25,
            time: donder_language::values::SampleDuration::from_ticks(0),
            duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        bound.evaluate(&context, &SPATIAL, &mut workspace);
        ALLOCATIONS.set(0);
        // This fixture releases every calculated array before evaluation returns.
        // Count only new allocation payloads, excluding the already-warmed workspace.
        LIVE_BYTES.set(0);
        PEAK_BYTES.set(0);
        COUNTING.set(true);
        let results = [0.25, 0.5].map(|progress| {
            bound.evaluate(
                &donder_language::dsl::RunContext {
                    progress,
                    ..context.clone()
                },
                &SPATIAL,
                &mut workspace,
            )
        });
        COUNTING.set(false);
        counts[case] = ALLOCATIONS.get();
        peaks[case] = PEAK_BYTES.get();
        assert_eq!(
            LIVE_BYTES.get(),
            0,
            "sample retained newly allocated storage"
        );
        assert_eq!(
            results,
            [
                donder_language::dsl::Color {
                    red: 64,
                    green: 89,
                    blue: 230
                },
                donder_language::dsl::Color {
                    red: 128,
                    green: 153,
                    blue: 230
                },
            ]
        );
    }
    assert_eq!(
        counts,
        [0, 0, 0],
        "allocation calls for two samples at 2, 64, and 9999 loop iterations; peak newly allocated payload bytes: {peaks:?}"
    );
}

#[test]
fn retained_array_results_do_not_allocate_on_the_first_evaluation() {
    use donder_language::dsl::{GeneratorContext, GeneratorInput, TargetValue, compile_effects};
    use donder_language::values::{SampleDuration, SampleTime};

    let generator = compile_effects(
        "effect Parent { param float value; void generate() {
            timeline.emit Child { start: 0.0, duration: 1.0, target: target,
                value: [[value + seconds()], [value]] };
        } }",
    )
    .unwrap()
    .remove(0)
    .effect
    .generator()
    .unwrap()
    .clone();
    let specialized =
        generator
            .bind(&[GeneratorInput::Live])
            .unwrap()
            .specialize(&GeneratorContext {
                start_time: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(1_000_000),
                target: Arc::new(TargetValue { groups: Vec::new() }),
            });
    let child = compile_effects(
        "effect Child { param array<array<float>> value; color sample() {
        return rgb(value[0][0], value[1][0], (len(value) + len(value[0]) + len(value[1])) * 0.125);
    } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let mut playback = retained_playback(
        &specialized.calculations[0].program,
        vec![Value::Float(0.25)],
        &child,
    );
    for time in [0, 500_000, 250_000, 0] {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let evaluated = playback.evaluate(SampleTime::from_ticks(time));
        COUNTING.set(false);
        assert_eq!(
            ALLOCATIONS.get(),
            0,
            "retained array result allocated at {time}"
        );
        assert_eq!(
            evaluated.colors(),
            &[donder_runtime::Color {
                red: ((0.25 + time as f32 / 1_000_000.0) * 255.0).round() as u8,
                green: 64,
                blue: 128,
            }]
        );
    }
}

// Exercise retained result storage through the same admitted graph used by playback.
// A raw calculation and its output bank are never independently executable.
fn retained_playback(
    calculation: &donder_runtime::CalculationProgram,
    values: Vec<Value>,
    child: &donder_language::dsl::CompiledEffect,
) -> donder_runtime::SequencePlayback {
    use donder_runtime::{
        PreparedEffectImplementation, PreparedParameterCalculation, PreparedParameterEnvironment,
        PreparedSequence, SampleDuration, SampleTime,
    };
    let (program, types, outputs) = calculation.clone().into_parts();
    let calculation = PreparedParameterCalculation { program, outputs };
    let (array_capacity, array_width) =
        PreparedParameterEnvironment::required_array_storage([], Some(&calculation));
    let params = BoundParams::from_values(types.iter().zip(values), &mut DslBindCache::default());
    let base = workload::show(
        1,
        child.sample_program().unwrap().clone().into_parts().0,
        BoundParams::default(),
    );
    let mut graph = base.signals().clone();
    let environment: PreparedParameterEnvironment = PreparedParameterEnvironment {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        params,
        types,
        bindings: Box::new([]),
        automation: Box::new([]),
        calculation: Some(calculation),
        array_capacity,
        array_width,
    };
    graph.parameter_environments = vec![environment].into();
    graph.effects[0].implementation = PreparedEffectImplementation::Bound {
        program: 0,
        environment: 0,
    };
    PreparedSequence::admit(graph, base.patch().clone(), base.outputs().into())
        .unwrap()
        .into_playback()
}

#[test]
fn prepared_calculated_arrays_do_not_allocate_on_the_first_frame() {
    let effect = donder_language::dsl::compile_effects(include_str!(
        "fixtures/array-lifetimes.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .effect;
    let params = BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
    let show = workload::layered_show(
        200,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
        4,
    );
    let mut workspace = show.clone().prepare().unwrap().into_playback();
    let mut buffers = [vec![0; 600]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in buffers
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
    }
    COUNTING.set(false);
    assert_eq!(ALLOCATIONS.get(), 0, "prepared array evaluation allocated");
}

#[test]
fn enum_local_assignment_and_constant_loads_do_not_allocate() {
    let effect = donder_language::dsl::compile_effects(
        "effect EnumLocals {
        param enum mode { short, much_longer_option } = short;
        param enum gate { off, on } = on;
        param enum shape { one, two } = two;
        color sample() {
            if (progress() > 0.5) { mode = much_longer_option; }
            if (progress() < 0.5) { gate = off; }
            if (mode == much_longer_option && gate == on && shape == two) { return #ffffff; }
            return #000000;
        }
    }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let bound = effect
        .bind(&IndexMap::new(), &mut DslBindCache::default())
        .unwrap();
    let mut workspace = donder_language::dsl::VmWorkspace::default();
    let mut context = donder_language::dsl::RunContext {
        progress: 0.0,
        time: donder_language::values::SampleDuration::from_ticks(0),
        duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    bound.evaluate(&context, &SPATIAL, &mut workspace);
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for progress in [1.0, 0.0, 1.0] {
        context.progress = progress;
        let color = bound.evaluate(&context, &SPATIAL, &mut workspace);
        assert_eq!(color.red, if progress > 0.5 { 255 } else { 0 });
    }
    COUNTING.set(false);
    assert_eq!(ALLOCATIONS.get(), 0, "enum copies allocated");
}

#[test]
fn many_signal_times_use_fixed_storage_from_the_first_frame() {
    let effect = donder_language::dsl::compile_effects(
        "effect Ramp { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
    let expected = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params.clone(),
    );
    let mut show = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
    );
    let operator = donder_language::dsl::compile_operators(
        "operator ManyTimes { input Signal source; color sample() {
            color saved = source.at(seconds());
            for (int i = 0; i < 1100; i = i + 1) { color sampled = source.at(i * 0.001); }
            return max(source.at(seconds() * 0.5), source.at(seconds()));
        } }",
    )
    .unwrap()
    .remove(0);
    workload::apply_operator(&mut show, operator.program().clone().into_parts().0, true);
    let mut workspace = show.clone().prepare().unwrap().into_playback();
    let mut expected_workspace = expected.clone().prepare().unwrap().into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected_bytes = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in expected_bytes
            .iter_mut()
            .zip(expected_workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        assert_eq!(ALLOCATIONS.get(), 0, "signal cache allocated");
        assert_eq!(actual, expected_bytes);
    }
}

#[test]
fn unautomated_effects_do_not_expand_the_evaluation_workspace() {
    let (effect, params) = fixtures::uniform_resources();
    let mut expected_bytes = None;
    for count in [1, 16, 128] {
        let show = workload::show(
            200,
            effect.sample_program().unwrap().clone().into_parts().0,
            params.clone(),
        );
        let mut signals = show.signals().clone();
        signals.effects = vec![signals.effects[0].clone(); count].into();
        signals.effects_by_layer[0] = (0..count).collect();
        let show = workload::rgb_output(signals);
        let prepared = show.prepare().unwrap();
        ALLOCATIONS.set(0);
        LIVE_BYTES.set(0);
        PEAK_BYTES.set(0);
        COUNTING.set(true);
        let mut workspace = prepared.into_playback();
        COUNTING.set(false);
        println!(
            "effects={count} workspace_bytes={} workspace_allocations={}",
            LIVE_BYTES.get(),
            ALLOCATIONS.get()
        );
        if let Some(expected) = expected_bytes {
            assert_eq!(LIVE_BYTES.get(), expected);
        } else {
            expected_bytes = Some(LIVE_BYTES.get());
        }
        let mut output = [vec![0; 600]];
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in output
            .iter_mut()
            .zip(workspace.evaluate(workload::time(0)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        assert_eq!(ALLOCATIONS.get(), 0);
    }
}

#[test]
fn hoisted_resources_and_curve_automation_do_not_allocate_from_the_first_frame() {
    use donder_runtime::{PreparedAutomation, PreparedEffectAutomation};
    use donder_runtime::{SampleDuration, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    for recursive in [false, true] {
        let mut show = workload::show(
            200,
            effect.sample_program().unwrap().clone().into_parts().0,
            params.clone(),
        );
        let mut signals = show.signals().clone();
        signals.effects[0].automation = Some(Box::new(PreparedEffectAutomation {
            workspace_slot: 0,
            bindings: vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                curve: params.curve(0).unwrap(),
                mapping: donder_runtime::AutomationMapping::Curve { min: 0.0, max: 1.0 },
                param_index: 0,
            }]
            .into(),
        }));
        show = workload::rgb_output(signals);
        if recursive {
            let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
                .unwrap()
                .remove(0);
            workload::apply_operator(&mut show, operator.program().clone().into_parts().0, true);
        }
        let mut workspace = show.clone().prepare().unwrap().into_playback();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in expected.iter_mut().zip(
                show.clone()
                    .prepare()
                    .unwrap()
                    .into_playback()
                    .evaluate(workload::time(frame))
                    .outputs(),
            ) {
                snapshot.copy_from_slice(output.bytes);
            }
            ALLOCATIONS.set(0);
            COUNTING.set(true);
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(workload::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
            COUNTING.set(false);
            assert_eq!(ALLOCATIONS.get(), 0, "resource frame allocated");
            assert_eq!(output, expected);
        }
    }
}

#[test]
fn dsl_curve_automation_releases_previous_sample_before_update() {
    let effect = donder_language::dsl::compile_effects(
        "effect Reference { color sample() { return rgb(0.0, 0.0, 0.0); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
    let mut show = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
    );
    let pulse = donder_language::dsl::compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .effect;
    workload::apply_pulse_automation(
        &mut show,
        pulse.sample_program().unwrap().clone().into_parts().0,
        false,
    );
    let mut workspace = show.clone().prepare().unwrap().into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    let mut counts = [0; 4];
    for (index, frame) in [0, 31, 4, 0].into_iter().enumerate() {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        counts[index] = ALLOCATIONS.get();
        for (snapshot, output) in expected.iter_mut().zip(
            show.clone()
                .prepare()
                .unwrap()
                .into_playback()
                .evaluate(workload::time(frame))
                .outputs(),
        ) {
            snapshot.copy_from_slice(output.bytes);
        }
        assert_eq!(actual, expected);
    }
    assert_eq!(counts, [0; 4]);
}

#[test]
fn nested_signal_nodes_do_not_displace_upstream_vm_storage() {
    let effect = donder_language::dsl::compile_effects(
        "effect Ramp { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = BoundParams::bind(effect.params(), &IndexMap::new()).unwrap();
    let reference = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params.clone(),
    );
    let mut show = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
    );
    let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    workload::apply_operator(&mut show, operator.program().clone().into_parts().0, true);
    let invert = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|operator| operator.name().as_str() == "Invert")
    .unwrap();
    workload::insert_invert(&mut show, invert.program().clone().into_parts().0);
    let mut workspace = show.clone().prepare().unwrap().into_playback();
    let mut reference_workspace = reference.clone().prepare().unwrap().into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in expected.iter_mut().zip(
            reference_workspace
                .evaluate(workload::time(frame))
                .outputs(),
        ) {
            snapshot.copy_from_slice(output.bytes);
        }
        for value in &mut expected[0] {
            *value = 255 - *value;
        }
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(workload::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        assert_eq!(actual, expected);
        assert_eq!(ALLOCATIONS.get(), 0, "nested operator displaced VM storage");
    }
}

#[test]
fn empty_curve_automation_reserves_its_fallback_point() {
    use donder_runtime::{Curve, SampleDuration, SampleTime};
    use donder_runtime::{PreparedAutomation, PreparedEffectAutomation};
    let effect = donder_language::dsl::compile_effects("effect Empty { param curve shape; color sample() { return rgb(shape[progress()], 0.0, 0.0); } }").unwrap().remove(0).effect;
    let params = BoundParams::bind(
        effect.params(),
        &IndexMap::from([(
            donder_language::dsl::Identifier::new("shape".into()).unwrap(),
            donder_language::dsl::Value::Curve(Curve { points: vec![] }.into()),
        )]),
    )
    .unwrap();
    let mut signals = workload::show(
        2,
        effect.sample_program().unwrap().clone().into_parts().0,
        params,
    )
    .signals()
    .clone();
    signals.effects[0].automation = Some(Box::new(PreparedEffectAutomation {
        workspace_slot: 0,
        bindings: vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            curve: Curve { points: vec![] }.into(),
            mapping: donder_runtime::AutomationMapping::Curve { min: 0.5, max: 1.0 },
            param_index: 0,
        }]
        .into(),
    }));
    let show = workload::rgb_output(signals);
    let mut workspace = show.clone().prepare().unwrap().into_playback();
    let mut buffers = [vec![0; 6]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for (snapshot, output) in buffers
        .iter_mut()
        .zip(workspace.evaluate(workload::time(0)).outputs())
    {
        snapshot.copy_from_slice(output.bytes);
    }
    COUNTING.set(false);
    assert!(buffers[0].iter().any(|&value| value != 0));
    assert_eq!(ALLOCATIONS.get(), 0, "empty automation window allocated");
}

#[test]
fn retained_nested_array_results_forward_without_first_or_repeated_sample_allocations() {
    use donder_language::dsl::{GeneratorBinding, GeneratorContext, TargetValue, compile_effects};
    use donder_language::values::{SampleDuration, SampleTime};
    let generator = compile_effects("effect Parent { void generate() { timeline.emit Child { start: 0.0, duration: 1.0, target: target, values: [[seconds(), seconds() + 1.0], [2.0, 3.0]] }; } }").unwrap()
        .remove(0).effect.generator().unwrap().clone().bind(&[]).unwrap().specialize(&GeneratorContext {
            start_time: SampleTime::from_ticks(0), duration: SampleDuration::from_ticks(1_000_000),
            target: Arc::new(TargetValue { groups: Vec::new() }),
        });
    let GeneratorBinding::Calculation { index, output } = generator.children[0].params[0].1 else {
        panic!("live calculation")
    };
    let calculation = &generator.calculations[index];
    assert!(calculation.inputs.is_empty());
    assert_eq!(output, 0);
    let child = compile_effects("effect Child { param array<array<float>> values; color sample() { return rgb(values[0][0], values[0][1] * 0.25, values[1][0] * 0.25); } }").unwrap().remove(0).effect;
    let mut playback = retained_playback(&calculation.program, vec![], &child);
    for tick in [0, 750_000, 250_000, 999_999, 0] {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = playback.evaluate(SampleTime::from_ticks(tick));
        COUNTING.set(false);
        assert_eq!(ALLOCATIONS.get(), 0);
        let seconds = tick as f32 / 1_000_000.0;
        assert_eq!(
            result.colors(),
            &[donder_runtime::Color {
                red: (seconds * 255.0).round() as u8,
                green: ((seconds + 1.0) * 0.25 * 255.0).round() as u8,
                blue: 128,
            }]
        );
    }
}
