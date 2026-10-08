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
use super::context::Clock;
use super::parameters::{CurveParameter, ParameterValues};
use super::{BoundParams, RunContext};
use crate::dsl::bytecode::{
    Bank, Banks, BytecodeProgram, ColorBinary, ColorComponent, CompareOp, ContextRead, FloatBinary,
    Input, Instruction, IntBinary, MAX_ITERATIONS, MarkOp, NO_FRAME_CACHE, Reducer, Resource,
    SignalPixel, Slot, SlotKind,
};
use crate::sampling::{
    add_colors, clamp_array_index, clamp_float, color_channel, color_hue, color_intensity,
    color_saturation, float_binary, float_unary, gradient_color_scaled, hsv, int_binary,
    invert_color, length_int, mark_at, max_colors, mix_colors, multiply_colors, previous_mark,
    previous_mark_index, query_progress, query_seconds, rgb, sample_curve, sample_gradient,
    scale_color, section_position,
};
use crate::sections::{PreparedSections, SectionContext, SectionPixel};
use alloc::vec::Vec;
use core::cell::{Cell, RefCell};
use donder_runtime_types::Value;
use donder_runtime_types::{Color, Curve, Gradient, Marks, SampleTime};

pub(crate) use crate::dsl::bytecode::STRIP;
const MASK: usize = STRIP - 1;

type Row<T> = [Cell<T>; STRIP];

fn row<T: Copy>(value: T) -> Row<T> {
    core::array::from_fn(|_| Cell::new(value))
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
}

/// Effects never query signals; admission rejects the instruction.
impl StripSignals for super::NoSignals {
    fn sample_strip(&mut self, _: usize, _: SampleTime, _: Option<usize>, _: &mut [Color; STRIP]) {
        unreachable!("effect admission excludes signal instructions")
    }

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
}

/// Per-pixel inputs. The traversal fills them for each strip.
pub(crate) struct Pixels {
    pub(crate) index: Row<i32>,
    pub(crate) fraction: Row<f32>,
    pub(crate) x: Row<f32>,
    pub(crate) y: Row<f32>,
    pub(crate) sections: [SectionPixel; STRIP],
}

impl Default for Pixels {
    fn default() -> Self {
        Self {
            index: row(0),
            fraction: row(0.0),
            x: row(0.0),
            y: row(0.0),
            sections: [SectionPixel::default(); STRIP],
        }
    }
}

/// A resource: a parameter or constant of the program, or an item of an
/// array parameter or constant.
#[derive(Clone, Copy, Debug, Default)]
enum Handle {
    #[default]
    Empty,
    Param(Resource, u16),
    Constant(Resource, u16),
    ParamItem(u16, u16),
    ConstantItem(u16, u16),
}

const NO_COLOR: Color = Color::BLACK;

/// One bank's slots: its scalars, then its rows, in one array. A bank without
/// rows keeps a row of padding, so every scalar has a row-sized window.
struct Store<T> {
    values: Vec<Cell<T>>,
    scalars: usize,
}

impl<T: Copy> Store<T> {
    fn new() -> Self {
        Self {
            values: Vec::new(),
            scalars: 0,
        }
    }

    /// Fit `scalars` scalars and `rows` rows.
    fn reserve(&mut self, scalars: u16, rows: u16, fill: T) {
        let rows_now = self.values.len().saturating_sub(self.scalars) / STRIP;
        self.scalars = self.scalars.max(usize::from(scalars));
        let rows = rows_now.max(usize::from(rows)).max(1);
        let len = self.scalars + rows * STRIP;
        if self.values.len() < len {
            self.values.resize_with(len, || Cell::new(fill));
        }
    }

    fn bytes(scalars: u16, rows: u16) -> Option<usize> {
        let rows = usize::from(rows.max(1)).checked_mul(STRIP)?;
        usize::from(scalars)
            .checked_add(rows)?
            .checked_mul(size_of::<T>())
    }

    fn scalar(&self, index: u16) -> &Cell<T> {
        &self.values[usize::from(index)]
    }

    fn window(&self, start: usize) -> &Row<T> {
        let Ok(window) = <&Row<T>>::try_from(&self.values[start..start + STRIP]) else {
            unreachable!("a window spans one strip")
        };
        window
    }

    fn row(&self, index: u16) -> &Row<T> {
        self.window(self.scalars + usize::from(index) * STRIP)
    }

    /// A scalar operand: the scalar masks every pixel to itself.
    fn one(&self, index: u16) -> Src<'_, T> {
        Src {
            row: self.window(usize::from(index)),
            mask: 0,
        }
    }

    fn many(&self, index: u16) -> Src<'_, T> {
        Src {
            row: self.row(index),
            mask: MASK,
        }
    }
}

/// Slots of every program a workspace runs, reserved before playback.
pub(crate) struct StripWorkspace {
    pixels: Pixels,
    floats: Store<f32>,
    ints: Store<i32>,
    bools: Store<bool>,
    colors: Store<Color>,
    handles: Store<Handle>,
    levels: Vec<Level>,
    /// Signal samples land here before they are written to the selection.
    samples: RefCell<[Color; STRIP]>,
}

impl Default for StripWorkspace {
    fn default() -> Self {
        Self {
            pixels: Pixels::default(),
            floats: Store::new(),
            ints: Store::new(),
            bools: Store::new(),
            colors: Store::new(),
            handles: Store::new(),
            levels: Vec::new(),
            samples: RefCell::new([NO_COLOR; STRIP]),
        }
    }
}

impl core::fmt::Debug for StripWorkspace {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("StripWorkspace")
            .field("floats", &self.floats.values.len())
            .field("ints", &self.ints.values.len())
            .field("bools", &self.bools.values.len())
            .field("colors", &self.colors.values.len())
            .field("handles", &self.handles.values.len())
            .finish_non_exhaustive()
    }
}

impl StripWorkspace {
    /// Grow every bank to fit `program`.
    pub(crate) fn reserve(&mut self, program: &BytecodeProgram) {
        let (scalars, rows) = (program.scalars, program.rows);
        self.floats.reserve(scalars.floats, rows.floats, 0.0);
        self.ints.reserve(scalars.ints, rows.ints, 0);
        self.bools.reserve(scalars.bools, rows.bools, false);
        self.colors.reserve(scalars.colors, rows.colors, NO_COLOR);
        self.handles
            .reserve(scalars.resources, rows.resources, Handle::Empty);
        while self.levels.len() < usize::from(program.depth) {
            self.levels.push(Level::default());
        }
    }

    /// Bytes a workspace reserves for programs with these slot counts.
    pub(crate) fn storage_estimate(scalars: Banks, rows: Banks, depth: u16) -> Option<usize> {
        let mut bytes = size_of::<Self>().checked_add(usize::from(depth) * size_of::<Level>())?;
        for bytes_of_bank in [
            Store::<f32>::bytes(scalars.floats, rows.floats)?,
            Store::<i32>::bytes(scalars.ints, rows.ints)?,
            Store::<bool>::bytes(scalars.bools, rows.bools)?,
            Store::<Color>::bytes(scalars.colors, rows.colors)?,
            Store::<Handle>::bytes(scalars.resources, rows.resources)?,
        ] {
            bytes = bytes.checked_add(bytes_of_bank)?;
        }
        Some(bytes)
    }
}

/// A run of pixels, `start..end`.
type Range = (u8, u8);

/// The pixels an instruction runs for: ascending, disjoint, nonempty ranges.
#[derive(Clone, Copy)]
struct Sel<'s>(&'s [Cell<Range>]);

/// Selections open in one construct: a branch partitions into `first`; a
/// reduction keeps its participating pixels in `first`, its contributing ones
/// in `second`, and which pixels still run in `running`.
struct Level {
    first: [Cell<Range>; STRIP],
    second: [Cell<Range>; STRIP],
    running: Row<bool>,
}

impl Default for Level {
    fn default() -> Self {
        Self {
            first: row((0, 0)),
            second: row((0, 0)),
            running: row(false),
        }
    }
}

#[inline(always)]
fn each(sel: Sel<'_>, mut visit: impl FnMut(usize)) {
    for range in sel.0 {
        let (start, end) = range.get();
        for index in usize::from(start)..usize::from(end).min(STRIP) {
            visit(index);
        }
    }
}

/// The runs of the selected pixels where `keep` holds, written into `buffer`;
/// returns how many.
fn filter(sel: Sel<'_>, buffer: &[Cell<Range>], keep: impl Fn(usize) -> bool) -> usize {
    let mut count = 0;
    let mut push = |start: usize, end: usize| {
        if let Some(slot) = buffer.get(count) {
            slot.set((start as u8, end as u8));
            count += 1;
        }
    };
    for range in sel.0 {
        let (start, end) = range.get();
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
    fn at(self, index: usize) -> T {
        self.row[index & self.mask & MASK].get()
    }

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
fn apply1<A: Copy, D: Copy>(sel: Sel<'_>, dst: Dst<'_, D>, a: Src<'_, A>, f: impl Fn(A) -> D) {
    match dst {
        Dst::One(dst) => dst.set(f(a.at(0))),
        Dst::Many(dst) => each(sel, |i| dst[i].set(f(a.at(i)))),
    }
}

#[inline(always)]
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
fn unary(op: crate::dsl::bytecode::FloatUnary, value: f32) -> f32 {
    float_unary(op, value)
}

#[inline(never)]
fn binary(op: FloatBinary, a: f32, b: f32) -> f32 {
    float_binary(op, a, b)
}

#[inline(never)]
fn int(op: IntBinary, a: i32, b: i32) -> i32 {
    int_binary(op, a, b)
}

#[inline(never)]
fn component(op: ColorComponent, color: Color) -> f32 {
    match op {
        ColorComponent::Hue => color_hue(color),
        ColorComponent::Red => color_channel(color.red),
        ColorComponent::Green => color_channel(color.green),
        ColorComponent::Blue => color_channel(color.blue),
        ColorComponent::Saturation => color_saturation(color),
        ColorComponent::Intensity => color_intensity(color),
    }
}

#[inline(never)]
fn mix_color(a: Color, b: Color, amount: f32) -> Color {
    mix_colors(a, b, amount)
}

#[inline(never)]
fn rgb_color(red: f32, green: f32, blue: f32) -> Color {
    rgb(red, green, blue)
}

#[inline(never)]
fn hsv_color(hue: f32, saturation: f32, value: f32) -> Color {
    hsv(hue, saturation, value)
}

#[inline(never)]
fn recolor(color: Color, hue: f32, shift: bool) -> Color {
    crate::sampling::recolor(color, hue, shift)
}

#[inline(never)]
fn power(base: f32, count: i32) -> f32 {
    let mut power = 1.0;
    for _ in 0..count.clamp(0, MAX_ITERATIONS) {
        power *= base;
    }
    power
}

/// A curve operand: a parameter keeps its prepared crossings.
#[derive(Clone, Copy)]
enum CurveRef<'r> {
    Parameter(&'r CurveParameter),
    Raw(&'r Curve),
}

impl<'r> CurveRef<'r> {
    fn curve(self) -> &'r Curve {
        match self {
            Self::Parameter(parameter) => parameter.raw(),
            Self::Raw(curve) => curve,
        }
    }

    fn sample(self, position: f32) -> f32 {
        sample_curve(self.curve(), position)
    }

    /// The first position where the curve reaches `value`, or NaN.
    fn crossing(self, value: f32) -> f32 {
        match self {
            Self::Parameter(parameter) => parameter.crossing(value, f32::NAN),
            Self::Raw(curve) => crate::sampling::curve_crossing(curve, value, f32::NAN),
        }
    }
}

static EMPTY_CURVE: Curve = Curve { points: Vec::new() };
static EMPTY_GRADIENT: Gradient = Gradient { stops: Vec::new() };
static EMPTY_MARKS: Marks = Marks::EMPTY;

pub(crate) struct Strip<'a> {
    program: &'a BytecodeProgram,
    params: &'a ParameterValues,
    context: RunContext,
    clock: Clock,
    sections: Option<&'a PreparedSections>,
    workspace: &'a mut StripWorkspace,
    /// Pixel count and bounds of the initialized target block.
    shape: Option<(i32, [u32; 4])>,
    min: [f32; 2],
    max: [f32; 2],
    /// The latest sample seconds and their time.
    query: Cell<Option<(u32, Option<SampleTime>)>>,
}

impl<'a> Strip<'a> {
    pub(crate) fn new(
        program: &'a BytecodeProgram,
        params: &'a BoundParams,
        context: &RunContext,
        sections: Option<&'a PreparedSections>,
        workspace: &'a mut StripWorkspace,
    ) -> Self {
        Self {
            program,
            params: &params.values,
            context: *context,
            clock: Clock::new(context),
            sections,
            workspace,
            shape: None,
            min: [0.0; 2],
            max: [0.0; 2],
            query: Cell::new(None),
        }
    }

    pub(crate) fn pixels(&mut self) -> &mut Pixels {
        &mut self.workspace.pixels
    }

    /// Evaluate the first `output.len()` pixels. They share a pixel count and
    /// target bounds.
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
            program: self.program,
            params: self.params,
            context: &self.context,
            clock: self.clock,
            sections: self.sections,
            min: self.min,
            max: self.max,
            ws: self.workspace,
            query: &self.query,
        };
        let (query, target) = self.program.prefix();
        let levels = &machine.ws.levels[..];
        let one = [Cell::new((0, 1))];
        match initialize {
            None => {
                machine.block(query, Sel(&one), levels, signals);
                machine.block(target, Sel(&one), levels, signals);
            }
            Some(true) => machine.block(target, Sel(&one), levels, signals),
            Some(false) => {}
        }
        self.shape = Some(shape);
        let strip = [Cell::new((0, len as u8))];
        machine.block(self.program.body(), Sel(&strip), levels, signals);
        let result = machine.color(self.program.result);
        for (index, output) in output.iter_mut().enumerate() {
            *output = result.at(index);
        }
    }
}

struct Machine<'m> {
    program: &'m BytecodeProgram,
    params: &'m ParameterValues,
    context: &'m RunContext,
    clock: Clock,
    sections: Option<&'m PreparedSections>,
    min: [f32; 2],
    max: [f32; 2],
    ws: &'m StripWorkspace,
    query: &'m Cell<Option<(u32, Option<SampleTime>)>>,
}

impl<'m> Machine<'m> {
    // Operand lookups run once per instruction, so they stay out of line: one
    // bounds check each rather than one per instruction arm.
    #[inline(never)]
    fn float(&self, slot: Slot) -> Src<'m, f32> {
        let ws = self.ws;
        let input = |row| Src { row, mask: MASK };
        match slot.kind() {
            SlotKind::Scalar(index) => ws.floats.one(index),
            SlotKind::Row(index) => ws.floats.many(index),
            SlotKind::Input(Input::PixelX) => input(&ws.pixels.x),
            SlotKind::Input(Input::PixelY) => input(&ws.pixels.y),
            SlotKind::Input(_) => input(&ws.pixels.fraction),
        }
    }

    #[inline(never)]
    fn int(&self, slot: Slot) -> Src<'m, i32> {
        let ws = self.ws;
        match slot.kind() {
            SlotKind::Scalar(index) => ws.ints.one(index),
            SlotKind::Row(index) => ws.ints.many(index),
            SlotKind::Input(_) => Src {
                row: &ws.pixels.index,
                mask: MASK,
            },
        }
    }

    #[inline(never)]
    fn boolean(&self, slot: Slot) -> Src<'m, bool> {
        operand(&self.ws.bools, slot)
    }

    #[inline(never)]
    fn color(&self, slot: Slot) -> Src<'m, Color> {
        operand(&self.ws.colors, slot)
    }

    #[inline(never)]
    fn handle(&self, slot: Slot) -> Src<'m, Handle> {
        operand(&self.ws.handles, slot)
    }

    #[inline(never)]
    fn float_dst(&self, slot: Slot) -> Dst<'m, f32> {
        destination(&self.ws.floats, slot)
    }

    #[inline(never)]
    fn int_dst(&self, slot: Slot) -> Dst<'m, i32> {
        destination(&self.ws.ints, slot)
    }

    #[inline(never)]
    fn bool_dst(&self, slot: Slot) -> Dst<'m, bool> {
        destination(&self.ws.bools, slot)
    }

    #[inline(never)]
    fn color_dst(&self, slot: Slot) -> Dst<'m, Color> {
        destination(&self.ws.colors, slot)
    }

    #[inline(never)]
    fn handle_dst(&self, slot: Slot) -> Dst<'m, Handle> {
        destination(&self.ws.handles, slot)
    }

    fn set_scalar<T: Copy>(dst: Dst<'_, T>, value: T) {
        if let Dst::One(dst) = dst {
            dst.set(value);
        }
    }

    fn block(
        &self,
        code: &[Instruction],
        sel: Sel<'_>,
        levels: &[Level],
        signals: &mut dyn StripSignals,
    ) {
        let mut at = 0;
        while at < code.len() {
            let instruction = &code[at];
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
                _ => self.step(instruction, sel, signals),
            }
        }
    }

    fn branch(
        &self,
        condition: Slot,
        then_code: &[Instruction],
        else_code: &[Instruction],
        sel: Sel<'_>,
        levels: &[Level],
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
        let (level, inner) = levels.split_at(1);
        let buffer = &level[0].first;
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

    fn reduce(
        &self,
        instruction: &Instruction,
        loop_code: &[Instruction],
        contribute: &[Instruction],
        sel: Sel<'_>,
        levels: &[Level],
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
        let (level, inner) = levels.split_at(1);
        let level = &level[0];
        let uniform_bounds = start.is_scalar() && end.is_scalar();
        // Early exits track the pixels still running; per-pixel bounds run
        // each pixel's own count.
        let mut running = 0;
        if early {
            each(sel, |i| {
                level.running[i].set(true);
                running += 1;
            });
        }
        let mut iterations = 0;
        each(sel, |i| iterations = iterations.max(count(i)));
        let index_dst = self.int_dst(index);
        for k in 0..iterations {
            let participating = if uniform_bounds && !early {
                sel
            } else {
                let kept = filter(sel, &level.first, |i| {
                    (!early || level.running[i].get()) && k < count(i)
                });
                Sel(&level.first[..kept])
            };
            if participating.0.is_empty() {
                continue;
            }
            match index_dst {
                Dst::One(index) => index.set(index_at(0, k)),
                Dst::Many(index) => each(participating, |i| index[i].set(index_at(i, k))),
            }
            self.block(loop_code, participating, inner, signals);
            let contributing = if filter_slot.is_none() {
                participating
            } else {
                let condition = self.boolean(filter_slot);
                let kept = filter(participating, &level.second, |i| condition.at(i));
                Sel(&level.second[..kept])
            };
            if contributing.0.is_empty() {
                continue;
            }
            self.block(contribute, contributing, inner, signals);
            if !early {
                self.combine(reducer, bank, acc, value, contributing);
                continue;
            }
            // The participating ranges are done with, so decisions reuse them.
            let decided = filter(contributing, &level.first, |i| {
                self.decides(reducer, value, i)
            });
            let decided = Sel(&level.first[..decided]);
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

    /// Whether `value` at pixel `i` ends an early-exit reduction there.
    fn decides(&self, reducer: Reducer, value: Slot, i: usize) -> bool {
        match reducer {
            Reducer::Any => self.boolean(value).at(i),
            Reducer::All => !self.boolean(value).at(i),
            _ => true,
        }
    }

    /// `acc = acc ⊕ value` for `max`, `min` and `sum`.
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
                match reducer {
                    Reducer::Max => map2(sel, dst, a, b, |a, b| if b > a { b } else { a }),
                    Reducer::Min => map2(sel, dst, a, b, |a, b| if b < a { b } else { a }),
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
            Bank::Bool | Bank::Resource => self.assign(bank, acc, value, sel),
        }
    }

    fn assign(&self, bank: Bank, dst: Slot, src: Slot, sel: Sel<'_>) {
        match bank {
            Bank::Float => map1(sel, self.float_dst(dst), self.float(src), |v| v),
            Bank::Int => map1(sel, self.int_dst(dst), self.int(src), |v| v),
            Bank::Bool => map1(sel, self.bool_dst(dst), self.boolean(src), |v| v),
            Bank::Color => map1(sel, self.color_dst(dst), self.color(src), |v| v),
            Bank::Resource => map1(sel, self.handle_dst(dst), self.handle(src), |v| v),
        }
    }

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

    fn curve(&self, handle: Handle) -> CurveRef<'m> {
        match handle {
            Handle::Param(Resource::Curve, bank) => {
                CurveRef::Parameter(&self.params.curves[usize::from(bank)])
            }
            Handle::Constant(Resource::Curve, index) => {
                CurveRef::Raw(&self.program.curves[usize::from(index)])
            }
            _ => match self.item(handle) {
                Some(Value::Curve(curve)) => CurveRef::Raw(curve),
                _ => CurveRef::Raw(&EMPTY_CURVE),
            },
        }
    }

    fn gradient(&self, handle: Handle) -> &'m Gradient {
        match handle {
            Handle::Param(Resource::Gradient, bank) => {
                self.params.gradients[usize::from(bank)].get()
            }
            Handle::Constant(Resource::Gradient, index) => {
                &self.program.gradients[usize::from(index)]
            }
            _ => match self.item(handle) {
                Some(Value::Gradient(gradient)) => gradient,
                _ => &EMPTY_GRADIENT,
            },
        }
    }

    fn marks(&self, handle: Handle) -> &'m Marks {
        match handle {
            Handle::Param(Resource::Marks, bank) => self.params.marks[usize::from(bank)].get(),
            Handle::Constant(Resource::Marks, index) => &self.program.marks[usize::from(index)],
            _ => match self.item(handle) {
                Some(Value::Marks(marks)) => marks,
                _ => &EMPTY_MARKS,
            },
        }
    }

    fn array(&self, handle: Handle) -> &'m [Value] {
        match handle {
            Handle::Param(Resource::Array, bank) => {
                self.params.array_values[usize::from(bank)].values()
            }
            Handle::Constant(Resource::Array, index) => &self.program.arrays[usize::from(index)],
            _ => &[],
        }
    }

    /// The item an item handle names; `Index` clamped it to its array.
    fn item(&self, handle: Handle) -> Option<&'m Value> {
        match handle {
            Handle::ParamItem(bank, index) => {
                Some(&self.params.array_values[usize::from(bank)].values()[usize::from(index)])
            }
            Handle::ConstantItem(array, index) => {
                Some(&self.program.arrays[usize::from(array)][usize::from(index)])
            }
            _ => None,
        }
    }

    #[inline(never)]
    fn curve_sample(&self, curve: Handle, position: f32) -> f32 {
        self.curve(curve).sample(position)
    }

    #[inline(never)]
    fn curve_integral(&self, curve: Handle, position: f32) -> f32 {
        crate::sampling::curve_integral(self.curve(curve).curve(), position)
    }

    #[inline(never)]
    fn curve_crossing(&self, curve: Handle, value: f32) -> f32 {
        self.curve(curve).crossing(value)
    }

    #[inline(never)]
    fn curve_last_crossing(&self, curve: Handle, value: f32, before: f32) -> f32 {
        crate::sampling::curve_last_crossing(self.curve(curve).curve(), value, before)
    }

    #[inline(never)]
    fn gradient_sample(&self, gradient: Handle, position: f32) -> Color {
        sample_gradient(self.gradient(gradient), position)
    }

    #[inline(never)]
    fn gradient_scaled(&self, gradient: Handle, position: f32, scale: f32) -> Color {
        gradient_color_scaled(self.gradient(gradient), position, scale)
    }

    #[inline(never)]
    fn previous_mark(&self, marks: Handle, seconds: f32) -> f32 {
        previous_mark(self.marks(marks), seconds).map_or(f32::NAN, |(_, time)| time)
    }

    /// An enum option's index in the program's names.
    fn enum_index(&self, name: &donder_runtime_types::Identifier) -> i32 {
        self.program
            .enums
            .iter()
            .position(|option| option == name)
            .map_or(-1, |index| index as i32)
    }

    fn step(&self, instruction: &Instruction, sel: Sel<'_>, signals: &mut dyn StripSignals) {
        use Instruction as I;
        let params = self.params;
        match *instruction {
            I::FloatConst { dst, bits } => {
                Self::set_scalar(self.float_dst(dst), f32::from_bits(bits))
            }
            I::IntConst { dst, value } => Self::set_scalar(self.int_dst(dst), value),
            I::BoolConst { dst, value } => Self::set_scalar(self.bool_dst(dst), value),
            I::ColorConst { dst, value } => Self::set_scalar(self.color_dst(dst), value),
            I::ResourceConst { dst, kind, index } => {
                Self::set_scalar(self.handle_dst(dst), Handle::Constant(kind, index));
            }
            I::FloatParam { dst, bank } => {
                Self::set_scalar(self.float_dst(dst), params.floats[usize::from(bank)]);
            }
            I::IntParam { dst, bank } => {
                Self::set_scalar(self.int_dst(dst), params.ints[usize::from(bank)]);
            }
            I::BoolParam { dst, bank } => {
                Self::set_scalar(self.bool_dst(dst), params.bools[usize::from(bank)]);
            }
            I::ColorParam { dst, bank } => {
                Self::set_scalar(self.color_dst(dst), params.colors[usize::from(bank)]);
            }
            I::EnumParam { dst, bank } => Self::set_scalar(
                self.int_dst(dst),
                self.enum_index(&params.enums[usize::from(bank)]),
            ),
            I::ResourceParam { dst, kind, bank } => {
                Self::set_scalar(self.handle_dst(dst), Handle::Param(kind, bank));
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
                            (a == b) != negate
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
                pick_each(sel, dst, |i| {
                    let value = self.curve_sample(curve.at(i), position.at(i));
                    clamp_float(value, min.at(i), max.at(i))
                });
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
                match op {
                    MarkOp::Count => apply1(sel, self.int_dst(dst), marks, |marks| {
                        length_int(self.marks(marks).as_slice().len())
                    }),
                    MarkOp::At => apply2(
                        sel,
                        self.float_dst(dst),
                        marks,
                        self.int(operand),
                        |marks, index| mark_at(self.marks(marks), index),
                    ),
                    MarkOp::Last => apply2(
                        sel,
                        self.float_dst(dst),
                        marks,
                        self.float(operand),
                        |marks, seconds| self.previous_mark(marks, seconds),
                    ),
                    MarkOp::LastIndex => apply2(
                        sel,
                        self.int_dst(dst),
                        marks,
                        self.float(operand),
                        |marks, seconds| previous_mark_index(self.marks(marks), seconds),
                    ),
                }
            }
            I::Len { dst, array } => apply1(sel, self.int_dst(dst), self.handle(array), |array| {
                length_int(self.array(array).len())
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
                let items = &self.program.operands[items.range()];
                let index = self.int(index);
                let pick = |i: usize| items[clamp_array_index(index.at(i), items.len())];
                match bank {
                    Bank::Float => {
                        pick_each(sel, self.float_dst(dst), |i| self.float(pick(i)).at(i))
                    }
                    Bank::Int => pick_each(sel, self.int_dst(dst), |i| self.int(pick(i)).at(i)),
                    Bank::Bool => {
                        pick_each(sel, self.bool_dst(dst), |i| self.boolean(pick(i)).at(i))
                    }
                    Bank::Color => {
                        pick_each(sel, self.color_dst(dst), |i| self.color(pick(i)).at(i))
                    }
                    Bank::Resource => {
                        pick_each(sel, self.handle_dst(dst), |i| self.handle(pick(i)).at(i));
                    }
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
                let pixels = &self.ws.pixels;
                pick_each(sel, dst, |i| {
                    let sections = match self.sections {
                        Some(target) => SectionContext::Prepared {
                            target,
                            pixel: pixels.sections[i & MASK],
                        },
                        None => SectionContext::Single {
                            index: pixels.index[i & MASK].get(),
                            count: self.context.pixel_count,
                        },
                    };
                    sections.query(width.at(i), index)
                });
            }
            I::SectionPosition {
                dst,
                width,
                inverse,
            } => {
                let (width, inverse) = (self.float(width), self.float(inverse));
                let pixels = &self.ws.pixels.index;
                pick_each(sel, self.float_dst(dst), |i| {
                    section_position(pixels[i & MASK].get(), width.at(i), inverse.at(i))
                });
            }
            I::Sample { .. } => self.sample(instruction, sel, signals),
            I::Branch { .. } | I::Reduce { .. } => unreachable!("dispatched by block"),
        }
    }

    /// Array items. Indices clamp; an empty array reads the default.
    fn index(&self, bank: Bank, dst: Slot, array: Slot, index: Slot, default: Slot, sel: Sel<'_>) {
        let (array, index) = (self.handle(array), self.int(index));
        let item = |i: usize| {
            let handle = array.at(i);
            let items = self.array(handle);
            (!items.is_empty()).then(|| {
                let at = clamp_array_index(index.at(i), items.len());
                (handle, at, &items[at])
            })
        };
        match bank {
            Bank::Float => {
                let default = self.float(default);
                pick_each(sel, self.float_dst(dst), |i| match item(i) {
                    Some((_, _, Value::Float(value))) => *value,
                    Some((_, _, Value::Int(value))) => *value as f32,
                    _ => default.at(i),
                });
            }
            Bank::Int => {
                let default = self.int(default);
                pick_each(sel, self.int_dst(dst), |i| match item(i) {
                    Some((_, _, Value::Int(value))) => *value,
                    Some((_, _, Value::Enum(name))) => self.enum_index(name),
                    _ => default.at(i),
                });
            }
            Bank::Bool => {
                let default = self.boolean(default);
                pick_each(sel, self.bool_dst(dst), |i| match item(i) {
                    Some((_, _, Value::Bool(value))) => *value,
                    _ => default.at(i),
                });
            }
            Bank::Color => {
                let default = self.color(default);
                pick_each(sel, self.color_dst(dst), |i| match item(i) {
                    Some((_, _, Value::Color(value))) => *value,
                    _ => default.at(i),
                });
            }
            Bank::Resource => {
                let default = self.handle(default);
                pick_each(sel, self.handle_dst(dst), |i| match item(i) {
                    Some((Handle::Param(Resource::Array, bank), at, _)) => {
                        Handle::ParamItem(bank, at as u16)
                    }
                    Some((Handle::Constant(Resource::Array, array), at, _)) => {
                        Handle::ConstantItem(array, at as u16)
                    }
                    _ => default.at(i),
                });
            }
        }
    }

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
            each(sel, |i| dst[i].set(samples[i]));
            return;
        }
        let address = pixel.map(|index| self.int(index));
        each(sel, |i| {
            let color = match self.query_time(seconds.at(i)) {
                Some(time) => signals.sample_pixel(
                    input,
                    time,
                    i,
                    address.map(|index| index.at(i)),
                    frame_cache,
                ),
                None => NO_COLOR,
            };
            dst[i].set(color);
        });
    }
}

fn choose<T>(condition: bool, yes: T, no: T) -> T {
    if condition { yes } else { no }
}

/// Write `value(i)` at the selected pixels, or once for a scalar destination.
#[inline(always)]
fn pick_each<T: Copy>(sel: Sel<'_>, dst: Dst<'_, T>, value: impl Fn(usize) -> T) {
    match dst {
        Dst::One(dst) => dst.set(value(0)),
        Dst::Many(dst) => each(sel, |i| dst[i].set(value(i))),
    }
}

/// An operand of a bank without pixel inputs.
fn operand<T: Copy>(store: &Store<T>, slot: Slot) -> Src<'_, T> {
    match slot.kind() {
        SlotKind::Scalar(index) => store.one(index),
        SlotKind::Row(index) => store.many(index),
        SlotKind::Input(_) => unreachable!("admission keeps pixel inputs in their banks"),
    }
}

fn destination<T: Copy>(store: &Store<T>, slot: Slot) -> Dst<'_, T> {
    match slot.kind() {
        SlotKind::Scalar(index) => Dst::One(store.scalar(index)),
        SlotKind::Row(index) => Dst::Many(store.row(index)),
        SlotKind::Input(_) => unreachable!("admission never writes pixel inputs"),
    }
}
