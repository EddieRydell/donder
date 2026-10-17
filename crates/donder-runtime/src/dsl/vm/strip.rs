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
//! for rows and for a row with a scalar.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
use super::RunContext;
use super::context::Clock;
use super::workspace::{NEIGHBORHOOD, Neighborhood, StripWorkspace};
use crate::dsl::bytecode::{
    Bank, ColorBinary, ColorComponent, CompareOp, ContextRead, Direction, Edges, FloatBinary,
    Input, Instruction, IntBinary, MAX_ITERATIONS, MarkOp, NO_FRAME_CACHE, Reducer, Resource,
    SignalPixel, Slot, SlotKind,
};
use crate::resources::{CurveView, ParamsView};
use crate::sampling::{
    add_colors, clamp_array_index, clamp_float, color_hue, color_intensity, color_saturation,
    float_binary, float_unary, gradient_color_scaled, hsv, int_binary, invert_color, length_int,
    mark_at, max_colors, mix_colors, multiply_colors, previous_mark, previous_mark_index,
    query_progress, query_seconds, rgb, sample_gradient_stops, scale_color, section_position,
};
use crate::sections::{RunSections, SectionsView, single_section_query};
use crate::show::{ProgramView, Show};
use core::cell::Cell;
use donder_runtime_types::image::{self, GradientStop, SectionPixel};
use donder_runtime_types::{Color, FloatColor, InstructionWord, MarkWindow, SampleTime};

pub(crate) use crate::dsl::bytecode::STRIP;
const MASK: usize = STRIP - 1;

pub(super) use crate::arena::Row;

/// The instruction a word of an admitted program encodes; one copy serves
/// every dispatch.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn decoded(word: &InstructionWord) -> Instruction {
    let Some(instruction) = word.decode_trusted() else {
        unreachable!("admission decodes every word")
    };
    instruction
}

/// A bound color word: red, green and blue in its low bytes.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn word_color(word: u32) -> Color {
    let [red, green, blue, _] = word.to_le_bytes();
    Color { red, green, blue }
}

/// Signal queries of a running strip. Pixel `n` of a strip is its `n`th
/// pixel; the provider knows which pixels the strip covers.
pub(crate) trait StripSignals {
    /// `input` at `time` for every pixel of the strip, each at its own pixel.
    fn sample_strip(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        output: &mut [Color; STRIP],
    );

    /// `input` at `time` for one pixel at an explicit address.
    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        offset: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color;

    /// The strip's pixels of `query`'s scan, computed for the whole frame
    /// when the query's frame cache does not hold it yet.
    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn SourceWeights,
        output: &mut [Color; STRIP],
    );

    /// `input` at `time` for the pixels from `start` pixels after the strip's
    /// first, in the target's order: their colors and indices in their
    /// fixtures, black and [`OUTSIDE`] past the target's ends.
    fn sample_range(
        &mut self,
        input: usize,
        time: SampleTime,
        start: isize,
        frame_cache: Option<usize>,
        colors: &mut [Color],
        locals: &mut [i32],
    );
}

/// A scan: its input, the frame cache of its input and of its result, and
/// how it runs.
#[derive(Clone, Copy)]
pub(crate) struct ScanQuery {
    pub(crate) input: usize,
    pub(crate) time: SampleTime,
    pub(crate) frame_cache: Option<usize>,
    pub(crate) cache: usize,
    pub(crate) direction: Direction,
    pub(crate) decay: f32,
}

/// A scan's weight of each input color, from its source block.
pub(crate) trait SourceWeights {
    fn weights(&mut self, colors: &[Color], weights: &mut [f32]);
}

/// Effects never query signals; admission rejects the instruction.
impl StripSignals for super::NoSignals {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_strip(&mut self, _: usize, _: SampleTime, _: Option<usize>, _: &mut [Color; STRIP]) {
        unreachable!("effect admission excludes signal instructions")
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_pixel(
        &mut self,
        _: usize,
        _: SampleTime,
        _: usize,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Color {
        unreachable!("effect admission excludes signal instructions")
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_range(
        &mut self,
        _: usize,
        _: SampleTime,
        _: isize,
        _: Option<usize>,
        _: &mut [Color],
        _: &mut [i32],
    ) {
        unreachable!("effect admission excludes signal instructions")
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scan(&mut self, _: &ScanQuery, _: &mut dyn SourceWeights, _: &mut [Color; STRIP]) {
        unreachable!("effect admission excludes signal instructions")
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

/// A resource: a parameter or constant of the program, or an item of an
/// array parameter or constant.
#[derive(Clone, Copy, Debug)]
pub(super) enum HandleKind {
    Empty,
    Param(Resource, u16),
    Constant(Resource, u16),
    ParamItem(u16, u16),
    ConstantItem(u16, u16),
}

/// A [`HandleKind`] as plain data, so a workspace bank can hold it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(super) struct Handle {
    tag: u16,
    a: u16,
    b: u16,
}

const RESOURCES: [Resource; 4] = [
    Resource::Curve,
    Resource::Gradient,
    Resource::Marks,
    Resource::Array,
];

impl Handle {
    pub(super) const EMPTY: Self = Self { tag: 0, a: 0, b: 0 };

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn new(kind: HandleKind) -> Self {
        let resource = |resource: Resource| resource as u16;
        let (tag, a, b) = match kind {
            HandleKind::Empty => (0, 0, 0),
            HandleKind::Param(kind, bank) => (1, resource(kind), bank),
            HandleKind::Constant(kind, index) => (2, resource(kind), index),
            HandleKind::ParamItem(bank, index) => (3, bank, index),
            HandleKind::ConstantItem(array, index) => (4, array, index),
        };
        Self { tag, a, b }
    }

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn kind(self) -> HandleKind {
        let resource = || RESOURCES[usize::from(self.a & 3)];
        match self.tag {
            1 => HandleKind::Param(resource(), self.b),
            2 => HandleKind::Constant(resource(), self.b),
            3 => HandleKind::ParamItem(self.a, self.b),
            4 => HandleKind::ConstantItem(self.a, self.b),
            _ => HandleKind::Empty,
        }
    }
}

pub(super) const NO_COLOR: FloatColor = FloatColor::BLACK;

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

/// Colors compare as the 8-bit colors they are written as.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn same_color(a: FloatColor, b: FloatColor) -> bool {
    a.to_color() == b.to_color()
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn int(op: IntBinary, a: i32, b: i32) -> i32 {
    int_binary(op, a, b)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn component(op: ColorComponent, color: FloatColor) -> f32 {
    match op {
        ColorComponent::Hue => color_hue(color),
        ColorComponent::Red => color.red,
        ColorComponent::Green => color.green,
        ColorComponent::Blue => color.blue,
        ColorComponent::Saturation => color_saturation(color),
        ColorComponent::Intensity => color_intensity(color),
    }
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn mix_color(a: FloatColor, b: FloatColor, amount: f32) -> FloatColor {
    mix_colors(a, b, amount)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn rgb_color(red: f32, green: f32, blue: f32) -> FloatColor {
    rgb(red, green, blue)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn hsv_color(hue: f32, saturation: f32, value: f32) -> FloatColor {
    hsv(hue, saturation, value)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn recolor(color: FloatColor, hue: f32, shift: bool) -> FloatColor {
    crate::sampling::recolor(color, hue, shift)
}

#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn power(base: f32, count: i32) -> f32 {
    let mut power = 1.0;
    for _ in 0..count.clamp(0, MAX_ITERATIONS) {
        power *= base;
    }
    power
}

/// Offsets of pixel `i`'s range, as a reduction counts its iterations.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn offset_count(start: Src<'_, i32>, end: Src<'_, i32>, i: usize) -> i32 {
    end.at(i).wrapping_sub(start.at(i)).clamp(0, MAX_ITERATIONS)
}

/// A stencil's neighborhood, read by every offset.
struct Term<'n, 'm> {
    edges: Edges,
    /// Pixels in each strip pixel's fixture, when reading past its ends.
    count: i32,
    /// The strip position of the neighborhood's first pixel.
    origin: i32,
    colors: &'n [Color],
    locals: &'n [i32],
    weights: &'n [f32],
    pixel: Src<'m, f32>,
}

/// A scan's source block, run over strips of input colors.
struct Weights<'s, 'm, 'w> {
    machine: &'s Machine<'m, 'w>,
    code: &'s [InstructionWord],
    sample: Slot,
    weight: Slot,
    levels: Levels<'w>,
}

impl SourceWeights for Weights<'_, '_, '_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn weights(&mut self, colors: &[Color], weights: &mut [f32]) {
        let Dst::Many(row) = self.machine.color_dst(self.sample) else {
            unreachable!()
        };
        for (colors, weights) in colors.chunks(STRIP).zip(weights.chunks_mut(STRIP)) {
            for (cell, color) in row.iter().zip(colors) {
                cell.set((*color).into());
            }
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
    /// The latest sample seconds and their time.
    query: Cell<Option<(u32, Option<SampleTime>)>>,
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
            query: Cell::new(None),
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
            query: &self.query,
            curves: Cell::new((Handle::EMPTY, CurveView::EMPTY)),
            marks: Cell::new((Handle::EMPTY, None)),
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
            *output = result.at(index).to_color();
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
    query: &'m Cell<Option<(u32, Option<SampleTime>)>>,
    /// The resource each kind's latest handle named: a strip's pixels and
    /// instructions mostly read one, which is resolved once.
    curves: Cell<(Handle, CurveView<'m>)>,
    marks: Cell<(Handle, Option<MarkWindow<'m>>)>,
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

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn color(&self, slot: Slot) -> Src<'m, FloatColor> {
        operand(&self.ws.colors, slot)
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

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn color_dst(&self, slot: Slot) -> Dst<'m, FloatColor> {
        destination(&self.ws.colors, slot)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn handle_dst(&self, slot: Slot) -> Dst<'m, Handle> {
        destination(&self.ws.handles, slot)
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
            let instruction = &decoded(&code[at]);
            at += 1;
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
                    source_len,
                    tap_len,
                    ..
                } => {
                    let source = &code[at..at + usize::from(source_len)];
                    at += usize::from(source_len);
                    let taps = &code[at..at + usize::from(tap_len)];
                    at += usize::from(tap_len);
                    self.stencil(instruction, source, taps, sel, levels, signals);
                }
                Instruction::Scan { source_len, .. } => {
                    let source = &code[at..at + usize::from(source_len)];
                    at += usize::from(source_len);
                    self.scan(instruction, source, sel, levels, signals);
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
        let count = |i: usize| end.at(i).wrapping_sub(start.at(i)).clamp(0, MAX_ITERATIONS);
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
        let (mut first, mut past) = (i64::MAX, i64::MIN);
        let mut iterations = 0;
        if shared {
            each(sel, |i| {
                let count = count(i);
                if count > 0 {
                    first = first.min(i64::from(start.at(i)));
                    past = past.max(i64::from(start.at(i)) + i64::from(count));
                }
            });
            iterations = past
                .saturating_sub(first)
                .clamp(0, i64::from(MAX_ITERATIONS)) as i32;
        } else {
            each(sel, |i| iterations = iterations.max(count(i)));
        }
        // The shared index at iteration `k`, ascending or, for `last`, descending.
        let shared_at = |k: i32| {
            if last {
                past - 1 - i64::from(k)
            } else {
                first + i64::from(k)
            }
        };
        let takes_part = |i: usize, k: i32| {
            if shared {
                let (index, start) = (shared_at(k), i64::from(start.at(i)));
                index >= start && index < start + i64::from(count(i))
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
                Dst::One(index) if shared => index.set(shared_at(k) as i32),
                Dst::One(index) => index.set(index_at(0, k)),
                Dst::Many(index) => each(participating, |i| index[i].set(index_at(i, k))),
            }
            self.block(loop_code, participating, inner, signals);
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
    /// strips: its input is read once and its source weights computed once,
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
        let Instruction::Sample {
            dst: sample,
            input,
            seconds,
            frame_cache,
            ..
        } = decoded(&source[0])
        else {
            unreachable!()
        };
        // Missing samples are black, and black terms leave the identity.
        let Some(time) = self.query_time(self.float(seconds).at(0)) else {
            return;
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
        } = &mut *neighborhood;
        let frame_cache = (frame_cache != NO_FRAME_CACHE).then_some(usize::from(frame_cache));
        signals.sample_range(
            usize::from(input),
            time,
            origin as isize,
            frame_cache,
            &mut colors[..len],
            &mut locals[..len],
        );
        // Black neighbors contribute nothing, so `acc` keeps its black.
        if colors[..len].iter().all(|color| *color == Color::BLACK) {
            return;
        }
        let Dst::Many(row) = self.color_dst(sample) else {
            unreachable!()
        };
        for chunk in (0..len).step_by(STRIP) {
            let size = (len - chunk).min(STRIP);
            for k in 0..size {
                row[k].set(colors[chunk + k].into());
            }
            self.block(&source[1..], Sel(&range(0, size as u8)), levels, signals);
            let computed = self.float(weight);
            for k in 0..size {
                weights[chunk + k] = computed.at(k);
            }
        }
        let index = self.int_dst(index);
        let Dst::Many(acc) = self.color_dst(acc) else {
            unreachable!()
        };
        let term = Term {
            edges,
            count: self.context.pixel_count,
            origin,
            colors: &colors[..len],
            locals: &locals[..len],
            weights: &weights[..len],
            pixel: self.float(pixel),
        };
        for k in 0..offsets {
            let offset = first + k as i32;
            Self::set_scalar(index, offset);
            self.block(taps, sel, levels, signals);
            let scale = self.float(scale).at(0);
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
        acc: &Row<FloatColor>,
    ) {
        let local = &self.ws.pixels.index;
        for range in sel.0 {
            let (first, past) = range.get().get();
            for i in usize::from(first)..usize::from(past).min(STRIP) {
                let from = offset.wrapping_sub(start.at(i)) as u32;
                if from >= offset_count(start, end, i) as u32 {
                    continue;
                }
                let local = local[i].get();
                let read = match term.edges {
                    Edges::Skip => local.wrapping_add(offset),
                    edges => match edges.local(local.wrapping_add(offset), term.count) {
                        Some(read) => read,
                        None => continue,
                    },
                };
                let at = (i as i32 + read - local - term.origin) as usize;
                let (Some(&weight), true) = (
                    term.weights.get(at),
                    term.edges != Edges::Skip || term.locals[at] == read,
                ) else {
                    continue;
                };
                if weight == 0.0 {
                    continue;
                }
                let color = scale_color(term.colors[at].into(), weight * scale * term.pixel.at(i));
                acc[i].set(match reducer {
                    Reducer::Max => max_colors(acc[i].get(), color),
                    _ => add_colors(acc[i].get(), color),
                });
            }
        }
    }

    /// A scan: the provider computes it for the whole frame, once per query,
    /// with the source block weighting each input color.
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
        let Instruction::Sample {
            dst: sample,
            input,
            seconds,
            frame_cache,
            ..
        } = decoded(&source[0])
        else {
            unreachable!()
        };
        let Dst::Many(dst) = self.color_dst(dst) else {
            unreachable!()
        };
        let Some(time) = self.query_time(self.float(seconds).at(0)) else {
            each(sel, |i| dst[i].set(NO_COLOR));
            return;
        };
        let query = ScanQuery {
            input: usize::from(input),
            time,
            frame_cache: (frame_cache != NO_FRAME_CACHE).then_some(usize::from(frame_cache)),
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
        each(sel, |i| dst[i].set(samples[i].into()));
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
            Bank::Float => {
                let (dst, a, b) = (self.float_dst(acc), self.float(acc), self.float(value));
                match reducer {
                    Reducer::Max => {
                        map2(sel, dst, a, b, |a, b| float_binary(FloatBinary::Max, a, b))
                    }
                    Reducer::Min => {
                        map2(sel, dst, a, b, |a, b| float_binary(FloatBinary::Min, a, b))
                    }
                    _ => map2(sel, dst, a, b, |a, b| a + b),
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
            Bank::Color => {
                let (dst, a, b) = (self.color_dst(acc), self.color(acc), self.color(value));
                match reducer {
                    Reducer::Max => map2(sel, dst, a, b, max_colors),
                    _ => map2(sel, dst, a, b, add_colors),
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

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn assign(&self, bank: Bank, dst: Slot, src: Slot, sel: Sel<'_>) {
        match bank {
            Bank::Float => map1(sel, self.float_dst(dst), self.float(src), |v| v),
            Bank::Int => map1(sel, self.int_dst(dst), self.int(src), |v| v),
            Bank::Bool => map1(sel, self.bool_dst(dst), self.boolean(src), |v| v),
            Bank::Color => map1(sel, self.color_dst(dst), self.color(src), |v| v),
            Bank::Resource => map1(sel, self.handle_dst(dst), self.handle(src), |v| v),
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn query_time(&self, seconds: f32) -> Option<SampleTime> {
        let bits = seconds.to_bits();
        if let Some((previous, time)) = self.query.get()
            && previous == bits
        {
            return time;
        }
        let time = donder_runtime_types::sample_time_from_seconds_f32(seconds).ok();
        self.query.set(Some((bits, time)));
        time
    }

    /// A bound resource parameter of `kind`, or none when it holds another.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn resource(&self, bank: u16, kind: u32) -> Option<crate::resources::ResourceSlot> {
        self.bound.params.resource(bank, kind)
    }

    /// The image resource of `kind` that constant `index` of the program
    /// names.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn constant(&self, kind: Resource, index: u16) -> u32 {
        let program = &self.bound.program;
        let pool = match kind {
            Resource::Curve => program.curves,
            Resource::Gradient => program.gradients,
            Resource::Marks => program.marks,
            Resource::Array => program.arrays,
        };
        pool[usize::from(index)].index
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve(&self, handle: Handle) -> CurveView<'m> {
        let (known, resource) = self.curves.get();
        if known == handle {
            return resource;
        }
        let resource = self.resolve_curve(handle);
        self.curves.set((handle, resource));
        resource
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn resolve_curve(&self, handle: Handle) -> CurveView<'m> {
        let show = self.bound.show;
        match handle.kind() {
            HandleKind::Param(Resource::Curve, bank) => self
                .resource(bank, image::RESOURCE_CURVE)
                .map_or(CurveView::EMPTY, |slot| self.bound.params.curve(show, slot)),
            HandleKind::Constant(Resource::Curve, index) => {
                show.curve(self.constant(Resource::Curve, index))
            }
            _ => match self.item(handle) {
                Some(item) if item.kind == image::VALUE_CURVE => show.curve(item.word),
                _ => CurveView::EMPTY,
            },
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient(&self, handle: Handle) -> &'m [GradientStop] {
        let show = self.bound.show;
        match handle.kind() {
            HandleKind::Param(Resource::Gradient, bank) => self
                .resource(bank, image::RESOURCE_GRADIENT)
                .map_or(&[], |slot| show.gradient(slot.index)),
            HandleKind::Constant(Resource::Gradient, index) => {
                show.gradient(self.constant(Resource::Gradient, index))
            }
            _ => match self.item(handle) {
                Some(item) if item.kind == image::VALUE_GRADIENT => show.gradient(item.word),
                _ => &[],
            },
        }
    }

    /// `None` for a handle that names no marks, which hold none.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn marks(&self, handle: Handle) -> Option<MarkWindow<'m>> {
        let (known, resource) = self.marks.get();
        if known == handle {
            return resource;
        }
        let resource = self.resolve_marks(handle);
        self.marks.set((handle, resource));
        resource
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn resolve_marks(&self, handle: Handle) -> Option<MarkWindow<'m>> {
        let show = self.bound.show;
        match handle.kind() {
            HandleKind::Param(Resource::Marks, bank) => self
                .resource(bank, image::RESOURCE_MARKS)
                .map(|slot| self.bound.params.marks(show, slot)),
            HandleKind::Constant(Resource::Marks, index) => {
                Some(show.marks(self.constant(Resource::Marks, index)))
            }
            _ => match self.item(handle) {
                Some(item) if item.kind == image::VALUE_MARKS => Some(show.marks(item.word)),
                _ => None,
            },
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn array(&self, handle: Handle) -> &'m [image::Value] {
        let show = self.bound.show;
        match handle.kind() {
            HandleKind::Param(Resource::Array, bank) => self
                .resource(bank, image::RESOURCE_ARRAY)
                .map_or(&[], |slot| show.array(slot.index)),
            HandleKind::Constant(Resource::Array, index) => {
                show.array(self.constant(Resource::Array, index))
            }
            _ => &[],
        }
    }

    /// The item an item handle names; `Index` clamped it to its array.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn item(&self, handle: Handle) -> Option<&'m image::Value> {
        let show = self.bound.show;
        match handle.kind() {
            HandleKind::ParamItem(bank, index) => self
                .resource(bank, image::RESOURCE_ARRAY)
                .and_then(|slot| show.array(slot.index).get(usize::from(index))),
            HandleKind::ConstantItem(array, index) => show
                .array(self.constant(Resource::Array, array))
                .get(usize::from(index)),
            _ => None,
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_sample(&self, curve: Handle, position: f32) -> f32 {
        self.curve(curve).sample(position)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_integral(&self, curve: Handle, position: f32) -> f32 {
        self.curve(curve).integral(position)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_crossing(&self, curve: Handle, value: f32) -> f32 {
        self.curve(curve).crossing(value)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_last_crossing(&self, curve: Handle, value: f32, before: f32) -> f32 {
        self.curve(curve).last_crossing(value, before)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient_sample(&self, gradient: Handle, position: f32) -> FloatColor {
        sample_gradient_stops(self.gradient(gradient), position)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn gradient_scaled(&self, gradient: Handle, position: f32, scale: f32) -> FloatColor {
        gradient_color_scaled(self.gradient(gradient), position, scale)
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn previous_mark(&self, marks: Handle, seconds: f32, cursor: &Cell<usize>) -> f32 {
        self.marks(marks)
            .and_then(|marks| previous_mark(&marks, seconds, cursor))
            .map_or(f32::NAN, |(_, time)| time)
    }

    // Per-pixel bodies too large to inline stay named methods, so that the
    // `iram` attribute places them; an out-of-line closure would land in flash.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn curve_clamped(
        &self,
        sel: Sel<'_>,
        dst: Dst<'_, f32>,
        curve: Src<'m, Handle>,
        position: Src<'m, f32>,
        min: Src<'m, f32>,
        max: Src<'m, f32>,
    ) {
        pick_each(sel, dst, |i| {
            let value = self.curve_sample(curve.at(i), position.at(i));
            clamp_float(value, min.at(i), max.at(i))
        });
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn mark_count(&self, marks: Handle) -> i32 {
        length_int(self.marks(marks).map_or(0, |marks| marks.len()))
    }

    /// Item `index` of `array`, clamped to it, with the item's position; none
    /// of an empty array.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn array_item(&self, array: Handle, index: i32) -> Option<(Handle, usize, &'m image::Value)> {
        let items = self.array(array);
        if items.is_empty() {
            return None;
        }
        let at = clamp_array_index(index, items.len());
        Some((array, at, &items[at]))
    }

    /// An int array item: an int, or an enum option's index.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn int_item(&self, item: Option<&image::Value>, default: i32) -> i32 {
        match item {
            Some(item) if matches!(item.kind, image::VALUE_INT | image::VALUE_ENUM) => {
                item.word as i32
            }
            _ => default,
        }
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn mark_at(&self, marks: Handle, index: i32) -> f32 {
        self.marks(marks)
            .map_or(f32::NAN, |marks| mark_at(&marks, index))
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn previous_mark_index(&self, marks: Handle, seconds: f32, cursor: &Cell<usize>) -> i32 {
        self.marks(marks)
            .map_or(-1, |marks| previous_mark_index(&marks, seconds, cursor))
    }

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn array_len(&self, array: Handle) -> i32 {
        length_int(self.array(array).len())
    }

    /// Pixel `i` of the item that `index` picks from `items`.
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn picked<T: Copy + 'm>(
        &self,
        operand: impl Fn(&Self, Slot) -> Src<'m, T>,
        items: &[Slot],
        index: i32,
        i: usize,
    ) -> T {
        operand(self, items[clamp_array_index(index, items.len())]).at(i)
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

    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn section_position(&self, width: f32, inverse: f32, i: usize) -> f32 {
        section_position(self.ws.pixels.index[i & MASK].get(), width, inverse)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn step(&self, instruction: &Instruction, sel: Sel<'_>, signals: &mut dyn StripSignals) {
        use Instruction as I;
        let params = self.bound.params;
        match *instruction {
            I::FloatConst { dst, bits } => {
                Self::set_scalar(self.float_dst(dst), f32::from_bits(bits))
            }
            I::IntConst { dst, value } => Self::set_scalar(self.int_dst(dst), value),
            I::BoolConst { dst, value } => Self::set_scalar(self.bool_dst(dst), value),
            I::ColorConst { dst, value } => Self::set_scalar(self.color_dst(dst), value.into()),
            I::ResourceConst { dst, kind, index } => {
                Self::set_scalar(
                    self.handle_dst(dst),
                    Handle::new(HandleKind::Constant(kind, index)),
                );
            }
            I::FloatParam { dst, bank } => {
                let word = params.words[usize::from(bank)];
                Self::set_scalar(self.float_dst(dst), f32::from_bits(word));
            }
            I::IntParam { dst, bank } => {
                Self::set_scalar(self.int_dst(dst), params.words[usize::from(bank)] as i32);
            }
            I::BoolParam { dst, bank } => {
                Self::set_scalar(self.bool_dst(dst), params.words[usize::from(bank)] != 0);
            }
            I::ColorParam { dst, bank } => {
                let word = params.words[usize::from(bank)];
                Self::set_scalar(self.color_dst(dst), word_color(word).into());
            }
            I::ResourceParam { dst, kind, bank } => {
                Self::set_scalar(
                    self.handle_dst(dst),
                    Handle::new(HandleKind::Param(kind, bank)),
                );
            }
            I::Context { dst, read } => match read {
                ContextRead::PixelCount => {
                    Self::set_scalar(self.int_dst(dst), self.context.pixel_count);
                }
                read => Self::set_scalar(
                    self.float_dst(dst),
                    match read {
                        ContextRead::Seconds => self.clock.seconds,
                        ContextRead::Progress => self.context.progress,
                        ContextRead::Duration => self.clock.duration,
                        ContextRead::FrameRate => self.context.frame_rate,
                        ContextRead::TargetMinX => self.min[0],
                        ContextRead::TargetMinY => self.min[1],
                        ContextRead::TargetMaxX => self.max[0],
                        _ => self.max[1],
                    },
                ),
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
            I::FloatBinary { op, dst, a, b } => {
                let (dst, a, b) = (self.float_dst(dst), self.float(a), self.float(b));
                use FloatBinary as B;
                match op {
                    B::Add => map2(sel, dst, a, b, |a, b| a + b),
                    B::Subtract => map2(sel, dst, a, b, |a, b| a - b),
                    B::Multiply => map2(sel, dst, a, b, |a, b| a * b),
                    B::Divide => map2(sel, dst, a, b, |a, b| a / b),
                    B::Remainder => apply2(sel, dst, a, b, |a, b| binary(B::Remainder, a, b)),
                    B::Min => map2(sel, dst, a, b, |a, b| float_binary(B::Min, a, b)),
                    B::Max => map2(sel, dst, a, b, |a, b| float_binary(B::Max, a, b)),
                    B::ValueOr => map2(sel, dst, a, b, |a, b| float_binary(B::ValueOr, a, b)),
                    op => apply2(sel, dst, a, b, |a, b| binary(op, a, b)),
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
                clamp_float,
            ),
            I::Mix { dst, a, b, amount } => map3(
                sel,
                self.float_dst(dst),
                self.float(a),
                self.float(b),
                self.float(amount),
                |a, b, t| a + (b - a) * t,
            ),
            I::Power { dst, base, count } => apply2(
                sel,
                self.float_dst(dst),
                self.float(base),
                self.int(count),
                power,
            ),
            I::Clock {
                progress,
                dst,
                seconds,
            } => {
                let duration = self.context.duration;
                let (dst, seconds) = (self.float_dst(dst), self.float(seconds));
                let clock = if progress {
                    query_progress
                } else {
                    query_seconds
                };
                apply1(sel, dst, seconds, |s| clock(s, duration));
            }
            I::IntNegate { dst, a } => map1(sel, self.int_dst(dst), self.int(a), i32::wrapping_neg),
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
            I::Not { dst, a } => map1(sel, self.bool_dst(dst), self.boolean(a), |v| !v),
            I::FloatCompare { op, dst, a, b } => {
                let (dst, a, b) = (self.bool_dst(dst), self.float(a), self.float(b));
                match op {
                    CompareOp::Less => map2(sel, dst, a, b, |a, b| a < b),
                    CompareOp::LessEqual => map2(sel, dst, a, b, |a, b| a <= b),
                    CompareOp::Greater => map2(sel, dst, a, b, |a, b| a > b),
                    CompareOp::GreaterEqual => map2(sel, dst, a, b, |a, b| a >= b),
                }
            }
            I::IntCompare { op, dst, a, b } => {
                let (dst, a, b) = (self.bool_dst(dst), self.int(a), self.int(b));
                match op {
                    CompareOp::Less => map2(sel, dst, a, b, |a, b| a < b),
                    CompareOp::LessEqual => map2(sel, dst, a, b, |a, b| a <= b),
                    CompareOp::Greater => map2(sel, dst, a, b, |a, b| a > b),
                    CompareOp::GreaterEqual => map2(sel, dst, a, b, |a, b| a >= b),
                }
            }
            I::Equal {
                bank,
                negate,
                dst,
                a,
                b,
            } => {
                let dst = self.bool_dst(dst);
                match bank {
                    Bank::Float => {
                        map2(sel, dst, self.float(a), self.float(b), |a, b| {
                            (a == b) != negate
                        });
                    }
                    Bank::Int => map2(sel, dst, self.int(a), self.int(b), |a, b| {
                        (a == b) != negate
                    }),
                    Bank::Bool => map2(sel, dst, self.boolean(a), self.boolean(b), |a, b| {
                        (a == b) != negate
                    }),
                    Bank::Color => {
                        map2(sel, dst, self.color(a), self.color(b), |a, b| {
                            same_color(a, b) != negate
                        });
                    }
                    Bank::Resource => unreachable!("admission rejects resource equality"),
                }
            }
            I::ColorBinary { op, dst, a, b } => {
                let (dst, a, b) = (self.color_dst(dst), self.color(a), self.color(b));
                match op {
                    ColorBinary::Add => map2(sel, dst, a, b, add_colors),
                    ColorBinary::Multiply => map2(sel, dst, a, b, multiply_colors),
                    ColorBinary::Max => map2(sel, dst, a, b, max_colors),
                }
            }
            I::ColorScale { dst, color, scale } => map2(
                sel,
                self.color_dst(dst),
                self.color(color),
                self.float(scale),
                scale_color,
            ),
            I::MixColor { dst, a, b, amount } => apply3(
                sel,
                self.color_dst(dst),
                self.color(a),
                self.color(b),
                self.float(amount),
                mix_color,
            ),
            I::ColorComponent { op, dst, color } => {
                apply1(sel, self.float_dst(dst), self.color(color), |color| {
                    component(op, color)
                })
            }
            I::Invert { dst, color } => {
                map1(sel, self.color_dst(dst), self.color(color), invert_color);
            }
            I::Rgb {
                dst,
                red,
                green,
                blue,
            } => apply3(
                sel,
                self.color_dst(dst),
                self.float(red),
                self.float(green),
                self.float(blue),
                rgb_color,
            ),
            I::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => apply3(
                sel,
                self.color_dst(dst),
                self.float(hue),
                self.float(saturation),
                self.float(value),
                hsv_color,
            ),
            I::Recolor {
                shift,
                dst,
                color,
                hue,
            } => apply2(
                sel,
                self.color_dst(dst),
                self.color(color),
                self.float(hue),
                |color, hue| recolor(color, hue, shift),
            ),
            I::CurveSample {
                dst,
                curve,
                position,
            } => apply2(
                sel,
                self.float_dst(dst),
                self.handle(curve),
                self.float(position),
                |curve, position| self.curve_sample(curve, position),
            ),
            I::CurveIntegral {
                dst,
                curve,
                position,
            } => apply2(
                sel,
                self.float_dst(dst),
                self.handle(curve),
                self.float(position),
                |curve, position| self.curve_integral(curve, position),
            ),
            I::CurveClamped {
                dst,
                curve,
                position,
                min,
                max,
            } => {
                let (dst, curve, position) = (
                    self.float_dst(dst),
                    self.handle(curve),
                    self.float(position),
                );
                let (min, max) = (self.float(min), self.float(max));
                self.curve_clamped(sel, dst, curve, position, min, max);
            }
            I::CurveCrossing {
                dst,
                curve,
                value,
                before,
            } => {
                let (dst, curve, value) =
                    (self.float_dst(dst), self.handle(curve), self.float(value));
                if before.is_none() {
                    apply2(sel, dst, curve, value, |curve, value| {
                        self.curve_crossing(curve, value)
                    });
                } else {
                    apply3(
                        sel,
                        dst,
                        curve,
                        value,
                        self.float(before),
                        |curve, value, before| self.curve_last_crossing(curve, value, before),
                    );
                }
            }
            I::GradientSample {
                dst,
                gradient,
                position,
            } => apply2(
                sel,
                self.color_dst(dst),
                self.handle(gradient),
                self.float(position),
                |gradient, position| self.gradient_sample(gradient, position),
            ),
            I::GradientScaled {
                dst,
                gradient,
                position,
                scale,
            } => apply3(
                sel,
                self.color_dst(dst),
                self.handle(gradient),
                self.float(position),
                self.float(scale),
                |gradient, position, scale| self.gradient_scaled(gradient, position, scale),
            ),
            I::Mark {
                op,
                dst,
                marks,
                operand,
            } => {
                let marks = self.handle(marks);
                // One search per strip; the pixels after it start from its place.
                let cursor = Cell::new(0);
                match op {
                    MarkOp::Count => apply1(sel, self.int_dst(dst), marks, |marks| {
                        self.mark_count(marks)
                    }),
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
            I::Len { dst, array } => apply1(sel, self.int_dst(dst), self.handle(array), |array| {
                self.array_len(array)
            }),
            I::Index {
                bank,
                dst,
                array,
                index,
                default,
            } => self.index(bank, dst, array, index, default, sel),
            I::Pick {
                bank,
                dst,
                index,
                items,
            } => {
                let items = &self.bound.program.operands[items.range()];
                let index = self.int(index);
                match bank {
                    Bank::Float => pick_each(sel, self.float_dst(dst), |i| {
                        self.picked(Self::float, items, index.at(i), i)
                    }),
                    Bank::Int => pick_each(sel, self.int_dst(dst), |i| {
                        self.picked(Self::int, items, index.at(i), i)
                    }),
                    Bank::Bool => pick_each(sel, self.bool_dst(dst), |i| {
                        self.picked(Self::boolean, items, index.at(i), i)
                    }),
                    Bank::Color => pick_each(sel, self.color_dst(dst), |i| {
                        self.picked(Self::color, items, index.at(i), i)
                    }),
                    Bank::Resource => pick_each(sel, self.handle_dst(dst), |i| {
                        self.picked(Self::handle, items, index.at(i), i)
                    }),
                }
            }
            I::Select {
                bank,
                dst,
                condition,
                yes,
                no,
            } => {
                let condition = self.boolean(condition);
                match bank {
                    Bank::Float => {
                        map3(
                            sel,
                            self.float_dst(dst),
                            condition,
                            self.float(yes),
                            self.float(no),
                            choose,
                        );
                    }
                    Bank::Int => {
                        map3(
                            sel,
                            self.int_dst(dst),
                            condition,
                            self.int(yes),
                            self.int(no),
                            choose,
                        );
                    }
                    Bank::Bool => map3(
                        sel,
                        self.bool_dst(dst),
                        condition,
                        self.boolean(yes),
                        self.boolean(no),
                        choose,
                    ),
                    Bank::Color => {
                        map3(
                            sel,
                            self.color_dst(dst),
                            condition,
                            self.color(yes),
                            self.color(no),
                            choose,
                        );
                    }
                    Bank::Resource => map3(
                        sel,
                        self.handle_dst(dst),
                        condition,
                        self.handle(yes),
                        self.handle(no),
                        choose,
                    ),
                }
            }
            I::Move { bank, dst, src } => self.assign(bank, dst, src, sel),
            I::SectionCount { dst, width } | I::SectionIndex { dst, width } => {
                let index = matches!(instruction, I::SectionIndex { .. });
                let (dst, width) = (self.int_dst(dst), self.int(width));
                let run = Cell::new(None);
                pick_each(sel, dst, |i| {
                    self.section_query(width.at(i), index, i, &run)
                });
            }
            I::SectionPosition {
                dst,
                width,
                inverse,
            } => {
                let (width, inverse) = (self.float(width), self.float(inverse));
                pick_each(sel, self.float_dst(dst), |i| {
                    self.section_position(width.at(i), inverse.at(i), i)
                });
            }
            I::Sample { .. } => self.sample(instruction, sel, signals),
            I::Branch { .. } | I::Reduce { .. } | I::Stencil { .. } | I::Scan { .. } => {
                unreachable!("dispatched by block")
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
                    Some((_, _, item)) if item.kind == image::VALUE_FLOAT => {
                        f32::from_bits(item.word)
                    }
                    Some((_, _, item)) if item.kind == image::VALUE_INT => item.word as i32 as f32,
                    _ => default.at(i),
                });
            }
            Bank::Int => {
                let default = self.int(default);
                pick_each(sel, self.int_dst(dst), |i| {
                    self.int_item(item(i).map(|(_, _, value)| value), default.at(i))
                });
            }
            Bank::Bool => {
                let default = self.boolean(default);
                pick_each(sel, self.bool_dst(dst), |i| match item(i) {
                    Some((_, _, item)) if item.kind == image::VALUE_BOOL => item.word != 0,
                    _ => default.at(i),
                });
            }
            Bank::Color => {
                let default = self.color(default);
                pick_each(sel, self.color_dst(dst), |i| match item(i) {
                    Some((_, _, item)) if item.kind == image::VALUE_COLOR => {
                        word_color(item.word).into()
                    }
                    _ => default.at(i),
                });
            }
            Bank::Resource => {
                let default = self.handle(default);
                pick_each(sel, self.handle_dst(dst), |i| match item(i) {
                    Some((array, at, _)) => match array.kind() {
                        HandleKind::Param(Resource::Array, bank) => {
                            Handle::new(HandleKind::ParamItem(bank, at as u16))
                        }
                        HandleKind::Constant(Resource::Array, array) => {
                            Handle::new(HandleKind::ConstantItem(array, at as u16))
                        }
                        _ => default.at(i),
                    },
                    _ => default.at(i),
                });
            }
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample(&self, instruction: &Instruction, sel: Sel<'_>, signals: &mut dyn StripSignals) {
        let Instruction::Sample {
            dst,
            input,
            seconds,
            pixel,
            frame_cache,
        } = *instruction
        else {
            unreachable!("dispatched by step")
        };
        let input = usize::from(input);
        let frame_cache = (frame_cache != NO_FRAME_CACHE).then_some(usize::from(frame_cache));
        let Dst::Many(dst) = self.color_dst(dst) else {
            unreachable!("admission keeps samples in rows")
        };
        let seconds = self.float(seconds);
        if seconds.is_scalar() && matches!(pixel, SignalPixel::Current) {
            let Some(time) = self.query_time(seconds.at(0)) else {
                each(sel, |i| dst[i].set(NO_COLOR));
                return;
            };
            let mut samples = self.ws.samples.borrow_mut();
            signals.sample_strip(input, time, frame_cache, &mut samples);
            each(sel, |i| dst[i].set(samples[i].into()));
            return;
        }
        let address = pixel.map(|index| self.int(index));
        each(sel, |i| {
            let color = match self.query_time(seconds.at(i)) {
                Some(time) => signals
                    .sample_pixel(
                        input,
                        time,
                        i,
                        address.map(|index| index.at(i)),
                        frame_cache,
                    )
                    .into(),
                None => NO_COLOR,
            };
            dst[i].set(color);
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
        SlotKind::Input(_) => unreachable!("admission keeps pixel inputs in their banks"),
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn destination<'s, T: Copy>(store: &'s Store<'_, T>, slot: Slot) -> Dst<'s, T> {
    match slot.kind() {
        SlotKind::Scalar(index) => Dst::One(store.scalar(index)),
        SlotKind::Row(index) => Dst::Many(store.row(index)),
        SlotKind::Input(_) => unreachable!("admission never writes pixel inputs"),
    }
}
