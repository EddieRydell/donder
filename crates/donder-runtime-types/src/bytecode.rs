//! Portable strip bytecode. A program runs over strips of up to [`STRIP`]
//! pixels: its query and target blocks once, as scalar code, then its body
//! once per strip. A value is a scalar or a per-pixel row; which one is fixed
//! when the program is compiled. Control flow is structured: a branch's arms
//! and a reduction's parts follow their instruction inline.
//!
//! A color is three floats: a color operand names the first of three
//! consecutive float slots of one kind, red, green and blue. Float
//! arithmetic, mixes, choices and copies take colors as operands and run once
//! per channel; a channel read is the channel's own slot.
use crate::types::{Identifier, Type, Value};
use crate::word::InstructionWord;
use alloc::{boxed::Box, string::String, vec::Vec};

/// Pixels per strip. A power of two, so a selection index masked to it is in
/// range of every row.
pub const STRIP: usize = 128;
/// Most selections a program may hold open at once.
pub const MAX_DEPTH: u16 = 16;
/// Most bytes of rows per pixel a program may use.
pub const MAX_ROW_BYTES: u32 = 192;
/// Most iterations of one reduction.
pub const MAX_ITERATIONS: i32 = 10_000;

const ROW: u16 = 1 << 15;
const INPUT: u16 = 1 << 14;
const INDEX: u16 = INPUT - 1;

/// An operand of the bank its instruction implies: a scalar, a per-pixel row,
/// or one of the strip's pixel inputs. The compiler numbers slots freely
/// before allocation; an instruction word holds an allocated slot in a byte.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(transparent)]
pub struct Slot(u16);

/// What a slot names.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SlotKind {
    Scalar(u16),
    Row(u16),
    Input(Input),
}

/// Per-pixel values the strip supplies.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Input {
    /// Int: the pixel's index in its fixture.
    PixelIndex,
    /// Floats.
    PixelFraction,
    PixelX,
    PixelY,
}

impl Input {
    pub(crate) const ALL: [Self; 4] = [
        Self::PixelIndex,
        Self::PixelFraction,
        Self::PixelX,
        Self::PixelY,
    ];

    pub(crate) fn number(self) -> u16 {
        match self {
            Self::PixelIndex => 0,
            Self::PixelFraction => 1,
            Self::PixelX => 2,
            Self::PixelY => 3,
        }
    }
}

impl Slot {
    /// An absent optional operand.
    pub const NONE: Self = Self(u16::MAX);
    /// Allocated scalars of one bank an instruction word can name.
    pub const SCALARS: u16 = 127;
    /// Allocated rows of one bank an instruction word can name.
    pub const ROWS: u16 = 124;
    /// Virtual slots of one bank and kind the compiler may number before
    /// allocation.
    pub const VIRTUAL: u16 = INDEX;

    /// The slot's encoding, as a program record stores its result.
    pub const fn bits(self) -> u16 {
        self.0
    }

    pub const fn from_bits(raw: u16) -> Self {
        Self(raw)
    }

    pub const fn scalar(index: u16) -> Self {
        Self(index & INDEX)
    }
    pub const fn row(index: u16) -> Self {
        Self(ROW | (index & INDEX))
    }
    pub fn input(input: Input) -> Self {
        Self(INPUT | input.number())
    }

    pub fn kind(self) -> SlotKind {
        if self.0 & ROW != 0 {
            SlotKind::Row(self.0 & INDEX)
        } else if self.0 & INPUT != 0 {
            SlotKind::Input(Input::ALL[usize::from(self.0 & 3)])
        } else {
            SlotKind::Scalar(self.0)
        }
    }

    pub fn is_scalar(self) -> bool {
        matches!(self.kind(), SlotKind::Scalar(_))
    }

    pub fn is_none(self) -> bool {
        self == Self::NONE
    }

    /// Channel `channel` of the color this slot names.
    pub fn channel(self, channel: u16) -> Self {
        Self(self.0 + channel)
    }
}

/// A slot bank.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Bank {
    Float,
    /// Ints, and enum options as indices into the program's names.
    Int,
    Bool,
    /// Three consecutive floats.
    Color,
    /// Curves, gradients, marks and arrays, by reference.
    Resource,
}

impl Bank {
    pub fn for_type(ty: &Type) -> Self {
        match ty {
            Type::Float => Self::Float,
            Type::Int | Type::Enum(_) => Self::Int,
            Type::Bool => Self::Bool,
            Type::Color => Self::Color,
            _ => Self::Resource,
        }
    }

    /// The bank whose slots hold this bank's values: floats hold colors.
    pub fn storage(self) -> Self {
        match self {
            Self::Color => Self::Float,
            bank => bank,
        }
    }

    /// Slots of its storage one value takes.
    pub fn width(self) -> u16 {
        match self {
            Self::Color => 3,
            _ => 1,
        }
    }

    /// Bytes of one pixel of a row of its storage.
    fn row_bytes(self) -> u32 {
        match self {
            Self::Bool => 1,
            _ => 4,
        }
    }
}

/// Slot counts of each storage bank.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Banks {
    pub floats: u16,
    pub ints: u16,
    pub bools: u16,
    pub resources: u16,
}

impl Banks {
    const STORAGE: [Bank; 4] = [Bank::Float, Bank::Int, Bank::Bool, Bank::Resource];

    pub fn get(&self, bank: Bank) -> u16 {
        match bank.storage() {
            Bank::Int => self.ints,
            Bank::Bool => self.bools,
            Bank::Resource => self.resources,
            _ => self.floats,
        }
    }

    pub fn get_mut(&mut self, bank: Bank) -> &mut u16 {
        match bank.storage() {
            Bank::Int => &mut self.ints,
            Bank::Bool => &mut self.bools,
            Bank::Resource => &mut self.resources,
            _ => &mut self.floats,
        }
    }

    /// Row bytes per pixel.
    pub fn row_bytes(&self) -> u32 {
        Self::STORAGE
            .iter()
            .map(|&bank| u32::from(self.get(bank)) * bank.row_bytes())
            .sum()
    }

    /// The bankwise maximum.
    pub fn max(self, other: Self) -> Self {
        Self {
            floats: self.floats.max(other.floats),
            ints: self.ints.max(other.ints),
            bools: self.bools.max(other.bools),
            resources: self.resources.max(other.resources),
        }
    }
}

/// Values the strip's context supplies once.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ContextRead {
    /// Floats of the query.
    Seconds,
    Progress,
    Duration,
    /// The sequence's frames per second.
    FrameRate,
    /// Int: frames since the frame before; one after a seek.
    FramesElapsed,
    /// Int of the target.
    PixelCount,
    /// Floats of the target.
    TargetMinX,
    TargetMinY,
    TargetMaxX,
    TargetMaxY,
}

impl ContextRead {
    pub fn bank(self) -> Bank {
        match self {
            Self::PixelCount | Self::FramesElapsed => Bank::Int,
            _ => Bank::Float,
        }
    }

    pub fn reads_target(self) -> bool {
        !matches!(
            self,
            Self::Seconds | Self::Progress | Self::Duration | Self::FrameRate | Self::FramesElapsed
        )
    }

    pub fn reads_geometry(self) -> bool {
        matches!(
            self,
            Self::TargetMinX | Self::TargetMinY | Self::TargetMaxX | Self::TargetMaxY
        )
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatUnary {
    Negate,
    Sin,
    Cos,
    Tan,
    Exp,
    /// Natural logarithm.
    Log,
    Abs,
    Floor,
    Ceil,
    Trunc,
    Sqrt,
    Smoothstep,
    Rand,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum FloatBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Min,
    Max,
    /// The left operand unless it is NaN, otherwise the right.
    ValueOr,
    /// Angle in radians: left is y, right is x.
    Atan2,
    /// Left raised to the right.
    Power,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntUnary {
    Negate,
    Abs,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IntBinary {
    Add,
    Subtract,
    Multiply,
    Remainder,
    /// Floored quotient; division by zero is zero.
    FloorDivide,
    Min,
    Max,
}

/// Which operands of a float instruction are colors, read channel by
/// channel; the others are floats every channel reads. Its destination is a
/// color when any operand is.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub struct Colors(u8);

impl Colors {
    /// Every operand a float.
    pub const NONE: Self = Self(0);

    /// Operand `index`, counting from 0, is a color.
    pub const fn with(self, index: u8) -> Self {
        Self(self.0 | 1 << index)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    pub fn is_color(self, index: u8) -> bool {
        self.0 & 1 << index != 0
    }

    /// Channels the instruction runs: three for a color, else one.
    #[inline(always)]
    pub fn channels(self) -> u16 {
        if self.0 == 0 { 1 } else { 3 }
    }

    /// The slot channel `channel` reads of operand `index`.
    #[inline(always)]
    pub fn operand(self, slot: Slot, index: u8, channel: u16) -> Slot {
        if self.is_color(index) {
            slot.channel(channel)
        } else {
            slot
        }
    }

    /// The bank of operand `index`, or of the destination for `None`.
    pub fn bank(self, index: Option<u8>) -> Bank {
        let color = match index {
            Some(index) => self.is_color(index),
            None => self.0 != 0,
        };
        if color { Bank::Color } else { Bank::Float }
    }
}

/// A comparison. Bools compare only for equality.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

/// A float of a whole color.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ColorComponent {
    Hue,
    Saturation,
    Intensity,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum CurveOp {
    /// The curve at a float position.
    Sample,
    /// Its area from position 0 to a float position.
    Integral,
    /// The first position where it reaches a float value.
    Crossing,
    /// The last such position before a float position.
    LastCrossing,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MarkOp {
    /// Float: the mark at an int index.
    At,
    /// Float: the latest mark at or before float seconds.
    Last,
    /// Int: its index.
    LastIndex,
}

/// A resource kind.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Resource {
    Curve,
    Gradient,
    Marks,
    Array,
}

impl Resource {
    pub fn for_type(ty: &Type) -> Option<Self> {
        match ty {
            Type::Curve => Some(Self::Curve),
            Type::Gradient => Some(Self::Gradient),
            Type::Marks => Some(Self::Marks),
            Type::Array(_) => Some(Self::Array),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Reducer {
    Max,
    Min,
    Sum,
    Any,
    All,
    First,
    Last,
}

/// Coordinate domain of a signal query. Local indices stay within the current
/// fixture. A
/// shifted query reads the current pixel's local index plus an offset, so a
/// strip-wide offset reads one shifted run of pixels; its edges say what it
/// reads past the fixture's ends.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum SignalPixel<T> {
    Current,
    Local(T),
    Shifted(T, Edges),
}

/// Which way a scan carries its value along each fixture's pixels.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Direction {
    Forward,
    Backward,
}

/// What a shifted read reads past its fixture's ends.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Edges {
    /// Black, so a neighborhood reduction skips the missing neighbors.
    Skip,
    /// The end pixel.
    Extend,
    /// The pixels reflected about the end pixel, which is not repeated.
    Mirror,
}

impl Edges {
    /// The local index read at local index `index` of a fixture of `count`
    /// pixels, or `None` past its ends when skipping.
    #[inline(always)]
    pub fn local(self, index: i32, count: i32) -> Option<i32> {
        let last = count - 1;
        if (0..count).contains(&index) {
            return Some(index);
        }
        match self {
            Self::Skip => None,
            Self::Extend => Some(index.clamp(0, last.max(0))),
            Self::Mirror => {
                let reflected = if index < 0 { -index } else { 2 * last - index };
                Some(reflected.clamp(0, last.max(0)))
            }
        }
    }
}

impl<T> SignalPixel<T> {
    pub fn map<U>(self, mut map: impl FnMut(T) -> U) -> SignalPixel<U> {
        match self {
            Self::Current => SignalPixel::Current,
            Self::Local(index) => SignalPixel::Local(map(index)),
            Self::Shifted(offset, edges) => SignalPixel::Shifted(map(offset), edges),
        }
    }

    pub fn index_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Current => None,
            Self::Local(index) | Self::Shifted(index, _) => Some(index),
        }
    }

    pub fn index(&self) -> Option<&T> {
        match self {
            Self::Current => None,
            Self::Local(index) | Self::Shifted(index, _) => Some(index),
        }
    }
}

/// One instruction. Operands name slots of the banks noted beside them. Each
/// takes one [`InstructionWord`], except a reduction and a stencil, which
/// take two; lengths of nested code count words.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Instruction {
    /// A scalar from bits: a float's, an int, a bool as 0 or 1, or the
    /// program's resource constant at that index.
    Const { bank: Bank, dst: Slot, bits: u32 },
    /// Bound parameter `index` of its storage (see [`ParamStorage`]): a word
    /// of a float, int, bool or color parameter, or a resource. An enum
    /// parameter is an int, its option's index in the program's names.
    Param { bank: Bank, dst: Slot, index: u16 },
    /// A scalar of the query or target, of its read's bank.
    Context { dst: Slot, read: ContextRead },
    /// Floats.
    FloatUnary { op: FloatUnary, dst: Slot, a: Slot },
    FloatBinary {
        op: FloatBinary,
        colors: Colors,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    Clamp {
        dst: Slot,
        value: Slot,
        min: Slot,
        max: Slot,
    },
    Mix {
        colors: Colors,
        dst: Slot,
        a: Slot,
        b: Slot,
        amount: Slot,
    },
    /// Ints.
    IntUnary { op: IntUnary, dst: Slot, a: Slot },
    IntBinary {
        op: IntBinary,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Float from int.
    IntToFloat { dst: Slot, a: Slot },
    /// Int from float: truncated, saturated, NaN to zero.
    FloatToInt { dst: Slot, a: Slot },
    /// Bool from two floats, ints or bools.
    Compare {
        op: CompareOp,
        bank: Bank,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Float from color.
    ColorComponent {
        op: ColorComponent,
        dst: Slot,
        color: Slot,
    },
    /// Color from floats.
    Hsv {
        dst: Slot,
        hue: Slot,
        saturation: Slot,
        value: Slot,
    },
    /// Color with its hue replaced by a float, or shifted by it when `shift`;
    /// its saturation and value stay.
    Recolor {
        shift: bool,
        dst: Slot,
        color: Slot,
        hue: Slot,
    },
    /// Float: a query of a curve resource with a float `value`, and for the
    /// last crossing the float position `before`.
    Curve {
        op: CurveOp,
        dst: Slot,
        curve: Slot,
        value: Slot,
        before: Slot,
    },
    /// Color: a gradient resource at a float position, scaled by a float
    /// clamped to the unit range when `scale` is present.
    Gradient {
        dst: Slot,
        gradient: Slot,
        position: Slot,
        scale: Slot,
    },
    /// A query of a marks resource; `operand` is an int index or float seconds.
    Mark {
        op: MarkOp,
        dst: Slot,
        marks: Slot,
        operand: Slot,
    },
    /// Int: the length of a marks or array resource of `kind`.
    Length {
        kind: Resource,
        dst: Slot,
        resource: Slot,
    },
    /// An array resource's item at a clamped int index, in `bank`; `default`
    /// when the array is empty.
    Index {
        bank: Bank,
        dst: Slot,
        array: Slot,
        index: Slot,
        default: Slot,
    },
    /// A choice by bool condition.
    Select {
        bank: Bank,
        dst: Slot,
        condition: Slot,
        yes: Slot,
        no: Slot,
    },
    /// A copy.
    Move { bank: Bank, dst: Slot, src: Slot },
    /// Int: the pixel's section index for an int width, or with `!index` the
    /// section count.
    Section { index: bool, dst: Slot, width: Slot },
    /// Color: an operator input, or the node's own previous frame as
    /// [`STATE_INPUT`], at this frame and a pixel.
    Sample {
        dst: Slot,
        input: u16,
        pixel: SignalPixel<Slot>,
    },
    /// A table, in a query or target block: for each index of `start..end`,
    /// which spans at most a strip, the next `len` words run as scalar code
    /// with `index` set, and `value`, of `bank`, lands in the row `dst` at the
    /// index less `start`. With `prefix`, entry `k` is instead the sum of the
    /// values before index `start + k`, through `end`, so the range spans less
    /// than a strip.
    Tabulate {
        bank: Bank,
        prefix: bool,
        dst: Slot,
        index: Slot,
        start: Slot,
        end: Slot,
        value: Slot,
        len: u8,
    },
    /// The entry of the table row `table`, of `bank`, at the int `index` less
    /// `start`.
    Lookup {
        bank: Bank,
        dst: Slot,
        table: Slot,
        index: Slot,
        start: Slot,
    },
    /// The next `then_len` words run where a bool condition holds, the
    /// `else_len` after them where it does not.
    Branch {
        condition: Slot,
        then_len: u16,
        else_len: u16,
    },
    /// A reduction into `acc` of `bank`, which holds its identity or default.
    /// Each iteration runs the next `loop_len` words with `index` set, then,
    /// where the bool `filter` holds (or always when it is absent), the
    /// `contribute_len` after them, and combines `value` into `acc`. A scalar
    /// `index` with per-pixel bounds is shared: iterations run over the union
    /// of the pixels' ranges, and each pixel takes part in its own.
    Reduce {
        reducer: Reducer,
        bank: Bank,
        acc: Slot,
        index: Slot,
        start: Slot,
        end: Slot,
        filter: Slot,
        value: Slot,
        loop_len: u16,
        contribute_len: u16,
    },
    /// A neighborhood reduction into the color `acc`, which holds black: for
    /// each pixel, the sum or max over shared offsets `o` in its `start..end`
    /// of the input `o` pixels along its fixture, read past its ends as
    /// `edges` say, scaled by the product of the float `weight` there,
    /// `scale` and the pixel's `pixel`. Skipped neighbors, and neighbors of
    /// zero weight, contribute nothing.
    ///
    /// The next `source_len` words are the source block: a `Sample` at the
    /// current pixel names the input, then code computing `weight` from that
    /// sample alone. The block runs over the strip's neighborhood, not its
    /// pixels. The `tap_len` after it run once per offset with `index` set
    /// and compute the scalar `scale`.
    Stencil {
        reducer: Reducer,
        edges: Edges,
        acc: Slot,
        index: Slot,
        start: Slot,
        end: Slot,
        weight: Slot,
        scale: Slot,
        pixel: Slot,
        source_len: u16,
        tap_len: u16,
    },
    /// Color: a trail along each fixture in `direction`. From black at the
    /// fixture's first pixel in that order, each pixel's value is the
    /// previous pixel's times the float `decay`, plus the input there times
    /// `weight`. The scan computes every pixel of the frame once per query
    /// into frame cache `cache`.
    ///
    /// The next `source_len` words are its source block, as a stencil's: a
    /// `Sample` at the current pixel names the input, then code computing
    /// `weight` from that sample alone.
    Scan {
        direction: Direction,
        dst: Slot,
        decay: Slot,
        weight: Slot,
        cache: u8,
        source_len: u16,
    },
}

/// The input a [`Instruction::Sample`] of the node's own previous frame reads:
/// an operator's own last output, or an effect's layer's.
pub const STATE_INPUT: u16 = u16::MAX;

#[derive(Clone, Debug, PartialEq)]
pub struct BytecodeProgram {
    /// The types of the parameters an invocation binds, in slot order.
    pub params: Box<[Type]>,
    pub code: Box<[InstructionWord]>,
    /// The query block is `code[..query_end]`, the target block runs to
    /// `target_end`, and the body follows.
    pub query_end: u16,
    pub target_end: u16,
    /// The color result.
    pub result: Slot,
    pub scalars: Banks,
    pub rows: Banks,
    /// Most selections open at once in the body.
    pub depth: u16,
    /// Constant curves, gradients, marks and arrays.
    pub resources: Box<[Value]>,
    /// Every option of the enums the program reads; enum values are indices.
    pub enums: Box<[Identifier]>,
    /// Whole-frame scans; each keeps its frame while the node runs.
    pub scans: u16,
}

/// A parameter's bound bank.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParameterKind {
    Void,
    Int,
    Float,
    Bool,
    Color,
    Marks,
    Curve,
    Gradient,
    Enum,
    Array,
}

impl ParameterKind {
    pub fn for_type(ty: &Type) -> Self {
        match ty {
            Type::Void => Self::Void,
            Type::Int => Self::Int,
            Type::Float => Self::Float,
            Type::Bool => Self::Bool,
            Type::Color => Self::Color,
            Type::Marks => Self::Marks,
            Type::Curve => Self::Curve,
            Type::Gradient => Self::Gradient,
            Type::Enum(_) => Self::Enum,
            Type::Array(_) => Self::Array,
        }
    }

    pub fn storage(self) -> Option<ParamStorage> {
        match self {
            Self::Void => None,
            Self::Int | Self::Float | Self::Bool | Self::Color | Self::Enum => {
                Some(ParamStorage::Word)
            }
            Self::Marks | Self::Curve | Self::Gradient | Self::Array => {
                Some(ParamStorage::Resource)
            }
        }
    }
}

/// Where a bound parameter lives. Each storage numbers its parameters in
/// declaration order; a void parameter has no storage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParamStorage {
    /// One 32-bit word: float bits, an int, a bool, a color's index in the
    /// image or an enum option's index in the program's names.
    Word,
    /// A curve, gradient, marks or array.
    Resource,
}

/// Parameter `index`'s bank within its storage.
pub fn param_bank(params: &[Type], index: usize) -> u16 {
    let storage = ParameterKind::for_type(&params[index]).storage();
    params[..index]
        .iter()
        .filter(|ty| ParameterKind::for_type(ty).storage() == storage)
        .count() as u16
}

/// How an instruction uses a slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Read,
    Write,
    /// A reduction's accumulator, read and written by every iteration.
    Update,
}

/// How an instruction's slots are read and written, for allocation: each
/// destination and operand with its bank.
pub struct Operands {
    pub dst: Option<(Bank, Slot)>,
    pub reads: Vec<(Bank, Slot)>,
}

impl Instruction {
    /// Visit every slot field with its bank and access, skipping absent
    /// optional operands.
    pub fn visit_slots(&mut self, visit: &mut impl FnMut(Bank, &mut Slot, Access)) {
        use Access::{Read, Update, Write};
        use Bank::*;
        use Instruction as I;
        let mut v = |bank, slot: &mut Slot, access| {
            if !slot.is_none() {
                visit(bank, slot, access);
            }
        };
        match self {
            I::Const { bank, dst, .. } | I::Param { bank, dst, .. } => v(*bank, dst, Write),
            I::Context { dst, read } => v(read.bank(), dst, Write),
            I::FloatUnary { dst, a, .. } => {
                v(Float, a, Read);
                v(Float, dst, Write);
            }
            I::FloatBinary {
                colors, dst, a, b, ..
            } => {
                v(colors.bank(Some(0)), a, Read);
                v(colors.bank(Some(1)), b, Read);
                v(colors.bank(None), dst, Write);
            }
            I::Mix {
                colors,
                dst,
                a,
                b,
                amount,
            } => {
                v(colors.bank(Some(0)), a, Read);
                v(colors.bank(Some(1)), b, Read);
                v(colors.bank(Some(2)), amount, Read);
                v(colors.bank(None), dst, Write);
            }
            I::Clamp {
                dst,
                value: a,
                min: b,
                max: c,
            } => {
                v(Float, a, Read);
                v(Float, b, Read);
                v(Float, c, Read);
                v(Float, dst, Write);
            }
            I::IntUnary { dst, a, .. } => {
                v(Int, a, Read);
                v(Int, dst, Write);
            }
            I::IntBinary { dst, a, b, .. } => {
                v(Int, a, Read);
                v(Int, b, Read);
                v(Int, dst, Write);
            }
            I::IntToFloat { dst, a } => {
                v(Int, a, Read);
                v(Float, dst, Write);
            }
            I::FloatToInt { dst, a } => {
                v(Float, a, Read);
                v(Int, dst, Write);
            }
            I::Compare {
                bank, dst, a, b, ..
            } => {
                v(*bank, a, Read);
                v(*bank, b, Read);
                v(Bool, dst, Write);
            }
            I::ColorComponent { dst, color, .. } => {
                v(Color, color, Read);
                v(Float, dst, Write);
            }
            I::Recolor {
                dst, color, hue, ..
            } => {
                v(Color, color, Read);
                v(Float, hue, Read);
                v(Color, dst, Write);
            }
            I::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => {
                v(Float, hue, Read);
                v(Float, saturation, Read);
                v(Float, value, Read);
                v(Color, dst, Write);
            }
            I::Curve {
                dst,
                curve,
                value,
                before,
                ..
            } => {
                v(Resource, curve, Read);
                v(Float, value, Read);
                v(Float, before, Read);
                v(Float, dst, Write);
            }
            I::Gradient {
                dst,
                gradient,
                position,
                scale,
            } => {
                v(Resource, gradient, Read);
                v(Float, position, Read);
                v(Float, scale, Read);
                v(Color, dst, Write);
            }
            I::Mark {
                op,
                dst,
                marks,
                operand,
            } => {
                v(Resource, marks, Read);
                match op {
                    MarkOp::At => {
                        v(Int, operand, Read);
                        v(Float, dst, Write);
                    }
                    MarkOp::Last => {
                        v(Float, operand, Read);
                        v(Float, dst, Write);
                    }
                    MarkOp::LastIndex => {
                        v(Float, operand, Read);
                        v(Int, dst, Write);
                    }
                }
            }
            I::Length { dst, resource, .. } => {
                v(Resource, resource, Read);
                v(Int, dst, Write);
            }
            I::Index {
                bank,
                dst,
                array,
                index,
                default,
            } => {
                v(Resource, array, Read);
                v(Int, index, Read);
                v(*bank, default, Read);
                v(*bank, dst, Write);
            }
            I::Select {
                bank,
                dst,
                condition,
                yes,
                no,
            } => {
                v(Bool, condition, Read);
                v(*bank, yes, Read);
                v(*bank, no, Read);
                v(*bank, dst, Write);
            }
            I::Move { bank, dst, src } => {
                v(*bank, src, Read);
                v(*bank, dst, Write);
            }
            I::Section { dst, width, .. } => {
                v(Int, width, Read);
                v(Int, dst, Write);
            }
            I::Sample { dst, pixel, .. } => {
                if let Some(index) = pixel.index_mut() {
                    v(Int, index, Read);
                }
                v(Color, dst, Write);
            }
            I::Tabulate {
                bank,
                dst,
                index,
                start,
                end,
                value,
                ..
            } => {
                v(Int, start, Read);
                v(Int, end, Read);
                v(Int, index, Write);
                v(*bank, value, Read);
                v(*bank, dst, Write);
            }
            I::Lookup {
                bank,
                dst,
                table,
                index,
                start,
            } => {
                v(*bank, table, Read);
                v(Int, index, Read);
                v(Int, start, Read);
                v(*bank, dst, Write);
            }
            I::Branch { condition, .. } => v(Bool, condition, Read),
            I::Reduce {
                bank,
                acc,
                index,
                start,
                end,
                filter,
                value,
                ..
            } => {
                v(Int, start, Read);
                v(Int, end, Read);
                v(Int, index, Write);
                v(Bool, filter, Read);
                v(*bank, value, Read);
                v(*bank, acc, Update);
            }
            I::Scan {
                dst, decay, weight, ..
            } => {
                v(Float, decay, Read);
                v(Float, weight, Read);
                v(Color, dst, Write);
            }
            I::Stencil {
                acc,
                index,
                start,
                end,
                weight,
                scale,
                pixel,
                ..
            } => {
                v(Int, start, Read);
                v(Int, end, Read);
                v(Int, index, Write);
                v(Float, weight, Read);
                v(Float, scale, Read);
                v(Float, pixel, Read);
                v(Color, acc, Update);
            }
        }
    }

    /// The destination and operands, with their banks; a reduction's index is
    /// left out and its accumulator is both.
    pub fn operands(&self) -> Operands {
        let mut operands = Operands {
            dst: None,
            reads: Vec::new(),
        };
        let mut copy = self.clone();
        let reduction = matches!(self, Self::Reduce { .. } | Self::Stencil { .. });
        copy.visit_slots(&mut |bank, slot, access| match access {
            Access::Read => operands.reads.push((bank, *slot)),
            Access::Write if !reduction => operands.dst = Some((bank, *slot)),
            Access::Write => {}
            Access::Update => {
                operands.reads.push((bank, *slot));
                operands.dst = Some((bank, *slot));
            }
        });
        operands
    }

    /// Words of code this instruction takes.
    #[inline(always)]
    pub fn width(&self) -> usize {
        match self {
            Self::Reduce { .. } | Self::Stencil { .. } => 2,
            _ => 1,
        }
    }

    /// Words of code nested in this one, in order.
    pub fn nested(&self) -> u32 {
        match *self {
            Self::Branch {
                then_len, else_len, ..
            } => u32::from(then_len) + u32::from(else_len),
            Self::Reduce {
                loop_len,
                contribute_len,
                ..
            } => u32::from(loop_len) + u32::from(contribute_len),
            Self::Stencil {
                source_len,
                tap_len,
                ..
            } => u32::from(source_len) + u32::from(tap_len),
            Self::Scan { source_len, .. } => u32::from(source_len),
            Self::Tabulate { len, .. } => u32::from(len),
            _ => 0,
        }
    }
}

/// The instructions of `code`, in order.
pub fn instructions(code: &[InstructionWord]) -> impl Iterator<Item = Instruction> + '_ {
    let mut at = 0;
    core::iter::from_fn(move || {
        let rest = code.get(at..).filter(|rest| !rest.is_empty())?;
        let instruction = InstructionWord::decode(rest);
        at += instruction.width();
        Some(instruction)
    })
}

impl BytecodeProgram {
    /// The query block's code, run once per query.
    pub fn query(&self) -> &[InstructionWord] {
        &self.code[..usize::from(self.query_end)]
    }

    /// The target block's code, run again for each target shape.
    pub fn target(&self) -> &[InstructionWord] {
        &self.code[usize::from(self.query_end)..usize::from(self.target_end)]
    }

    pub fn body(&self) -> &[InstructionWord] {
        &self.code[usize::from(self.target_end)..]
    }

    /// Every instruction, in order.
    pub fn instructions(&self) -> impl Iterator<Item = Instruction> + '_ {
        instructions(&self.code)
    }

    /// Whether the result varies by pixel.
    pub fn uses_pixel_context(&self) -> bool {
        !self.result.is_scalar()
    }

    fn reads(&self, read: impl Fn(&Instruction) -> bool) -> bool {
        self.instructions().any(|instruction| read(&instruction))
    }

    /// Reads pixel positions or target bounds.
    pub fn uses_spatial_context(&self) -> bool {
        self.reads(|instruction| {
            matches!(instruction, Instruction::Context { read, .. } if read.reads_geometry())
                || instruction.operands().reads.iter().any(|(_, slot)| {
                    matches!(slot.kind(), SlotKind::Input(Input::PixelX | Input::PixelY))
                })
        })
    }

    /// Reads the target's pixel count or bounds, so strips must not mix them.
    /// Neighbors read past a fixture's ends depend on its pixel count.
    pub fn reads_target(&self) -> bool {
        self.reads(|instruction| match instruction {
            Instruction::Context { read, .. } => read.reads_target(),
            Instruction::Stencil { edges, .. }
            | Instruction::Sample {
                pixel: SignalPixel::Shifted(_, edges),
                ..
            } => *edges != Edges::Skip,
            _ => false,
        })
    }

    /// Reads its own previous frame, or its layer's.
    pub fn reads_state(&self) -> bool {
        self.reads(|instruction| {
            matches!(
                instruction,
                Instruction::Sample {
                    input: STATE_INPUT,
                    ..
                }
            )
        })
    }

    pub fn uses_sections(&self) -> bool {
        self.reads(|instruction| matches!(instruction, Instruction::Section { .. }))
    }

    /// Whether its body holds a neighborhood reduction.
    pub fn has_stencil(&self) -> bool {
        instructions(self.body())
            .any(|instruction| matches!(instruction, Instruction::Stencil { .. }))
    }

    /// Whole-frame scans, each keeping its frame while the node runs.
    pub fn scan_count(&self) -> usize {
        usize::from(self.scans)
    }
}

/// One instruction per line, nested code indented, for inspecting lowered
/// programs.
pub fn listing(program: &BytecodeProgram) -> String {
    use core::fmt::Write;
    let mut text = String::new();
    // Words left in each open construct.
    let mut open: Vec<u32> = Vec::new();
    let mut ip = 0;
    for instruction in program.instructions() {
        if ip == usize::from(program.query_end) {
            let _ = writeln!(text, "target:");
        }
        if ip == usize::from(program.target_end) {
            let _ = writeln!(text, "body:");
        }
        let indent = "  ".repeat(open.len() + 1);
        let _ = writeln!(text, "{ip:>4}{indent}{instruction:?}");
        let width = instruction.width() as u32;
        ip += instruction.width();
        for remaining in &mut open {
            *remaining -= width;
        }
        open.retain(|&remaining| remaining != 0);
        if instruction.nested() != 0 {
            open.push(instruction.nested());
        }
    }
    let _ = writeln!(text, "result {:?}", program.result);
    text
}
