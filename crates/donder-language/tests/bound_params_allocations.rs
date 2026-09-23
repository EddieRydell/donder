use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use donder_language::dsl::{BoundParams, Identifier, ParamDecl, Type, Value};
use donder_language::sequence::{AutomationClip, AutomationClipId, AutomationMapping};
use donder_language::values::{Curve, CurvePoint, DonderDuration, DonderTime};
use indexmap::IndexMap;

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
    use donder_runtime::values::{Color, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    let show = workload::show(200, effect.bytecode, params);
    let sequence = &show.signals;
    let mut workspace = sequence.workspace();
    let expected = sequence
        .evaluate(workload::time(4), &mut sequence.workspace())
        .unwrap()
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
        let result = sequence.evaluate(time, &mut workspace);
        COUNTING.set(false);
        let colors = result.unwrap();
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
        anchor_lane_index: 0,
        lane_index: 0,
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
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut workspace = donder_language::dsl::VmWorkspace::default();
    let context = donder_language::dsl::RunContext {
        progress: 0.0,
        time: donder_language::values::SampleDuration::from_ticks(0),
        duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let expected = effect
        .sample_bound(&params, &context, &mut workspace)
        .unwrap();
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let sampled = effect.sample_bound(&params, &context, &mut workspace);
    COUNTING.set(false);
    assert_eq!(sampled.unwrap(), expected);
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
        let params = effect
            .bind_params(&IndexMap::from([(
                Identifier::new("iterations".into()).unwrap(),
                Value::Int(iterations),
            )]))
            .unwrap();
        let context = donder_language::dsl::RunContext {
            progress: 0.25,
            time: donder_language::values::SampleDuration::from_ticks(0),
            duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        effect
            .sample_bound(&params, &context, &mut workspace)
            .unwrap();
        ALLOCATIONS.set(0);
        // This fixture releases every calculated array before sample_bound returns.
        // Count only new allocation payloads, excluding the already-warmed workspace.
        LIVE_BYTES.set(0);
        PEAK_BYTES.set(0);
        COUNTING.set(true);
        let results = [0.25, 0.5].map(|progress| {
            effect.sample_bound(
                &params,
                &donder_language::dsl::RunContext {
                    progress,
                    ..context.clone()
                },
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
            results.map(Result::unwrap),
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
fn prepared_calculated_arrays_do_not_allocate_on_the_first_frame() {
    let effect = donder_language::dsl::compile_effects(include_str!(
        "fixtures/array-lifetimes.effect.donder"
    ))
    .unwrap()
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let show = workload::layered_show(200, effect.bytecode, params, 4);
    let mut workspace = show.workspace();
    let mut buffers = [vec![0; 600]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = [0, 31, 4, 0]
        .into_iter()
        .try_for_each(|frame| show.evaluate(workload::time(frame), &mut buffers, &mut workspace));
    COUNTING.set(false);
    result.unwrap();
    assert_eq!(ALLOCATIONS.get(), 0, "prepared array evaluation allocated");
}

#[test]
fn enum_local_assignment_and_constant_loads_do_not_allocate() {
    let effect = donder_language::dsl::compile_effects(
        "effect EnumLocals {
        param enum mode { short, much_longer_option } = short;
        color sample() {
            if (progress() > 0.5) { mode = much_longer_option; }
            if (mode == much_longer_option) { return #ffffff; }
            return #000000;
        }
    }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut workspace = donder_language::dsl::VmWorkspace::default();
    let mut context = donder_language::dsl::RunContext {
        progress: 0.0,
        time: donder_language::values::SampleDuration::from_ticks(0),
        duration: donder_language::values::SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    effect
        .sample_bound(&params, &context, &mut workspace)
        .unwrap();
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = [1.0, 0.0, 1.0].into_iter().try_for_each(|progress| {
        context.progress = progress;
        effect
            .sample_bound(&params, &context, &mut workspace)
            .map(|_| ())
    });
    COUNTING.set(false);
    result.unwrap();
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
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let expected = workload::show(2, effect.bytecode.clone(), params.clone());
    let mut show = workload::show(2, effect.bytecode, params);
    let operator = donder_language::dsl::compile_operators(
        "operator ManyTimes { input Signal source; color sample() {
            color saved = source.at(seconds());
            for (int i = 0; i < 1100; i = i + 1) { color sampled = source.at(i * 0.001); }
            return max(source.at(seconds() * 0.5), source.at(seconds()));
        } }",
    )
    .unwrap()
    .remove(0);
    workload::apply_operator(&mut show, operator.bytecode, true);
    let mut workspace = show.workspace();
    let mut expected_workspace = expected.workspace();
    let mut actual = [vec![0; 6]];
    let mut expected_bytes = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        expected
            .evaluate(
                workload::time(frame),
                &mut expected_bytes,
                &mut expected_workspace,
            )
            .unwrap();
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = show.evaluate(workload::time(frame), &mut actual, &mut workspace);
        COUNTING.set(false);
        result.unwrap();
        assert_eq!(ALLOCATIONS.get(), 0, "signal cache allocated");
        assert_eq!(actual, expected_bytes);
    }
}

#[test]
fn unautomated_effects_do_not_expand_the_evaluation_workspace() {
    let (effect, params) = fixtures::uniform_resources();
    let mut expected_bytes = None;
    for count in [1, 16, 128] {
        let mut show = workload::show(200, effect.bytecode.clone(), params.clone());
        show.signals.effects = vec![show.signals.effects[0].clone(); count].into();
        show.signals.effects_by_layer[0] = (0..count).collect();
        ALLOCATIONS.set(0);
        LIVE_BYTES.set(0);
        PEAK_BYTES.set(0);
        COUNTING.set(true);
        let mut workspace = show.workspace();
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
        let result = show.evaluate(workload::time(0), &mut output, &mut workspace);
        COUNTING.set(false);
        result.unwrap();
        assert_eq!(ALLOCATIONS.get(), 0);
    }
}

#[test]
fn hoisted_resources_and_curve_automation_do_not_allocate_from_the_first_frame() {
    use donder_runtime::signal::{PreparedAutomation, PreparedEffectAutomation};
    use donder_runtime::values::{SampleDuration, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    for recursive in [false, true] {
        let mut show = workload::show(200, effect.bytecode.clone(), params.clone());
        show.signals.effects[0].automation = Some(Box::new(PreparedEffectAutomation {
            workspace_slot: 0,
            bindings: vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                curve: params.curve(0).unwrap(),
                mapping: donder_runtime::automation::AutomationMapping::Curve {
                    min: 0.0,
                    max: 1.0,
                },
                param_index: 0,
            }]
            .into(),
        }));
        if recursive {
            let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
                .unwrap()
                .remove(0);
            workload::apply_operator(&mut show, operator.bytecode, true);
        }
        let mut workspace = show.workspace();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            show.evaluate(workload::time(frame), &mut expected, &mut show.workspace())
                .unwrap();
            ALLOCATIONS.set(0);
            COUNTING.set(true);
            let result = show.evaluate(workload::time(frame), &mut output, &mut workspace);
            COUNTING.set(false);
            result.unwrap();
            assert_eq!(ALLOCATIONS.get(), 0, "resource frame allocated");
            assert_eq!(output, expected);
        }
    }
}

#[test]
fn native_curve_automation_releases_previous_sample_before_update() {
    let effect = donder_language::dsl::compile_effects(
        "effect Reference { color sample() { return rgb(0.0, 0.0, 0.0); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut show = workload::show(2, effect.bytecode, params);
    workload::apply_native_automation(&mut show, false);
    let reference = donder_language::dsl::compile_effects(
        "effect Reference { param gradient ramp; param curve shape; color sample() { return ramp[progress()] * shape[progress()]; } }",
    ).unwrap().remove(0).effect;
    let donder_runtime::signal::PreparedEffectImplementation::Native {
        params: Some((_, params)),
        ..
    } = &show.signals.effects[0].implementation
    else {
        panic!("expected native parameters")
    };
    let mut reference_show = workload::show(2, reference.bytecode, params.clone());
    workload::apply_native_automation(&mut reference_show, false);
    reference_show.signals.effects[0].implementation =
        donder_runtime::signal::PreparedEffectImplementation::Dsl {
            program: 0,
            bound_params: params.clone(),
        };
    let mut workspace = show.workspace();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    let mut counts = [0; 4];
    for (index, frame) in [0, 31, 4, 0].into_iter().enumerate() {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = show.evaluate(workload::time(frame), &mut actual, &mut workspace);
        COUNTING.set(false);
        counts[index] = ALLOCATIONS.get();
        result.unwrap();
        show.evaluate(workload::time(frame), &mut expected, &mut show.workspace())
            .unwrap();
        assert_eq!(actual, expected);
        reference_show
            .evaluate(
                workload::time(frame),
                &mut expected,
                &mut reference_show.workspace(),
            )
            .unwrap();
        assert_eq!(actual, expected, "native pulse differs from DSL");
    }
    assert_eq!(counts, [0; 4]);
}

#[test]
fn native_signal_nodes_do_not_displace_upstream_vm_storage() {
    let effect = donder_language::dsl::compile_effects(
        "effect Ramp { color sample() { return rgb(pixel_fraction(), progress(), 0.25); } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let reference = workload::show(2, effect.bytecode.clone(), params.clone());
    let mut show = workload::show(2, effect.bytecode, params);
    let operator = donder_language::dsl::compile_operators(workload::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    workload::apply_operator(&mut show, operator.bytecode, true);
    workload::insert_native_invert(&mut show);
    let mut workspace = show.workspace();
    let mut reference_workspace = reference.workspace();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        reference
            .evaluate(
                workload::time(frame),
                &mut expected,
                &mut reference_workspace,
            )
            .unwrap();
        for value in &mut expected[0] {
            *value = 255 - *value;
        }
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = show.evaluate(workload::time(frame), &mut actual, &mut workspace);
        COUNTING.set(false);
        result.unwrap();
        assert_eq!(actual, expected);
        assert_eq!(ALLOCATIONS.get(), 0, "native node displaced VM storage");
    }
}

#[test]
fn empty_curve_automation_reserves_its_fallback_point() {
    use donder_runtime::signal::{PreparedAutomation, PreparedEffectAutomation};
    use donder_runtime::values::{Curve, SampleDuration, SampleTime};
    let effect = donder_language::dsl::compile_effects("effect Empty { param curve shape; color sample() { return rgb(shape[progress()], 0.0, 0.0); } }").unwrap().remove(0).effect;
    let params = effect
        .bind_params(&IndexMap::from([(
            donder_language::dsl::Identifier::new("shape".into()).unwrap(),
            donder_language::dsl::Value::Curve(Curve { points: vec![] }.into()),
        )]))
        .unwrap();
    let mut show = workload::show(2, effect.bytecode, params);
    show.signals.effects[0].automation = Some(Box::new(PreparedEffectAutomation {
        workspace_slot: 0,
        bindings: vec![PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            curve: Curve { points: vec![] }.into(),
            mapping: donder_runtime::automation::AutomationMapping::Curve { min: 0.5, max: 1.0 },
            param_index: 0,
        }]
        .into(),
    }));
    let mut workspace = show.workspace();
    let mut buffers = [vec![0; 6]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    let result = show.evaluate(workload::time(0), &mut buffers, &mut workspace);
    COUNTING.set(false);
    result.unwrap();
    assert!(buffers[0].iter().any(|&value| value != 0));
    assert_eq!(ALLOCATIONS.get(), 0, "empty automation window allocated");
}

#[test]
fn retained_nested_array_results_forward_without_first_or_repeated_sample_allocations() {
    use donder_language::dsl::{
        GeneratorBinding, GeneratorContext, RunContext, TargetValue, VmWorkspace, compile_effects,
    };
    use donder_language::values::{SampleDuration, SampleTime};
    let generator = compile_effects("effect Parent { void generate() { timeline.emit Child { start: 0.0, duration: 1.0, target: target, values: [[seconds(), seconds() + 1.0], [2.0, 3.0]] }; } }").unwrap()
        .remove(0).generator.unwrap().specialize(&[], &GeneratorContext {
            start_time: SampleTime::from_ticks(0), duration: SampleDuration::from_ticks(1_000_000),
            target: Arc::new(TargetValue { groups: Vec::new() }),
        }, 1).unwrap();
    let GeneratorBinding::Calculation { index, output } = generator.children[0].params[0].1 else {
        panic!("live calculation")
    };
    let calculation = &generator.calculations[index as usize];
    assert!(calculation.inputs.is_empty());
    let child = compile_effects("effect Child { param array<array<float>> values; color sample() { return rgb(values[0][0], values[0][1] * 0.25, values[1][0] * 0.25); } }").unwrap().remove(0).effect;
    let mut vm = VmWorkspace::for_program(&calculation.program);
    let mut child_vm = VmWorkspace::for_program(&child.bytecode);
    let mut results = BoundParams::result_workspace(
        calculation.output_types.len(),
        calculation.program.array_capacity,
        calculation.program.array_width,
    );
    let mut child_params = BoundParams::result_workspace(
        1,
        calculation.program.array_capacity,
        calculation.program.array_width,
    );
    for tick in [0, 750_000, 250_000, 999_999, 0] {
        let context = RunContext {
            progress: tick as f32 / 1_000_000.0,
            time: SampleDuration::from_ticks(tick),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = (|| {
            child_params.clear_results();
            calculation.program.evaluate_bindings(
                &BoundParams::default(),
                &context,
                &mut vm,
                &mut results,
                &calculation.output_types,
            )?;
            child_params.copy_parameter(0, &results, usize::from(output), &child.params[0].ty)?;
            child.sample_bound(&child_params, &context, &mut child_vm)
        })();
        COUNTING.set(false);
        let actual = result.unwrap();
        assert_eq!(ALLOCATIONS.get(), 0);
        let expected_params = child
            .bind_params([(
                &child.params[0].name,
                &results.value(usize::from(output)).unwrap(),
            )])
            .unwrap();
        let expected = child
            .sample_bound(&expected_params, &context, &mut VmWorkspace::default())
            .unwrap();
        assert_eq!(actual, expected);
    }
}
