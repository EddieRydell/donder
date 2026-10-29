//! The strip interpreter. A program runs over strips of up to [`STRIP`]
//! pixels: its query and target blocks once, then its body once per strip. A
//! scalar instruction runs once; a row instruction loops over the strip's
//! selected pixels, held as ascending ranges so that every instruction has
//! one dense loop. Rows are cells, so an instruction reads its operands and
//! writes its destination through shared references.
//!
//! Each bank stores its scalars and then its rows in one array. Every operand
//! is a row-sized window of it with a mask: a scalar's window starts at the
//! scalar and masks every pixel to it, a row's masks nothing. Reading a pixel
//! is then one branch-free load whatever the operand's kind, so one loop serves
//! every kind of operand; the cheapest operations also have loops of their own
//! for rows and for a row with a scalar. A color is three consecutive floats.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
use super::RunContext;
use super::context::Clock;
use super::workspace::{NEIGHBORHOOD, Neighborhood, Segment, StripWorkspace};
use crate::dsl::bytecode::{
    Bank, ColorComponent, CompareOp, ContextRead, CurveOp, Direction, Edges, FloatBinary, Input,
    Instruction, IntBinary, IntUnary, MarkOp, Reducer, Resource, SignalPixel, Slot, SlotKind,
};
use crate::format::{self, GradientStop, SectionPixel};
use crate::resources::{CurveView, ParamsView};
use crate::sampling::{
    clamp_array_index, color_hue, color_intensity, color_saturation, float_binary, float_unary,
    gradient_color_scaled, hsv, int_binary, mark_at, max_float, min_float, previous_mark,
    previous_mark_index, sample_gradient_stops, value_or,
};
use crate::sections::{RunSections, SectionsView, single_section_query};
use crate::view::{ProgramView, Show};
use core::cell::Cell;
use donder_runtime_types::{Color, InstructionWord, MarkWindow};

pub(crate) use crate::dsl::bytecode::STRIP;
const MASK: usize = STRIP - 1;

pub(super) use crate::arena::Row;

/// The instruction at the start of trusted code; one copy serves every
/// dispatch.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn decoded(code: &[InstructionWord]) -> Instruction {
    InstructionWord::decode(code)
}

/// A bound color word: the index of its color in the show.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn word_color(show: Show<'_>, word: u32) -> Color {
    show.s.colors[word as usize]
}

/// Signal queries of a running strip. Pixel `n` of a strip is its `n`th
/// pixel; the provider knows which pixels the strip covers.
pub(crate) trait StripSignals {
    /// `input` for every pixel of the strip, each at its own pixel.
    fn read_strip(&mut self, input: usize, output: &mut [Color; STRIP]);

    /// `input` for one pixel at an explicit address.
    fn read_pixel(&mut self, input: usize, offset: usize, pixel: SignalPixel<i32>) -> Color;

    /// The strip's pixels of `query`'s scan, computed for the whole frame
    /// the first time the node reads it.
    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn NeighborWeights,
        output: &mut [Color; STRIP],
    );

    /// `input` for the pixels from `start` pixels after the strip's
    /// first, in the target's order: their colors and indices in their
    /// fixtures, black and [`OUTSIDE`] past the target's ends.
    fn sample_range(
        &mut self,
        input: usize,
        start: isize,
        colors: &mut [Color],
        locals: &mut [i32],
    );
}

/// A scan: its input, the scratch frame of its result, and how it runs.
#[derive(Clone, Copy)]
pub(crate) struct ScanQuery {
    pub(crate) input: usize,
    pub(crate) cache: usize,
    pub(crate) direction: Direction,
    pub(crate) decay: f32,
}

/// A scan's weight of each input color, from its weight block.
pub(crate) trait NeighborWeights {
    fn weights(&mut self, colors: &[Color], weights: &mut [f32]);
}

/// A clip sampled alone: it has no layer, so its previous frame is black.
impl StripSignals for super::NoSignals {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn read_strip(&mut self, _: usize, output: &mut [Color; STRIP]) {
        output.fill(Color::BLACK);
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn read_pixel(&mut self, _: usize, _: usize, _: SignalPixel<i32>) -> Color {
        Color::BLACK
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_range(&mut self, _: usize, _: isize, colors: &mut [Color], locals: &mut [i32]) {
        colors.fill(Color::BLACK);
        locals.fill(super::OUTSIDE);
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scan(&mut self, _: &ScanQuery, _: &mut dyn NeighborWeights, output: &mut [Color; STRIP]) {
        output.fill(Color::BLACK);
    }
}

/// Per-pixel inputs. The traversal fills them for each strip.
pub(crate) struct Pixels<'w> {
    pub(crate) index: &'w Row<i32>,
    pub(crate) fraction: &'w Row<f32>,
    pub(crate) x: &'w Row<f32>,
    pub(crate) y: &'w Row<f32>,
    pub(crate) sections: &'w Row<SectionPixel>,
}

/// A resource: one of the show's, by index into its kind's section, or a
/// parameter the invocation rewrites (an automated curve's window, live
/// marks), by its resource bank.
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(transparent)]
pub(super) struct Handle(u32);

const PARAM_HANDLE: u32 = 1 << 31;

impl Handle {
    pub(super) const EMPTY: Self = Self(0);

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn show(index: u32) -> Self {
        Self(index)
    }

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn param(bank: u16) -> Self {
        Self(PARAM_HANDLE | u32::from(bank))
    }

    /// The parameter bank of a rewritten resource, or the show index.
    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn kind(self) -> Result<u32, u16> {
        if self.0 & PARAM_HANDLE == 0 {
            Ok(self.0)
        } else {
            Err(self.0 as u16)
        }
    }
}

/// One bank's slots: its scalars, then its rows, in one array. A bank with
/// scalars but no rows keeps a row of padding, so every scalar has a
/// row-sized window; a bank with neither holds nothing.
pub(super) struct Store<'w, T> {
    pub(super) values: &'w [Cell<T>],
    pub(super) scalars: usize,
}

impl<'w, T: Copy> Store<'w, T> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scalar(&self, index: u16) -> &'w Cell<T> {
        &self.values[usize::from(index)]
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn window(&self, start: usize) -> &'w Row<T> {
        let Ok(window) = <&Row<T>>::try_from(&self.values[start..start + STRIP]) else {
            unreachable!("a window spans one strip")
        };
        window
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn row(&self, index: u16) -> &'w Row<T> {
        self.window(self.scalars + usize::from(index) * STRIP)
    }

    /// A scalar operand: the scalar masks every pixel to itself.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn one(&self, index: u16) -> Src<'w, T> {
        Src {
            row: self.window(usize::from(index)),
            mask: 0,
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn many(&self, index: u16) -> Src<'w, T> {
        Src {
            row: self.row(index),
            mask: MASK,
        }
    }
}

/// A run of pixels, `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(super) struct Range {
    start: u8,
    end: u8,
}

impl Range {
    pub(super) const EMPTY: Self = Self { start: 0, end: 0 };

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn new(start: u8, end: u8) -> Self {
        Self { start, end }
    }

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn get(self) -> (u8, u8) {
        (self.start, self.end)
    }
}

/// A selection of one range.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn range(start: u8, end: u8) -> [Cell<Range>; 1] {
    [Cell::new(Range::new(start, end))]
}

/// The pixels an instruction runs for: ascending, disjoint, nonempty ranges.
#[derive(Clone, Copy)]
struct Sel<'s>(&'s [Cell<Range>]);

/// Selections open in one construct: a branch partitions into `sets[0]`; a
/// reduction keeps its participating pixels in `sets[0]`, its filtered
/// contributing ones in `sets[1]`, an early exit's decided ones in the set its
/// contributing pixels are not in, and which pixels still run in `running`.
struct Level<'w> {
    sets: [&'w [Cell<Range>]; 2],
    running: &'w Row<bool>,
}

/// The levels a block may open, innermost last: a workspace's selection
/// storage from some depth down.
#[derive(Clone, Copy)]
pub(super) struct Levels<'w> {
    sets: &'w [Cell<Range>],
    running: &'w [Cell<bool>],
}

impl<'w> Levels<'w> {
    pub(super) fn new(sets: &'w [Cell<Range>], running: &'w [Cell<bool>]) -> Self {
        Self { sets, running }
    }

    /// The next level, and the levels inside it.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn split(self) -> (Level<'w>, Self) {
        let (sets, inner_sets) = self.sets.split_at(2 * STRIP);
        let (running, inner_running) = self.running.split_at(STRIP);
        let (first, second) = sets.split_at(STRIP);
        let Ok(running) = <&Row<bool>>::try_from(running) else {
            unreachable!("a level holds a strip of flags")
        };
        (
            Level {
                sets: [first, second],
                running,
            },
            Self {
                sets: inner_sets,
                running: inner_running,
            },
        )
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn each(sel: Sel<'_>, mut visit: impl FnMut(usize)) {
    for range in sel.0 {
        let (start, end) = range.get().get();
        for index in usize::from(start)..usize::from(end).min(STRIP) {
            visit(index);
        }
    }
}

/// The runs of the selected pixels where `keep` holds, written into `buffer`;
/// returns how many.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn filter(sel: Sel<'_>, buffer: &[Cell<Range>], keep: impl Fn(usize) -> bool) -> usize {
    let mut count = 0;
    let mut push = |start: usize, end: usize| {
        if let Some(slot) = buffer.get(count) {
            slot.set(Range::new(start as u8, end as u8));
            count += 1;
        }
    };
    for range in sel.0 {
        let (start, end) = range.get().get();
        let (start, end) = (usize::from(start), usize::from(end).min(STRIP));
        let mut run = None;
        for index in start..end {
            match (keep(index), run) {
                (true, None) => run = Some(index),
                (false, Some(first)) => {
                    push(first, index);
                    run = None;
                }
                _ => {}
            }
        }
        if let Some(first) = run {
            push(first, end);
        }
    }
    count
}

/// An operand: a row-sized window and a mask, 0 for a scalar.
#[derive(Clone, Copy)]
struct Src<'r, T> {
    row: &'r Row<T>,
    mask: usize,
}

impl<T: Copy> Src<'_, T> {
    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn at(self, index: usize) -> T {
        self.row[index & self.mask & MASK].get()
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn is_scalar(self) -> bool {
        self.mask == 0
    }
}

/// A destination: one value, or a row written at the selected pixels.
#[derive(Clone, Copy)]
enum Dst<'r, T> {
    One(&'r Cell<T>),
    Many(&'r Row<T>),
}

/// A color operand: its channels.
type ColorSrc<'r> = [Src<'r, f32>; 3];
type ColorDst<'r> = [Dst<'r, f32>; 3];

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn color_at(color: ColorSrc<'_>, index: usize) -> Color {
    Color {
        red: color[0].at(index),
        green: color[1].at(index),
        blue: color[2].at(index),
    }
}

/// Write `value(i)` at the selected pixels of a color, or once for a scalar.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn pick_color(sel: Sel<'_>, dst: ColorDst<'_>, value: impl Fn(usize) -> Color) {
    match dst {
        [Dst::Many(red), Dst::Many(green), Dst::Many(blue)] => each(sel, |i| {
            let color = value(i);
            red[i].set(color.red);
            green[i].set(color.green);
            blue[i].set(color.blue);
        }),
        [red, green, blue] => {
            let color = value(0);
            for (dst, channel) in [(red, color.red), (green, color.green), (blue, color.blue)] {
                if let Dst::One(dst) = dst {
                    dst.set(channel);
                }
            }
        }
    }
}

/// Cheap operations: rows, and a row with a scalar, have their own loops.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn map1<A: Copy, D: Copy>(sel: Sel<'_>, dst: Dst<'_, D>, a: Src<'_, A>, f: impl Fn(A) -> D) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0))),
        Dst::Many(dst) if a.is_scalar() => {
            let value = f(a.at(0));
            each(sel, |i| dst[i].set(value));
        }
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.row[i].get()))),
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn map2<A: Copy, B: Copy, D: Copy>(
    sel: Sel<'_>,
    dst: Dst<'_, D>,
    a: Src<'_, A>,
    b: Src<'_, B>,
    f: impl Fn(A, B) -> D,
) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0), b.at(0))),
        Dst::Many(dst) if !a.is_scalar() && !b.is_scalar() => {
            each(sel, |i| dst[i].set(f(a.row[i].get(), b.row[i].get())));
        }
        Dst::Many(dst) if !a.is_scalar() => {
            let b = b.at(0);
            each(sel, |i| dst[i].set(f(a.row[i].get(), b)));
        }
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i), b.at(i)))),
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn map3<A: Copy, B: Copy, C: Copy, D: Copy>(
    sel: Sel<'_>,
    dst: Dst<'_, D>,
    a: Src<'_, A>,
    b: Src<'_, B>,
    c: Src<'_, C>,
    f: impl Fn(A, B, C) -> D,
) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0), b.at(0), c.at(0))),
        Dst::Many(dst) if !a.is_scalar() && b.is_scalar() && c.is_scalar() => {
            let (b, c) = (b.at(0), c.at(0));
            each(sel, |i| dst[i].set(f(a.row[i].get(), b, c)));
        }
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i), b.at(i), c.at(i)))),
    }
}

/// One operand of a costly operation: the operation, not operand dispatch,
/// dominates, so one loop serves every operand kind.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn apply1<A: Copy, D: Copy>(sel: Sel<'_>, dst: Dst<'_, D>, a: Src<'_, A>, f: impl Fn(A) -> D) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0))),
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i)))),
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn apply2<A: Copy, B: Copy, D: Copy>(
    sel: Sel<'_>,
    dst: Dst<'_, D>,
    a: Src<'_, A>,
    b: Src<'_, B>,
    f: impl Fn(A, B) -> D,
) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0), b.at(0))),
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i), b.at(i)))),
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn apply3<A: Copy, B: Copy, C: Copy, D: Copy>(
    sel: Sel<'_>,
    dst: Dst<'_, D>,
    a: Src<'_, A>,
    b: Src<'_, B>,
    c: Src<'_, C>,
    f: impl Fn(A, B, C) -> D,
) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0), b.at(0), c.at(0))),
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i), b.at(i), c.at(i)))),
    }
}

/// Costly per-pixel operations, called rather than inlined into every loop.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn unary(op: crate::dsl::bytecode::FloatUnary, value: f32) -> f32 {
    float_unary(op, value)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn binary(op: FloatBinary, a: f32, b: f32) -> f32 {
    float_binary(op, a, b)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn int(op: IntBinary, a: i32, b: i32) -> i32 {
    int_binary(op, a, b)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn component(op: ColorComponent, color: Color) -> f32 {
    match op {
        ColorComponent::Hue => color_hue(color),
        ColorComponent::Saturation => color_saturation(color),
        ColorComponent::Intensity => color_intensity(color),
    }
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn hsv_color(hue: f32, saturation: f32, value: f32) -> Color {
    hsv(hue, saturation, value)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn recolor(color: Color, hue: f32, shift: bool) -> Color {
    crate::sampling::recolor(color, hue, shift)
}

/// Offsets of pixel `i`'s range, as a reduction counts its iterations.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn offset_count(start: Src<'_, i32>, end: Src<'_, i32>, i: usize) -> i32 {
    end.at(i).wrapping_sub(start.at(i)).max(0)
}

/// A stencil's neighborhood, read by every offset.
struct Term<'n, 'm> {
    edges: Edges,
    /// Pixels in each strip pixel's fixture, when reading past its ends.
    count: i32,
    /// The strip position of the neighborhood's first pixel.
    origin: i32,
    colors: &'n [Color],
    weights: &'n [f32],
    pixel: Src<'m, f32>,
    /// Skipped edges' neighborhood segments and selected pixel runs.
    segments: &'n [Segment],
    runs: &'n [Segment],
}

/// A scan's weight block, run over strips of input colors.
struct Weights<'s, 'm, 'w> {
    machine: &'s Machine<'m, 'w>,
    code: &'s [InstructionWord],
    sample: Slot,
    weight: Slot,
    levels: Levels<'w>,
}

impl NeighborWeights for Weights<'_, '_, '_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn weights(&mut self, colors: &[Color], weights: &mut [f32]) {
        for (colors, weights) in colors.chunks(STRIP).zip(weights.chunks_mut(STRIP)) {
            self.machine.fill_sample(self.sample, colors);
            let all = range(0, colors.len() as u8);
            self.machine
                .block(self.code, Sel(&all), self.levels, &mut super::NoSignals);
            let computed = self.machine.float(self.weight);
            for (k, weight) in weights.iter_mut().enumerate() {
                *weight = computed.at(k);
            }
        }
    }
}

/// What a program runs with: its show, code, parameters and target.
#[derive(Clone, Copy)]
pub(crate) struct Bound<'a> {
    pub(crate) show: Show<'a>,
    pub(crate) program: ProgramView<'a>,
    pub(crate) params: ParamsView<'a>,
    pub(crate) sections: Option<SectionsView<'a>>,
}

pub(crate) struct Strip<'a, 'w> {
    bound: Bound<'a>,
    context: RunContext,
    clock: Clock,
    workspace: &'a mut StripWorkspace<'w>,
    /// Pixel count and bounds of the initialized target block.
    shape: Option<(i32, [u32; 4])>,
    min: [f32; 2],
    max: [f32; 2],
}

impl<'a, 'w> Strip<'a, 'w> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn new(
        bound: Bound<'a>,
        context: &RunContext,
        workspace: &'a mut StripWorkspace<'w>,
    ) -> Self {
        Self {
            bound,
            context: *context,
            clock: Clock::new(context),
            workspace,
            shape: None,
            min: [0.0; 2],
            max: [0.0; 2],
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn pixels(&self) -> &Pixels<'w> {
        &self.workspace.pixels
    }

    /// Evaluate the first `output.len()` pixels. They share a pixel count and
    /// target bounds.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn run(
        &mut self,
        pixel_count: usize,
        min: [f32; 2],
        max: [f32; 2],
        signals: &mut dyn StripSignals,
        output: &mut [Color],
    ) {
        let len = output.len();
        assert!(len != 0 && len <= STRIP);
        let shape = (
            pixel_count as i32,
            [
                min[0].to_bits(),
                min[1].to_bits(),
                max[0].to_bits(),
                max[1].to_bits(),
            ],
        );
        let initialize = self.shape.map(|current| current != shape);
        if initialize != Some(false) {
            self.context.pixel_count = shape.0;
            self.min = min;
            self.max = max;
        }
        let machine = Machine {
            bound: self.bound,
            context: &self.context,
            clock: self.clock,
            min: self.min,
            max: self.max,
            ws: self.workspace,
        };
        let code = self.bound.program.code;
        let record = self.bound.program.record;
        let (query, rest) = code.split_at(record.query_end as usize);
        let (target, body) = rest.split_at((record.target_end - record.query_end) as usize);
        let levels = machine.ws.levels;
        let one = range(0, 1);
        match initialize {
            None => {
                machine.block(query, Sel(&one), levels, signals);
                machine.block(target, Sel(&one), levels, signals);
            }
            Some(true) => machine.block(target, Sel(&one), levels, signals),
            Some(false) => {}
        }
        self.shape = Some(shape);
        let strip = range(0, len as u8);
        machine.block(body, Sel(&strip), levels, signals);
        let result = machine.color(Slot::from_bits(record.result as u16));
        for (index, output) in output.iter_mut().enumerate() {
            *output = color_at(result, index);
        }
    }
}

struct Machine<'m, 'w> {
    bound: Bound<'m>,
    context: &'m RunContext,
    clock: Clock,
    min: [f32; 2],
    max: [f32; 2],
    ws: &'m StripWorkspace<'w>,
}

impl<'m, 'w: 'm> Machine<'m, 'w> {
    // Operand lookups run once per instruction, so they stay out of line: one
    // bounds check each rather than one per instruction arm.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn float(&self, slot: Slot) -> Src<'m, f32> {
        let ws = self.ws;
        let input = |row| Src { row, mask: MASK };
        match slot.kind() {
            SlotKind::Scalar(index) => ws.floats.one(index),
            SlotKind::Row(index) => ws.floats.many(index),
            SlotKind::Input(Input::PixelX) => input(ws.pixels.x),
            SlotKind::Input(Input::PixelY) => input(ws.pixels.y),
            SlotKind::Input(_) => input(ws.pixels.fraction),
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn int(&self, slot: Slot) -> Src<'m, i32> {
        let ws = self.ws;
        match slot.kind() {
            SlotKind::Scalar(index) => ws.ints.one(index),
            SlotKind::Row(index) => ws.ints.many(index),
            SlotKind::Input(_) => Src {
                row: ws.pixels.index,
                mask: MASK,
            },
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn boolean(&self, slot: Slot) -> Src<'m, bool> {
        operand(&self.ws.bools, slot)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn color(&self, slot: Slot) -> ColorSrc<'m> {
        [
            self.float(slot),
            self.float(slot.channel(1)),
            self.float(slot.channel(2)),
        ]
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn handle(&self, slot: Slot) -> Src<'m, Handle> {
        operand(&self.ws.handles, slot)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn float_dst(&self, slot: Slot) -> Dst<'m, f32> {
        destination(&self.ws.floats, slot)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn int_dst(&self, slot: Slot) -> Dst<'m, i32> {
        destination(&self.ws.ints, slot)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn bool_dst(&self, slot: Slot) -> Dst<'m, bool> {
        destination(&self.ws.bools, slot)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn color_dst(&self, slot: Slot) -> ColorDst<'m> {
        [
            self.float_dst(slot),
            self.float_dst(slot.channel(1)),
            self.float_dst(slot.channel(2)),
        ]
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn handle_dst(&self, slot: Slot) -> Dst<'m, Handle> {
        destination(&self.ws.handles, slot)
    }

    /// The rows of the color row `slot`.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn color_rows(&self, slot: Slot) -> [&'m Row<f32>; 3] {
        let row = |dst| match dst {
            Dst::Many(row) => row,
            Dst::One(_) => unreachable!("the compiler keeps samples in rows"),
        };
        let [red, green, blue] = self.color_dst(slot);
        [row(red), row(green), row(blue)]
    }

    /// Write the first pixels of the color row `slot` from `colors`.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn fill_sample(&self, slot: Slot, colors: &[Color]) {
        let [red, green, blue] = self.color_rows(slot);
        for (k, color) in colors.iter().enumerate() {
            red[k].set(color.red);
            green[k].set(color.green);
            blue[k].set(color.blue);
        }
    }

    /// The first entry of a table row.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn set_first<T: Copy>(dst: Dst<'_, T>, value: T) {
        if let Dst::Many(row) = dst {
            row[0].set(value);
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn set_scalar<T: Copy>(dst: Dst<'_, T>, value: T) {
        if let Dst::One(dst) = dst {
            dst.set(value);
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn block(
        &self,
        code: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let mut at = 0;
        while at < code.len() {
            let instruction = &decoded(&code[at..]);
            at += instruction.width();
            match *instruction {
                Instruction::Branch {
                    condition,
                    then_len,
                    else_len,
                } => {
                    let then_code = &code[at..at + usize::from(then_len)];
                    at += usize::from(then_len);
                    let else_code = &code[at..at + usize::from(else_len)];
                    at += usize::from(else_len);
                    self.branch(condition, then_code, else_code, sel, levels, signals);
                }
                Instruction::Reduce {
                    loop_len,
                    contribute_len,
                    ..
                } => {
                    let loop_code = &code[at..at + usize::from(loop_len)];
                    at += usize::from(loop_len);
                    let contribute = &code[at..at + usize::from(contribute_len)];
                    at += usize::from(contribute_len);
                    self.reduce(instruction, loop_code, contribute, sel, levels, signals);
                }
                Instruction::Stencil {
                    weight_len,
                    tap_len,
                    ..
                } => {
                    let source = &code[at..at + usize::from(weight_len)];
                    at += usize::from(weight_len);
                    let taps = &code[at..at + usize::from(tap_len)];
                    at += usize::from(tap_len);
                    self.stencil(instruction, source, taps, sel, levels, signals);
                }
                Instruction::Scan { weight_len, .. } => {
                    let source = &code[at..at + usize::from(weight_len)];
                    at += usize::from(weight_len);
                    self.scan(instruction, source, sel, levels, signals);
                }
                Instruction::Tabulate { len, .. } => {
                    let body = &code[at..at + usize::from(len)];
                    at += usize::from(len);
                    self.tabulate(instruction, body, sel, levels, signals);
                }
                _ => self.step(instruction, sel, signals),
            }
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn branch(
        &self,
        condition: Slot,
        then_code: &[InstructionWord],
        else_code: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let condition = self.boolean(condition);
        if condition.is_scalar() {
            let code = if condition.at(0) {
                then_code
            } else {
                else_code
            };
            self.block(code, sel, levels, signals);
            return;
        }
        let condition = condition.row;
        let (level, inner) = levels.split();
        let buffer = level.sets[0];
        let holding = filter(sel, buffer, |i| condition[i].get());
        let failing = filter(sel, &buffer[holding..], |i| !condition[i].get());
        if failing == 0 {
            self.block(then_code, sel, inner, signals);
        } else if holding == 0 {
            self.block(else_code, sel, inner, signals);
        } else {
            self.block(then_code, Sel(&buffer[..holding]), inner, signals);
            let failing = Sel(&buffer[holding..holding + failing]);
            self.block(else_code, failing, inner, signals);
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn reduce(
        &self,
        instruction: &Instruction,
        loop_code: &[InstructionWord],
        contribute: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let Instruction::Reduce {
            reducer,
            bank,
            acc,
            index,
            start,
            end,
            filter: filter_slot,
            value,
            ..
        } = *instruction
        else {
            unreachable!("dispatched by block")
        };
        let (start, end) = (self.int(start), self.int(end));
        let last = reducer == Reducer::Last;
        let early = matches!(
            reducer,
            Reducer::Any | Reducer::All | Reducer::First | Reducer::Last
        );
        let count = |i: usize| end.at(i).wrapping_sub(start.at(i)).max(0);
        let index_at = |i: usize, k: i32| {
            if last {
                end.at(i).wrapping_sub(1).wrapping_sub(k)
            } else {
                start.at(i).wrapping_add(k)
            }
        };
        if acc.is_scalar() {
            let count = count(0);
            let index = self.int_dst(index);
            for k in 0..count {
                Self::set_scalar(index, index_at(0, k));
                self.block(loop_code, sel, levels, signals);
                if !filter_slot.is_none() && !self.boolean(filter_slot).at(0) {
                    continue;
                }
                self.block(contribute, sel, levels, signals);
                if !early {
                    self.combine(reducer, bank, acc, value, sel);
                } else if self.decides(reducer, value, 0) {
                    self.assign(bank, acc, value, sel);
                    break;
                }
            }
            return;
        }
        let (level, inner) = levels.split();
        // Over rows, `any` and `all` combine every iteration branch-free and
        // stop once every pixel has decided: splitting the selection where
        // pixels decide costs more than finishing their iterations.
        let early = matches!(reducer, Reducer::First | Reducer::Last);
        let uniform_bounds = start.is_scalar() && end.is_scalar();
        let index_dst = self.int_dst(index);
        // A scalar index with per-pixel bounds is shared: the loop runs the
        // union of the pixels' ranges, and each pixel takes part in its own.
        let shared = !uniform_bounds && matches!(index_dst, Dst::One(_));
        // Early exits track the pixels still running; per-pixel bounds run
        // each pixel's own count.
        let mut running = 0;
        if early {
            each(sel, |i| {
                level.running[i].set(true);
                running += 1;
            });
        }
        let (mut first, mut past) = (i32::MAX, i32::MIN);
        let mut iterations = 0;
        if shared {
            each(sel, |i| {
                let count = count(i);
                if count > 0 {
                    first = first.min(start.at(i));
                    past = past.max(start.at(i).wrapping_add(count));
                }
            });
            // No pixel takes part when none has a count.
            iterations = if past > first { past - first } else { 0 };
        } else {
            each(sel, |i| iterations = iterations.max(count(i)));
        }
        // The shared index at iteration `k`, ascending or, for `last`, descending.
        let shared_at = |k: i32| {
            if last { past - 1 - k } else { first + k }
        };
        let takes_part = |i: usize, k: i32| {
            if shared {
                let (index, start) = (shared_at(k), start.at(i));
                index >= start && index < start.wrapping_add(count(i))
            } else {
                k < count(i)
            }
        };
        for k in 0..iterations {
            let participating = if uniform_bounds && !early {
                sel
            } else {
                let kept = filter(sel, level.sets[0], |i| {
                    (!early || level.running[i].get()) && takes_part(i, k)
                });
                Sel(&level.sets[0][..kept])
            };
            if participating.0.is_empty() {
                continue;
            }
            match index_dst {
                Dst::One(index) if shared => index.set(shared_at(k)),
                Dst::One(index) => index.set(index_at(0, k)),
                Dst::Many(index) => each(participating, |i| index[i].set(index_at(i, k))),
            }
            self.block(loop_code, participating, inner, signals);
            // With nothing to run behind it, a filter masks the combination
            // instead of narrowing the selection every iteration.
            if !early && !filter_slot.is_none() && contribute.is_empty() {
                self.combine_where(reducer, bank, acc, value, filter_slot, participating);
                if matches!(reducer, Reducer::Any | Reducer::All) {
                    let (decided, any) = (self.boolean(acc), reducer == Reducer::Any);
                    let mut open = false;
                    each(sel, |i| open |= decided.at(i) != any);
                    if !open {
                        break;
                    }
                }
                continue;
            }
            let contributing = if filter_slot.is_none() {
                participating
            } else {
                let condition = self.boolean(filter_slot);
                let kept = filter(participating, level.sets[1], |i| condition.at(i));
                Sel(&level.sets[1][..kept])
            };
            if contributing.0.is_empty() {
                continue;
            }
            self.block(contribute, contributing, inner, signals);
            if !early {
                self.combine(reducer, bank, acc, value, contributing);
                if matches!(reducer, Reducer::Any | Reducer::All) {
                    let (decided, any) = (self.boolean(acc), reducer == Reducer::Any);
                    let mut open = false;
                    each(sel, |i| open |= decided.at(i) != any);
                    if !open {
                        break;
                    }
                }
                continue;
            }
            // Decisions can split a range into several runs, so they go to
            // the set `contributing` is not read from. An index, not a branch,
            // so the loop is not duplicated per case in instruction RAM.
            let free = level.sets[usize::from(filter_slot.is_none())];
            let decided = filter(contributing, free, |i| self.decides(reducer, value, i));
            let decided = Sel(&free[..decided]);
            self.assign(bank, acc, value, decided);
            each(decided, |i| {
                level.running[i].set(false);
                running -= 1;
            });
            if running == 0 {
                break;
            }
        }
    }

    /// A neighborhood reduction. The union of the selected pixels' offset
    /// ranges and the pixels they read make a neighborhood of at most two
    /// strips: its input is read once and its neighbor weights computed once,
    /// a strip at a time; then each offset runs its scalar code and
    /// accumulates its term at every selected pixel.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn stencil(
        &self,
        instruction: &Instruction,
        source: &[InstructionWord],
        taps: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let Instruction::Stencil {
            reducer,
            edges,
            acc,
            index,
            start,
            end,
            weight,
            scale,
            pixel,
            ..
        } = *instruction
        else {
            unreachable!()
        };
        let Instruction::Read {
            dst: sample, input, ..
        } = decoded(source)
        else {
            unreachable!()
        };
        let (start, end) = (self.int(start), self.int(end));
        let (mut first, mut past) = (i32::MAX, i32::MIN);
        each(sel, |i| {
            let count = offset_count(start, end, i);
            if count > 0 {
                first = first.min(start.at(i));
                past = past.max(start.at(i).wrapping_add(count));
            }
        });
        if past <= first {
            return;
        }
        let (Some(low), Some(high)) = (sel.0.first(), sel.0.last()) else {
            return;
        };
        let (low, high) = (
            usize::from(low.get().get().0),
            usize::from(high.get().get().1).min(STRIP),
        );
        // The compiler admits offsets spanning at most a strip, reaching at
        // most half a strip each way past a fixture's ends.
        let offsets = (past.wrapping_sub(first) as usize).min(STRIP);
        let (origin, len) = match edges {
            Edges::Skip => (low as i32 + first, high - low + offsets - 1),
            Edges::Extend | Edges::Mirror => {
                let reach = first
                    .unsigned_abs()
                    .max((past - 1).unsigned_abs())
                    .min(STRIP as u32 / 2);
                (
                    low as i32 - reach as i32,
                    (high - low + 2 * reach as usize).min(NEIGHBORHOOD),
                )
            }
        };
        let Some(neighborhood) = &self.ws.neighborhood else {
            unreachable!()
        };
        let mut neighborhood = neighborhood.borrow_mut();
        let Neighborhood {
            colors,
            locals,
            weights,
            segments,
            runs,
        } = &mut *neighborhood;
        signals.sample_range(
            usize::from(input),
            origin as isize,
            &mut colors[..len],
            &mut locals[..len],
        );
        // Black neighbors contribute nothing, so `acc` keeps its black.
        if colors[..len].iter().all(|color| *color == Color::BLACK) {
            return;
        }
        for chunk in (0..len).step_by(STRIP) {
            let size = (len - chunk).min(STRIP);
            self.fill_sample(sample, &colors[chunk..chunk + size]);
            self.block(&source[1..], Sel(&range(0, size as u8)), levels, signals);
            let computed = self.float(weight);
            for k in 0..size {
                weights[chunk + k] = computed.at(k);
            }
        }
        let index = self.int_dst(index);
        let acc = self.color_rows(acc);
        // Skipped edges read a neighbor only in the pixel's own fixture: where
        // the neighbor's key, its fixture index less its position, is the
        // pixel's. Both sides are runs of one key, so each offset combines
        // whole runs.
        let (segments, runs) = if edges == Edges::Skip {
            let local = &self.ws.pixels.index;
            let mut neighbors = 0;
            for (at, &read) in locals[..len].iter().enumerate() {
                if read != super::OUTSIDE {
                    extend(segments, &mut neighbors, at as i32, read - at as i32);
                }
            }
            let mut pixels = 0;
            each(sel, |i| {
                let key = local[i].get() - (i as i32 - origin);
                extend(runs, &mut pixels, i as i32, key);
            });
            (&segments[..neighbors], &runs[..pixels])
        } else {
            (&segments[..0], &runs[..0])
        };
        let term = Term {
            edges,
            count: self.context.pixel_count,
            origin,
            colors: &colors[..len],
            weights: &weights[..len],
            pixel: self.float(pixel),
            segments,
            runs,
        };
        // A row scale is a table of the offsets.
        let scales = self.float(scale);
        for k in 0..offsets {
            let offset = first + k as i32;
            Self::set_scalar(index, offset);
            self.block(taps, sel, levels, signals);
            let scale = scales.at(k);
            self.accumulate(reducer, &term, offset, scale, start, end, sel, acc);
        }
    }

    /// Combine offset `offset`'s term into `acc` at the selected pixels
    /// whose range holds it and whose neighbor there is in their fixture.
    #[allow(clippy::too_many_arguments)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn accumulate(
        &self,
        reducer: Reducer,
        term: &Term<'_, '_>,
        offset: i32,
        scale: f32,
        start: Src<'_, i32>,
        end: Src<'_, i32>,
        sel: Sel<'_>,
        acc: [&Row<f32>; 3],
    ) {
        let max = reducer == Reducer::Max;
        // Uniform bounds hold every offset the stencil runs.
        let bounded = !(start.is_scalar() && end.is_scalar());
        let inside = |i: usize| {
            !bounded
                || (offset.wrapping_sub(start.at(i)) as u32) < offset_count(start, end, i) as u32
        };
        let shift = offset - term.origin;
        if term.edges == Edges::Skip {
            for run in term.runs {
                for segment in term
                    .segments
                    .iter()
                    .filter(|segment| segment.key == run.key)
                {
                    // The run's pixels whose neighbor lies in the segment.
                    let first = i32::from(run.start).max(i32::from(segment.start) - shift);
                    let past = i32::from(run.end).min(i32::from(segment.end) - shift);
                    for i in first.max(0) as usize..past.max(0) as usize {
                        if inside(i) {
                            let at = (i as i32 + shift) as usize;
                            combine_term(
                                acc,
                                i,
                                term.colors[at],
                                term.weights[at] * scale * term.pixel.at(i),
                                max,
                            );
                        }
                    }
                }
            }
            return;
        }
        let local = &self.ws.pixels.index;
        each(sel, |i| {
            let local = local[i].get();
            if let (true, Some(read)) = (
                inside(i),
                term.edges.local(local.wrapping_add(offset), term.count),
            ) {
                let at = (i as i32 + read - local - term.origin) as usize;
                combine_term(
                    acc,
                    i,
                    term.colors[at],
                    term.weights[at] * scale * term.pixel.at(i),
                    max,
                );
            }
        });
    }

    /// A table: its block runs once per index, and each index's value lands
    /// in the table row.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn tabulate(
        &self,
        instruction: &Instruction,
        body: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let Instruction::Tabulate {
            bank,
            prefix,
            dst,
            index,
            start,
            end,
            value,
            ..
        } = *instruction
        else {
            unreachable!("dispatched by block")
        };
        let (start, end) = (self.int(start).at(0), self.int(end).at(0));
        let index = self.int_dst(index);
        // Prefix sums start from nothing and land one entry later.
        let shift = usize::from(prefix);
        if prefix {
            match bank {
                Bank::Int => Self::set_first(self.int_dst(dst), 0),
                _ => Self::set_first(self.float_dst(dst), 0.0),
            }
        }
        for (k, at) in (start..end).take(STRIP - shift).enumerate() {
            Self::set_scalar(index, at);
            self.block(body, sel, levels, signals);
            match bank {
                Bank::Int => {
                    if let Dst::Many(row) = self.int_dst(dst) {
                        let sum = if prefix { row[k].get() } else { 0 };
                        row[k + shift].set(sum.wrapping_add(self.int(value).at(0)));
                    }
                }
                Bank::Bool => {
                    if let Dst::Many(row) = self.bool_dst(dst) {
                        row[k].set(self.boolean(value).at(0));
                    }
                }
                _ => {
                    if let Dst::Many(row) = self.float_dst(dst) {
                        let sum = if prefix { row[k].get() } else { 0.0 };
                        row[k + shift].set(sum + self.float(value).at(0));
                    }
                }
            }
        }
    }

    /// A scan: the provider computes it for the whole frame, once per query,
    /// with the weight block weighting each input color.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scan(
        &self,
        instruction: &Instruction,
        source: &[InstructionWord],
        sel: Sel<'_>,
        levels: Levels<'w>,
        signals: &mut dyn StripSignals,
    ) {
        let Instruction::Scan {
            direction,
            dst,
            decay,
            weight,
            cache,
            ..
        } = *instruction
        else {
            unreachable!()
        };
        let Instruction::Read {
            dst: sample, input, ..
        } = decoded(source)
        else {
            unreachable!()
        };
        let query = ScanQuery {
            input: usize::from(input),
            cache: usize::from(cache),
            direction,
            decay: self.float(decay).at(0),
        };
        let mut weights = Weights {
            machine: self,
            code: &source[1..],
            sample,
            weight,
            levels,
        };
        let mut samples = self.ws.samples.borrow_mut();
        signals.scan(&query, &mut weights, &mut samples);
        let samples = &**samples;
        pick_color(sel, self.color_dst(dst), |i| samples[i]);
    }

    /// Whether `value` at pixel `i` ends an early-exit reduction there.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn decides(&self, reducer: Reducer, value: Slot, i: usize) -> bool {
        match reducer {
            Reducer::Any => self.boolean(value).at(i),
            Reducer::All => !self.boolean(value).at(i),
            _ => true,
        }
    }

    /// `acc = acc ⊕ value` for `max`, `min` and `sum`.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn combine(&self, reducer: Reducer, bank: Bank, acc: Slot, value: Slot, sel: Sel<'_>) {
        match bank {
            Bank::Float | Bank::Color => {
                for channel in 0..bank.width() {
                    let (acc, value) = (acc.channel(channel), value.channel(channel));
                    let (dst, a, b) = (self.float_dst(acc), self.float(acc), self.float(value));
                    match reducer {
                        Reducer::Max => map2(sel, dst, a, b, max_float),
                        Reducer::Min => map2(sel, dst, a, b, min_float),
                        _ => map2(sel, dst, a, b, |a, b| a + b),
                    }
                }
            }
            Bank::Int => {
                let (dst, a, b) = (self.int_dst(acc), self.int(acc), self.int(value));
                // One loop for both orders keeps instruction RAM within budget.
                let max = reducer == Reducer::Max;
                match reducer {
                    Reducer::Max | Reducer::Min => {
                        map2(
                            sel,
                            dst,
                            a,
                            b,
                            |a, b| if (b > a) == max && b != a { b } else { a },
                        )
                    }
                    _ => map2(sel, dst, a, b, i32::wrapping_add),
                }
            }
            Bank::Bool => {
                let (a, b) = (self.boolean(acc), self.boolean(value));
                let any = reducer == Reducer::Any;
                pick_each(sel, self.bool_dst(acc), |i| {
                    if any {
                        a.at(i) | b.at(i)
                    } else {
                        a.at(i) & b.at(i)
                    }
                });
            }
            Bank::Resource => self.assign(bank, acc, value, sel),
        }
    }

    /// `acc = acc ⊕ value` where the bool `mask` holds.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn combine_where(
        &self,
        reducer: Reducer,
        bank: Bank,
        acc: Slot,
        value: Slot,
        mask: Slot,
        sel: Sel<'_>,
    ) {
        let mask = self.boolean(mask);
        match bank {
            Bank::Int => {
                let (dst, a, b) = (self.int_dst(acc), self.int(acc), self.int(value));
                let max = reducer == Reducer::Max;
                pick_each(sel, dst, |i| {
                    let (a, b) = (a.at(i), b.at(i));
                    let combined = match reducer {
                        Reducer::Max | Reducer::Min => {
                            if (b > a) == max && b != a {
                                b
                            } else {
                                a
                            }
                        }
                        _ => a.wrapping_add(b),
                    };
                    choose(mask.at(i), combined, a)
                });
            }
            Bank::Bool => {
                let (a, b) = (self.boolean(acc), self.boolean(value));
                let any = reducer == Reducer::Any;
                pick_each(sel, self.bool_dst(acc), |i| {
                    let (a, b, m) = (a.at(i), b.at(i), mask.at(i));
                    if any { a | (m & b) } else { a & (!m | b) }
                });
            }
            _ => {
                for channel in 0..bank.width() {
                    let (acc, value) = (acc.channel(channel), value.channel(channel));
                    let (dst, a, b) = (self.float_dst(acc), self.float(acc), self.float(value));
                    pick_each(sel, dst, |i| {
                        let (a, b) = (a.at(i), b.at(i));
                        let combined = match reducer {
                            Reducer::Max => max_float(a, b),
                            Reducer::Min => min_float(a, b),
                            _ => a + b,
                        };
                        choose(mask.at(i), combined, a)
                    });
                }
            }
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn assign(&self, bank: Bank, dst: Slot, src: Slot, sel: Sel<'_>) {
        match bank {
            Bank::Float | Bank::Color => {
                for channel in 0..bank.width() {
                    let (dst, src) = (dst.channel(channel), src.channel(channel));
                    map1(sel, self.float_dst(dst), self.float(src), |v| v);
                }
            }
            Bank::Int => map1(sel, self.int_dst(dst), self.int(src), |v| v),
            Bank::Bool => map1(sel, self.bool_dst(dst), self.boolean(src), |v| v),
            Bank::Resource => map1(sel, self.handle_dst(dst), self.handle(src), |v| v),
        }
    }

    /// A bound resource parameter.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn resource(&self, bank: u16) -> crate::resources::ResourceSlot {
        self.bound.params.resource(bank)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve(&self, handle: Handle) -> CurveView<'m> {
        let show = self.bound.show;
        match handle.kind() {
            Ok(index) => show.curve(index),
            Err(bank) => self.bound.params.curve(show, self.resource(bank)),
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient(&self, handle: Handle) -> &'m [GradientStop] {
        match handle.kind() {
            Ok(index) => self.bound.show.gradient(index),
            Err(_) => unreachable!("show gradients resolve when bound"),
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn marks(&self, handle: Handle) -> MarkWindow<'m> {
        let show = self.bound.show;
        match handle.kind() {
            Ok(index) => show.marks(index),
            Err(bank) => self.bound.params.marks(show, self.resource(bank)),
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn array(&self, handle: Handle) -> &'m [format::Value] {
        match handle.kind() {
            Ok(index) => self.bound.show.array(index),
            Err(_) => unreachable!("show arrays resolve when bound"),
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_query(&self, op: CurveOp, curve: Handle, value: f32, before: f32) -> f32 {
        let curve = self.curve(curve);
        match op {
            CurveOp::Sample => curve.sample(value),
            CurveOp::Integral => curve.integral(value),
            CurveOp::Crossing => curve.crossing(value),
            CurveOp::LastCrossing => curve.last_crossing(value, before),
        }
    }

    /// `gradient_color_scaled` of one gradient over a strip.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient_strip(
        sel: Sel<'_>,
        dst: ColorDst<'_>,
        stops: &[GradientStop],
        position: Src<'_, f32>,
        scale: Option<Src<'_, f32>>,
    ) {
        match scale {
            Some(scale) => pick_color(sel, dst, |i| {
                gradient_color_scaled(stops, position.at(i), scale.at(i))
            }),
            None => pick_color(sel, dst, |i| sample_gradient_stops(stops, position.at(i))),
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient_at(&self, gradient: Handle, position: f32, scale: Option<f32>) -> Color {
        let stops = self.gradient(gradient);
        match scale {
            Some(scale) => gradient_color_scaled(stops, position, scale),
            None => sample_gradient_stops(stops, position),
        }
    }

    /// A gradient of each pixel's own at its position.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradients(
        &self,
        sel: Sel<'_>,
        dst: ColorDst<'_>,
        gradient: Src<'_, Handle>,
        position: Src<'_, f32>,
        scale: Option<Src<'_, f32>>,
    ) {
        pick_color(sel, dst, |i| {
            self.gradient_at(gradient.at(i), position.at(i), scale.map(|s| s.at(i)))
        });
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn previous_mark(&self, marks: Handle, seconds: f32, cursor: &Cell<usize>) -> f32 {
        previous_mark(&self.marks(marks), seconds, cursor).map_or(f32::NAN, |(_, time)| time)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn length(&self, kind: Resource, resource: Handle) -> i32 {
        match kind {
            Resource::Marks => self.marks(resource).len() as i32,
            _ => self.array(resource).len() as i32,
        }
    }

    /// Item `index` of `array`, clamped to it; none of an empty array.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn array_item(&self, array: Handle, index: i32) -> Option<&'m format::Value> {
        let items = self.array(array);
        if items.is_empty() {
            return None;
        }
        Some(&items[clamp_array_index(index, items.len())])
    }

    /// An int array item: an int, or an enum option's index.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn int_item(&self, item: Option<&format::Value>, default: i32) -> i32 {
        match item {
            Some(item) if matches!(item.kind, format::VALUE_INT | format::VALUE_ENUM) => {
                item.word as i32
            }
            _ => default,
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn mark_at(&self, marks: Handle, index: i32) -> f32 {
        mark_at(&self.marks(marks), index)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn previous_mark_index(&self, marks: Handle, seconds: f32, cursor: &Cell<usize>) -> i32 {
        previous_mark_index(&self.marks(marks), seconds, cursor)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn section_query(
        &self,
        width: i32,
        index: bool,
        i: usize,
        run: &Cell<Option<RunSections>>,
    ) -> i32 {
        let pixels = &self.ws.pixels;
        let Some(target) = self.bound.sections else {
            return single_section_query(
                pixels.index[i & MASK].get(),
                self.context.pixel_count,
                width,
                index,
            );
        };
        let pixel = pixels.sections[i & MASK].get();
        let sections = match run.get() {
            Some(sections) if sections.key == (pixel.run, width) => sections,
            _ => {
                let sections = target.run_sections(pixel, width);
                run.set(Some(sections));
                sections
            }
        };
        sections.query(pixel, index)
    }

    /// A scalar of the query or target.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn context_float(&self, read: ContextRead) -> f32 {
        match read {
            ContextRead::Seconds => self.clock.seconds,
            ContextRead::Progress => self.context.progress,
            ContextRead::Duration => self.clock.duration,
            ContextRead::FrameRate => self.context.frame_rate,
            ContextRead::TargetMinX => self.min[0],
            ContextRead::TargetMinY => self.min[1],
            ContextRead::TargetMaxX => self.max[0],
            _ => self.max[1],
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn step(&self, instruction: &Instruction, sel: Sel<'_>, signals: &mut dyn StripSignals) {
        use Instruction as I;
        let params = self.bound.params;
        match *instruction {
            I::Const { bank, dst, bits } => match bank {
                Bank::Float => Self::set_scalar(self.float_dst(dst), f32::from_bits(bits)),
                Bank::Int => Self::set_scalar(self.int_dst(dst), bits as i32),
                Bank::Bool => Self::set_scalar(self.bool_dst(dst), bits != 0),
                _ => {
                    let index = self.bound.program.resources[bits as usize];
                    Self::set_scalar(self.handle_dst(dst), Handle::show(index));
                }
            },
            I::Param { bank, dst, index } => {
                let word = || params.words[usize::from(index)];
                match bank {
                    Bank::Float => Self::set_scalar(self.float_dst(dst), f32::from_bits(word())),
                    Bank::Int => Self::set_scalar(self.int_dst(dst), word() as i32),
                    Bank::Bool => Self::set_scalar(self.bool_dst(dst), word() != 0),
                    Bank::Color => {
                        let color = word_color(self.bound.show, word());
                        pick_color(sel, self.color_dst(dst), |_| color);
                    }
                    Bank::Resource => {
                        // A show resource resolves now; a rewritten one
                        // when read.
                        let slot = self.resource(index);
                        let handle = if slot.store == crate::resources::STORE_SHOW {
                            Handle::show(slot.index)
                        } else {
                            Handle::param(index)
                        };
                        Self::set_scalar(self.handle_dst(dst), handle);
                    }
                }
            }
            I::Context { dst, read } => match read {
                ContextRead::PixelCount => {
                    Self::set_scalar(self.int_dst(dst), self.context.pixel_count);
                }
                ContextRead::FramesElapsed => {
                    Self::set_scalar(self.int_dst(dst), self.context.frames_elapsed as i32);
                }
                read => Self::set_scalar(self.float_dst(dst), self.context_float(read)),
            },
            I::FloatUnary { op, dst, a } => {
                let (dst, a) = (self.float_dst(dst), self.float(a));
                use crate::dsl::bytecode::FloatUnary as U;
                match op {
                    U::Negate => map1(sel, dst, a, |v| -v),
                    U::Abs => map1(sel, dst, a, |v| float_unary(U::Abs, v)),
                    op => apply1(sel, dst, a, |v| unary(op, v)),
                }
            }
            I::FloatBinary {
                op,
                colors,
                dst,
                a,
                b,
            } => {
                use FloatBinary as B;
                for channel in 0..colors.channels() {
                    let (dst, a, b) = (
                        self.float_dst(dst.channel(channel)),
                        self.float(colors.operand(a, 0, channel)),
                        self.float(colors.operand(b, 1, channel)),
                    );
                    match op {
                        B::Add => map2(sel, dst, a, b, |a, b| a + b),
                        B::Subtract => map2(sel, dst, a, b, |a, b| a - b),
                        B::Multiply => map2(sel, dst, a, b, |a, b| a * b),
                        B::Divide => map2(sel, dst, a, b, |a, b| a / b),
                        B::Min => map2(sel, dst, a, b, min_float),
                        B::Max => map2(sel, dst, a, b, max_float),
                        B::ValueOr => map2(sel, dst, a, b, value_or),
                        op => apply2(sel, dst, a, b, |a, b| binary(op, a, b)),
                    }
                }
            }
            I::Clamp {
                dst,
                value,
                min,
                max,
            } => map3(
                sel,
                self.float_dst(dst),
                self.float(value),
                self.float(min),
                self.float(max),
                crate::sampling::clamp_float,
            ),
            I::Mix {
                colors,
                dst,
                a,
                b,
                amount,
            } => {
                for channel in 0..colors.channels() {
                    map3(
                        sel,
                        self.float_dst(dst.channel(channel)),
                        self.float(colors.operand(a, 0, channel)),
                        self.float(colors.operand(b, 1, channel)),
                        self.float(colors.operand(amount, 2, channel)),
                        |a, b, t| a + (b - a) * t,
                    );
                }
            }
            I::IntUnary { op, dst, a } => {
                let (dst, a) = (self.int_dst(dst), self.int(a));
                match op {
                    IntUnary::Negate => map1(sel, dst, a, i32::wrapping_neg),
                    IntUnary::Abs => map1(sel, dst, a, i32::wrapping_abs),
                }
            }
            I::IntBinary { op, dst, a, b } => {
                let (dst, a, b) = (self.int_dst(dst), self.int(a), self.int(b));
                match op {
                    IntBinary::Add => map2(sel, dst, a, b, i32::wrapping_add),
                    IntBinary::Subtract => map2(sel, dst, a, b, i32::wrapping_sub),
                    IntBinary::Multiply => map2(sel, dst, a, b, i32::wrapping_mul),
                    op => apply2(sel, dst, a, b, |a, b| int(op, a, b)),
                }
            }
            I::IntToFloat { dst, a } => map1(sel, self.float_dst(dst), self.int(a), |v| v as f32),
            // `as` truncates toward zero, saturates, and maps NaN to zero.
            I::FloatToInt { dst, a } => map1(sel, self.int_dst(dst), self.float(a), |v| v as i32),
            I::Compare {
                op,
                bank,
                dst,
                a,
                b,
            } => self.compare(op, bank, dst, a, b, sel),
            I::ColorComponent { op, dst, color } => {
                let color = self.color(color);
                pick_each(sel, self.float_dst(dst), |i| {
                    component(op, color_at(color, i))
                });
            }
            I::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => {
                let (hue, saturation, value) =
                    (self.float(hue), self.float(saturation), self.float(value));
                pick_color(sel, self.color_dst(dst), |i| {
                    hsv_color(hue.at(i), saturation.at(i), value.at(i))
                });
            }
            I::Recolor {
                shift,
                dst,
                color,
                hue,
            } => {
                let (color, hue) = (self.color(color), self.float(hue));
                pick_color(sel, self.color_dst(dst), |i| {
                    recolor(color_at(color, i), hue.at(i), shift)
                });
            }
            I::Curve {
                op,
                dst,
                curve,
                value,
                before,
            } => {
                let (dst, curve, value) =
                    (self.float_dst(dst), self.handle(curve), self.float(value));
                // One curve for the strip resolves once.
                match (op, curve.mask) {
                    (CurveOp::Sample, 0) => {
                        let view = self.curve(curve.at(0));
                        map1(sel, dst, value, |value| view.sample(value));
                    }
                    (CurveOp::Crossing, 0) => {
                        let view = self.curve(curve.at(0));
                        map1(sel, dst, value, |value| view.crossing(value));
                    }
                    (CurveOp::LastCrossing, _) => apply3(
                        sel,
                        dst,
                        curve,
                        value,
                        self.float(before),
                        |curve, value, before| self.curve_query(op, curve, value, before),
                    ),
                    _ => apply2(sel, dst, curve, value, |curve, value| {
                        self.curve_query(op, curve, value, 0.0)
                    }),
                }
            }
            I::Gradient {
                dst,
                gradient,
                position,
                scale,
            } => {
                let (dst, gradient, position) = (
                    self.color_dst(dst),
                    self.handle(gradient),
                    self.float(position),
                );
                let scale = (!scale.is_none()).then(|| self.float(scale));
                if gradient.mask == 0 {
                    let stops = self.gradient(gradient.at(0));
                    Self::gradient_strip(sel, dst, stops, position, scale);
                } else {
                    self.gradients(sel, dst, gradient, position, scale);
                }
            }
            I::Mark {
                op,
                dst,
                marks,
                operand,
            } => {
                let marks = self.handle(marks);
                // One search per strip; the pixels after it start from its place.
                let cursor = Cell::new(0);
                if marks.mask == 0 {
                    let window = self.marks(marks.at(0));
                    match op {
                        MarkOp::At => map1(sel, self.float_dst(dst), self.int(operand), |index| {
                            mark_at(&window, index)
                        }),
                        MarkOp::Last => {
                            map1(sel, self.float_dst(dst), self.float(operand), |seconds| {
                                previous_mark(&window, seconds, &cursor)
                                    .map_or(f32::NAN, |(_, time)| time)
                            });
                        }
                        MarkOp::LastIndex => {
                            map1(sel, self.int_dst(dst), self.float(operand), |seconds| {
                                previous_mark_index(&window, seconds, &cursor)
                            });
                        }
                    }
                    return;
                }
                match op {
                    MarkOp::At => apply2(
                        sel,
                        self.float_dst(dst),
                        marks,
                        self.int(operand),
                        |marks, index| self.mark_at(marks, index),
                    ),
                    MarkOp::Last => apply2(
                        sel,
                        self.float_dst(dst),
                        marks,
                        self.float(operand),
                        |marks, seconds| self.previous_mark(marks, seconds, &cursor),
                    ),
                    MarkOp::LastIndex => apply2(
                        sel,
                        self.int_dst(dst),
                        marks,
                        self.float(operand),
                        |marks, seconds| self.previous_mark_index(marks, seconds, &cursor),
                    ),
                }
            }
            I::Length {
                kind,
                dst,
                resource,
            } => apply1(sel, self.int_dst(dst), self.handle(resource), |resource| {
                self.length(kind, resource)
            }),
            I::Index {
                bank,
                dst,
                array,
                index,
                default,
            } => self.index(bank, dst, array, index, default, sel),
            I::Select {
                bank,
                dst,
                condition,
                yes,
                no,
            } => {
                let condition = self.boolean(condition);
                match bank {
                    Bank::Int => map3(
                        sel,
                        self.int_dst(dst),
                        condition,
                        self.int(yes),
                        self.int(no),
                        choose,
                    ),
                    Bank::Bool => map3(
                        sel,
                        self.bool_dst(dst),
                        condition,
                        self.boolean(yes),
                        self.boolean(no),
                        choose,
                    ),
                    Bank::Resource => map3(
                        sel,
                        self.handle_dst(dst),
                        condition,
                        self.handle(yes),
                        self.handle(no),
                        choose,
                    ),
                    _ => {
                        for channel in 0..bank.width() {
                            map3(
                                sel,
                                self.float_dst(dst.channel(channel)),
                                condition,
                                self.float(yes.channel(channel)),
                                self.float(no.channel(channel)),
                                choose,
                            );
                        }
                    }
                }
            }
            I::Move { bank, dst, src } => self.assign(bank, dst, src, sel),
            I::Section { index, dst, width } => {
                let (dst, width) = (self.int_dst(dst), self.int(width));
                let run = Cell::new(None);
                pick_each(sel, dst, |i| {
                    self.section_query(width.at(i), index, i, &run)
                });
            }
            I::Read { .. } => self.sample(instruction, sel, signals),
            I::Lookup {
                bank,
                dst,
                table,
                index,
                start,
            } => {
                let (index, start) = (self.int(index), self.int(start));
                let at = |i: usize| index.at(i).wrapping_sub(start.at(i)) as usize;
                match bank {
                    Bank::Int => {
                        let table = self.int(table);
                        pick_each(sel, self.int_dst(dst), |i| table.at(at(i)));
                    }
                    Bank::Bool => {
                        let table = self.boolean(table);
                        pick_each(sel, self.bool_dst(dst), |i| table.at(at(i)));
                    }
                    _ => {
                        let table = self.float(table);
                        pick_each(sel, self.float_dst(dst), |i| table.at(at(i)));
                    }
                }
            }
            I::Branch { .. }
            | I::Reduce { .. }
            | I::Stencil { .. }
            | I::Scan { .. }
            | I::Tabulate { .. } => {
                unreachable!("dispatched by block")
            }
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn compare(&self, op: CompareOp, bank: Bank, dst: Slot, a: Slot, b: Slot, sel: Sel<'_>) {
        let dst = self.bool_dst(dst);
        match bank {
            Bank::Bool => {
                let negate = op == CompareOp::NotEqual;
                map2(sel, dst, self.boolean(a), self.boolean(b), |a, b| {
                    (a == b) != negate
                });
            }
            Bank::Int => {
                let (a, b) = (self.int(a), self.int(b));
                match op {
                    CompareOp::Less => map2(sel, dst, a, b, |a, b| a < b),
                    CompareOp::LessEqual => map2(sel, dst, a, b, |a, b| a <= b),
                    CompareOp::Greater => map2(sel, dst, a, b, |a, b| a > b),
                    CompareOp::GreaterEqual => map2(sel, dst, a, b, |a, b| a >= b),
                    CompareOp::Equal => map2(sel, dst, a, b, |a, b| a == b),
                    CompareOp::NotEqual => map2(sel, dst, a, b, |a, b| a != b),
                }
            }
            _ => {
                let (a, b) = (self.float(a), self.float(b));
                match op {
                    CompareOp::Less => map2(sel, dst, a, b, |a, b| a < b),
                    CompareOp::LessEqual => map2(sel, dst, a, b, |a, b| a <= b),
                    CompareOp::Greater => map2(sel, dst, a, b, |a, b| a > b),
                    CompareOp::GreaterEqual => map2(sel, dst, a, b, |a, b| a >= b),
                    CompareOp::Equal => map2(sel, dst, a, b, |a, b| a == b),
                    CompareOp::NotEqual => map2(sel, dst, a, b, |a, b| a != b),
                }
            }
        }
    }

    /// Array items. Indices clamp; an empty array reads the default.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn index(&self, bank: Bank, dst: Slot, array: Slot, index: Slot, default: Slot, sel: Sel<'_>) {
        let (array, index) = (self.handle(array), self.int(index));
        let item = |i: usize| self.array_item(array.at(i), index.at(i));
        match bank {
            Bank::Float => {
                let default = self.float(default);
                pick_each(sel, self.float_dst(dst), |i| match item(i) {
                    Some(item) if item.kind == format::VALUE_FLOAT => f32::from_bits(item.word),
                    Some(item) if item.kind == format::VALUE_INT => item.word as i32 as f32,
                    _ => default.at(i),
                });
            }
            Bank::Int => {
                let default = self.int(default);
                pick_each(sel, self.int_dst(dst), |i| {
                    self.int_item(item(i), default.at(i))
                });
            }
            Bank::Bool => {
                let default = self.boolean(default);
                pick_each(sel, self.bool_dst(dst), |i| match item(i) {
                    Some(item) if item.kind == format::VALUE_BOOL => item.word != 0,
                    _ => default.at(i),
                });
            }
            Bank::Color => {
                let default = self.color(default);
                pick_color(sel, self.color_dst(dst), |i| match item(i) {
                    Some(item) if item.kind == format::VALUE_COLOR => {
                        word_color(self.bound.show, item.word)
                    }
                    _ => color_at(default, i),
                });
            }
            Bank::Resource => {
                let default = self.handle(default);
                pick_each(sel, self.handle_dst(dst), |i| match item(i) {
                    Some(item) => Handle::show(item.word),
                    _ => default.at(i),
                });
            }
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample(&self, instruction: &Instruction, sel: Sel<'_>, signals: &mut dyn StripSignals) {
        let Instruction::Read { dst, input, pixel } = *instruction else {
            unreachable!("dispatched by step")
        };
        let input = usize::from(input);
        let dst = self.color_dst(dst);
        if matches!(pixel, SignalPixel::Current) {
            let mut samples = self.ws.samples.borrow_mut();
            signals.read_strip(input, &mut samples);
            let samples = &**samples;
            pick_color(sel, dst, |i| samples[i]);
            return;
        }
        let address = pixel.map(|index| self.int(index));
        let [Dst::Many(red), Dst::Many(green), Dst::Many(blue)] = dst else {
            unreachable!("the compiler keeps samples in rows")
        };
        each(sel, |i| {
            let color = signals.read_pixel(input, i, address.map(|index| index.at(i)));
            red[i].set(color.red);
            green[i].set(color.green);
            blue[i].set(color.blue);
        });
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn choose<T>(condition: bool, yes: T, no: T) -> T {
    if condition { yes } else { no }
}

/// Write `value(i)` at the selected pixels, or once for a scalar destination.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn pick_each<T: Copy>(sel: Sel<'_>, dst: Dst<'_, T>, value: impl Fn(usize) -> T) {
    match dst {
        Dst::One(dst) => dst.set(value(0)),
        Dst::Many(dst) => each(sel, |i| dst[i].set(value(i))),
    }
}

/// An operand of a bank without pixel inputs.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn operand<'s, T: Copy>(store: &'s Store<'_, T>, slot: Slot) -> Src<'s, T> {
    match slot.kind() {
        SlotKind::Scalar(index) => store.one(index),
        SlotKind::Row(index) => store.many(index),
        SlotKind::Input(_) => unreachable!("the compiler keeps pixel inputs in their banks"),
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn destination<'s, T: Copy>(store: &'s Store<'_, T>, slot: Slot) -> Dst<'s, T> {
    match slot.kind() {
        SlotKind::Scalar(index) => Dst::One(store.scalar(index)),
        SlotKind::Row(index) => Dst::Many(store.row(index)),
        SlotKind::Input(_) => unreachable!("the compiler never writes pixel inputs"),
    }
}

/// Add a position to the last segment when it continues it, or start one.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn extend(segments: &mut [Segment], count: &mut usize, at: i32, key: i32) {
    if let Some(last) = count.checked_sub(1).map(|last| &mut segments[last])
        && i32::from(last.end) == at
        && last.key == key
    {
        last.end += 1;
        return;
    }
    segments[*count] = Segment {
        start: at as i16,
        end: at as i16 + 1,
        key,
    };
    *count += 1;
}

/// `acc += color * factor` at pixel `i`, or the channelwise max.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn combine_term(acc: [&Row<f32>; 3], i: usize, color: Color, factor: f32, max: bool) {
    for (acc, channel) in acc.iter().zip([color.red, color.green, color.blue]) {
        let term = channel * factor;
        let current = acc[i].get();
        acc[i].set(if max {
            max_float(current, term)
        } else {
            current + term
        });
    }
}
