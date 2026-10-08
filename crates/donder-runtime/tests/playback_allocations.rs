use donder_test_support::fixtures;
use donder_test_support::fixtures::layered;
use donder_test_support::playback::{self, compile_effect};
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::Arc;

use donder_language::compiler::{
    CompiledEffect, Instance, Invocation, ParamDecl, ParamRange, ProgramConstants, compile_effects,
    compile_operators,
};
use donder_runtime::{PreparedSequence, SequenceBuilder, SequenceRoot, TargetHandle};
use donder_runtime_types::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};
use donder_runtime_types::{
    FixtureGeometry, OutputEncoding, PreparedAutomation, RgbOrder, TargetScope,
};
use donder_runtime_types::{Identifier, SampleInvocation, Type, Value};

struct CountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static LIVE_BYTES: Cell<isize> = const { Cell::new(0) };
}

fn record_memory_change(allocated: usize, freed: usize) {
    // Ignore allocations on other test threads and during thread-local teardown.
    if COUNTING.try_with(Cell::get).unwrap_or(false) {
        if allocated != 0 {
            let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        }
        let _ =
            LIVE_BYTES.try_with(|live| live.set(live.get() + allocated as isize - freed as isize));
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

/// Allocation calls `run` makes on this thread.
fn allocations(run: impl FnOnce()) -> usize {
    ALLOCATIONS.set(0);
    COUNTING.set(true);
    run();
    COUNTING.set(false);
    ALLOCATIONS.get()
}

const FRAMES: [usize; 4] = [0, 31, 4, 0];

/// Irregular times through the eight-second sequence, seeking forward and back.
const SPREAD: [u32; 8] = [
    0, 1_037_291, 3_008_333, 4_219_067, 5_489_517, 6_917_003, 7_999_999, 1_037_291,
];

/// Evaluate `SPREAD` from the first frame: allocation calls, and whether any pixel lit.
fn first_frames(sequence: PreparedSequence) -> (usize, bool) {
    let mut playback = sequence.into_playback();
    let mut lit = false;
    let count = allocations(|| {
        for ticks in SPREAD {
            lit |= playback
                .evaluate(SampleTime::from_ticks(ticks))
                .colors()
                .iter()
                .any(|&color| color != Color::BLACK);
        }
    });
    (count, lit)
}

/// Two stacked 8x4 grids of `COUNT` pixels under one whole target, for
/// effects that use layout space, sections and fixture-local addressing.
fn grid(
    build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>, TargetHandle<'id>) -> SequenceRoot<'id>,
) -> PreparedSequence {
    PreparedSequence::build(playback::timing(8_000_000), |builder| {
        let fixtures = [0, 1].map(|fixture| {
            let pixels = (0..32)
                .map(|pixel| [(pixel % 8) as f32, (fixture * 4 + pixel / 8) as f32])
                .collect();
            builder.fixture(fixture, FixtureGeometry::admit(pixels).unwrap())
        });
        let target = builder.target(fixtures, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Grb), None);
        build(builder, target)
    })
}

fn single_effect(sample: &SampleInvocation) -> PreparedSequence {
    grid(|builder, target| {
        let effect = builder.sample(sample, builder.whole_sequence(), target);
        let layer = builder.layer(true, [effect]);
        builder.output([layer])
    })
}

fn identifier(name: &str) -> Identifier {
    Identifier::new(name.into()).unwrap()
}

/// `effect` with named values, defaults for the rest, and `automation`.
fn invoke(
    effect: &CompiledEffect,
    values: &[(&str, Value)],
    automation: Vec<PreparedAutomation>,
) -> Invocation {
    let values: Vec<_> = values
        .iter()
        .map(|(name, value)| (identifier(name), value.clone()))
        .collect();
    let params = donder_language::compiler::bind_params(
        effect.params(),
        values.iter().map(|(name, value)| (name, value)),
    )
    .unwrap();
    effect
        .invoke(params.iter_values().collect(), automation.into())
        .unwrap()
}

fn whole_sequence_automation(
    param: &ParamDecl,
    param_index: usize,
    curve: Arc<Curve>,
) -> PreparedAutomation {
    PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(8_000_000),
        curve,
        mapping: param.automation_mapping().unwrap(),
        quantity: donder_runtime_types::AutomatedQuantity::Value,
        param_index: param_index as u16,
    }
}

fn curve(points: &[(f32, f32)]) -> Arc<Curve> {
    Arc::new(Curve {
        points: points
            .iter()
            .map(|&(position, value)| CurvePoint { position, value })
            .collect(),
    })
}

const RAMP_SOURCE: &str = "effect Ramp { sample { rgb(pixel.fraction, progress, 0.25) } }";

#[test]
fn borrowed_sequence_output_seeks_and_clears_without_allocating() {
    let invocation = playback::lower_sample(&fixtures::uniform_resources());
    let sequence = playback::show(200, &invocation, 1);
    let duration = sequence.duration();
    let mut playback = sequence.into_playback();
    let expected = playback::show(200, &invocation, 1)
        .into_playback()
        .evaluate(playback::time(4))
        .colors()
        .to_vec();
    assert!(expected.iter().any(|&color| color != Color::BLACK));
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
            assert!(colors.iter().all(|&color| color == Color::BLACK));
        } else {
            assert_eq!(colors, expected);
        }
    }
}

#[test]
fn warmed_enum_automation_and_constant_arrays_do_not_allocate() {
    let effect = compile_effect(
        "effect Mode {
            param mode: enum { Short, MuchLongerOption } = Short;
            sample { if mode == MuchLongerOption { rgb(1.0, 0.0, 0.0) } else { rgb(0.0, 0.0, 0.0) } }
        }",
    );
    let automation = whole_sequence_automation(
        &effect.params()[0],
        0,
        curve(
            &[0.0, 1.0, 0.25, 0.75, 0.0]
                .into_iter()
                .enumerate()
                .map(|(index, value)| (index as f32 * 0.25, value))
                .collect::<Vec<_>>(),
        ),
    );
    let invocation = playback::lower_sample(&invoke(
        &effect,
        &[],
        vec![PreparedAutomation {
            duration: SampleDuration::from_ticks(1_000_000),
            ..automation
        }],
    ));
    let mut playback = playback::show(2, &invocation, 1).into_playback();
    // Warm the longest option, then verify actual rendered colors while seeking.
    playback.evaluate(SampleTime::from_ticks(250_000));
    let mut observed = [[Color::BLACK; 2]; 5];
    let count = allocations(|| {
        for (colors, ticks) in observed
            .iter_mut()
            .zip([0, 250_000, 500_000, 750_000, 1_000_000])
        {
            colors.copy_from_slice(playback.evaluate(SampleTime::from_ticks(ticks)).colors());
        }
    });
    assert_eq!(count, 0, "prepared enum automation allocated");
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

    // A pixel-dependent index keeps the constant array at playback.
    let effect = compile_effect(
        "effect Constants { sample {
            let values = [0.1, 0.2, 0.3, 0.4];
            rgb(values[pixel.index + 1], values[pixel.index + 2], values[pixel.index + 3])
        } }",
    );
    let mut workspace = playback::show(1, &playback::sample(&effect), 1).into_playback();
    let expected = workspace.evaluate(SampleTime::from_ticks(0)).colors()[0];
    assert_eq!(
        expected,
        Color {
            red: 51,
            green: 77,
            blue: 102
        }
    );
    let mut sampled = Color::BLACK;
    let count = allocations(|| sampled = workspace.evaluate(SampleTime::from_ticks(0)).colors()[0]);
    assert_eq!(sampled, expected);
    assert_eq!(count, 0, "constant arrays allocated");
}

/// Every reduction kind, guards in and around reduction bodies, and array
/// literals of computed items, independent of the iteration count.
const REDUCTIONS_SOURCE: &str = "effect Reductions {
    param iterations: int in 2..10000 = 4;

    sample {
        guard any for i in 0..iterations { i == iterations - 1 };
        guard all for i in 0..iterations { i < iterations };
        guard (min for i in 0..iterations { i }) == 0 && (sum for i in 0..iterations { 1 }) == iterations;
        let saved = first for i in 0..iterations { progress + i * 0.1 } else { 0.0 };
        let retained = last for i in 0..iterations {
            guard i <= 1;
            [progress, progress + 0.1][i]
        } else { 0.0 };
        let current = max for i in 0..iterations { [0.9, 0.8][i % 2] };
        rgb(saved, retained, current)
    }
}";

#[test]
fn reductions_do_not_allocate_at_any_iteration_count() {
    let effect = compile_effect(REDUCTIONS_SOURCE);
    let mut counts = Vec::new();
    // Fixed counts specialize the loop bounds; automation leaves them to playback.
    let cases = [2, 64, 9_999]
        .map(|iterations| invoke(&effect, &[("iterations", Value::Int(iterations))], vec![]))
        .into_iter()
        .chain([invoke(
            &effect,
            &[],
            vec![whole_sequence_automation(
                &effect.params()[0],
                0,
                curve(&[(0.0, 1.0)]),
            )],
        )]);
    for invocation in cases {
        let mut workspace =
            playback::show(1, &playback::lower_sample(&invocation), 1).into_playback();
        workspace.evaluate(SampleTime::from_ticks(2_000_000));
        let mut results = [Color::BLACK; 2];
        counts.push(allocations(|| {
            results = [2_000_000, 4_000_000]
                .map(|ticks| workspace.evaluate(SampleTime::from_ticks(ticks)).colors()[0]);
        }));
        assert_eq!(
            results,
            [
                Color {
                    red: 64,
                    green: 89,
                    blue: 230
                },
                Color {
                    red: 128,
                    green: 153,
                    blue: 230
                },
            ]
        );
    }
    assert_eq!(
        counts, [0; 4],
        "allocation calls for two samples at 2, 64, 9999 and automated loop iterations"
    );
}

#[test]
fn prepared_reductions_do_not_allocate_on_the_first_frame() {
    let effect = compile_effect(REDUCTIONS_SOURCE);
    let show = playback::show(200, &playback::sample(&effect), 4);
    let mut workspace = show.into_playback();
    let mut buffers = [vec![0; 600]];
    let count = allocations(|| {
        for frame in FRAMES {
            for (snapshot, output) in buffers
                .iter_mut()
                .zip(workspace.evaluate(playback::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
        }
    });
    assert_eq!(count, 0, "prepared reduction evaluation allocated");
}

#[test]
fn enum_choices_and_constant_loads_do_not_allocate() {
    let effect = compile_effect(
        "effect EnumChoices {
            param mode: enum { Short, MuchLongerOption } = Short;
            param late: enum { Short, MuchLongerOption } = MuchLongerOption;
            param gate: enum { Off, On } = On;
            param closed: enum { Off, On } = Off;
            param shape: enum { One, Two } = Two;
            sample {
                let mode = if progress > 0.5 { late } else { mode };
                let gate = if progress < 0.5 { closed } else { gate };
                if mode == MuchLongerOption && gate == On && shape == Two { #ffffff } else { #000000 }
            }
        }",
    );
    // Automation keeps `mode` an enum value at playback.
    let invocation = invoke(
        &effect,
        &[],
        vec![whole_sequence_automation(
            &effect.params()[0],
            0,
            curve(&[(0.0, 0.0)]),
        )],
    );
    let mut workspace = playback::show(1, &playback::lower_sample(&invocation), 1).into_playback();
    workspace.evaluate(SampleTime::from_ticks(0));
    let mut reds = [0; 3];
    let count = allocations(|| {
        for (red, ticks) in reds.iter_mut().zip([6_000_000, 0, 6_000_000]) {
            *red = workspace.evaluate(SampleTime::from_ticks(ticks)).colors()[0].red;
        }
    });
    assert_eq!(count, 0, "enum copies allocated");
    assert_eq!(reds, [255, 0, 255]);
}

#[test]
fn many_signal_times_use_fixed_storage_from_the_first_frame() {
    let sample = playback::sample(&compile_effect(RAMP_SOURCE));
    let expected = playback::show(2, &sample, 1);
    // The ramp brightens with time, so earlier samples never exceed the current one.
    let operator = compile_operators(
        "operator ManyTimes { input source; sample {
            let history = max for i in 0..1100 { source.at(time - i * 0.001) };
            max(history, max(source.at(time * 0.5), source.at(time)))
        } }",
    )
    .unwrap()
    .remove(0);
    let operator = playback::operator(&operator);
    let mut workspace = playback::chain(2, &sample, 1, &[operator]).into_playback();
    let mut expected_workspace = expected.into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected_bytes = [vec![0; 6]];
    for frame in FRAMES {
        for (snapshot, output) in expected_bytes
            .iter_mut()
            .zip(expected_workspace.evaluate(playback::time(frame)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
        let count = allocations(|| {
            for (snapshot, output) in actual
                .iter_mut()
                .zip(workspace.evaluate(playback::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
        });
        assert_eq!(count, 0, "signal cache allocated");
        assert_eq!(actual, expected_bytes);
    }
}

#[test]
fn unautomated_effects_do_not_expand_the_evaluation_workspace() {
    let invocation = playback::lower_sample(&fixtures::uniform_resources());
    let mut expected_bytes = None;
    for count in [1, 16, 128] {
        let prepared = playback::build(200, playback::timing(8_000_000), |builder, target| {
            let window = builder.whole_sequence();
            let effects: Vec<_> = (0..count)
                .map(|_| builder.sample(&invocation, window, target))
                .collect();
            let layer = builder.layer(true, effects);
            builder.output([layer])
        });
        LIVE_BYTES.set(0);
        let mut workspace = None;
        let workspace_allocations = allocations(|| workspace = Some(prepared.into_playback()));
        let mut workspace = workspace.unwrap();
        println!(
            "effects={count} workspace_bytes={} workspace_allocations={workspace_allocations}",
            LIVE_BYTES.get(),
        );
        if let Some(expected) = expected_bytes {
            assert_eq!(LIVE_BYTES.get(), expected);
        } else {
            expected_bytes = Some(LIVE_BYTES.get());
        }
        let mut output = [vec![0; 600]];
        let frame_allocations = allocations(|| {
            for (snapshot, output) in output
                .iter_mut()
                .zip(workspace.evaluate(playback::time(0)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
        });
        assert_eq!(frame_allocations, 0);
    }
}

#[test]
fn hoisted_resources_and_curve_automation_do_not_allocate_from_the_first_frame() {
    // The benchmark's uniform resources, with the level curve automated.
    let effect = compile_effect(
        "effect AutomatedResources { param shape: curve in 0.0..1.0; param colors: gradient; sample {
            colors[shape[progress]] * pixel.fraction
        } }",
    );
    let values: Vec<_> = fixtures::uniform_resources()
        .params()
        .iter_values()
        .collect();
    let Value::Curve(shape) = &values[0] else {
        panic!("the first uniform resource is a curve");
    };
    let automation = whole_sequence_automation(&effect.params()[0], 0, Arc::clone(shape));
    let invocation = playback::lower_sample(&effect.invoke(values, [automation].into()).unwrap());
    for recursive in [false, true] {
        let operators = if recursive {
            let operator = compile_operators(playback::IDENTITY_SOURCE)
                .unwrap()
                .remove(0);
            vec![playback::operator(&operator)]
        } else {
            vec![]
        };
        let show = || playback::chain(200, &invocation, 1, &operators);
        let mut workspace = show().into_playback();
        let mut output = [vec![0; 600]];
        let mut expected = [vec![0; 600]];
        for frame in FRAMES {
            for (snapshot, output) in expected.iter_mut().zip(
                show()
                    .into_playback()
                    .evaluate(playback::time(frame))
                    .outputs(),
            ) {
                snapshot.copy_from_slice(output.bytes);
            }
            let count = allocations(|| {
                for (snapshot, output) in output
                    .iter_mut()
                    .zip(workspace.evaluate(playback::time(frame)).outputs())
                {
                    snapshot.copy_from_slice(output.bytes);
                }
            });
            assert_eq!(count, 0, "resource frame allocated");
            assert_eq!(output, expected);
        }
    }
}

#[test]
fn dsl_curve_automation_releases_previous_sample_before_update() {
    let pulse = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|effect| effect.name().as_str() == "Pulse")
    .unwrap();
    let shape = curve(&[(0.0, 0.0), (0.4, 1.0), (1.0, 0.0)]);
    let gradient = Value::Gradient(Arc::new(Gradient {
        stops: vec![GradientStop {
            position: 0.0,
            color: Color {
                red: 255,
                green: 128,
                blue: 64,
            },
        }],
    }));
    let automation = whole_sequence_automation(&pulse.params()[1], 1, Arc::clone(&shape));
    let invocation = playback::lower_sample(&invoke(
        &pulse,
        &[("gradient", gradient), ("pulse_shape", Value::Curve(shape))],
        vec![automation],
    ));
    let show = || playback::show(2, &invocation, 1);
    let mut workspace = show().into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    let mut counts = [0; 4];
    for (index, frame) in FRAMES.into_iter().enumerate() {
        counts[index] = allocations(|| {
            for (snapshot, output) in actual
                .iter_mut()
                .zip(workspace.evaluate(playback::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
        });
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
    let sample = playback::sample(&compile_effect(RAMP_SOURCE));
    let reference = playback::show(2, &sample, 1);
    let identity = compile_operators(playback::IDENTITY_SOURCE)
        .unwrap()
        .remove(0);
    let invert = compile_operators(include_str!(
        "../../../examples/starter/operators/standard.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|operator| operator.name().as_str() == "Invert")
    .unwrap();
    let identity = playback::operator(&identity);
    let invert = playback::operator(&invert);
    let operators = [identity.clone(), invert, identity];
    let mut workspace = playback::chain(2, &sample, 1, &operators).into_playback();
    let mut reference_workspace = reference.into_playback();
    let mut actual = [vec![0; 6]];
    let mut expected = [vec![0; 6]];
    for frame in FRAMES {
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
        let count = allocations(|| {
            for (snapshot, output) in actual
                .iter_mut()
                .zip(workspace.evaluate(playback::time(frame)).outputs())
            {
                snapshot.copy_from_slice(output.bytes);
            }
        });
        assert_eq!(actual, expected);
        assert_eq!(count, 0, "nested operator displaced VM storage");
    }
}

#[test]
fn empty_curve_automation_preserves_missingness_without_allocating() {
    let effect = compile_effect(
        "effect Empty { param shape: curve in 0.0..1.0; sample { rgb(shape[progress], 0.0, 0.0) } }",
    );
    let empty = curve(&[]);
    let automation = whole_sequence_automation(&effect.params()[0], 0, Arc::clone(&empty));
    let invocation = playback::lower_sample(&invoke(
        &effect,
        &[("shape", Value::Curve(empty))],
        vec![PreparedAutomation {
            mapping: donder_runtime_types::AutomationMapping::Curve { min: 0.5, max: 1.0 },
            quantity: donder_runtime_types::AutomatedQuantity::Value,
            ..automation
        }],
    ));
    let mut workspace = playback::show(2, &invocation, 1).into_playback();
    let mut buffers = [vec![0; 6]];
    let count = allocations(|| {
        for (snapshot, output) in buffers
            .iter_mut()
            .zip(workspace.evaluate(playback::time(0)).outputs())
        {
            snapshot.copy_from_slice(output.bytes);
        }
    });
    assert!(buffers[0].iter().all(|&value| value == 0));
    assert_eq!(count, 0, "empty automation window allocated");
}

const STARTER_EFFECTS: [&str; 7] = [
    include_str!("../../../examples/starter/effects/impact-burst.donder"),
    include_str!("../../../examples/starter/effects/mark-impact-burst.donder"),
    include_str!("../../../examples/starter/effects/scan-sweep.donder"),
    include_str!("../../../examples/starter/effects/shimmer-field.donder"),
    include_str!("../../../examples/starter/effects/sparkle-comet.donder"),
    include_str!("../../../examples/starter/effects/standard.donder"),
    include_str!("../../../examples/starter/effects/vixen.donder"),
];

const OPERATOR_LIBRARIES: [&str; 4] = [
    include_str!("../../../examples/starter/operators/standard.donder"),
    include_str!("../../../examples/starter/operators/gain.donder"),
    include_str!("../../../examples/starter/operators/time-warp.donder"),
    // FreezeFrame and HueMap; its other operators repeat the starter's.
    include_str!("../../../examples/stanford_room/operators/standard.donder"),
];

/// Operators that address pixels explicitly, alone and in reductions.
const ADDRESSED_SOURCE: &str = "
operator Mirror {
  input source;

  sample { source.at(time, target.count - 1 - pixel.index) }
}

operator Scatter {
  input source;
  param taps: int in 1..8 = 4;
  param spacing: float in 0.0..1.0 = 0.05;

  sample {
    let blur = sum for i in 0..taps { source.at(time - spacing * i, pixel.index - i) * 0.25 };
    let darkest = min for i in 0..taps { intensity(source.at_global(time, pixel.index + i)) };
    guard (all for i in 0..taps { pixel.index >= i }) else blur;
    let brightest = first for i in 0..taps {
      guard intensity(source.at(time, i)) > darkest;
      i
    } else { 0 };
    blur * (darkest + 0.5) + source.at_global(time + spacing, brightest)
  }
}";

/// A value an author might supply for a parameter without a default.
fn authored_value(ty: &Type, range: Option<ParamRange>) -> Value {
    let (min, max) = match range {
        Some(ParamRange::Float { min, max }) => (min, max),
        _ => (0.0, 1.0),
    };
    match ty {
        Type::Curve => Value::Curve(curve(
            &[
                (0.0, 0.0),
                (0.12, 1.0),
                (0.45, 0.35),
                (0.78, 0.8),
                (1.0, 0.0),
            ]
            .map(|(position, value)| (position, min + (max - min) * value)),
        )),
        Type::Gradient => Value::Gradient(Arc::new(Gradient {
            stops: [
                (0.0, [255, 32, 16]),
                (0.5, [24, 220, 255]),
                (1.0, [160, 64, 255]),
            ]
            .into_iter()
            .map(|(position, [red, green, blue])| GradientStop {
                position,
                color: Color { red, green, blue },
            })
            .collect(),
        })),
        Type::Marks => Value::Marks(Arc::new(Marks::new(
            (0..32).map(|index| SampleDuration::from_ticks(100_000 + index * 250_000)),
        ))),
        Type::Array(item) => Value::Array(vec![authored_value(item, None); 3].into()),
        ty => panic!("starter parameters of type {ty:?} have defaults"),
    }
}

fn authored_values(params: &[ParamDecl]) -> Vec<Value> {
    params
        .iter()
        .map(|param| {
            param
                .default
                .clone()
                .unwrap_or_else(|| authored_value(&param.ty, param.range))
        })
        .collect()
}

/// Every automatable parameter follows one ramp through the sequence.
fn full_automation(params: &[ParamDecl]) -> Box<[PreparedAutomation]> {
    let ramp = curve(&[(0.0, 0.0), (1.0, 1.0)]);
    params
        .iter()
        .enumerate()
        .filter(|(_, param)| param.supports_automation())
        .map(|(index, param)| whole_sequence_automation(param, index, Arc::clone(&ramp)))
        .collect()
}

const COUNT: usize = 64;

#[test]
fn library_effects_do_not_allocate_from_the_first_frame() {
    // Authored configurations, which must light the grid, and the most
    // general programs, with every automatable parameter automated.
    let mut effects = Vec::new();
    for source in STARTER_EFFECTS {
        for effect in compile_effects(source).unwrap() {
            let values = authored_values(effect.params());
            let name = effect.name().as_str().to_owned();
            effects.push((
                format!("{name} automated"),
                false,
                effect
                    .invoke(values.clone(), full_automation(effect.params()))
                    .unwrap(),
            ));
            effects.push((name, true, effect.invoke(values, Box::new([])).unwrap()));
        }
    }
    // Benchmark configurations, including array literals of computed items.
    for (name, source, params) in fixtures::cases().into_iter().chain(fixtures::layer_cases()) {
        effects.push((
            format!("{name} benchmark"),
            true,
            fixtures::prepared_effect(name, source, params),
        ));
    }
    let constants = ProgramConstants {
        pixel_count: Some(COUNT as i32),
        duration_seconds: Some(8.0),
    };
    let mut allocating = Vec::new();
    let mut dark = Vec::new();
    for (name, authored, invocation) in &effects {
        // As preparation places the clip, and with nothing known about it.
        for (placement, constants) in [
            ("placed", constants),
            ("unplaced", ProgramConstants::default()),
        ] {
            let sample = invocation.instance(constants).sample();
            let (count, lit) = first_frames(single_effect(&sample));
            if count != 0 {
                allocating.push(format!("{name} {placement}: {count}"));
            }
            if *authored && !lit {
                dark.push(format!("{name} {placement}"));
            }
        }
    }
    assert!(allocating.is_empty(), "allocating effects: {allocating:?}");
    assert!(dark.is_empty(), "effects rendered only black: {dark:?}");
}

/// Where an operator input reads from: the chain so far, or the second layer.
#[derive(Clone, Copy, Debug)]
enum Input {
    Chain,
    Second,
}

/// Two layers under an operator graph, each node listing its inputs.
fn operator_show(nodes: &[(Instance, Vec<Input>)]) -> PreparedSequence {
    let first = playback::sample(&compile_effect(RAMP_SOURCE));
    let second = playback::sample(&compile_effect(
        "effect Hues { sample { hsv(pixel.fraction + time * 0.25, 1.0, 1.0) } }",
    ));
    let lowered: Vec<_> = nodes
        .iter()
        .map(|(instance, inputs)| (instance.operator(), inputs))
        .collect();
    grid(|builder, target| {
        let window = builder.whole_sequence();
        let [first, second] = [&first, &second].map(|sample| {
            let effect = builder.sample(sample, window, target);
            builder.layer(true, [effect])
        });
        let mut signal = first;
        for (operator, inputs) in &lowered {
            signal = builder.operator(operator, |input| match inputs[input] {
                Input::Chain => signal,
                Input::Second => second,
            });
        }
        builder.output([signal])
    })
}

#[test]
fn operators_do_not_allocate_from_the_first_frame() {
    let constants = ProgramConstants {
        pixel_count: None,
        duration_seconds: Some(8.0),
    };
    let mut operators: Vec<(String, [Instance; 2], Vec<Input>)> = Vec::new();
    for source in OPERATOR_LIBRARIES.into_iter().chain([ADDRESSED_SOURCE]) {
        for operator in compile_operators(source).unwrap() {
            if operators
                .iter()
                .any(|(name, ..)| name == operator.name().as_str())
            {
                continue;
            }
            let values = authored_values(operator.params());
            let fixed = operator.invoke(values.clone(), Box::new([])).unwrap();
            let automated = operator
                .invoke(values, full_automation(operator.params()))
                .unwrap();
            let inputs: Vec<_> = (0..operator.inputs().len())
                .map(|input| {
                    if input == 0 {
                        Input::Chain
                    } else {
                        Input::Second
                    }
                })
                .collect();
            operators.push((
                operator.name().as_str().to_owned(),
                [fixed, automated].map(|invocation| invocation.instance(constants)),
                inputs,
            ));
        }
    }
    let mut allocating = Vec::new();
    for (name, instances, inputs) in &operators {
        for (automated, instance) in [false, true].into_iter().zip(instances) {
            let (count, lit) = first_frames(operator_show(&[(instance.clone(), inputs.clone())]));
            // Automated delays can reach before the sequence start.
            assert!(lit || automated, "{name} rendered only black");
            if count != 0 {
                allocating.push(format!("{name} automated={automated}: {count}"));
            }
        }
    }
    // Every operator consumes the one before it, separately and fused where
    // preparation would fuse them.
    let chain: Vec<_> = operators
        .iter()
        .map(|(_, [fixed, _], inputs)| (fixed.clone(), inputs.clone()))
        .collect();
    let mut fused: Vec<(Instance, Vec<Input>)> = Vec::new();
    for (instance, inputs) in &chain {
        let upstream = fused.last();
        match upstream.and_then(|(upstream, _)| instance.fuse_input(0, upstream)) {
            Some(combined) => {
                let (_, upstream_inputs) = fused.pop().unwrap();
                let inputs = inputs[1..]
                    .iter()
                    .chain(&upstream_inputs)
                    .copied()
                    .collect();
                fused.push((combined, inputs));
            }
            None => fused.push((instance.clone(), inputs.clone())),
        }
    }
    assert!(fused.len() < chain.len(), "no operator fused");
    let mut separate = operator_show(&chain).into_playback();
    let mut combined = operator_show(&fused).into_playback();
    for ticks in SPREAD {
        let time = SampleTime::from_ticks(ticks);
        assert_eq!(
            combined.evaluate(time).colors(),
            separate.evaluate(time).colors(),
            "fused chain at {ticks} ticks"
        );
    }
    for (name, sequence) in [
        ("chain", operator_show(&chain)),
        ("fused chain", operator_show(&fused)),
        ("layered benchmark", layered::layered_600()),
    ] {
        let (count, lit) = first_frames(sequence);
        assert!(lit, "{name} rendered only black");
        if count != 0 {
            allocating.push(format!("{name}: {count}"));
        }
    }
    assert!(
        allocating.is_empty(),
        "allocating operators: {allocating:?}"
    );
}
