//! The strip interpreter: every pixel of a multi-pixel strip gets exactly the
//! color it gets alone, as a one-pixel strip, however the target is split
//! into strips and whatever mix of scalars and rows its program computes.
use super::evaluation::{
    OperatorEvaluation, PixelContext, SignalSampler, bind, compile_effect, lower_runtime_effect,
    runtime_instance,
};
use super::playback;
use super::std;
use crate::dsl::bytecode::{BytecodeProgram, ContextRead, Instruction, SignalPixel};
use crate::dsl::{
    BoundParams, DslBindCache, NoSignals, RunContext, RuntimeError, STRIP, SampleProgram,
    SpatialContext, Strip, StripSignals, StripWorkspace,
};
use donder_language::dsl::{
    CompiledEffect, ParamDecl, ParamRange, SampleInvocation, Type, Value, compile_effects,
    compile_operators, listing,
};
use donder_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};
use std::prelude::rust_2024::*;

const DURATION: u32 = 8_000_000;

#[derive(Clone, Copy, Debug)]
struct Pixel {
    index: i32,
    fraction: f32,
    position: [f32; 2],
}

/// `count` pixels in rows of 20, and their bounds.
struct Target {
    pixels: Vec<Pixel>,
    min: [f32; 2],
    max: [f32; 2],
}

impl Target {
    fn new(count: usize) -> Self {
        let pixels: Vec<Pixel> = (0..count)
            .map(|index| Pixel {
                index: index as i32,
                fraction: index as f32 / (count - 1).max(1) as f32,
                position: [
                    (index % 20) as f32 * 0.125 - 0.75,
                    (index / 20) as f32 * 0.25 + (index % 3) as f32 * 0.0625,
                ],
            })
            .collect();
        let bound = |axis: usize, pick: fn(f32, f32) -> f32| {
            pixels
                .iter()
                .map(|pixel| pixel.position[axis])
                .reduce(pick)
                .unwrap()
        };
        let min = [bound(0, f32::min), bound(1, f32::min)];
        let max = [bound(0, f32::max), bound(1, f32::max)];
        Self { pixels, min, max }
    }

    fn len(&self) -> usize {
        self.pixels.len()
    }
}

fn run_context(ticks: u32, count: usize) -> RunContext {
    RunContext {
        progress: ticks as f32 / DURATION as f32,
        time: SampleDuration::from_ticks(ticks),
        duration: SampleDuration::from_ticks(DURATION),
        pixel_count: count as i32,
    }
}

/// Strips and lone pixels keep separate workspaces, each shared by every
/// program it runs as playback shares one.
#[derive(Default)]
struct Workspaces {
    strips: StripWorkspace,
    alone: StripWorkspace,
}

/// Whole strips, uneven runs, and runs of one and two pixels.
const SPLITS: [&[usize]; 3] = [&[STRIP], &[37, STRIP, 1, 90], &[2, 1]];
const COUNTS: [usize; 4] = [1, 7, STRIP, 300];
const TICKS: [u32; 5] = [0, 610_000, 3_020_000, 4_470_000, 7_999_999];

/// A named program with its bound parameters.
struct Bound {
    name: String,
    invocation: SampleInvocation,
    params: BoundParams,
}

impl Bound {
    fn new(name: impl Into<String>, invocation: SampleInvocation) -> Self {
        let params = BoundParams::from_validated(invocation.params(), &mut DslBindCache::default());
        Self {
            name: name.into(),
            invocation,
            params,
        }
    }

    fn program(&self) -> &SampleProgram {
        self.invocation.program()
    }

    /// Every pixel of `target` through one strip, in runs of `runs`' lengths
    /// in turn.
    fn strips(
        &self,
        run: &RunContext,
        target: &Target,
        runs: &[usize],
        workspace: &mut StripWorkspace,
    ) -> Vec<Color> {
        let program = self.program().bytecode();
        workspace.reserve(program);
        let mut strip = Strip::new(program, &self.params, run, None, workspace);
        run_strips(&mut strip, target, runs, |_| NoSignals)
    }

    /// Pixel `index` of `target` alone.
    fn alone(
        &self,
        run: &RunContext,
        target: &Target,
        index: usize,
        workspace: &mut StripWorkspace,
    ) -> Color {
        let pixel = target.pixels[index];
        crate::dsl::sample_once(
            self.program(),
            &self.params,
            run,
            (pixel.index, pixel.fraction),
            &SpatialContext {
                position: pixel.position,
                min: target.min,
                max: target.max,
            },
            workspace,
        )
    }

    /// Strips of `target` at `ticks` match its pixels alone in every split of
    /// `splits`; returns the colors.
    fn assert_strips(
        &self,
        ticks: u32,
        target: &Target,
        splits: &[&[usize]],
        workspaces: &mut Workspaces,
    ) -> Vec<Color> {
        let run = run_context(ticks, target.len());
        let expected: Vec<Color> = (0..target.len())
            .map(|index| self.alone(&run, target, index, &mut workspaces.alone))
            .collect();
        for runs in splits {
            let actual = self.strips(&run, target, runs, &mut workspaces.strips);
            assert_same(
                format_args!(
                    "{}: ticks={ticks} count={} runs={runs:?}",
                    self.name,
                    target.len()
                ),
                &actual,
                &expected,
                self.program().bytecode(),
            );
        }
        expected
    }

    /// The colors of a target of `count` pixels at `ticks`, checked against
    /// its pixels alone.
    fn colors(&self, count: usize, ticks: u32, workspaces: &mut Workspaces) -> Vec<Color> {
        self.assert_strips(ticks, &Target::new(count), &SPLITS, workspaces)
    }
}

fn fill(strip: &mut Strip<'_>, pixels: &[Pixel]) {
    let cells = strip.pixels();
    for (offset, pixel) in pixels.iter().enumerate() {
        cells.index[offset].set(pixel.index);
        cells.fraction[offset].set(pixel.fraction);
        cells.x[offset].set(pixel.position[0]);
        cells.y[offset].set(pixel.position[1]);
    }
}

/// Every pixel of `target` through `strip`, in runs of `runs`' lengths in
/// turn; `signals` serves the run that starts at a pixel.
fn run_strips<S: StripSignals>(
    strip: &mut Strip<'_>,
    target: &Target,
    runs: &[usize],
    signals: impl Fn(usize) -> S,
) -> Vec<Color> {
    let mut colors = vec![Color::BLACK; target.len()];
    let mut start = 0;
    for &len in runs.iter().cycle() {
        if start == target.len() {
            break;
        }
        let end = (start + len).min(target.len());
        fill(strip, &target.pixels[start..end]);
        strip.run(
            target.len(),
            target.min,
            target.max,
            &mut signals(start),
            &mut colors[start..end],
        );
        start = end;
    }
    colors
}

/// Strips computed `actual` and lone pixels `expected`.
fn assert_same(
    what: core::fmt::Arguments<'_>,
    actual: &[Color],
    expected: &[Color],
    program: &BytecodeProgram,
) {
    if let Some(pixel) = (0..actual.len()).find(|&pixel| actual[pixel] != expected[pixel]) {
        std::panic!(
            "{what} pixel {pixel}: strip {:?}, alone {:?}\n{}",
            actual[pixel],
            expected[pixel],
            listing(program),
        );
    }
}

fn curve(range: Option<ParamRange>, variant: usize) -> Value {
    let (min, max) = match range {
        Some(ParamRange::Float { min, max }) => (min, max),
        _ => (0.0, 1.0),
    };
    let peak = [0.8, 0.6, 0.45][variant % 3];
    Value::Curve(
        Curve {
            points: [(0.0, 0.0), (0.35, peak), (0.7, 0.3), (1.0, 1.0)]
                .map(|(position, value)| CurvePoint {
                    position,
                    value: min + (max - min) * value,
                })
                .into(),
        }
        .into(),
    )
}

fn gradient(variant: usize) -> Value {
    let colors = ["#ff2000", "#20ff40", "#3040ff"];
    Value::Gradient(
        Gradient {
            stops: [0.0, 0.5, 1.0]
                .into_iter()
                .enumerate()
                .map(|(stop, position)| GradientStop {
                    position,
                    color: Color::from_hex(colors[(stop + variant) % 3]).unwrap(),
                })
                .collect(),
        }
        .into(),
    )
}

/// Marks shortly before several of [`TICKS`].
fn marks() -> Value {
    Value::Marks(
        Marks::new(
            [
                500_000, 1_200_000, 3_000_000, 3_010_000, 4_400_000, 6_050_000,
            ]
            .map(SampleDuration::from_ticks),
        )
        .into(),
    )
}

/// A value for a parameter without a default.
fn required(ty: &Type, range: Option<ParamRange>, variant: usize) -> Value {
    match ty {
        Type::Curve => curve(range, variant),
        Type::Gradient => gradient(variant),
        Type::Marks => marks(),
        Type::Array(item) => Value::Array(
            (0..3)
                .map(|variant| required(item, None, variant))
                .collect::<Vec<_>>()
                .into(),
        ),
        ty => std::panic!("no test value for {ty:?}"),
    }
}

fn value(param: &ParamDecl) -> Value {
    param
        .default
        .clone()
        .unwrap_or_else(|| required(&param.ty, param.range, 0))
}

/// The other options of an enum parameter, or the other value of a bool.
fn alternatives(param: &ParamDecl) -> Vec<Value> {
    match (&param.ty, &param.default) {
        (Type::Enum(options), default) => options
            .iter()
            .map(|option| Value::Enum(option.clone()))
            .filter(|option| Some(option) != default.as_ref())
            .collect(),
        (Type::Bool, Some(Value::Bool(value))) => vec![Value::Bool(!value)],
        _ => Vec::new(),
    }
}

/// The sources of the starter project's `directory` whose names end in
/// `extension`.
fn starter_sources(directory: &str, extension: &str) -> Vec<String> {
    let directory = format!(
        "{}/../../examples/starter/{directory}",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut paths: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.to_string_lossy().ends_with(extension))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| std::fs::read_to_string(path).unwrap())
        .collect()
}

fn starter_effects() -> Vec<CompiledEffect> {
    let effects: Vec<_> = starter_sources("effects", ".effect.donder")
        .iter()
        .flat_map(|source| compile_effects(source).unwrap())
        .collect();
    assert!(effects.len() >= 20, "{} starter effects", effects.len());
    effects
}

/// Every starter effect with its defaults, prepared and left to playback, and
/// with each other enum option and bool value.
#[test]
fn starter_effects_compute_each_pixel_of_a_strip_as_alone() {
    let mut workspaces = Workspaces::default();
    let targets = COUNTS.map(Target::new);
    let variant_target = Target::new(STRIP + 9);
    for effect in starter_effects() {
        let name = effect.name().as_str();
        let params = effect.params();
        let values: Vec<(&str, Value)> = params
            .iter()
            .map(|param| (param.name.as_str(), value(param)))
            .collect();
        // Playback cannot automate the other parameters, so they stay fixed.
        let (runtime, fixed): (Vec<_>, Vec<_>) = values
            .iter()
            .cloned()
            .zip(params)
            .partition(|(_, param)| param.supports_automation());
        let runtime: Vec<_> = runtime.into_iter().map(|(value, _)| value).collect();
        let fixed: Vec<_> = fixed.into_iter().map(|(value, _)| value).collect();
        let (instance, slots) = runtime_instance(&effect, &fixed, &runtime);
        for (how, invocation) in [
            ("prepared", playback::lower_sample(&bind(&effect, &values))),
            ("runtime", lower_runtime_effect(&instance, slots.clone())),
        ] {
            let bound = Bound::new(format!("{name} {how}"), invocation);
            for target in &targets {
                for ticks in TICKS {
                    bound.assert_strips(ticks, target, &SPLITS, &mut workspaces);
                }
            }
        }
        for (index, param) in params.iter().enumerate() {
            let slot = runtime
                .iter()
                .position(|(runtime, _)| *runtime == param.name.as_str());
            for alternative in alternatives(param) {
                let what = format!("{name} {}={alternative:?}", param.name.as_str());
                let mut values = values.clone();
                values[index].1 = alternative.clone();
                let mut slots = slots.clone();
                slots[slot.unwrap()] = alternative;
                for (how, invocation) in [
                    ("prepared", playback::lower_sample(&bind(&effect, &values))),
                    ("runtime", lower_runtime_effect(&instance, slots)),
                ] {
                    let bound = Bound::new(format!("{what} {how}"), invocation);
                    for ticks in [TICKS[1], TICKS[2]] {
                        bound.assert_strips(ticks, &variant_target, &SPLITS[..2], &mut workspaces);
                    }
                }
            }
        }
    }
}

fn has_branch(invocation: &SampleInvocation, row: bool) -> bool {
    let body = invocation.program().bytecode().body();
    body.iter().any(|instruction| match instruction {
        Instruction::Branch { condition, .. } => condition.is_scalar() != row,
        _ => false,
    })
}

fn has_row_branch(invocation: &SampleInvocation) -> bool {
    has_branch(invocation, true)
}

fn has_row_and_scalar_branches(invocation: &SampleInvocation) -> bool {
    has_branch(invocation, true) && has_branch(invocation, false)
}

fn any_program(_: &SampleInvocation) -> bool {
    true
}

/// `source`'s only effect with its defaults, whose program `check` accepts,
/// computes each pixel as alone at every count and time.
fn assert_program(
    source: &str,
    check: fn(&SampleInvocation) -> bool,
    workspaces: &mut Workspaces,
) -> Bound {
    let invocation = playback::lower_sample(&bind(&compile_effect(source), &[]));
    assert!(
        check(&invocation),
        "{source}\n{}",
        listing(invocation.program().bytecode())
    );
    let bound = Bound::new(source, invocation);
    for count in [1, 2, 7, 64, STRIP - 1, STRIP, STRIP + 1, 300] {
        let target = Target::new(count);
        for ticks in TICKS {
            bound.assert_strips(ticks, &target, &SPLITS, workspaces);
        }
    }
    bound
}

fn rgb(red: f32, green: f32, blue: f32) -> Color {
    crate::sampling::rgb(red, green, blue)
}

#[test]
fn interleaved_per_pixel_branches_take_each_pixels_arm() {
    let mut workspaces = Workspaces::default();
    let interleaved = assert_program(
        "effect Interleaved { sample {
            let i = pixel.index;
            if i % 2 == 0 {
                rgb(sin(i * 0.37) * 0.5 + 0.5, pixel.fraction * 0.75, cos(i * 0.21) * 0.25 + 0.5)
            } else if i % 3 == 0 {
                hsv(pixel.fraction + progress, 0.75, 0.5 + 0.5 * sin(i * 1.3))
            } else {
                rgb(pixel.fraction, progress, (i % 7) / 8.0)
            }
        } }",
        has_row_branch,
        &mut workspaces,
    );
    // Odd pixels that are not multiples of three take the last arm.
    let colors = interleaved.colors(9, 0, &mut workspaces);
    assert_eq!(colors[5], rgb(5.0 / 8.0, 0.0, 5.0 / 8.0));
    assert_eq!(colors[7], rgb(7.0 / 8.0, 0.0, 0.0));
    let exact = assert_program(
        "effect Exact { sample {
            let i = pixel.index;
            if i % 3 == 0 {
                let a = i * 0.0625;
                rgb(a, a * 0.5 + 0.125, 1.0 - a)
            } else if i % 3 == 1 {
                let b = (i + 1) * 0.03125;
                rgb(0.25, b, b + b * 0.5)
            } else {
                rgb(progress, i / 16.0, 0.5 - progress * 0.5)
            }
        } }",
        has_row_branch,
        &mut workspaces,
    );
    let progress = 0.5;
    for (index, color) in exact
        .colors(12, DURATION / 2, &mut workspaces)
        .iter()
        .enumerate()
    {
        let i = index as f32;
        let expected = match index % 3 {
            0 => rgb(i * 0.0625, i * 0.0625 * 0.5 + 0.125, 1.0 - i * 0.0625),
            1 => {
                let b = (i + 1.0) * 0.03125;
                rgb(0.25, b, b + b * 0.5)
            }
            _ => rgb(progress, i / 16.0, 0.5 - progress * 0.5),
        };
        assert_eq!(*color, expected, "pixel {index}");
    }
}

#[test]
fn nested_branches_and_uniform_branches_inside_per_pixel_arms() {
    let mut workspaces = Workspaces::default();
    let source = "effect Nested {
        param level: float in 0.0..1.0 = 0.5;
        sample {
            let i = pixel.index;
            if i % 2 == 0 {
                if i % 4 == 0 {
                    rgb(i / 64.0, 0.25 + level * 0.5, pixel.fraction * 0.5)
                } else {
                    rgb(0.75 - i / 128.0, pixel.fraction, level * level)
                }
            } else if progress > 0.5 {
                rgb(pixel.fraction * 0.25 + 0.5, progress * 0.5, i / 256.0)
            } else if level > 0.25 {
                rgb(progress, 1.0 - pixel.fraction, (i % 5) / 4.0)
            } else {
                rgb(0.0625, 1.0 - pixel.fraction * 0.5, (i % 3) / 4.0)
            }
        }
    }";
    let nested = assert_program(source, has_row_and_scalar_branches, &mut workspaces);
    let early = nested.colors(9, DURATION / 8, &mut workspaces);
    let late = nested.colors(9, DURATION / 8 * 6, &mut workspaces);
    for (index, (early, late)) in early.iter().zip(&late).enumerate() {
        let (i, f) = (index as f32, index as f32 / 8.0);
        let (early_expected, late_expected) = match index % 4 {
            0 => {
                let color = rgb(i / 64.0, 0.5, f * 0.5);
                (color, color)
            }
            2 => {
                let color = rgb(0.75 - i / 128.0, f, 0.25);
                (color, color)
            }
            _ => (
                rgb(0.125, 1.0 - f, (index % 5) as f32 / 4.0),
                rgb(f * 0.25 + 0.5, 0.375, i / 256.0),
            ),
        };
        assert_eq!(*early, early_expected, "early pixel {index}");
        assert_eq!(*late, late_expected, "late pixel {index}");
    }
    // A condition on a parameter left to playback is uniform too.
    let effect = compile_effect(source);
    for level in [0.125, 0.75] {
        let (instance, slots) = runtime_instance(&effect, &[], &[("level", Value::Float(level))]);
        let nested = Bound::new(
            format!("{source} level={level}"),
            lower_runtime_effect(&instance, slots),
        );
        assert!(has_row_and_scalar_branches(&nested.invocation));
        for count in [7, STRIP + 3] {
            let target = Target::new(count);
            for ticks in [DURATION / 8, DURATION / 8 * 6] {
                nested.assert_strips(ticks, &target, &SPLITS, &mut workspaces);
            }
        }
        let colors = nested.colors(9, DURATION / 8, &mut workspaces);
        let f = 1.0 / 8.0;
        assert_eq!(
            colors[1],
            if level > 0.25 {
                rgb(0.125, 1.0 - f, 0.25)
            } else {
                rgb(0.0625, 1.0 - f * 0.5, 0.25)
            },
            "level={level}"
        );
    }
}

/// `sum`, `max` and `min` over uniform bounds with per-pixel filters.
#[test]
fn reductions_filter_each_pixel() {
    let mut workspaces = Workspaces::default();
    let filtered = assert_program(
        "effect Filtered { sample {
            let i = pixel.index;
            let total = sum for k in 0..8 { guard (k + i) % 3 != 0; k + 1 };
            let lowest = min for k in 0..6 { guard k != i % 3; (k * 5 + i) % 9 };
            let level = max for k in 0..3 { guard i % 3 != k; pixel.fraction * (k + 1) * 0.25 };
            rgb(total / 64.0, lowest / 16.0, level)
        } }",
        any_program,
        &mut workspaces,
    );
    let count = 40;
    for (index, color) in filtered
        .colors(count, 0, &mut workspaces)
        .iter()
        .enumerate()
    {
        let i = index as i32;
        let total: i32 = (0..8).filter(|k| (k + i) % 3 != 0).map(|k| k + 1).sum();
        let lowest = (0..6)
            .filter(|&k| k != i % 3)
            .map(|k| (k * 5 + i) % 9)
            .min()
            .unwrap();
        let fraction = index as f32 / (count - 1) as f32;
        let level = (0..3)
            .filter(|k| i % 3 != *k)
            .map(|k| fraction * (k + 1) as f32 * 0.25)
            .fold(f32::NEG_INFINITY, f32::max);
        assert_eq!(
            *color,
            rgb(total as f32 / 64.0, lowest as f32 / 16.0, level),
            "pixel {index}"
        );
    }
}

/// `sum`, `max` and `min` whose bounds vary by pixel, and nested reductions
/// whose inner bounds follow the outer index.
#[test]
fn reductions_bound_each_pixel() {
    let mut workspaces = Workspaces::default();
    let bounded = assert_program(
        "effect Bounded { sample {
            let i = pixel.index;
            let total = sum for k in 0..i % 5 { k + 1 };
            let highest = max for k in i % 3..6 { (k * 7 + i) % 11 };
            let lowest = min for k in i % 2..i % 7 + 1 { guard k != i % 3; (k * 5 + i) % 9 };
            rgb(total / 16.0, highest / 16.0, if lowest > 100 { 1.0 } else { lowest / 16.0 })
        } }",
        any_program,
        &mut workspaces,
    );
    for (index, color) in bounded.colors(40, 0, &mut workspaces).iter().enumerate() {
        let i = index as i32;
        let total: i32 = (0..i % 5).map(|k| k + 1).sum();
        let highest = (i % 3..6).map(|k| (k * 7 + i) % 11).max().unwrap();
        let lowest = (i % 2..i % 7 + 1)
            .filter(|&k| k != i % 3)
            .map(|k| (k * 5 + i) % 9)
            .min()
            .unwrap_or(i32::MAX);
        let blue = if lowest > 100 {
            1.0
        } else {
            lowest as f32 / 16.0
        };
        assert_eq!(
            *color,
            rgb(total as f32 / 16.0, highest as f32 / 16.0, blue),
            "pixel {index}"
        );
    }
    let nested = assert_program(
        "effect Nested { sample {
            let i = pixel.index;
            let triangle = sum for a in 0..i % 4 { sum for b in 0..a + 1 { guard (b + i) % 2 == 0; b + 1 } };
            let steps = sum for a in 0..3 { sum for b in a..a + i % 3 + 1 { b * 2 } };
            rgb(triangle / 32.0, steps / 64.0, 0.0)
        } }",
        any_program,
        &mut workspaces,
    );
    for (index, color) in nested.colors(30, 0, &mut workspaces).iter().enumerate() {
        let i = index as i32;
        let triangle: i32 = (0..i % 4)
            .map(|a| {
                (0..a + 1)
                    .filter(|b| (b + i) % 2 == 0)
                    .map(|b| b + 1)
                    .sum::<i32>()
            })
            .sum();
        let steps: i32 = (0..3)
            .map(|a| (a..a + i % 3 + 1).map(|b| b * 2).sum::<i32>())
            .sum();
        assert_eq!(
            *color,
            rgb(triangle as f32 / 32.0, steps as f32 / 64.0, 0.0),
            "pixel {index}"
        );
    }
}

/// A reduction reads its bounds in every iteration, so its loop must not
/// overwrite them: a per-pixel start, a per-pixel end, and the end of a
/// counting-down `last`.
#[test]
fn reduction_bounds_hold_through_every_iteration() {
    let mut workspaces = Workspaces::default();
    let invocation = playback::lower_sample(&bind(
        &compile_effect(
            "effect Bounds { sample {
                let i = pixel.index;
                let highest = max for k in i % 3..6 { (k * 7) % 11 };
                let total = sum for k in 0..i % 4 { i + k };
                let latest = sum for a in 0..4 {
                    last for b in 0..a + 1 { guard int(progress * 8.0 + b) % 2 == 0; b } else { 0 }
                };
                rgb(highest / 16.0, total / 64.0, latest / 16.0)
            } }",
        ),
        &[],
    ));
    let bound = Bound::new("Bounds", invocation);
    let target = Target::new(8);
    let run = run_context(0, target.len());
    for index in 0..target.len() {
        let i = index as i32;
        let highest = (i % 3..6).map(|k| (k * 7) % 11).max().unwrap();
        let total: i32 = (0..i % 4).map(|k| i + k).sum();
        let latest: i32 = (0..4)
            .map(|a| (0..a + 1).rev().find(|b| b % 2 == 0).unwrap_or(0))
            .sum();
        assert_eq!(
            bound.alone(&run, &target, index, &mut workspaces.alone),
            rgb(
                highest as f32 / 16.0,
                total as f32 / 64.0,
                latest as f32 / 16.0
            ),
            "pixel {index}\n{}",
            listing(bound.program().bytecode())
        );
    }
    bound.assert_strips(0, &target, &SPLITS, &mut workspaces);
}

fn flags(some: bool, every: bool) -> f32 {
    (if some { 0.5 } else { 0.0 }) + (if every { 0.25 } else { 0.0 })
}

/// `first`, `last`, `any` and `all` over uniform bounds stop each pixel at
/// its own iteration.
#[test]
fn early_exit_reductions_decide_each_pixel() {
    let mut workspaces = Workspaces::default();
    let early = assert_program(
        "effect Early { sample {
            let i = pixel.index;
            let first_hit = first for k in 0..12 { guard (k * 5 + i) % 7 == 0; k } else { -1 };
            let last_hit = last for k in 0..12 { guard (k * 3 + i) % 5 == 1; k } else { -1 };
            let some = any for k in 0..10 { (k + i) % 9 == 0 };
            let every = all for k in 0..6 { (k * 2 + i) % 13 != 0 };
            rgb((first_hit + 1) / 16.0, (last_hit + 1) / 16.0,
                (if some { 0.5 } else { 0.0 }) + (if every { 0.25 } else { 0.0 }))
        } }",
        any_program,
        &mut workspaces,
    );
    for (index, color) in early.colors(40, 0, &mut workspaces).iter().enumerate() {
        let i = index as i32;
        let first_hit = (0..12).find(|k| (k * 5 + i) % 7 == 0).unwrap_or(-1);
        let last_hit = (0..12).rev().find(|k| (k * 3 + i) % 5 == 1).unwrap_or(-1);
        let some = (0..10).any(|k| (k + i) % 9 == 0);
        let every = (0..6).all(|k| (k * 2 + i) % 13 != 0);
        assert_eq!(
            *color,
            rgb(
                (first_hit + 1) as f32 / 16.0,
                (last_hit + 1) as f32 / 16.0,
                flags(some, every)
            ),
            "pixel {index}"
        );
    }
    // Colors and floats, with fallbacks.
    assert_program(
        "effect Values { sample {
            let i = pixel.index;
            let shade = first for k in 0..5 { guard (k + i) % 4 == 0; [#ff0000, #00ff00, #0000ff, #808080, #ffffff][k] } else { #102030 };
            let level = last for k in 0..9 { guard (k + i) % 4 == 1; pixel.fraction * k * 0.125 } else { progress };
            shade * level
        } }",
        any_program,
        &mut workspaces,
    );
}

/// `first`, `last`, `any` and `all` whose bounds vary by pixel.
#[test]
fn early_exit_reductions_with_per_pixel_bounds() {
    let mut workspaces = Workspaces::default();
    let bounded = assert_program(
        "effect Bounded { sample {
            let i = pixel.index;
            let first_hit = first for k in 0..i % 6 { guard (k + i) % 4 == 3; k } else { -1 };
            let last_hit = last for k in i % 4..10 { guard (k + i) % 3 == 0; k } else { -1 };
            let some = any for k in 0..i % 7 { (k * 3 + i) % 8 == 5 };
            let every = all for k in i % 3..5 { guard k != 2; (k + i) % 6 != 0 };
            rgb((first_hit + 1) / 16.0, (last_hit + 1) / 16.0,
                (if some { 0.5 } else { 0.0 }) + (if every { 0.25 } else { 0.0 }))
        } }",
        any_program,
        &mut workspaces,
    );
    for (index, color) in bounded.colors(40, 0, &mut workspaces).iter().enumerate() {
        let i = index as i32;
        let first_hit = (0..i % 6).find(|k| (k + i) % 4 == 3).unwrap_or(-1);
        let last_hit = (i % 4..10).rev().find(|k| (k + i) % 3 == 0).unwrap_or(-1);
        let some = (0..i % 7).any(|k| (k * 3 + i) % 8 == 5);
        let every = (i % 3..5).filter(|&k| k != 2).all(|k| (k + i) % 6 != 0);
        assert_eq!(
            *color,
            rgb(
                (first_hit + 1) as f32 / 16.0,
                (last_hit + 1) as f32 / 16.0,
                flags(some, every)
            ),
            "pixel {index}"
        );
    }
    assert_program(
        "effect Values { sample {
            let level = last for k in 0..pixel.index % 9 { guard k % 2 == 1; pixel.fraction * k * 0.125 } else { progress };
            #ffc080 * level
        } }",
        any_program,
        &mut workspaces,
    );
}

/// Arithmetic, comparisons, choices and clamps over every mix of scalar and
/// row operands.
#[test]
fn scalar_and_row_operands_mix_in_every_position() {
    let mut workspaces = Workspaces::default();
    for body in [
        // Subtraction and division in both orders, and of two rows.
        "rgb(progress - pixel.fraction + 0.5, pixel.fraction - progress + 0.5, pixel.fraction * pixel.x)",
        "rgb(progress / (pixel.fraction + 1.0), (pixel.fraction + 1.0) / (progress + 1.0), pixel.y / (pixel.x + 2.0))",
        "rgb(atan2(progress, pixel.fraction), atan2(pixel.fraction, progress), pixel.fraction % (progress + 0.1))",
        // Comparisons of a scalar and a row both ways, of two rows, and of ints.
        "rgb(if progress < pixel.fraction { 1.0 } else { 0.25 }, if pixel.fraction < progress { 0.75 } else { 0.0 }, if pixel.x < pixel.y { 0.5 } else { 0.125 })",
        "rgb(if 3 < pixel.index { 1.0 } else { 0.0 }, if pixel.index <= 9 { 0.5 } else { 0.25 }, if pixel.index % 3 == 1 { 0.75 } else { 0.125 })",
        "rgb(if pixel.fraction >= progress { 0.5 } else { 0.0 }, if progress != pixel.fraction { 0.25 } else { 1.0 }, if 0.5 > pixel.fraction { 1.0 } else { 0.0 })",
        // Choices of scalars and rows, on scalar and row conditions.
        "rgb(if progress < pixel.fraction { progress } else { pixel.fraction }, if progress > 0.5 { pixel.fraction } else { 0.25 }, if pixel.index % 2 == 0 { 0.75 } else { 0.125 })",
        "if pixel.index % 3 == 0 { #ff8000 } else { rgb(pixel.fraction, progress, 0.5) }",
        "if progress > 0.375 { rgb(pixel.fraction, 0.0, 0.0) } else { #0080ff }",
        // Clamps with each operand a row.
        "rgb(clamp(pixel.fraction, 0.2, 0.8), clamp(0.5, pixel.fraction * 0.5, 0.9), clamp(progress, 0.1, pixel.fraction + 0.1))",
        "rgb(clamp(pixel.fraction, pixel.y * 0.125, progress + 0.75), clamp(progress, pixel.fraction * 0.5, pixel.fraction + 0.5), clamp(pixel.x, -0.25, pixel.y + 0.5))",
        // Mixes, items and integer arithmetic.
        "rgb(mix(progress, pixel.fraction, 0.25), mix(pixel.fraction, 0.5, progress), mix(0.125, progress, pixel.fraction))",
        "rgb([progress, pixel.fraction, 0.25][pixel.index % 3], [0.5, pixel.y][pixel.index], (pixel.index * 3 - 7) / 64.0 + (20 - pixel.index) / 64.0)",
        "rgb((pixel.index % 4) / 4.0, ((0 - pixel.index) % 5) / 8.0, min(pixel.fraction, progress) + max(progress * 0.5, pixel.y))",
        // Colors of rows and scalars.
        "max(rgb(pixel.fraction, 0.0, 0.0) + #102030, #808080 * pixel.fraction) * mix(#ff0000, rgb(0.0, pixel.fraction, 0.0), progress)",
        "hsv(progress + pixel.fraction, 0.5 + pixel.y * 0.25, 1.0 - progress * 0.5) * invert(rgb(progress, pixel.fraction, 0.25))",
    ] {
        assert_program(
            &format!("effect Mix {{ sample {{ {body} }} }}"),
            any_program,
            &mut workspaces,
        );
    }
    let exact = assert_program(
        "effect Exact { sample {
            let f = pixel.fraction;
            rgb(
                if progress < f { progress } else { f },
                clamp(0.25, f, 1.0),
                [progress, f, 0.25][pixel.index % 3]
            )
        } }",
        any_program,
        &mut workspaces,
    );
    let progress = 0.5;
    for (index, color) in exact
        .colors(9, DURATION / 2, &mut workspaces)
        .iter()
        .enumerate()
    {
        let f = index as f32 / 8.0;
        let item = [progress, f, 0.25][index % 3];
        assert_eq!(
            *color,
            rgb(progress.min(f), f.max(0.25), item),
            "pixel {index}"
        );
    }
}

/// A strip whose target changes shape runs its target block again: each run
/// sees its own pixel count and bounds.
#[test]
fn a_new_target_shape_reinitializes_the_target_block() {
    for source in [
        "effect Shape { sample {
            rgb(pixel.index / target.count, target.count / 256.0,
                (target.max_x - target.min_x) / 8.0 + section_count(3) / 128.0)
        } }",
        "effect Uniform { sample {
            rgb(target.count / 256.0, (target.max_x - target.min_x) / 4.0, target.min_y + 0.5)
        } }",
    ] {
        let bound = Bound::new(
            source,
            playback::lower_sample(&bind(&compile_effect(source), &[])),
        );
        let program = bound.program().bytecode();
        let (_, target_block) = program.prefix();
        assert!(
            target_block.iter().any(|instruction| matches!(
                instruction,
                Instruction::Context {
                    read: ContextRead::PixelCount,
                    ..
                }
            )),
            "{}",
            listing(program)
        );
        // Fewer pixels than a strip, more, the first count again, and the
        // same count within other bounds.
        let small = Target::new(50);
        let large = Target::new(80);
        let mut wider = Target::new(80);
        wider.min[0] -= 0.5;
        wider.max[0] += 1.0;
        let runs: [(&Target, core::ops::Range<usize>); 5] = [
            (&small, 0..50),
            (&large, 0..80),
            (&small, 10..30),
            (&wider, 40..80),
            (&large, 20..80),
        ];
        let mut workspaces = Workspaces::default();
        workspaces.strips.reserve(program);
        let mut strip = Strip::new(
            program,
            &bound.params,
            &run_context(DURATION / 4, 1),
            None,
            &mut workspaces.strips,
        );
        for (target, pixels) in runs {
            let mut colors = vec![Color::BLACK; pixels.len()];
            fill(&mut strip, &target.pixels[pixels.clone()]);
            strip.run(
                target.len(),
                target.min,
                target.max,
                &mut NoSignals,
                &mut colors,
            );
            let run = run_context(DURATION / 4, target.len());
            for (color, index) in colors.iter().zip(pixels) {
                assert_eq!(
                    *color,
                    bound.alone(&run, target, index, &mut workspaces.alone),
                    "{source}: count {} pixel {index}",
                    target.len()
                );
            }
        }
    }
}

/// A signal with a color of its own at each input, time and pixel.
fn signal(input: usize, time: SampleTime, pixel: usize) -> Color {
    let (ticks, pixel) = (time.as_ticks(), pixel as u32);
    Color {
        red: (ticks / 7_919 + pixel * 13) as u8,
        green: (input as u32 * 101 + pixel * 7 + ticks / 100_000) as u8,
        blue: ((ticks / 1_000) ^ pixel) as u8,
    }
}

/// A query from pixel `current` of a fixture of `count` pixels. Global
/// queries read other colors than local ones.
fn query(
    input: usize,
    time: SampleTime,
    current: usize,
    count: usize,
    pixel: SignalPixel<i32>,
) -> Color {
    let (input, index) = match pixel {
        SignalPixel::Current => (input, Some(current)),
        SignalPixel::Local(index) => (input, usize::try_from(index).ok()),
        SignalPixel::Global(index) => (input + 7, usize::try_from(index).ok()),
    };
    index
        .filter(|&index| index < count)
        .map_or(Color::BLACK, |index| signal(input, time, index))
}

/// The signals of one lone pixel.
struct Lone {
    pixel: usize,
    count: usize,
}

impl SignalSampler for Lone {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        Ok(query(input, time, self.pixel, self.count, pixel))
    }
}

/// The signals of a strip whose first pixel is `first`.
struct Run {
    first: usize,
    count: usize,
}

impl StripSignals for Run {
    fn sample_strip(
        &mut self,
        input: usize,
        time: SampleTime,
        _: Option<usize>,
        output: &mut [Color; STRIP],
    ) {
        for (offset, color) in output.iter_mut().enumerate() {
            *color = signal(input, time, self.first + offset);
        }
    }

    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        offset: usize,
        pixel: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Color {
        query(input, time, self.first + offset, self.count, pixel)
    }
}

/// Starter operators, and samples at per-pixel times and addresses, inside
/// per-pixel branches and inside reductions with per-pixel bounds.
#[test]
fn operator_samples_in_strips_match_lone_pixels() {
    let mut sources = starter_sources("operators", ".operator.donder");
    sources.push(
        "operator Smear { input source; sample { source.at(time - pixel.fraction * 0.5) } }
        operator Shift { input source; sample {
            max(source.at(time, pixel.index + 1), source.at(time - 0.125, pixel.index - 2))
        } }
        operator Mirror { input source; sample { source.at_global(time, target.count - 1 - pixel.index) } }
        operator Split { input source; input other; sample {
            if pixel.index % 2 == 0 { source.at(time) * 0.75 + #101010 }
            else { invert(other.at(time - 0.25)) * pixel.fraction }
        } }
        operator Trail { input source; sample {
            max for k in 0..pixel.index % 4 + 1 { source.at(time - k * 0.125) * (1.0 - k * 0.25) }
        } }
        operator Gate { input source; input other; sample {
            guard intensity(source) > 0.5 else other.at(time, pixel.index - pixel.index % 2);
            source
        } }"
        .into(),
    );
    let operators: Vec<_> = sources
        .iter()
        .flat_map(|source| compile_operators(source).unwrap())
        .collect();
    assert!(operators.len() >= 18, "{} operators", operators.len());
    let mut workspaces = Workspaces::default();
    for operator in &operators {
        let invocation = playback::operator(operator);
        let params = BoundParams::from_validated(invocation.params(), &mut DslBindCache::default());
        let program = invocation.program().bytecode();
        for count in COUNTS {
            let target = Target::new(count);
            for ticks in [0, 120_000, 3_020_000, 7_999_999] {
                let run = run_context(ticks, count);
                let expected: Vec<Color> = target
                    .pixels
                    .iter()
                    .enumerate()
                    .map(|(index, pixel)| {
                        let context = PixelContext {
                            run,
                            index: pixel.index,
                            fraction: pixel.fraction,
                        };
                        let spatial = SpatialContext {
                            position: pixel.position,
                            min: target.min,
                            max: target.max,
                        };
                        let mut signals = Lone {
                            pixel: index,
                            count,
                        };
                        invocation
                            .evaluate(&context, &spatial, &mut signals, &mut workspaces.alone)
                            .unwrap()
                    })
                    .collect();
                for runs in SPLITS {
                    workspaces.strips.reserve(program);
                    let mut strip =
                        Strip::new(program, &params, &run, None, &mut workspaces.strips);
                    let actual =
                        run_strips(&mut strip, &target, runs, |first| Run { first, count });
                    assert_same(
                        format_args!(
                            "{}: ticks={ticks} count={count} runs={runs:?}",
                            operator.name().as_str()
                        ),
                        &actual,
                        &expected,
                        program,
                    );
                }
            }
        }
    }
}
