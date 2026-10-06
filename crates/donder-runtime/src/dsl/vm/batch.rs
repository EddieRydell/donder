//! Batched execution, the only interpreter. One instruction dispatch drives up
//! to [`LANES`] pixels that share a program, time and target stage; a single
//! sample is a one-lane run. Every lane follows its own control flow: a
//! divergent branch parks one side, and the lowest parked instruction always
//! runs next, so lanes reconverge at joins and loop exits.
use super::{
    ArrayStorage, BoundParams, Clock, CurveRegister, GradientRegister, MarksRegister, ReadContext,
    RunContext, RuntimeValue, clamp_array_index, clamp_float, clone_runtime, color_hue,
    color_intensity, color_saturation, curve_crossing_raw, float_binary, float_unary,
    gradient_color_scaled, int_len, invert_color, mark_at_from, prev_index, previous_mark,
    query_progress, query_seconds, runtime_refs_equal, sample_curve, sample_gradient, scale_color,
    section_position, smoothstep,
};
use super::{
    BytecodeProgram, ColorBinary, ColorComponent, ColorSlot, CompareOp, ContextRead, FloatBinary,
    FloatUnary, Instruction, MarkOp, NumberSlot, SignalPixel, ValueSlot,
};
use crate::sampling::{
    add_colors, float_remainder, int_remainder, max_colors, mix_colors, multiply_colors,
};
use crate::sections::{PreparedSections, SectionContext, SectionPixel};
use crate::values::{Color, Curve, Gradient, Marks, SampleTime};
use alloc::vec::Vec;
use donder_language::Shared as Arc;
use donder_language::dsl::{BatchPlan, LaneOp, NO_REGISTER, SignalAccess, Value, lane_registers};

pub(crate) const LANES: usize = 32;
pub(crate) type Mask = u32;

pub(crate) type Program = BytecodeProgram<ContextRead, SignalAccess, ColorSlot>;

/// Signal queries of a running batch. Lane `n` of a run is the run's `n`th
/// pixel; the provider knows which pixels the run covers.
pub(crate) trait BatchSignals {
    /// `input` at `time` for the lanes in `mask`, each at its own pixel.
    fn sample_run(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        mask: Mask,
        output: &mut [Color; LANES],
    );

    /// `input` at `time` for one lane at an explicit address.
    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        lane: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color;
}

/// Effects never query signals; admission rejects the instruction.
impl BatchSignals for super::NoSignals {
    fn sample_run(
        &mut self,
        _: usize,
        _: SampleTime,
        _: Option<usize>,
        _: Mask,
        _: &mut [Color; LANES],
    ) {
        unreachable!("sample admission excludes signal instructions")
    }

    fn sample_pixel(
        &mut self,
        _: usize,
        _: SampleTime,
        _: usize,
        _: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Color {
        unreachable!("sample admission excludes signal instructions")
    }
}

/// Per-pixel inputs. The traversal fills the lanes of each run.
#[derive(Clone, Copy, Default)]
pub(crate) struct Lanes {
    pub pixel_index: [i32; LANES],
    pub pixel_fraction: [f32; LANES],
    pub x: [f32; LANES],
    pub y: [f32; LANES],
    pub sections: [SectionPixel; LANES],
}

#[derive(Default)]
struct Rows {
    ints: Vec<[i32; LANES]>,
    floats: Vec<[f32; LANES]>,
    bools: Vec<Mask>,
    colors: Vec<[Color; LANES]>,
    loops: Vec<[u32; LANES]>,
}

/// Reference registers. One written by a single load holds its value for every
/// lane (`shared`); the others hold a value per lane (`lanes`, by plan row).
/// Local arrays of every lane share one arena.
#[derive(Default)]
struct References {
    shared: Vec<RuntimeValue>,
    lanes: Vec<[RuntimeValue; LANES]>,
    storage: ArrayStorage,
}

impl References {
    fn put_shared(&mut self, index: usize, value: RuntimeValue) {
        let old = core::mem::replace(&mut self.shared[index], value);
        self.storage.release(old);
    }

    fn put(&mut self, row: usize, lane: usize, value: RuntimeValue) {
        self.storage.retain(&value);
        let old = core::mem::replace(&mut self.lanes[row][lane], value);
        self.storage.release(old);
    }

    /// Parameter resources must not stay shared between invocations. Only
    /// the program's own registers can hold values.
    fn clear(&mut self, shared: usize, rows: usize) {
        for value in &mut self.shared[..shared] {
            let old = core::mem::replace(value, RuntimeValue::Void);
            self.storage.release(old);
        }
        for row in &mut self.lanes[..rows] {
            for value in row {
                let old = core::mem::replace(value, RuntimeValue::Void);
                self.storage.release(old);
            }
        }
    }
}

/// Where a reference operand's lanes live.
#[derive(Clone, Copy)]
enum Source<'a> {
    Shared(&'a RuntimeValue),
    Lanes(&'a [RuntimeValue; LANES]),
}

impl<'a> Source<'a> {
    #[inline(always)]
    fn lane(self, lane: usize) -> &'a RuntimeValue {
        match self {
            Self::Shared(value) => value,
            Self::Lanes(row) => &row[lane],
        }
    }
}

// Empty registers read as empty resources, as in parameter binding.
static EMPTY_CURVE: Curve = Curve { points: Vec::new() };
static EMPTY_GRADIENT: Gradient = Gradient { stops: Vec::new() };
static EMPTY_MARKS: Marks = Marks::EMPTY;

fn curve_of(value: &RuntimeValue) -> &Curve {
    match value {
        RuntimeValue::Curve(curve) => curve,
        RuntimeValue::PreparedCurve(curve) => &curve.raw,
        RuntimeValue::Void => &EMPTY_CURVE,
        _ => unreachable!("typed registers hold curves"),
    }
}

fn gradient_of(value: &RuntimeValue) -> &Gradient {
    match value {
        RuntimeValue::Gradient(gradient) => gradient,
        RuntimeValue::Void => &EMPTY_GRADIENT,
        _ => unreachable!("typed registers hold gradients"),
    }
}

fn marks_of(value: &RuntimeValue) -> &Marks {
    match value {
        RuntimeValue::Marks(marks) => marks,
        RuntimeValue::Void => &EMPTY_MARKS,
        _ => unreachable!("typed registers hold marks"),
    }
}

fn array_of<'a>(
    value: &'a RuntimeValue,
    storage: &'a ArrayStorage,
) -> super::arrays::ArrayView<'a> {
    use super::arrays::ArrayView;
    match value {
        RuntimeValue::Array(values) => ArrayView::Shared(values),
        RuntimeValue::ArraySlot(index) => ArrayView::Local(storage.items(*index)),
        RuntimeValue::Void => ArrayView::Shared(&[]),
        _ => unreachable!("typed registers hold arrays"),
    }
}

fn curve_value(register: &CurveRegister) -> RuntimeValue {
    match register {
        CurveRegister::Empty => RuntimeValue::Void,
        CurveRegister::Raw(curve) => RuntimeValue::Curve(Arc::clone(curve)),
        CurveRegister::Prepared(curve) => RuntimeValue::PreparedCurve(Arc::clone(curve)),
    }
}

fn gradient_value(register: &GradientRegister) -> RuntimeValue {
    match register {
        GradientRegister::Empty => RuntimeValue::Void,
        GradientRegister::Shared(gradient) => RuntimeValue::Gradient(Arc::clone(gradient)),
    }
}

fn marks_value(register: &MarksRegister) -> RuntimeValue {
    match register {
        MarksRegister::Empty => RuntimeValue::Void,
        MarksRegister::Shared(marks) => RuntimeValue::Marks(Arc::clone(marks)),
    }
}

#[derive(Default)]
pub(crate) struct BatchWorkspace {
    rows: Rows,
    references: References,
    /// Registers whose value is the same in every live lane (`LaneOp` numbering).
    uniform: Vec<u64>,
    pub(crate) lanes: Lanes,
}

impl core::fmt::Debug for BatchWorkspace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BatchWorkspace")
            .field("floats", &self.rows.floats.len())
            .field("ints", &self.rows.ints.len())
            .field("colors", &self.rows.colors.len())
            .finish()
    }
}

fn reference_count(layout: super::SlotLayout) -> usize {
    (layout.arrays + layout.enums + layout.marks + layout.curves + layout.gradients) as usize
}

impl BatchWorkspace {
    pub(crate) fn reserve(&mut self, program: &Program, plan: &BatchPlan) {
        let layout = program.layout;
        grow(&mut self.rows.ints, layout.ints, [0; LANES]);
        grow(&mut self.rows.floats, layout.floats, [0.0; LANES]);
        grow(&mut self.rows.bools, layout.bools, 0);
        grow(&mut self.rows.colors, layout.colors, [Color::BLACK; LANES]);
        grow(&mut self.rows.loops, program.loop_count, [0; LANES]);
        let references = &mut self.references;
        let shared = reference_count(layout);
        if references.shared.len() < shared {
            references.shared.resize(shared, RuntimeValue::Void);
        }
        while references.lanes.len() < plan.row_count() {
            references
                .lanes
                .push(core::array::from_fn(|_| RuntimeValue::Void));
        }
        let words = lane_registers(program).div_ceil(64);
        if self.uniform.len() < words {
            self.uniform.resize(words, 0);
        }
        // Every lane may hold the program's whole live array set.
        let capacity = program.array_capacity as usize * LANES;
        let width = program.array_width as usize;
        let storage = &references.storage;
        if capacity > storage.references.len() || width > storage.width {
            references.storage = ArrayStorage::new(
                capacity.max(storage.references.len()),
                width.max(storage.width),
            );
        }
    }

    /// Bytes owned by a workspace reserved for programs whose largest banks
    /// are `layout`, with `loop_count` loops and local arrays of `capacity` x
    /// `width` per lane. Every reference register is budgeted a lane row.
    pub(crate) fn storage_estimate(
        layout: super::SlotLayout,
        loop_count: u32,
        capacity: usize,
        width: usize,
    ) -> Option<usize> {
        let rows = reference_count(layout);
        let row = |count: usize, size: usize| count.checked_mul(size);
        let lanes = capacity.checked_mul(LANES)?;
        [
            Some(size_of::<Self>()),
            row(layout.ints as usize, size_of::<[i32; LANES]>()),
            row(layout.floats as usize, size_of::<[f32; LANES]>()),
            row(layout.bools as usize, size_of::<Mask>()),
            row(layout.colors as usize, size_of::<[Color; LANES]>()),
            row(loop_count as usize, size_of::<[u32; LANES]>()),
            row(reference_count(layout), size_of::<RuntimeValue>()),
            row(rows, size_of::<[RuntimeValue; LANES]>()),
            row(
                (layout.ints + layout.floats + layout.bools + layout.colors + loop_count)
                    .div_ceil(64) as usize,
                size_of::<u64>(),
            ),
            row(lanes, 3 * size_of::<usize>()),
            lanes
                .checked_mul(width)
                .and_then(|values| row(values, size_of::<RuntimeValue>())),
        ]
        .into_iter()
        .try_fold(0usize, |total, bytes| total.checked_add(bytes?))
    }
}

fn grow<T: Clone>(values: &mut Vec<T>, count: u32, value: T) {
    if values.len() < count as usize {
        values.resize(count as usize, value);
    }
}

/// Lanes parked at a later instruction. Masks are disjoint and nonempty, so at
/// most one group per lane exists.
struct Parked {
    pcs: [usize; LANES],
    masks: [Mask; LANES],
    len: usize,
    next: usize,
}

impl Parked {
    fn new() -> Self {
        Self {
            pcs: [0; LANES],
            masks: [0; LANES],
            len: 0,
            next: usize::MAX,
        }
    }

    #[inline(never)]
    fn park(&mut self, pc: usize, mask: Mask) {
        for group in 0..self.len {
            if self.pcs[group] == pc {
                self.masks[group] |= mask;
                return;
            }
        }
        self.pcs[self.len] = pc;
        self.masks[self.len] = mask;
        self.len += 1;
        self.next = self.next.min(pc);
    }

    /// Remove the group parked at `pc`, the lowest one.
    #[inline(never)]
    fn take(&mut self, pc: usize) -> Mask {
        let mut mask = 0;
        for group in 0..self.len {
            if self.pcs[group] == pc {
                mask = self.masks[group];
                self.len -= 1;
                self.pcs[group] = self.pcs[self.len];
                self.masks[group] = self.masks[self.len];
                break;
            }
        }
        self.next = self.pcs[..self.len]
            .iter()
            .copied()
            .min()
            .unwrap_or(usize::MAX);
        mask
    }

    /// Park lanes that ran past the lowest group, which runs instead.
    fn exchange(&mut self, pc: usize, mask: Mask) -> (usize, Mask) {
        let lowest = self.next;
        let resumed = self.take(lowest);
        self.park(pc, mask);
        (lowest, resumed)
    }

    /// Remove the lowest group.
    #[inline(never)]
    fn resume(&mut self) -> Option<(usize, Mask)> {
        if self.len == 0 {
            return None;
        }
        let mut lowest = 0;
        for group in 1..self.len {
            if self.pcs[group] < self.pcs[lowest] {
                lowest = group;
            }
        }
        let resumed = (self.pcs[lowest], self.masks[lowest]);
        self.len -= 1;
        self.pcs[lowest] = self.pcs[self.len];
        self.masks[lowest] = self.masks[self.len];
        self.next = self.pcs[..self.len]
            .iter()
            .copied()
            .min()
            .unwrap_or(usize::MAX);
        Some(resumed)
    }
}

/// List the lanes of `mask` in order and return their count.
fn activate(mask: Mask, lanes: &mut [u8; LANES]) -> usize {
    if mask & mask.wrapping_add(1) == 0 {
        // Contiguous from lane zero: every converged run.
        let count = mask.count_ones() as usize;
        for (lane, slot) in lanes.iter_mut().enumerate().take(count) {
            *slot = lane as u8;
        }
        return count;
    }
    let mut count = 0;
    for lane in 0..LANES {
        if mask >> lane & 1 != 0 {
            lanes[count] = lane as u8;
            count += 1;
        }
    }
    count
}

enum Flow {
    Next,
    /// Lanes of the current mask that jump to the target.
    Branch(Mask, usize),
    Return,
}

/// Run `$body` for each active lane. One loop body per instruction keeps code
/// small; the lane mask removes per-access bounds checks.
macro_rules! each {
    ($lanes:expr, |$lane:ident| $body:expr) => {
        for &lane in $lanes {
            let $lane = usize::from(lane) & (LANES - 1);
            $body;
        }
    };
}

/// Active lanes for which `$condition` holds.
macro_rules! select {
    ($lanes:expr, |$lane:ident| $condition:expr) => {{
        let mut bits: Mask = 0;
        each!($lanes, |$lane| bits |= Mask::from($condition) << $lane);
        bits
    }};
}

/// A comparison as `less` and/or `equal`, after optionally swapping its
/// operands: `a > b` is `b < a`. NaN operands satisfy neither.
#[derive(Clone, Copy)]
struct Test {
    swap: bool,
    less: bool,
    equal: bool,
}

const LESS: Test = Test {
    swap: false,
    less: true,
    equal: false,
};
const LESS_EQUAL: Test = Test {
    swap: false,
    less: true,
    equal: true,
};
const GREATER: Test = Test {
    swap: true,
    less: true,
    equal: false,
};
const GREATER_EQUAL: Test = Test {
    swap: true,
    less: true,
    equal: true,
};
const EQUAL: Test = Test {
    swap: false,
    less: false,
    equal: true,
};

impl Test {
    fn new(op: CompareOp) -> Self {
        match op {
            CompareOp::Less => LESS,
            CompareOp::LessEqual => LESS_EQUAL,
            CompareOp::Greater => GREATER,
            CompareOp::GreaterEqual => GREATER_EQUAL,
        }
    }
}

/// Lanes where `left(lane) <op> right(lane)` holds, with one loop per
/// comparison kind.
macro_rules! test_lanes {
    ($lanes:expr, $test:expr, |$lane:ident| ($left:expr, $right:expr)) => {
        match ($test.less, $test.equal) {
            (true, false) => select!($lanes, |$lane| $left < $right),
            (true, true) => select!($lanes, |$lane| $left <= $right),
            _ => select!($lanes, |$lane| $left == $right),
        }
    };
}

// Shared by every jump and comparison of a kind.
#[inline(never)]
fn test_floats(lanes: &[u8], left: &[f32; LANES], right: &[f32; LANES], test: Test) -> Mask {
    let (left, right) = if test.swap {
        (right, left)
    } else {
        (left, right)
    };
    test_lanes!(lanes, test, |lane| (left[lane], right[lane]))
}

/// Compares `value` with `constant`, or `constant` with `value`.
#[inline(never)]
fn test_float_constant(
    lanes: &[u8],
    value: &[f32; LANES],
    constant: f32,
    constant_left: bool,
    test: Test,
) -> Mask {
    if constant_left != test.swap {
        test_lanes!(lanes, test, |lane| (constant, value[lane]))
    } else {
        test_lanes!(lanes, test, |lane| (value[lane], constant))
    }
}

#[inline(never)]
fn test_ints(lanes: &[u8], left: &[i32; LANES], right: &[i32; LANES], test: Test) -> Mask {
    let (left, right) = if test.swap {
        (right, left)
    } else {
        (left, right)
    };
    test_lanes!(lanes, test, |lane| (left[lane], right[lane]))
}

/// One program at one time over runs of pixels with the same target stage.
pub(crate) struct Batch<'a> {
    program: &'a Program,
    plan: &'a BatchPlan,
    target_entry: usize,
    params: &'a BoundParams,
    context: RunContext,
    clock: Clock,
    /// Prepared section runs; without them a query uses the pixel's index
    /// and count.
    sections: Option<&'a PreparedSections>,
    min: [f32; 2],
    max: [f32; 2],
    /// First shared register of each reference bank, and their total.
    bases: [usize; 5],
    shared: usize,
    /// First register of the ints, floats, bools, colors and loops in the
    /// `LaneOp` numbering.
    lane_bases: [u16; 5],
    /// Source query seconds and their converted clock time.
    query: Option<(u32, Option<SampleTime>)>,
    workspace: &'a mut BatchWorkspace,
    /// Pixel count and bounds of the initialized target stage.
    stage: Option<(i32, [u32; 4])>,
}

impl Drop for Batch<'_> {
    fn drop(&mut self) {
        let rows = self.plan.row_count();
        self.workspace.references.clear(self.shared, rows);
    }
}

impl<'a> Batch<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        program: &'a Program,
        target_entry: usize,
        plan: &'a BatchPlan,
        params: &'a BoundParams,
        context: &RunContext,
        sections: Option<&'a PreparedSections>,
        workspace: &'a mut BatchWorkspace,
    ) -> Self {
        let layout = program.layout;
        let mut bases = [0; 5];
        let mut base = 0;
        for (slot, count) in bases.iter_mut().zip([
            layout.arrays,
            layout.enums,
            layout.marks,
            layout.curves,
            layout.gradients,
        ]) {
            *slot = base;
            base += count as usize;
        }
        let mut lane_bases = [0; 5];
        let mut lane_base = 0;
        for (slot, count) in
            lane_bases
                .iter_mut()
                .zip([layout.ints, layout.floats, layout.bools, layout.colors])
        {
            *slot = lane_base as u16;
            lane_base += count;
        }
        lane_bases[4] = lane_base as u16;
        Self {
            program,
            plan,
            target_entry,
            params,
            context: *context,
            clock: Clock::new(context),
            sections,
            min: [0.0; 2],
            max: [0.0; 2],
            bases,
            shared: base,
            lane_bases,
            query: None,
            workspace,
            stage: None,
        }
    }

    pub(crate) fn lanes(&mut self) -> &mut Lanes {
        &mut self.workspace.lanes
    }

    /// Evaluate the first `output.len()` lanes. They share a pixel count and
    /// target bounds.
    pub(crate) fn run(
        &mut self,
        pixel_count: usize,
        min: [f32; 2],
        max: [f32; 2],
        signals: &mut dyn BatchSignals,
        output: &mut [Color],
    ) {
        let count = output.len();
        assert!(count != 0 && count <= LANES);
        let stage = (
            pixel_count as i32,
            [
                min[0].to_bits(),
                min[1].to_bits(),
                max[0].to_bits(),
                max[1].to_bits(),
            ],
        );
        let start = match self.stage {
            None => Some(0),
            Some(current) if current != stage => Some(self.target_entry),
            Some(_) => None,
        };
        if let Some(start) = start {
            self.context.pixel_count = stage.0;
            self.min = min;
            self.max = max;
            // Initialization is uniform straight-line code: run it in lane
            // zero, then copy.
            let code = self.program.instructions.as_ref();
            for op in &code[start..self.program.pixel_entry as usize] {
                self.step(op, 1, &[0], signals, output);
            }
            self.broadcast(start != 0);
            self.stage = Some(stage);
        }
        let lanes = Mask::MAX >> (LANES - count);
        let initial = self.plan.initial();
        self.workspace.uniform[..initial.len()].copy_from_slice(initial);
        self.execute(self.program.pixel_entry as usize, lanes, signals, output);
    }

    fn uniform(&self, register: u16) -> bool {
        self.workspace.uniform[usize::from(register) / 64] >> (register % 64) & 1 != 0
    }

    fn mark(&mut self, register: u16, uniform: bool) {
        let word = &mut self.workspace.uniform[usize::from(register) / 64];
        let bit = 1 << (register % 64);
        if uniform {
            *word |= bit;
        } else {
            *word &= !bit;
        }
    }

    /// Copy one lane's value of `register` into every lane.
    fn splat(&mut self, register: u16, lane: usize) {
        let rows = &mut self.workspace.rows;
        let [ints, floats, bools, colors, loops] = self.lane_bases;
        let index = usize::from(register);
        if register >= loops {
            let row = &mut rows.loops[index - usize::from(loops)];
            *row = [row[lane]; LANES];
        } else if register >= colors {
            let row = &mut rows.colors[index - usize::from(colors)];
            *row = [row[lane]; LANES];
        } else if register >= bools {
            let row = &mut rows.bools[index - usize::from(bools)];
            *row = if *row >> lane & 1 != 0 { Mask::MAX } else { 0 };
        } else if register >= floats {
            let row = &mut rows.floats[index - usize::from(floats)];
            *row = [row[lane]; LANES];
        } else {
            let row = &mut rows.ints[index - usize::from(ints)];
            *row = [row[lane]; LANES];
        }
    }

    fn broadcast(&mut self, from_target: bool) {
        let rows = &mut self.workspace.rows;
        for slot in self.plan.outputs(from_target) {
            match *slot {
                ValueSlot::Int(slot) => {
                    let row = &mut rows.ints[slot.0 as usize];
                    *row = [row[0]; LANES];
                }
                ValueSlot::Float(slot) => {
                    let row = &mut rows.floats[slot.0 as usize];
                    *row = [row[0]; LANES];
                }
                ValueSlot::Bool(slot) => {
                    let row = &mut rows.bools[slot.0 as usize];
                    *row = if *row & 1 != 0 { Mask::MAX } else { 0 };
                }
                ValueSlot::Color(slot) => {
                    let row = &mut rows.colors[slot.0 as usize];
                    *row = [row[0]; LANES];
                }
                _ => unreachable!("batch plans list primitive outputs"),
            }
        }
    }

    /// Run lanes from `start` until every lane returns. While no lane is
    /// parked, an instruction whose inputs are uniform runs in one lane and
    /// its result is copied to the others.
    #[inline(never)]
    fn execute(
        &mut self,
        start: usize,
        mask: Mask,
        signals: &mut dyn BatchSignals,
        output: &mut [Color],
    ) {
        let code = self.program.instructions.as_ref();
        let ops = self.plan.ops();
        let mut parked = Parked::new();
        let mut live = mask;
        let (mut pc, mut mask) = (start, mask);
        let mut lanes = [0; LANES];
        let mut count = 0;
        let mut active = 0;
        loop {
            if mask != active {
                count = activate(mask, &mut lanes);
                active = mask;
            }
            if pc >= parked.next {
                if pc == parked.next {
                    mask |= parked.take(pc);
                } else {
                    (pc, mask) = parked.exchange(pc, mask);
                }
                continue;
            }
            let LaneOp {
                inputs,
                output: written,
                uniform,
            } = ops[pc];
            let single = uniform
                && mask == live
                && inputs
                    .iter()
                    .all(|&input| input == NO_REGISTER || self.uniform(input));
            // One dispatch site keeps a single copy of every handler.
            let lane = usize::from(lanes[0]);
            let (step_mask, step_lanes) = if single {
                (1 << lane, &lanes[..1])
            } else {
                (mask, &lanes[..count])
            };
            let mut flow = self.step(&code[pc], step_mask, step_lanes, signals, output);
            if written != NO_REGISTER {
                if single {
                    self.splat(written, lane);
                }
                self.mark(written, single);
            }
            if single && let Flow::Branch(taken, target) = flow {
                flow = Flow::Branch(if taken != 0 { mask } else { 0 }, target);
            }
            match flow {
                Flow::Next => pc += 1,
                Flow::Branch(taken, target) => {
                    if taken == 0 {
                        pc += 1;
                    } else if taken == mask {
                        pc = target;
                    } else {
                        let (stay, jump) = ((pc + 1, mask & !taken), (target, taken));
                        let (run, wait) = if stay.0 <= jump.0 {
                            (stay, jump)
                        } else {
                            (jump, stay)
                        };
                        parked.park(wait.0, wait.1);
                        (pc, mask) = run;
                    }
                }
                Flow::Return => {
                    live &= !mask;
                    match parked.resume() {
                        Some(resumed) => (pc, mask) = resumed,
                        None => return,
                    }
                }
            }
        }
    }

    fn shared_index(&self, slot: ValueSlot) -> usize {
        let bank = match slot {
            ValueSlot::Array(_) => 0,
            ValueSlot::Enum(_) => 1,
            ValueSlot::Marks(_) => 2,
            ValueSlot::Curve(_) => 3,
            ValueSlot::Gradient(_) => 4,
            _ => unreachable!("reference slot"),
        };
        self.bases[bank] + slot.index() as usize
    }

    /// Store a reference value into the lanes of `dst`.
    fn load(&mut self, dst: ValueSlot, value: RuntimeValue, lanes: &[u8]) {
        match self.plan.row(dst) {
            None => {
                let index = self.shared_index(dst);
                self.workspace.references.put_shared(index, value);
            }
            Some(row) => {
                let references = &mut self.workspace.references;
                each!(lanes, |lane| references.put(row, lane, value.clone()));
            }
        }
    }

    fn source(&self, slot: ValueSlot) -> Source<'_> {
        let references = &self.workspace.references;
        match self.plan.row(slot) {
            None => Source::Shared(&references.shared[self.shared_index(slot)]),
            Some(row) => Source::Lanes(&references.lanes[row]),
        }
    }

    /// One lane's value of any register.
    fn value(&self, slot: ValueSlot, lane: usize) -> RuntimeValue {
        let rows = &self.workspace.rows;
        match slot {
            ValueSlot::Void => RuntimeValue::Void,
            ValueSlot::Int(slot) => RuntimeValue::Int(rows.ints[slot.0 as usize][lane]),
            ValueSlot::Float(slot) => RuntimeValue::Float(rows.floats[slot.0 as usize][lane]),
            ValueSlot::Bool(slot) => {
                RuntimeValue::Bool(rows.bools[slot.0 as usize] >> lane & 1 != 0)
            }
            ValueSlot::Color(slot) => RuntimeValue::Color(rows.colors[slot.0 as usize][lane]),
            slot => clone_runtime(self.source(slot).lane(lane)),
        }
    }

    /// Store one lane's value. Typing gives the value its destination bank;
    /// integers widen into float registers.
    fn store(&mut self, dst: ValueSlot, lane: usize, value: RuntimeValue) {
        let rows = &mut self.workspace.rows;
        let index = dst.index() as usize;
        match (dst, value) {
            (ValueSlot::Void, _) | (_, RuntimeValue::Void) => {}
            (ValueSlot::Float(_), RuntimeValue::Int(value)) => {
                rows.floats[index][lane] = value as f32
            }
            (ValueSlot::Int(_), RuntimeValue::Int(value)) => rows.ints[index][lane] = value,
            (ValueSlot::Float(_), RuntimeValue::Float(value)) => rows.floats[index][lane] = value,
            (ValueSlot::Bool(_), RuntimeValue::Bool(value)) => {
                let row = &mut rows.bools[index];
                *row = (*row & !(1 << lane)) | (Mask::from(value) << lane);
            }
            (ValueSlot::Color(_), RuntimeValue::Color(value)) => rows.colors[index][lane] = value,
            (dst, value) => match self.plan.row(dst) {
                Some(row) => self.workspace.references.put(row, lane, value),
                None => unreachable!("only loads write shared references"),
            },
        }
    }

    fn query_time(&mut self, seconds: f32) -> Option<SampleTime> {
        let bits = seconds.to_bits();
        if let Some((previous, time)) = self.query
            && previous == bits
        {
            return time;
        }
        let time = crate::values::sample_time_from_seconds_f32(seconds).ok();
        self.query = Some((bits, time));
        time
    }

    /// Reference and signal instructions: per-lane values, out of the hot loop.
    #[inline(never)]
    fn step_values(
        &mut self,
        op: &Instruction<ContextRead, SignalAccess, ColorSlot>,
        mask: Mask,
        lanes: &[u8],
        signals: &mut dyn BatchSignals,
    ) {
        let params = &self.params.values;
        match op {
            Instruction::LoadCurveConst { dst, constant } => {
                let value = RuntimeValue::Curve(Arc::clone(&self.program.curves[*constant]));
                self.load(ValueSlot::Curve(*dst), value, lanes);
            }
            Instruction::LoadCurveParam { dst, source, .. } => {
                let value = curve_value(&params.curves[source.0 as usize]);
                self.load(ValueSlot::Curve(*dst), value, lanes);
            }
            Instruction::LoadGradientConst { dst, constant } => {
                let value = RuntimeValue::Gradient(Arc::clone(&self.program.gradients[*constant]));
                self.load(ValueSlot::Gradient(*dst), value, lanes);
            }
            Instruction::LoadGradientParam { dst, source, .. } => {
                let value = gradient_value(&params.gradients[source.0 as usize]);
                self.load(ValueSlot::Gradient(*dst), value, lanes);
            }
            Instruction::LoadMarksConst { dst, value } => {
                let value = RuntimeValue::Marks(Arc::clone(value));
                self.load(ValueSlot::Marks(*dst), value, lanes);
            }
            Instruction::LoadMarksParam { dst, source, .. } => {
                let value = marks_value(&params.marks[source.0 as usize]);
                self.load(ValueSlot::Marks(*dst), value, lanes);
            }
            Instruction::LoadArrayConst { dst, constant } => {
                let value =
                    RuntimeValue::Array(Arc::clone(&self.program.array_constants[*constant]));
                self.load(ValueSlot::Array(*dst), value, lanes);
            }
            Instruction::LoadArrayParam { dst, source, .. } => {
                let value = params.array_values[source.0 as usize].runtime();
                self.load(ValueSlot::Array(*dst), value, lanes);
            }
            Instruction::LoadEnumConst { dst, constant } => {
                let value = RuntimeValue::Enum(self.program.enums[*constant].clone());
                self.load(ValueSlot::Enum(*dst), value, lanes);
            }
            Instruction::LoadEnumParam { dst, source, .. } => {
                let value = RuntimeValue::Enum(params.enums[source.0 as usize].clone());
                self.load(ValueSlot::Enum(*dst), value, lanes);
            }
            Instruction::Move { dst, src } => {
                let src = dst.with_index(*src);
                for &lane in lanes {
                    let lane = usize::from(lane);
                    let value = self.value(src, lane);
                    self.store(*dst, lane, value);
                }
            }
            Instruction::Index {
                dst,
                target,
                index,
                default,
            } => self.index(*dst, ValueSlot::Array(*target), *index, *default, lanes),
            Instruction::Select {
                dst,
                items,
                index,
                default,
            } => {
                let sources = &self.program.value_operands[items.range()];
                for &lane in lanes {
                    let lane = usize::from(lane);
                    let source = if sources.is_empty() {
                        dst.with_index(*default)
                    } else {
                        sources[clamp_array_index(self.number(*index, lane), sources.len())]
                    };
                    let value = self.value(source, lane);
                    self.store(*dst, lane, value);
                }
            }
            Instruction::MakeArray { dst, items } => {
                let items = &self.program.value_operands[items.range()];
                for &lane in lanes {
                    let lane = usize::from(lane);
                    let array = self.workspace.references.storage.allocate(items.len());
                    for (offset, item) in items.iter().enumerate() {
                        let value = self.value(*item, lane);
                        let storage = &mut self.workspace.references.storage;
                        storage.retain(&value);
                        storage.values[array * storage.width + offset] = value;
                    }
                    self.store(ValueSlot::Array(*dst), lane, RuntimeValue::ArraySlot(array));
                    // The register now owns the construction root.
                    self.workspace
                        .references
                        .storage
                        .release(RuntimeValue::ArraySlot(array));
                }
            }
            Instruction::ValueEqual {
                dst,
                negate,
                left,
                right,
            } => {
                let mut equal: Mask = 0;
                for &lane in lanes {
                    let lane = usize::from(lane);
                    let same = matches!((left, right), (ValueSlot::Array(_), ValueSlot::Array(_)))
                        .then_some(false)
                        .unwrap_or_else(|| {
                            runtime_refs_equal(&self.value(*left, lane), &self.value(*right, lane))
                        });
                    equal |= Mask::from(same != *negate) << lane;
                }
                let row = &mut self.workspace.rows.bools[dst.0 as usize];
                *row = (*row & !mask) | equal;
            }
            Instruction::SignalSample {
                dst,
                input,
                seconds,
                pixel,
                frame_cache,
                ..
            } => {
                let frame_cache = (*frame_cache != u32::MAX).then_some(*frame_cache as usize);
                let row = self.workspace.rows.floats[seconds.0 as usize];
                let first = row[usize::from(lanes[0])].to_bits();
                let uniform = lanes
                    .iter()
                    .all(|&lane| row[usize::from(lane)].to_bits() == first);
                let mut colors = [Color::BLACK; LANES];
                if uniform && matches!(pixel, SignalPixel::Current) {
                    if let Some(time) = self.query_time(f32::from_bits(first)) {
                        signals.sample_run(*input, time, frame_cache, mask, &mut colors);
                    }
                } else {
                    for &lane in lanes {
                        let lane = usize::from(lane);
                        let address = match *pixel {
                            SignalPixel::Current => SignalPixel::Current,
                            SignalPixel::Local(index) => {
                                SignalPixel::Local(self.workspace.rows.ints[index.0 as usize][lane])
                            }
                            SignalPixel::Global(index) => SignalPixel::Global(
                                self.workspace.rows.ints[index.0 as usize][lane],
                            ),
                        };
                        if let Some(time) = self.query_time(row[lane]) {
                            colors[lane] =
                                signals.sample_pixel(*input, time, lane, address, frame_cache);
                        }
                    }
                }
                let destination = &mut self.workspace.rows.colors[dst.0 as usize];
                each!(lanes, |lane| destination[lane] = colors[lane]);
            }
            _ => unreachable!("dispatched by step"),
        }
    }

    /// Array element reads. Indices clamp; an empty array reads the default
    /// register. A lane that already holds the selected resource keeps it.
    fn index(
        &mut self,
        dst: ValueSlot,
        target: ValueSlot,
        index: NumberSlot,
        default: u32,
        lanes: &[u8],
    ) {
        let default = dst.with_index(default);
        let target_row = self.plan.row(target);
        let target_shared = self.shared_index(target);
        let row = self.plan.row(dst);
        for &lane in lanes {
            let lane = usize::from(lane);
            let index = self.number(index, lane);
            let value = {
                let references = &self.workspace.references;
                let array = match target_row {
                    Some(row) => &references.lanes[row][lane],
                    None => &references.shared[target_shared],
                };
                match array {
                    RuntimeValue::Array(values) if !values.is_empty() => {
                        let element = &values[clamp_array_index(index, values.len())];
                        if let Some(row) = row
                            && same_resource(&references.lanes[row][lane], element)
                        {
                            continue;
                        }
                        Some(RuntimeValue::from_value(element))
                    }
                    RuntimeValue::ArraySlot(slot) => {
                        let items = references.storage.items(*slot);
                        (!items.is_empty())
                            .then(|| clone_runtime(&items[clamp_array_index(index, items.len())]))
                    }
                    _ => None,
                }
            };
            let value = value.unwrap_or_else(|| self.value(default, lane));
            self.store(dst, lane, value);
        }
    }

    fn number(&self, slot: NumberSlot, lane: usize) -> i32 {
        match slot {
            NumberSlot::Int(slot) => self.workspace.rows.ints[slot.0 as usize][lane],
            NumberSlot::Float(slot) => self.workspace.rows.floats[slot.0 as usize][lane] as i32,
        }
    }

    // Out of line: the dispatch loop stays small, and one copy serves both
    // one-lane and per-lane execution.
    #[inline(never)]
    fn step(
        &mut self,
        op: &Instruction<ContextRead, SignalAccess, ColorSlot>,
        mask: Mask,
        lanes: &[u8],
        signals: &mut dyn BatchSignals,
        output: &mut [Color],
    ) -> Flow {
        // Primitive moves and choices stay in the hot loop; everything that
        // touches reference registers or signals takes the per-lane path.
        match op {
            Instruction::LoadCurveConst { .. }
            | Instruction::LoadCurveParam { .. }
            | Instruction::LoadGradientConst { .. }
            | Instruction::LoadGradientParam { .. }
            | Instruction::LoadMarksConst { .. }
            | Instruction::LoadMarksParam { .. }
            | Instruction::LoadArrayConst { .. }
            | Instruction::LoadArrayParam { .. }
            | Instruction::LoadEnumConst { .. }
            | Instruction::LoadEnumParam { .. }
            | Instruction::Select { .. }
            | Instruction::MakeArray { .. }
            | Instruction::SignalSample { .. } => {
                self.step_values(op, mask, lanes, signals);
                return Flow::Next;
            }
            Instruction::Index { .. } => {
                self.step_values(op, mask, lanes, signals);
                return Flow::Next;
            }
            Instruction::Move { dst, .. }
                if !matches!(
                    dst,
                    ValueSlot::Int(_)
                        | ValueSlot::Float(_)
                        | ValueSlot::Bool(_)
                        | ValueSlot::Color(_)
                ) =>
            {
                self.step_values(op, mask, lanes, signals);
                return Flow::Next;
            }
            Instruction::ValueEqual { left, right, .. }
                if !matches!(
                    (left, right),
                    (
                        ValueSlot::Int(_) | ValueSlot::Float(_),
                        ValueSlot::Int(_) | ValueSlot::Float(_)
                    ) | (ValueSlot::Bool(_), ValueSlot::Bool(_))
                        | (ValueSlot::Color(_), ValueSlot::Color(_))
                ) =>
            {
                self.step_values(op, mask, lanes, signals);
                return Flow::Next;
            }
            _ => {}
        }
        let params = &self.params.values;
        let bases = self.bases;
        let plan = self.plan;
        let BatchWorkspace {
            rows,
            references,
            lanes: inputs,
            ..
        } = &mut *self.workspace;
        let references = &*references;
        // A reference operand's lanes, resolved once per instruction.
        let source = |slot: ValueSlot| -> Source<'_> {
            match plan.row(slot) {
                Some(row) => Source::Lanes(&references.lanes[row]),
                None => {
                    let bank = match slot {
                        ValueSlot::Array(_) => 0,
                        ValueSlot::Enum(_) => 1,
                        ValueSlot::Marks(_) => 2,
                        ValueSlot::Curve(_) => 3,
                        _ => 4,
                    };
                    Source::Shared(&references.shared[bases[bank] + slot.index() as usize])
                }
            }
        };
        // Local slices keep bank lengths in registers across lane stores.
        let ints = &mut rows.ints[..];
        let floats = &mut rows.floats[..];
        let bools = &mut rows.bools[..];
        let colors = &mut rows.colors[..];
        let loops = &mut rows.loops[..];
        macro_rules! float1 {
            ($dst:expr, $src:expr, |$value:ident| $body:expr) => {{
                let (dst, src) = ($dst.0 as usize, $src.0 as usize);
                assert!(dst < floats.len() && src < floats.len());
                each!(lanes, |lane| {
                    let $value = floats[src][lane];
                    floats[dst][lane] = $body;
                });
            }};
        }
        macro_rules! float2 {
            ($dst:expr, $left:expr, $right:expr, |$a:ident, $b:ident| $body:expr) => {{
                let (dst, left, right) = ($dst.0 as usize, $left.0 as usize, $right.0 as usize);
                assert!(dst < floats.len() && left < floats.len() && right < floats.len());
                each!(lanes, |lane| {
                    let ($a, $b) = (floats[left][lane], floats[right][lane]);
                    floats[dst][lane] = $body;
                });
            }};
        }
        macro_rules! int2 {
            ($dst:expr, $left:expr, $right:expr, |$a:ident, $b:ident| $body:expr) => {{
                let (dst, left, right) = ($dst.0 as usize, $left.0 as usize, $right.0 as usize);
                assert!(dst < ints.len() && left < ints.len() && right < ints.len());
                each!(lanes, |lane| {
                    let ($a, $b) = (ints[left][lane], ints[right][lane]);
                    ints[dst][lane] = $body;
                });
            }};
        }
        macro_rules! set_bool {
            ($dst:expr, $bits:expr) => {{
                let bits: Mask = $bits;
                let row = &mut bools[$dst.0 as usize];
                *row = (*row & !mask) | bits;
            }};
        }
        // A jump takes the lanes whose test equals `when`.
        let branch = |holds: Mask, when: bool, target: usize| {
            Flow::Branch(if when { holds } else { mask & !holds }, target)
        };
        macro_rules! float_jump {
            ($left:expr, $right:expr, $when:expr, $target:expr, $accept:expr) => {{
                let holds = test_floats(
                    lanes,
                    &floats[$left.0 as usize],
                    &floats[$right.0 as usize],
                    $accept,
                );
                return branch(holds, $when, *$target);
            }};
        }
        macro_rules! float_jump_const {
            ($value:expr, $bits:expr, $when:expr, $target:expr, $accept:expr) => {{
                let holds = test_float_constant(
                    lanes,
                    &floats[$value.0 as usize],
                    f32::from_bits(*$bits),
                    false,
                    $accept,
                );
                return branch(holds, $when, *$target);
            }};
        }
        macro_rules! int_jump {
            ($left:expr, $right:expr, $when:expr, $target:expr, $accept:expr) => {{
                let holds = test_ints(
                    lanes,
                    &ints[$left.0 as usize],
                    &ints[$right.0 as usize],
                    $accept,
                );
                return branch(holds, $when, *$target);
            }};
        }
        macro_rules! colors1 {
            ($dst:expr, |$lane:ident| $body:expr) => {{
                let dst = $dst.0 as usize;
                assert!(dst < colors.len());
                each!(lanes, |$lane| colors[dst][$lane] = $body);
            }};
        }
        // A reference operand: one shared value, or one per lane.
        macro_rules! with_source {
            ($source:expr, |$value:ident, $lane:ident| $body:expr) => {
                match $source {
                    Source::Shared($value) => each!(lanes, |$lane| $body),
                    Source::Lanes(row) => each!(lanes, |$lane| {
                        let $value = &row[$lane];
                        $body
                    }),
                }
            };
        }
        macro_rules! context {
            ($dst:expr, |$lane:ident| $value:expr) => {
                match $dst {
                    NumberSlot::Int(slot) => {
                        each!(lanes, |$lane| ints[slot.0 as usize][$lane] = $value as i32)
                    }
                    NumberSlot::Float(slot) => {
                        each!(lanes, |$lane| floats[slot.0 as usize][$lane] =
                            $value as f32)
                    }
                }
            };
        }
        match op {
            Instruction::LoadIntConst { dst, value } => {
                each!(lanes, |lane| ints[dst.0 as usize][lane] = *value)
            }
            Instruction::LoadFloatConst { dst, bits } => {
                let value = f32::from_bits(*bits);
                each!(lanes, |lane| floats[dst.0 as usize][lane] = value)
            }
            Instruction::LoadBoolConst { dst, value } => {
                set_bool!(dst, if *value { mask } else { 0 })
            }
            Instruction::LoadColorConst { dst, value } => colors1!(dst, |_lane| *value),
            Instruction::LoadIntParam { dst, source, .. } => {
                let value = params.ints[source.0 as usize];
                each!(lanes, |lane| ints[dst.0 as usize][lane] = value)
            }
            Instruction::LoadFloatParam { dst, source, .. } => {
                let value = params.floats[source.0 as usize];
                each!(lanes, |lane| floats[dst.0 as usize][lane] = value)
            }
            Instruction::LoadBoolParam { dst, source, .. } => {
                set_bool!(
                    dst,
                    if params.bools[source.0 as usize] {
                        mask
                    } else {
                        0
                    }
                )
            }
            Instruction::LoadColorParam { dst, source, .. } => {
                let value = params.colors[source.0 as usize];
                colors1!(dst, |_lane| value)
            }
            Instruction::Move { dst, src } => {
                let src = *src as usize;
                match *dst {
                    ValueSlot::Int(dst) => {
                        each!(lanes, |lane| ints[dst.0 as usize][lane] = ints[src][lane])
                    }
                    ValueSlot::Float(dst) => {
                        each!(lanes, |lane| floats[dst.0 as usize][lane] =
                            floats[src][lane])
                    }
                    ValueSlot::Bool(dst) => set_bool!(dst, bools[src] & mask),
                    ValueSlot::Color(dst) => {
                        each!(lanes, |lane| colors[dst.0 as usize][lane] =
                            colors[src][lane])
                    }
                    _ => unreachable!("reference moves take the per-lane path"),
                }
            }
            Instruction::Choose {
                dst,
                condition,
                when_true,
                when_false,
            } => {
                let condition = bools[condition.0 as usize];
                let (yes, no) = (*when_true as usize, *when_false as usize);
                let pick = |lane: usize| if condition >> lane & 1 != 0 { yes } else { no };
                match *dst {
                    ValueSlot::Int(dst) => {
                        each!(lanes, |lane| ints[dst.0 as usize][lane] =
                            ints[pick(lane)][lane])
                    }
                    ValueSlot::Float(dst) => {
                        each!(lanes, |lane| floats[dst.0 as usize][lane] =
                            floats[pick(lane)][lane])
                    }
                    ValueSlot::Bool(dst) => {
                        let bits = (bools[yes] & condition) | (bools[no] & !condition);
                        set_bool!(dst, bits & mask)
                    }
                    ValueSlot::Color(dst) => {
                        each!(lanes, |lane| colors[dst.0 as usize][lane] =
                            colors[pick(lane)][lane])
                    }
                    _ => unreachable!("choose writes primitive banks"),
                }
            }
            Instruction::CurveParamSample {
                dst,
                source,
                position,
                ..
            } => {
                let curve = params.curves[source.0 as usize].raw();
                float1!(dst, position, |position| sample_curve(curve, position))
            }
            Instruction::GradientParamSample {
                dst,
                source,
                position,
                ..
            } => {
                let gradient = params.gradients[source.0 as usize].get();
                let position = &floats[position.0 as usize];
                colors1!(dst, |lane| sample_gradient(gradient, position[lane]))
            }
            Instruction::CurveSample {
                dst,
                curve,
                position,
            } => {
                with_source!(source(ValueSlot::Curve(*curve)), |curve, lane| {
                    let value = sample_curve(curve_of(curve), floats[position.0 as usize][lane]);
                    floats[dst.0 as usize][lane] = value;
                })
            }
            Instruction::GradientSample {
                dst,
                gradient,
                position,
            } => {
                with_source!(source(ValueSlot::Gradient(*gradient)), |gradient, lane| {
                    let color =
                        sample_gradient(gradient_of(gradient), floats[position.0 as usize][lane]);
                    colors[dst.0 as usize][lane] = color;
                })
            }
            Instruction::IntToFloat { dst, src } => each!(lanes, |lane| {
                floats[dst.0 as usize][lane] = ints[src.0 as usize][lane] as f32
            }),
            // `as` truncates toward zero, saturates, and maps NaN to zero.
            Instruction::FloatToInt { dst, src } => each!(lanes, |lane| {
                ints[dst.0 as usize][lane] = floats[src.0 as usize][lane] as i32
            }),
            Instruction::Not { dst, src } => set_bool!(dst, !bools[src.0 as usize] & mask),
            Instruction::NegInt { dst, src } => each!(lanes, |lane| {
                ints[dst.0 as usize][lane] = ints[src.0 as usize][lane].wrapping_neg()
            }),
            Instruction::NegFloat { dst, src } => float1!(dst, src, |value| -value),
            Instruction::FloatAdd { dst, left, right } => float2!(dst, left, right, |a, b| a + b),
            Instruction::FloatSubtract { dst, left, right } => {
                float2!(dst, left, right, |a, b| a - b)
            }
            Instruction::FloatMultiply { dst, left, right } => {
                float2!(dst, left, right, |a, b| a * b)
            }
            Instruction::FloatDivide { dst, left, right } => {
                float2!(dst, left, right, |a, b| a / b)
            }
            Instruction::FloatRemainder { dst, left, right } => {
                float2!(dst, left, right, |a, b| float_remainder(a, b))
            }
            Instruction::FloatAddConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| value + constant)
            }
            Instruction::FloatSubtractConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| value - constant)
            }
            Instruction::FloatMultiplyConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| value * constant)
            }
            Instruction::FloatDivideConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| value / constant)
            }
            Instruction::FloatRemainderConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| float_remainder(value, constant))
            }
            Instruction::FloatSubtractFromConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| constant - value)
            }
            Instruction::FloatDivideIntoConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| constant / value)
            }
            Instruction::FloatRemainderFromConst {
                dst,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                float1!(dst, value, |value| float_remainder(constant, value))
            }
            Instruction::IntAdd { dst, left, right } => {
                int2!(dst, left, right, |a, b| a.wrapping_add(b))
            }
            Instruction::IntSubtract { dst, left, right } => {
                int2!(dst, left, right, |a, b| a.wrapping_sub(b))
            }
            Instruction::IntMultiply { dst, left, right } => {
                int2!(dst, left, right, |a, b| a.wrapping_mul(b))
            }
            Instruction::IntRemainder { dst, left, right } => {
                int2!(dst, left, right, |a, b| int_remainder(a, b))
            }
            Instruction::FloatCompare {
                dst,
                op,
                left,
                right,
            } => {
                let bits = test_floats(
                    lanes,
                    &floats[left.0 as usize],
                    &floats[right.0 as usize],
                    Test::new(*op),
                );
                set_bool!(dst, bits)
            }
            Instruction::IntCompare {
                dst,
                op,
                left,
                right,
            } => {
                let bits = test_ints(
                    lanes,
                    &ints[left.0 as usize],
                    &ints[right.0 as usize],
                    Test::new(*op),
                );
                set_bool!(dst, bits)
            }
            Instruction::FloatCompareConst {
                dst,
                op,
                value,
                constant_bits,
                constant_left,
            } => {
                let value = &floats[value.0 as usize];
                let constant = f32::from_bits(*constant_bits);
                let bits =
                    test_float_constant(lanes, value, constant, *constant_left, Test::new(*op));
                set_bool!(dst, bits)
            }
            Instruction::ValueEqual {
                dst,
                negate,
                left,
                right,
            } => {
                let equal = match (*left, *right) {
                    (ValueSlot::Int(left), ValueSlot::Int(right)) => {
                        let (left, right) = (&ints[left.0 as usize], &ints[right.0 as usize]);
                        select!(lanes, |lane| left[lane] == right[lane])
                    }
                    (ValueSlot::Float(left), ValueSlot::Float(right)) => {
                        let (left, right) = (&floats[left.0 as usize], &floats[right.0 as usize]);
                        select!(lanes, |lane| left[lane] == right[lane])
                    }
                    (ValueSlot::Int(left), ValueSlot::Float(right)) => {
                        let (left, right) = (&ints[left.0 as usize], &floats[right.0 as usize]);
                        select!(lanes, |lane| left[lane] as f32 == right[lane])
                    }
                    (ValueSlot::Float(left), ValueSlot::Int(right)) => {
                        let (left, right) = (&floats[left.0 as usize], &ints[right.0 as usize]);
                        select!(lanes, |lane| left[lane] == right[lane] as f32)
                    }
                    (ValueSlot::Bool(left), ValueSlot::Bool(right)) => {
                        !(bools[left.0 as usize] ^ bools[right.0 as usize]) & mask
                    }
                    (ValueSlot::Color(left), ValueSlot::Color(right)) => {
                        let (left, right) = (&colors[left.0 as usize], &colors[right.0 as usize]);
                        select!(lanes, |lane| left[lane] == right[lane])
                    }
                    _ => unreachable!("other operands take the per-lane path"),
                };
                set_bool!(dst, if *negate { !equal & mask } else { equal })
            }
            Instruction::EnumParamEqualConst {
                dst,
                source,
                constant,
                negate,
                ..
            } => {
                let equal = params.enums[source.0 as usize] == self.program.enums[*constant];
                set_bool!(dst, if equal != *negate { mask } else { 0 })
            }
            Instruction::IntJumpLess {
                left,
                right,
                when,
                target,
            } => int_jump!(left, right, *when, target, LESS),
            Instruction::IntJumpLessEqual {
                left,
                right,
                when,
                target,
            } => int_jump!(left, right, *when, target, LESS_EQUAL),
            Instruction::IntJumpGreater {
                left,
                right,
                when,
                target,
            } => int_jump!(left, right, *when, target, GREATER),
            Instruction::IntJumpGreaterEqual {
                left,
                right,
                when,
                target,
            } => int_jump!(left, right, *when, target, GREATER_EQUAL),
            Instruction::IntJumpEqual {
                left,
                right,
                when,
                target,
            } => int_jump!(left, right, *when, target, EQUAL),
            Instruction::FloatJumpLess {
                left,
                right,
                when,
                target,
            } => float_jump!(left, right, *when, target, LESS),
            Instruction::FloatJumpLessEqual {
                left,
                right,
                when,
                target,
            } => float_jump!(left, right, *when, target, LESS_EQUAL),
            Instruction::FloatJumpGreater {
                left,
                right,
                when,
                target,
            } => float_jump!(left, right, *when, target, GREATER),
            Instruction::FloatJumpGreaterEqual {
                left,
                right,
                when,
                target,
            } => float_jump!(left, right, *when, target, GREATER_EQUAL),
            Instruction::FloatJumpEqual {
                left,
                right,
                when,
                target,
            } => float_jump!(left, right, *when, target, EQUAL),
            Instruction::FloatJumpLessConst {
                value,
                constant_bits,
                when,
                target,
            } => float_jump_const!(value, constant_bits, *when, target, LESS),
            Instruction::FloatJumpLessEqualConst {
                value,
                constant_bits,
                when,
                target,
            } => float_jump_const!(value, constant_bits, *when, target, LESS_EQUAL),
            Instruction::FloatJumpGreaterConst {
                value,
                constant_bits,
                when,
                target,
            } => float_jump_const!(value, constant_bits, *when, target, GREATER),
            Instruction::FloatJumpGreaterEqualConst {
                value,
                constant_bits,
                when,
                target,
            } => float_jump_const!(value, constant_bits, *when, target, GREATER_EQUAL),
            Instruction::FloatJumpEqualConst {
                value,
                constant_bits,
                when,
                target,
            } => float_jump_const!(value, constant_bits, *when, target, EQUAL),
            Instruction::Jump(target) => return Flow::Branch(mask, *target),
            Instruction::JumpIfFalse { condition, target } => {
                return Flow::Branch(!bools[condition.0 as usize] & mask, *target);
            }
            Instruction::JumpIfTrue { condition, target } => {
                return Flow::Branch(bools[condition.0 as usize] & mask, *target);
            }
            Instruction::LoopRangeStart {
                id,
                count,
                cap,
                end,
            } => {
                let (remaining, count) = (&mut loops[*id as usize], &ints[count.0 as usize]);
                let empty = select!(lanes, |lane| {
                    let count = count[lane].max(0).min(*cap);
                    remaining[lane] = count as u32;
                    count == 0
                });
                return Flow::Branch(empty, end + 1);
            }
            Instruction::LoopMarksStart { id, marks, end } => {
                let marks = source(ValueSlot::Marks(*marks));
                let remaining = &mut loops[*id as usize];
                let empty = select!(lanes, |lane| {
                    let count = marks_of(marks.lane(lane)).as_slice().len();
                    remaining[lane] = u32::try_from(count).unwrap_or(u32::MAX);
                    count == 0
                });
                return Flow::Branch(empty, end + 1);
            }
            Instruction::LoopEnd { id, start } => {
                let remaining = &mut loops[*id as usize];
                let again = select!(lanes, |lane| {
                    let more = remaining[lane] > 1;
                    remaining[lane] = if more { remaining[lane] - 1 } else { 0 };
                    more
                });
                return Flow::Branch(again, *start);
            }
            Instruction::ContextRead { dst, read } => {
                let row = match read {
                    ContextRead::PixelIndex => {
                        let row = &inputs.pixel_index;
                        context!(*dst, |lane| row[lane]);
                        return Flow::Next;
                    }
                    ContextRead::PixelFraction => &inputs.pixel_fraction,
                    ContextRead::PixelX => &inputs.x,
                    ContextRead::PixelY => &inputs.y,
                    read => {
                        let spatial = super::SpatialContext {
                            position: [0.0; 2],
                            min: self.min,
                            max: self.max,
                        };
                        let (int, float) = match read.read(&self.context, self.clock, &spatial) {
                            super::context::Number::Int(value) => (value, value as f32),
                            super::context::Number::Float(value) => (value as i32, value),
                        };
                        match *dst {
                            NumberSlot::Int(slot) => {
                                each!(lanes, |lane| ints[slot.0 as usize][lane] = int)
                            }
                            NumberSlot::Float(slot) => {
                                each!(lanes, |lane| floats[slot.0 as usize][lane] = float)
                            }
                        }
                        return Flow::Next;
                    }
                };
                context!(*dst, |lane| row[lane]);
            }
            Instruction::QuerySeconds { dst, seconds } => {
                let duration = self.context.duration;
                float1!(dst, seconds, |seconds| query_seconds(seconds, duration))
            }
            Instruction::QueryProgress { dst, seconds } => {
                let duration = self.context.duration;
                float1!(dst, seconds, |seconds| query_progress(seconds, duration))
            }
            Instruction::SectionPosition {
                dst,
                width,
                inverse,
            } => each!(lanes, |lane| {
                floats[dst.0 as usize][lane] = section_position(
                    inputs.pixel_index[lane],
                    floats[width.0 as usize][lane],
                    floats[inverse.0 as usize][lane],
                )
            }),
            Instruction::SectionQuery { dst, width, index } => each!(lanes, |lane| {
                let sections = match self.sections {
                    Some(target) => SectionContext::Prepared {
                        target,
                        pixel: inputs.sections[lane],
                    },
                    None => SectionContext::Single {
                        index: inputs.pixel_index[lane],
                        count: self.context.pixel_count,
                    },
                };
                ints[dst.0 as usize][lane] = sections.query(ints[width.0 as usize][lane], *index);
            }),
            Instruction::FloatUnary { dst, op, value } => match op {
                FloatUnary::Sin => float1!(dst, value, |v| float_unary(FloatUnary::Sin, v)),
                FloatUnary::Cos => float1!(dst, value, |v| float_unary(FloatUnary::Cos, v)),
                FloatUnary::Abs => float1!(dst, value, |v| float_unary(FloatUnary::Abs, v)),
                FloatUnary::Floor => float1!(dst, value, |v| float_unary(FloatUnary::Floor, v)),
                FloatUnary::Ceil => float1!(dst, value, |v| float_unary(FloatUnary::Ceil, v)),
                FloatUnary::Trunc => float1!(dst, value, |v| float_unary(FloatUnary::Trunc, v)),
                FloatUnary::RoundEven => {
                    float1!(dst, value, |v| float_unary(FloatUnary::RoundEven, v))
                }
                FloatUnary::Sqrt => float1!(dst, value, |v| float_unary(FloatUnary::Sqrt, v)),
            },
            Instruction::FloatBinary {
                dst,
                op,
                left,
                right,
            } => match op {
                FloatBinary::Min => {
                    float2!(dst, left, right, |a, b| float_binary(
                        FloatBinary::Min,
                        a,
                        b
                    ))
                }
                FloatBinary::Max => {
                    float2!(dst, left, right, |a, b| float_binary(
                        FloatBinary::Max,
                        a,
                        b
                    ))
                }
                op => float2!(dst, left, right, |a, b| float_binary(*op, a, b)),
            },
            Instruction::FloatBinaryConst {
                dst,
                op,
                value,
                constant_bits,
            } => {
                let constant = f32::from_bits(*constant_bits);
                match op {
                    FloatBinary::Min => {
                        float1!(dst, value, |v| float_binary(FloatBinary::Min, v, constant))
                    }
                    FloatBinary::Max => {
                        float1!(dst, value, |v| float_binary(FloatBinary::Max, v, constant))
                    }
                    op => float1!(dst, value, |v| float_binary(*op, v, constant)),
                }
            }
            Instruction::Clamp {
                dst,
                value,
                min,
                max,
            } => each!(lanes, |lane| {
                floats[dst.0 as usize][lane] = clamp_float(
                    floats[value.0 as usize][lane],
                    floats[min.0 as usize][lane],
                    floats[max.0 as usize][lane],
                )
            }),
            Instruction::ClampConst {
                dst,
                value,
                min_bits,
                max_bits,
            } => {
                let (min, max) = (f32::from_bits(*min_bits), f32::from_bits(*max_bits));
                float1!(dst, value, |value| clamp_float(value, min, max))
            }
            Instruction::Smoothstep { dst, value } => {
                float1!(dst, value, |value| smoothstep(value))
            }
            Instruction::MixFloat {
                dst,
                left,
                right,
                amount,
            } => each!(lanes, |lane| {
                let (left, right) = (
                    floats[left.0 as usize][lane],
                    floats[right.0 as usize][lane],
                );
                floats[dst.0 as usize][lane] =
                    left + (right - left) * floats[amount.0 as usize][lane]
            }),
            Instruction::MixColor {
                dst,
                left,
                right,
                amount,
            } => each!(lanes, |lane| {
                colors[dst.0 as usize][lane] = mix_colors(
                    colors[left.0 as usize][lane],
                    colors[right.0 as usize][lane],
                    floats[amount.0 as usize][lane],
                )
            }),
            Instruction::ColorBinary {
                dst,
                op,
                left,
                right,
            } => {
                let combine = match op {
                    ColorBinary::Add => add_colors,
                    ColorBinary::Multiply => multiply_colors,
                    ColorBinary::Max => max_colors,
                };
                each!(lanes, |lane| {
                    colors[dst.0 as usize][lane] = combine(
                        colors[left.0 as usize][lane],
                        colors[right.0 as usize][lane],
                    )
                })
            }
            Instruction::ColorScale { dst, color, scale } => each!(lanes, |lane| {
                colors[dst.0 as usize][lane] = scale_color(
                    colors[color.0 as usize][lane],
                    floats[scale.0 as usize][lane],
                )
            }),
            Instruction::ColorComponent { dst, op, color } => {
                let component = match op {
                    ColorComponent::Hue => color_hue,
                    ColorComponent::Saturation => color_saturation,
                    ColorComponent::Intensity => color_intensity,
                };
                each!(lanes, |lane| {
                    floats[dst.0 as usize][lane] = component(colors[color.0 as usize][lane])
                })
            }
            Instruction::ColorInvert { dst, color } => each!(lanes, |lane| {
                colors[dst.0 as usize][lane] = invert_color(colors[color.0 as usize][lane])
            }),
            Instruction::Rgb {
                dst,
                red,
                green,
                blue,
            } => each!(lanes, |lane| {
                colors[dst.0 as usize][lane] = crate::sampling::rgb(
                    floats[red.0 as usize][lane],
                    floats[green.0 as usize][lane],
                    floats[blue.0 as usize][lane],
                )
            }),
            Instruction::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => each!(lanes, |lane| {
                colors[dst.0 as usize][lane] = crate::sampling::hsv(
                    floats[hue.0 as usize][lane],
                    floats[saturation.0 as usize][lane],
                    floats[value.0 as usize][lane],
                )
            }),
            Instruction::Rand { dst, seed } => float1!(dst, seed, |seed| {
                crate::sampling::deterministic_random_seed(seed)
            }),
            Instruction::CurveFloatClamped {
                dst,
                curve,
                position,
                min,
                max,
            } => {
                with_source!(source(ValueSlot::Curve(*curve)), |curve, lane| {
                    let value = sample_curve(curve_of(curve), floats[position.0 as usize][lane]);
                    floats[dst.0 as usize][lane] = clamp_float(
                        value,
                        floats[min.0 as usize][lane],
                        floats[max.0 as usize][lane],
                    );
                })
            }
            Instruction::CurveParamFloatClamped {
                dst,
                source,
                position,
                min,
                max,
                ..
            } => {
                let curve = &params.curves[source.0 as usize];
                each!(lanes, |lane| {
                    floats[dst.0 as usize][lane] = clamp_float(
                        curve.sample(floats[position.0 as usize][lane]),
                        floats[min.0 as usize][lane],
                        floats[max.0 as usize][lane],
                    )
                })
            }
            Instruction::GradientColorScaled {
                dst,
                gradient,
                position,
                scale,
            } => {
                with_source!(source(ValueSlot::Gradient(*gradient)), |gradient, lane| {
                    let color = gradient_color_scaled(
                        gradient_of(gradient),
                        floats[position.0 as usize][lane],
                        floats[scale.0 as usize][lane],
                    );
                    colors[dst.0 as usize][lane] = color;
                })
            }
            Instruction::GradientParamColorScaled {
                dst,
                source,
                position,
                scale,
                ..
            } => {
                let gradient = params.gradients[source.0 as usize].get();
                each!(lanes, |lane| {
                    colors[dst.0 as usize][lane] = gradient_color_scaled(
                        gradient,
                        floats[position.0 as usize][lane],
                        floats[scale.0 as usize][lane],
                    )
                })
            }
            Instruction::CurveCrossing {
                dst,
                curve,
                value,
                before,
            } => {
                with_source!(source(ValueSlot::Curve(*curve)), |curve, lane| {
                    let curve = curve_of(curve);
                    let value = floats[value.0 as usize][lane];
                    floats[dst.0 as usize][lane] = match before {
                        Some(position) => crate::sampling::curve_last_crossing(
                            curve,
                            value,
                            floats[position.0 as usize][lane],
                        ),
                        None => curve_crossing_raw(curve, value, f32::NAN),
                    };
                })
            }
            Instruction::CurveParamCrossing {
                dst,
                source,
                value,
                before,
                ..
            } => {
                let curve = &params.curves[source.0 as usize];
                match before {
                    Some(position) => each!(lanes, |lane| {
                        floats[dst.0 as usize][lane] = crate::sampling::curve_last_crossing(
                            curve.raw(),
                            floats[value.0 as usize][lane],
                            floats[position.0 as usize][lane],
                        )
                    }),
                    None => float1!(dst, value, |value| curve.crossing(value, f32::NAN)),
                }
            }
            Instruction::Len { dst, value } => {
                with_source!(source(ValueSlot::Array(*value)), |array, lane| {
                    ints[dst.0 as usize][lane] = int_len(array_of(array, &references.storage).len())
                })
            }
            Instruction::Mark { marks, op } => {
                let marks = source(ValueSlot::Marks(*marks));
                match *op {
                    MarkOp::Count { dst } => with_source!(marks, |marks, lane| {
                        ints[dst.0 as usize][lane] = int_len(marks_of(marks).as_slice().len())
                    }),
                    MarkOp::At { dst, index } => with_source!(marks, |marks, lane| {
                        floats[dst.0 as usize][lane] =
                            mark_at_from(marks_of(marks), ints[index.0 as usize][lane])
                    }),
                    MarkOp::Last { dst, seconds } => with_source!(marks, |marks, lane| {
                        floats[dst.0 as usize][lane] =
                            previous_mark(marks_of(marks), floats[seconds.0 as usize][lane])
                                .map_or(f32::NAN, |(_, time)| time)
                    }),
                    MarkOp::LastIndex { dst, seconds } => with_source!(marks, |marks, lane| {
                        ints[dst.0 as usize][lane] =
                            prev_index(marks_of(marks), floats[seconds.0 as usize][lane])
                    }),
                }
            }
            Instruction::ReturnColor(slot) => {
                let row = &colors[slot.0 as usize];
                each!(lanes, |lane| output[lane] = row[lane]);
                return Flow::Return;
            }
            Instruction::LoadCurveConst { .. }
            | Instruction::LoadCurveParam { .. }
            | Instruction::LoadGradientConst { .. }
            | Instruction::LoadGradientParam { .. }
            | Instruction::LoadMarksConst { .. }
            | Instruction::LoadMarksParam { .. }
            | Instruction::LoadArrayConst { .. }
            | Instruction::LoadArrayParam { .. }
            | Instruction::LoadEnumConst { .. }
            | Instruction::LoadEnumParam { .. }
            | Instruction::Index { .. }
            | Instruction::Select { .. }
            | Instruction::MakeArray { .. }
            | Instruction::SignalSample { .. } => unreachable!("per-lane path"),
        }
        Flow::Next
    }
}

/// Whether a lane already holds this array element's resource.
fn same_resource(current: &RuntimeValue, element: &Value) -> bool {
    match (current, element) {
        (RuntimeValue::Curve(current), Value::Curve(element)) => Arc::ptr_eq(current, element),
        (RuntimeValue::Gradient(current), Value::Gradient(element)) => {
            Arc::ptr_eq(current, element)
        }
        (RuntimeValue::Marks(current), Value::Marks(element)) => Arc::ptr_eq(current, element),
        (RuntimeValue::Array(current), Value::Array(element)) => Arc::ptr_eq(current, element),
        _ => false,
    }
}
