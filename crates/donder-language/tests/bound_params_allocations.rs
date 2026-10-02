use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use donder_language::dsl::{BoundParams, DslBindCache, Identifier, Value};
use donder_language::sequence::AutomationMapping;
use donder_language::values::{Curve, CurvePoint};
use donder_runtime::SpatialContext;
use indexmap::IndexMap;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

#[allow(dead_code)]
#[path = "support/playback.rs"]
mod playback;

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
    let invocation = playback::sample(&effect, &params);
    let sequence = playback::show(200, &invocation, 1);
    let duration = sequence.duration();
    let mut playback = sequence.into_playback();
    let expected = playback::show(200, &invocation, 1)
        .into_playback()
        .evaluate(playback::time(4))
        .colors()
        .to_vec();
    let black = Color {
        red: 0,
        green: 0,
        blue: 0,
    };
    assert!(expected.iter().any(|&color| color != black));
    for time in [
        playback::time(4),
        SampleTime::from_ticks(duration.as_ticks()),
        SampleTime::from_ticks(u32::MAX),
        playback::time(4),
    ] {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        let result = playback.evaluate(time);
        COUNTING.set(false);
        let colors = result.colors();
        assert_eq!(colors.len(), 200);
        assert_eq!(ALLOCATIONS.get(), 0);
        if time.as_ticks() >= duration.as_ticks() {
            assert!(colors.iter().all(|&color| color == black));
        } else {
            assert_eq!(colors, expected);
        }
    }
}

#[test]
fn warmed_enum_automation_and_constant_arrays_do_not_allocate() {
    use donder_runtime::{Color, PreparedAutomation, SampleDuration, SampleTime};
    let effect = donder_language::dsl::compile_effects(
        "effect Mode {
            param enum mode { short, much_longer_option } = short;
            color sample() {
                if (mode == much_longer_option) { return rgb(1.0, 0.0, 0.0); }
                return rgb(0.0, 0.0, 0.0);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = donder_language::dsl::bind_params(
        effect.params(),
        &IndexMap::new(),
        &mut DslBindCache::default(),
    )
    .unwrap();
    let options =
        ["short", "much_longer_option"].map(|value| Identifier::new(value.into()).unwrap());
    let invocation = playback::sample(&effect, &params)
        .with_automation(
            vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(1_000_000),
                curve: Curve {
                    points: [0.0, 1.0, 0.25, 0.75, 0.0]
                        .into_iter()
                        .enumerate()
                        .map(|(index, value)| CurvePoint {
                            position: index as f32 * 0.25,
                            value,
                        })
                        .collect(),
                }
                .into(),
                mapping: AutomationMapping::Enum {
                    values: options.into(),
                },
                param_index: 0,
            }]
            .into(),
        )
        .unwrap();
    let mut playback = playback::show(2, &invocation, 1).into_playback();
    // Warm the longest option, then verify actual rendered colors while seeking.
    playback.evaluate(SampleTime::from_ticks(250_000));
    let mut observed = [[Color::BLACK; 2]; 5];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for (colors, ticks) in observed
        .iter_mut()
        .zip([0, 250_000, 500_000, 750_000, 1_000_000])
    {
        colors.copy_from_slice(playback.evaluate(SampleTime::from_ticks(ticks)).colors());
    }
    COUNTING.set(false);
    assert_eq!(ALLOCATIONS.get(), 0, "prepared enum automation allocated");
    for (colors, red) in observed.into_iter().zip([0, 255, 0, 255, 0]) {
        assert_eq!(
            colors,
            [Color {
                red,
                green: 0,
                blue: 0
            }; 2]
        );
    }

    let effect = donder_language::dsl::compile_effects(
        "effect Constants { color sample() {
            array<array<float>> values = [[0.1, 0.2], [0.3, 0.4]];
            return rgb(values[0][1], values[1][0], values[1][1]);
        } }",
    )
    .unwrap()
    .remove(0);
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
    .remove(0);
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
fn prepared_calculated_arrays_do_not_allocate_on_the_first_frame() {
    let effect = donder_language::dsl::compile_effects(include_str!(
        "fixtures/array-lifetimes.effect.donder"
    ))
    .unwrap()
    .remove(0);
    let params = donder_language::dsl::bind_params(
        effect.params(),
        &IndexMap::new(),
        &mut donder_runtime::DslBindCache::default(),
    )
    .unwrap();
    let show = playback::show(200, &playback::sample(&effect, &params), 4);
    let mut workspace = show.into_playback();
    let mut buffers = [vec![0; 600]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in buffers
            .iter_mut()
            .zip(workspace.evaluate(playback::time(frame)).outputs())
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
    .remove(0);
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
    .remove(0);
    let params = donder_language::dsl::bind_params(
        effect.params(),
        &IndexMap::new(),
        &mut donder_runtime::DslBindCache::default(),
    )
    .unwrap();
    let sample = playback::sample(&effect, &params);
    let expected = playback::show(2, &sample, 1);
    let operator = donder_language::dsl::compile_operators(
        "operator ManyTimes { input Signal source; color sample() {
            color saved = source.at(seconds());
            for (int i = 0; i < 1100; i = i + 1) { color sampled = source.at(i * 0.001); }
            return max(source.at(seconds() * 0.5), source.at(seconds()));
        } }",
    )
    .unwrap()
    .remove(0);
    let operator = playback::operator(&operator, &BoundParams::default());
    let mut workspace = playback::chain(2, &sample, 1, &[operator]).into_playback();
    let mut expected_workspace = expected.into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected_bytes = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in expected_bytes
            .iter_mut()
            .zip(expected_workspace.evaluate(playback::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(playback::time(frame)).outputs())
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
        let invocation = playback::sample(&effect, &params);
        let prepared = playback::build(200, playback::timing(8_000_000), |builder, target| {
            let window = builder.whole_sequence();
            let effects: Vec<_> = (0..count)
                .map(|_| builder.sample(&invocation, window, target))
                .collect();
            let layer = builder.layer(true, effects);
            builder.output([layer])
        });
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
            .zip(workspace.evaluate(playback::time(0)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        assert_eq!(ALLOCATIONS.get(), 0);
    }
}

#[test]
fn hoisted_resources_and_curve_automation_do_not_allocate_from_the_first_frame() {
    use donder_runtime::PreparedAutomation;
    use donder_runtime::{SampleDuration, SampleTime};
    let (effect, params) = fixtures::uniform_resources();
    let Some(Value::Curve(curve)) = params.iter_values().next() else {
        panic!("first uniform resource must be a curve");
    };
    for recursive in [false, true] {
        let invocation = playback::sample(&effect, &params)
            .with_automation(
                vec![PreparedAutomation {
                    start: SampleTime::from_ticks(0),
                    duration: SampleDuration::from_ticks(8_000_000),
                    curve: curve.clone(),
                    mapping: donder_runtime::AutomationMapping::Curve { min: 0.0, max: 1.0 },
                    param_index: 0,
                }]
                .into(),
            )
            .unwrap();
        let operators = if recursive {
            let operator = donder_language::dsl::compile_operators(playback::IDENTITY_SOURCE)
                .unwrap()
                .remove(0);
            vec![playback::operator(&operator, &BoundParams::default())]
        } else {
            vec![]
        };
        let show = || playback::chain(200, &invocation, 1, &operators);
        let mut workspace = show().into_playback();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in [0, 31, 4, 0] {
            for (snapshot, output) in expected.iter_mut().zip(
                show()
                    .into_playback()
                    .evaluate(playback::time(frame))
                    .outputs(),
            ) {
                snapshot.copy_from_slice(output.bytes);
            }
            ALLOCATIONS.set(0);
            COUNTING.set(true);
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(playback::time(frame)).outputs())
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
    use donder_runtime::{
        Color, Gradient, GradientStop, PreparedAutomation, SampleDuration, SampleTime,
    };
    let pulse = donder_language::dsl::compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap()
    .remove(0);
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.4,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    };
    let params = donder_language::dsl::bind_params(
        pulse.params(),
        [
            (
                Identifier::new("gradient".into()).unwrap(),
                Value::Gradient(
                    Gradient {
                        stops: vec![GradientStop {
                            position: 0.0,
                            color: Color {
                                red: 255,
                                green: 128,
                                blue: 64,
                            },
                        }],
                    }
                    .into(),
                ),
            ),
            (
                Identifier::new("pulse_shape".into()).unwrap(),
                Value::Curve(curve.clone().into()),
            ),
        ]
        .iter()
        .map(|(name, value)| (name, value)),
        &mut donder_runtime::DslBindCache::default(),
    )
    .unwrap();
    let invocation = playback::sample(&pulse, &params)
        .with_automation(
            vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                curve: curve.into(),
                mapping: donder_runtime::AutomationMapping::Curve { min: 0.0, max: 1.0 },
                param_index: 1,
            }]
            .into(),
        )
        .unwrap();
    let show = || playback::show(2, &invocation, 1);
    let mut workspace = show().into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    let mut counts = [0; 4];
    for (index, frame) in [0, 31, 4, 0].into_iter().enumerate() {
        ALLOCATIONS.set(0);
        COUNTING.set(true);
        for (snapshot, output) in actual
            .iter_mut()
            .zip(workspace.evaluate(playback::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        counts[index] = ALLOCATIONS.get();
        for (snapshot, output) in expected.iter_mut().zip(
            show()
                .into_playback()
                .evaluate(playback::time(frame))
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
    .remove(0);
    let params = donder_language::dsl::bind_params(
        effect.params(),
        &IndexMap::new(),
        &mut donder_runtime::DslBindCache::default(),
    )
    .unwrap();
    let sample = playback::sample(&effect, &params);
    let reference = playback::show(2, &sample, 1);
    let operator = donder_language::dsl::compile_operators(playback::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    let invert = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|operator| operator.name().as_str() == "Invert")
    .unwrap();
    let identity = playback::operator(&operator, &BoundParams::default());
    let invert = playback::operator(&invert, &BoundParams::default());
    let operators = [identity.clone(), invert, identity];
    let mut workspace = playback::chain(2, &sample, 1, &operators).into_playback();
    let mut reference_workspace = reference.into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    for frame in [0, 31, 4, 0] {
        for (snapshot, output) in expected.iter_mut().zip(
            reference_workspace
                .evaluate(playback::time(frame))
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
            .zip(workspace.evaluate(playback::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        COUNTING.set(false);
        assert_eq!(actual, expected);
        assert_eq!(ALLOCATIONS.get(), 0, "nested operator displaced VM storage");
    }
}

#[test]
fn empty_curve_automation_preserves_missingness_without_allocating() {
    use donder_runtime::PreparedAutomation;
    use donder_runtime::{Curve, SampleDuration, SampleTime};
    let effect = donder_language::dsl::compile_effects("effect Empty { param curve shape; color sample() { return rgb(shape[progress()], 0.0, 0.0); } }").unwrap().remove(0);
    let params = donder_language::dsl::bind_params(
        effect.params(),
        &IndexMap::from([(
            donder_language::dsl::Identifier::new("shape".into()).unwrap(),
            donder_language::dsl::Value::Curve(Curve { points: vec![] }.into()),
        )]),
        &mut donder_runtime::DslBindCache::default(),
    )
    .unwrap();
    let invocation = playback::sample(&effect, &params)
        .with_automation(
            vec![PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(8_000_000),
                curve: Curve { points: vec![] }.into(),
                mapping: donder_runtime::AutomationMapping::Curve { min: 0.5, max: 1.0 },
                param_index: 0,
            }]
            .into(),
        )
        .unwrap();
    let mut workspace = playback::show(2, &invocation, 1).into_playback();
    let mut buffers = [vec![0; 6]];
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    for (snapshot, output) in buffers
        .iter_mut()
        .zip(workspace.evaluate(playback::time(0)).outputs())
    {
        snapshot.copy_from_slice(output.bytes);
    }
    COUNTING.set(false);
    assert!(buffers[0].iter().all(|&value| value == 0));
    assert_eq!(ALLOCATIONS.get(), 0, "empty automation window allocated");
}
