//! Portable strip bytecode. A program runs over strips of up to [`STRIP`]
//! pixels: its query and target blocks once, as scalar code, then its body
//! once per strip. A value is a scalar or a per-pixel row; which one is fixed
//! when the program is compiled. Control flow is structured: a branch's arms
//! and a reduction's parts follow their instruction inline.
use super::types::{Identifier, Type, Value};
use crate::Shared as Arc;
use crate::values::{Color, Curve, Gradient, Marks};
use alloc::{boxed::Box, vec::Vec};

/// Pixels per strip. A power of two, so a selection index masked to it is in
/// range of every row.
pub const STRIP: usize = 128;
/// Most selections a program may hold open at once.
pub const MAX_DEPTH: u16 = 16;
/// Most bytes of rows per pixel a program may use.
pub const MAX_ROW_BYTES: u32 = 192;
/// Most iterations of one reduction.
pub const MAX_ITERATIONS: i32 = super::MAX_DSL_LOOP_ITERATIONS as i32;

const ROW: u16 = 1 << 15;
const INPUT: u16 = 1 << 14;
const INDEX: u16 = INPUT - 1;

/// An operand of the bank its instruction implies: a scalar, a per-pixel row,
/// or one of the strip's pixel inputs.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
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
    const ALL: [Self; 4] = [
        Self::PixelIndex,
        Self::PixelFraction,
        Self::PixelX,
        Self::PixelY,
    ];

    fn bank(self) -> Bank {
        match self {
            Self::PixelIndex => Bank::Int,
            _ => Bank::Float,
        }
    }
}

impl Slot {
    /// An absent optional operand.
    pub const NONE: Self = Self(u16::MAX);
    /// Most slots of one kind in one bank.
    pub const LIMIT: u16 = INDEX;

    pub const fn scalar(index: u16) -> Self {
        Self(index & INDEX)
    }
    pub const fn row(index: u16) -> Self {
        Self(ROW | (index & INDEX))
    }
    pub const fn input(input: Input) -> Self {
        Self(
            INPUT
                | match input {
                    Input::PixelIndex => 0,
                    Input::PixelFraction => 1,
                    Input::PixelX => 2,
                    Input::PixelY => 3,
                },
        )
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
}

/// A slot bank.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum Bank {
    Float,
    /// Ints, and enum options as indices into the program's names.
    Int,
    Bool,
    Color,
    /// Curves, gradients, marks and arrays, by reference.
    Resource,
}

impl Bank {
    pub const ALL: [Self; 5] = [
        Self::Float,
        Self::Int,
        Self::Bool,
        Self::Color,
        Self::Resource,
    ];

    pub fn for_type(ty: &Type) -> Self {
        match ty {
            Type::Float => Self::Float,
            Type::Int | Type::Enum(_) => Self::Int,
            Type::Bool => Self::Bool,
            Type::Color => Self::Color,
            _ => Self::Resource,
        }
    }

    /// Bytes of one pixel of a row.
    pub fn row_bytes(self) -> u32 {
        match self {
            Self::Float | Self::Int => 4,
            Self::Bool => 1,
            Self::Color => 3,
            Self::Resource => 6,
        }
    }
}

/// Slot counts of each bank.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    Hash,
    PartialEq,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
pub struct Banks {
    pub floats: u16,
    pub ints: u16,
    pub bools: u16,
    pub colors: u16,
    pub resources: u16,
}

impl Banks {
    pub fn get(&self, bank: Bank) -> u16 {
        match bank {
            Bank::Float => self.floats,
            Bank::Int => self.ints,
            Bank::Bool => self.bools,
            Bank::Color => self.colors,
            Bank::Resource => self.resources,
        }
    }

    pub fn get_mut(&mut self, bank: Bank) -> &mut u16 {
        match bank {
            Bank::Float => &mut self.floats,
            Bank::Int => &mut self.ints,
            Bank::Bool => &mut self.bools,
            Bank::Color => &mut self.colors,
            Bank::Resource => &mut self.resources,
        }
    }

    /// Row bytes per pixel.
    pub fn row_bytes(&self) -> u32 {
        Bank::ALL
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
            colors: self.colors.max(other.colors),
            resources: self.resources.max(other.resources),
        }
    }
}

/// Values the strip's context supplies once.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ContextRead {
    /// Floats of the query.
    Seconds,
    Progress,
    Duration,
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
            Self::PixelCount => Bank::Int,
            _ => Bank::Float,
        }
    }

    pub fn reads_target(self) -> bool {
        !matches!(self, Self::Seconds | Self::Progress | Self::Duration)
    }

    pub fn reads_geometry(self) -> bool {
        matches!(
            self,
            Self::TargetMinX | Self::TargetMinY | Self::TargetMaxX | Self::TargetMaxY
        )
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum FloatUnary {
    Negate,
    Sin,
    Cos,
    Abs,
    Floor,
    Ceil,
    Trunc,
    /// Rounds to the nearest integer, ties to even.
    RoundEven,
    Sqrt,
    Smoothstep,
    Rand,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
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
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum IntBinary {
    Add,
    Subtract,
    Multiply,
    Remainder,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ColorBinary {
    Add,
    Multiply,
    Max,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ColorComponent {
    Hue,
    Saturation,
    Intensity,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum MarkOp {
    /// Int: the number of marks.
    Count,
    /// Float: the mark at an int index.
    At,
    /// Float: the latest mark at or before float seconds.
    Last,
    /// Int: its index.
    LastIndex,
}

/// A resource kind.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
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

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum Reducer {
    Max,
    Min,
    Sum,
    Any,
    All,
    First,
    Last,
}

/// Coordinate domain of a signal query. Global indices use the prepared rig's
/// full color-pixel order; local indices stay within the current fixture.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum SignalPixel<T> {
    Current,
    Local(T),
    Global(T),
}

impl<T> SignalPixel<T> {
    pub fn map<U>(self, mut map: impl FnMut(T) -> U) -> SignalPixel<U> {
        match self {
            Self::Current => SignalPixel::Current,
            Self::Local(index) => SignalPixel::Local(map(index)),
            Self::Global(index) => SignalPixel::Global(map(index)),
        }
    }

    pub fn index(&self) -> Option<&T> {
        match self {
            Self::Current => None,
            Self::Local(index) | Self::Global(index) => Some(index),
        }
    }
}

/// A run of the program's operand pool.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct Span {
    pub start: u16,
    pub len: u16,
}

impl Span {
    pub fn range(self) -> core::ops::Range<usize> {
        let start = usize::from(self.start);
        start..start + usize::from(self.len)
    }
}

/// One instruction. Operands name slots of the banks noted beside them.
#[derive(Clone, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Instruction {
    /// Float scalar from bits.
    FloatConst {
        dst: Slot,
        bits: u32,
    },
    IntConst {
        dst: Slot,
        value: i32,
    },
    BoolConst {
        dst: Slot,
        value: bool,
    },
    ColorConst {
        dst: Slot,
        value: Color,
    },
    /// A resource of the program's pool of its kind.
    ResourceConst {
        dst: Slot,
        kind: Resource,
        index: u16,
    },
    /// Parameter `bank` of the bound bank of `dst`'s kind: float, int, bool
    /// and color parameters.
    FloatParam {
        dst: Slot,
        bank: u16,
    },
    IntParam {
        dst: Slot,
        bank: u16,
    },
    BoolParam {
        dst: Slot,
        bank: u16,
    },
    ColorParam {
        dst: Slot,
        bank: u16,
    },
    /// Int: the bound option's index in the program's names.
    EnumParam {
        dst: Slot,
        bank: u16,
    },
    ResourceParam {
        dst: Slot,
        kind: Resource,
        bank: u16,
    },
    /// A scalar of the query or target, of its read's bank.
    Context {
        dst: Slot,
        read: ContextRead,
    },
    /// Floats.
    FloatUnary {
        op: FloatUnary,
        dst: Slot,
        a: Slot,
    },
    FloatBinary {
        op: FloatBinary,
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
        dst: Slot,
        a: Slot,
        b: Slot,
        amount: Slot,
    },
    /// Float base multiplied by itself an int number of times.
    Power {
        dst: Slot,
        base: Slot,
        count: Slot,
    },
    /// A fused source's clock at float seconds: its quantized seconds, or
    /// its progress; NaN outside the sequence.
    Clock {
        progress: bool,
        dst: Slot,
        seconds: Slot,
    },
    /// Ints.
    IntNegate {
        dst: Slot,
        a: Slot,
    },
    IntBinary {
        op: IntBinary,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Float from int.
    IntToFloat {
        dst: Slot,
        a: Slot,
    },
    /// Int from float: truncated, saturated, NaN to zero.
    FloatToInt {
        dst: Slot,
        a: Slot,
    },
    /// Bools.
    Not {
        dst: Slot,
        a: Slot,
    },
    /// Bool from floats.
    FloatCompare {
        op: CompareOp,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Bool from ints.
    IntCompare {
        op: CompareOp,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Bool from two values of a primitive bank.
    Equal {
        bank: Bank,
        negate: bool,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Colors.
    ColorBinary {
        op: ColorBinary,
        dst: Slot,
        a: Slot,
        b: Slot,
    },
    /// Color by float.
    ColorScale {
        dst: Slot,
        color: Slot,
        scale: Slot,
    },
    /// Colors by float.
    MixColor {
        dst: Slot,
        a: Slot,
        b: Slot,
        amount: Slot,
    },
    /// Float from color.
    ColorComponent {
        op: ColorComponent,
        dst: Slot,
        color: Slot,
    },
    Invert {
        dst: Slot,
        color: Slot,
    },
    /// Color from floats.
    Rgb {
        dst: Slot,
        red: Slot,
        green: Slot,
        blue: Slot,
    },
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
    /// Float: a curve resource at a float position.
    CurveSample {
        dst: Slot,
        curve: Slot,
        position: Slot,
    },
    /// The sample clamped between floats.
    CurveClamped {
        dst: Slot,
        curve: Slot,
        position: Slot,
        min: Slot,
        max: Slot,
    },
    /// Float: the first position where the curve reaches a float value, or
    /// the last before a float position when `before` is present.
    CurveCrossing {
        dst: Slot,
        curve: Slot,
        value: Slot,
        before: Slot,
    },
    /// Color: a gradient resource at a float position.
    GradientSample {
        dst: Slot,
        gradient: Slot,
        position: Slot,
    },
    /// The sample scaled by a float clamped to the unit range.
    GradientScaled {
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
    /// Int: an array resource's length.
    Len {
        dst: Slot,
        array: Slot,
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
    /// One of the operand pool's `items` at a clamped int index.
    Pick {
        bank: Bank,
        dst: Slot,
        index: Slot,
        items: Span,
    },
    /// A choice by bool condition.
    Select {
        bank: Bank,
        dst: Slot,
        condition: Slot,
        yes: Slot,
        no: Slot,
    },
    Move {
        bank: Bank,
        dst: Slot,
        src: Slot,
    },
    /// Ints: the pixel's section count, or index, for an int width.
    SectionCount {
        dst: Slot,
        width: Slot,
    },
    SectionIndex {
        dst: Slot,
        width: Slot,
    },
    /// Float: the position within the pixel's section, from a float width
    /// and its reciprocal.
    SectionPosition {
        dst: Slot,
        width: Slot,
        inverse: Slot,
    },
    /// Color: an operator input at float seconds and a pixel. A frame cache
    /// holds whole input frames of a query-uniform time.
    Sample {
        dst: Slot,
        input: u16,
        seconds: Slot,
        pixel: SignalPixel<Slot>,
        frame_cache: u16,
    },
    /// The next `then_len` instructions run where a bool condition holds, the
    /// `else_len` after them where it does not.
    Branch {
        condition: Slot,
        then_len: u16,
        else_len: u16,
    },
    /// A reduction into `acc` of `bank`, which holds its identity or default.
    /// Each iteration runs the next `loop_len` instructions with `index` set,
    /// then, where the bool `filter` holds (or always when it is absent), the
    /// `contribute_len` after them, and combines `value` into `acc`.
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
}

/// No frame cache.
pub const NO_FRAME_CACHE: u16 = u16::MAX;

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BytecodeProgram {
    pub code: Box<[Instruction]>,
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
    pub curves: Box<[Arc<Curve>]>,
    pub gradients: Box<[Arc<Gradient>]>,
    pub marks: Box<[Arc<Marks>]>,
    pub arrays: Box<[Arc<[Value]>]>,
    /// Every option of the enums the program reads; enum values are indices.
    pub enums: Box<[Identifier]>,
    /// Item lists of `Pick`.
    pub operands: Box<[Slot]>,
    pub frame_caches: u16,
}

/// What a program may read besides its parameters.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramContext {
    Effect,
    Operator { inputs: usize },
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
            Type::Void | Type::Signal => Self::Void,
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

    fn for_resource(kind: Resource) -> Self {
        match kind {
            Resource::Curve => Self::Curve,
            Resource::Gradient => Self::Gradient,
            Resource::Marks => Self::Marks,
            Resource::Array => Self::Array,
        }
    }
}

/// How an instruction uses a slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Read,
    Write,
    /// A reduction's accumulator, read and written by every iteration.
    Update,
}

/// How an instruction's slots are read and written, for checking and
/// allocation: each destination and operand with its bank.
pub struct Operands {
    pub dst: Option<(Bank, Slot)>,
    pub reads: Vec<(Bank, Slot)>,
}

impl Instruction {
    /// Visit every slot field with its bank and access, skipping absent
    /// optional operands. `Pick` items live in the program's operand pool.
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
            I::FloatConst { dst, .. } | I::FloatParam { dst, .. } => v(Float, dst, Write),
            I::IntConst { dst, .. } | I::IntParam { dst, .. } | I::EnumParam { dst, .. } => {
                v(Int, dst, Write)
            }
            I::BoolConst { dst, .. } | I::BoolParam { dst, .. } => v(Bool, dst, Write),
            I::ColorConst { dst, .. } | I::ColorParam { dst, .. } => v(Color, dst, Write),
            I::ResourceConst { dst, .. } | I::ResourceParam { dst, .. } => v(Resource, dst, Write),
            I::Context { dst, read } => v(read.bank(), dst, Write),
            I::FloatUnary { dst, a, .. }
            | I::Clock {
                dst, seconds: a, ..
            } => {
                v(Float, a, Read);
                v(Float, dst, Write);
            }
            I::FloatBinary { dst, a, b, .. } => {
                v(Float, a, Read);
                v(Float, b, Read);
                v(Float, dst, Write);
            }
            I::Clamp {
                dst,
                value: a,
                min: b,
                max: c,
            }
            | I::Mix {
                dst,
                a,
                b,
                amount: c,
            } => {
                v(Float, a, Read);
                v(Float, b, Read);
                v(Float, c, Read);
                v(Float, dst, Write);
            }
            I::Power { dst, base, count } => {
                v(Float, base, Read);
                v(Int, count, Read);
                v(Float, dst, Write);
            }
            I::IntNegate { dst, a } => {
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
            I::Not { dst, a } => {
                v(Bool, a, Read);
                v(Bool, dst, Write);
            }
            I::FloatCompare { dst, a, b, .. } => {
                v(Float, a, Read);
                v(Float, b, Read);
                v(Bool, dst, Write);
            }
            I::IntCompare { dst, a, b, .. } => {
                v(Int, a, Read);
                v(Int, b, Read);
                v(Bool, dst, Write);
            }
            I::Equal {
                bank, dst, a, b, ..
            } => {
                v(*bank, a, Read);
                v(*bank, b, Read);
                v(Bool, dst, Write);
            }
            I::ColorBinary { dst, a, b, .. } => {
                v(Color, a, Read);
                v(Color, b, Read);
                v(Color, dst, Write);
            }
            I::ColorScale { dst, color, scale } => {
                v(Color, color, Read);
                v(Float, scale, Read);
                v(Color, dst, Write);
            }
            I::MixColor { dst, a, b, amount } => {
                v(Color, a, Read);
                v(Color, b, Read);
                v(Float, amount, Read);
                v(Color, dst, Write);
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
            I::Invert { dst, color } => {
                v(Color, color, Read);
                v(Color, dst, Write);
            }
            I::Rgb {
                dst,
                red: a,
                green: b,
                blue: c,
            }
            | I::Hsv {
                dst,
                hue: a,
                saturation: b,
                value: c,
            } => {
                v(Float, a, Read);
                v(Float, b, Read);
                v(Float, c, Read);
                v(Color, dst, Write);
            }
            I::CurveSample {
                dst,
                curve,
                position,
            } => {
                v(Resource, curve, Read);
                v(Float, position, Read);
                v(Float, dst, Write);
            }
            I::CurveClamped {
                dst,
                curve,
                position,
                min,
                max,
            } => {
                v(Resource, curve, Read);
                v(Float, position, Read);
                v(Float, min, Read);
                v(Float, max, Read);
                v(Float, dst, Write);
            }
            I::CurveCrossing {
                dst,
                curve,
                value,
                before,
            } => {
                v(Resource, curve, Read);
                v(Float, value, Read);
                v(Float, before, Read);
                v(Float, dst, Write);
            }
            I::GradientSample {
                dst,
                gradient,
                position,
            } => {
                v(Resource, gradient, Read);
                v(Float, position, Read);
                v(Color, dst, Write);
            }
            I::GradientScaled {
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
                    MarkOp::Count => v(Int, dst, Write),
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
            I::Len { dst, array } => {
                v(Resource, array, Read);
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
            I::Pick {
                bank, dst, index, ..
            } => {
                v(Int, index, Read);
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
            I::SectionCount { dst, width } | I::SectionIndex { dst, width } => {
                v(Int, width, Read);
                v(Int, dst, Write);
            }
            I::SectionPosition {
                dst,
                width,
                inverse,
            } => {
                v(Float, width, Read);
                v(Float, inverse, Read);
                v(Float, dst, Write);
            }
            I::Sample {
                dst,
                seconds,
                pixel,
                ..
            } => {
                v(Float, seconds, Read);
                if let SignalPixel::Local(index) | SignalPixel::Global(index) = pixel {
                    v(Int, index, Read);
                }
                v(Color, dst, Write);
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
        }
    }

    /// The destination and operands, with their banks; a reduction's index is
    /// left out and its accumulator is both.
    pub fn operands(&self, pool: &[Slot]) -> Operands {
        let mut operands = Operands {
            dst: None,
            reads: Vec::new(),
        };
        let mut copy = self.clone();
        let reduction = matches!(self, Self::Reduce { .. });
        copy.visit_slots(&mut |bank, slot, access| match access {
            Access::Read => operands.reads.push((bank, *slot)),
            Access::Write if !reduction => operands.dst = Some((bank, *slot)),
            Access::Write => {}
            Access::Update => {
                operands.reads.push((bank, *slot));
                operands.dst = Some((bank, *slot));
            }
        });
        if let Self::Pick { bank, items, .. } = *self
            && let Some(items) = pool.get(items.range())
        {
            operands
                .reads
                .extend(items.iter().map(|&item| (bank, item)));
        }
        operands
    }

    /// Instructions nested in this one, in order.
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
            _ => 0,
        }
    }
}

impl BytecodeProgram {
    fn query(&self) -> &[Instruction] {
        &self.code[..usize::from(self.query_end)]
    }

    fn target(&self) -> &[Instruction] {
        &self.code[usize::from(self.query_end)..usize::from(self.target_end)]
    }

    pub fn body(&self) -> &[Instruction] {
        &self.code[usize::from(self.target_end)..]
    }

    /// The query and target blocks: the query block runs once per query and
    /// the target block again for each target shape.
    pub fn prefix(&self) -> (&[Instruction], &[Instruction]) {
        (self.query(), self.target())
    }

    /// Whether the result varies by pixel.
    pub fn uses_pixel_context(&self) -> bool {
        !self.result.is_scalar()
    }

    fn reads(&self, read: impl Fn(&Instruction) -> bool) -> bool {
        self.code.iter().any(read)
    }

    fn reads_input(&self, input: impl Fn(Input) -> bool) -> bool {
        self.code.iter().any(|instruction| {
            instruction
                .operands(&self.operands)
                .reads
                .iter()
                .any(|(_, slot)| matches!(slot.kind(), SlotKind::Input(read) if input(read)))
        }) || self
            .operands
            .iter()
            .any(|slot| matches!(slot.kind(), SlotKind::Input(read) if input(read)))
    }

    /// Reads pixel positions or target bounds.
    pub fn uses_spatial_context(&self) -> bool {
        self.reads_input(|input| matches!(input, Input::PixelX | Input::PixelY))
            || self.reads(|instruction| {
                matches!(instruction, Instruction::Context { read, .. } if read.reads_geometry())
            })
    }

    /// Reads the target's pixel count or bounds, so strips must not mix them.
    pub fn reads_target(&self) -> bool {
        self.reads(|instruction| {
            matches!(instruction, Instruction::Context { read, .. } if read.reads_target())
        })
    }

    pub fn uses_sections(&self) -> bool {
        self.reads(|instruction| {
            matches!(
                instruction,
                Instruction::SectionCount { .. }
                    | Instruction::SectionIndex { .. }
                    | Instruction::SectionPosition { .. }
            )
        })
    }

    pub fn uses_progress(&self) -> bool {
        self.reads(|instruction| {
            matches!(
                instruction,
                Instruction::Context {
                    read: ContextRead::Progress,
                    ..
                }
            )
        })
    }

    /// Whole-frame input caches the program's samples use.
    pub fn frame_cache_count(&self) -> usize {
        usize::from(self.frame_caches)
    }

    /// Whether every slot, length, pool index and parameter read is in range,
    /// rows and selections fit their limits, and `context` allows its reads.
    /// A well-formed program cannot index out of bounds when it runs; one
    /// whose resource kinds disagree produces empty values instead.
    pub fn is_well_formed(&self, context: ProgramContext, params: &[ParameterKind]) -> bool {
        let code_fits = u16::try_from(self.code.len()).is_ok();
        let blocks =
            self.query_end <= self.target_end && usize::from(self.target_end) <= self.code.len();
        if !code_fits
            || !blocks
            || self.rows.row_bytes() > MAX_ROW_BYTES
            || self.depth > MAX_DEPTH
            || !self.slot_fits(Bank::Color, self.result)
        {
            return false;
        }
        let bank_count = |kind: ParameterKind| params.iter().filter(|&&k| k == kind).count();
        let checker = Checker {
            program: self,
            context,
            bank_count: &bank_count,
        };
        let (query, target) = self.prefix();
        checker.prefix(query)
            && checker.prefix(target)
            && checker
                .block(self.body(), 0)
                .is_some_and(|depth| depth <= self.depth)
    }

    fn slot_fits(&self, bank: Bank, slot: Slot) -> bool {
        match slot.kind() {
            SlotKind::Scalar(index) => index < self.scalars.get(bank),
            SlotKind::Row(index) => index < self.rows.get(bank),
            SlotKind::Input(input) => input.bank() == bank,
        }
    }
}

struct Checker<'a> {
    program: &'a BytecodeProgram,
    context: ProgramContext,
    bank_count: &'a dyn Fn(ParameterKind) -> usize,
}

impl Checker<'_> {
    /// Scalar code without pixel queries.
    fn prefix(&self, code: &[Instruction]) -> bool {
        self.block(code, 0) == Some(0)
            && code.iter().all(|instruction| {
                !matches!(
                    instruction,
                    Instruction::Sample { .. }
                        | Instruction::SectionCount { .. }
                        | Instruction::SectionIndex { .. }
                        | Instruction::SectionPosition { .. }
                ) && {
                    let operands = instruction.operands(&self.program.operands);
                    operands
                        .dst
                        .iter()
                        .chain(&operands.reads)
                        .all(|(_, slot)| slot.is_scalar())
                }
            })
    }

    /// The selection depth a block reaches, if it is well formed.
    fn block(&self, code: &[Instruction], depth: u16) -> Option<u16> {
        let mut deepest = depth;
        let mut at = 0;
        while at < code.len() {
            let instruction = &code[at];
            if !self.instruction(instruction) {
                return None;
            }
            at += 1;
            let nested = usize::try_from(instruction.nested()).ok()?;
            let inner = code.get(at..at + nested)?;
            match *instruction {
                Instruction::Branch {
                    condition,
                    then_len,
                    ..
                } => {
                    let split = usize::from(then_len);
                    let opened = depth + u16::from(!condition.is_scalar());
                    deepest = deepest
                        .max(self.block(&inner[..split], opened)?)
                        .max(self.block(&inner[split..], opened)?);
                }
                Instruction::Reduce { acc, loop_len, .. } => {
                    let split = usize::from(loop_len);
                    let opened = depth + u16::from(!acc.is_scalar());
                    deepest = deepest
                        .max(self.block(&inner[..split], opened)?)
                        .max(self.block(&inner[split..], opened)?);
                }
                _ => {}
            }
            at += nested;
        }
        Some(deepest)
    }

    fn instruction(&self, instruction: &Instruction) -> bool {
        let program = self.program;
        let operands = instruction.operands(&program.operands);
        let slots_fit = operands
            .dst
            .iter()
            .chain(&operands.reads)
            .all(|&(bank, slot)| program.slot_fits(bank, slot));
        // A value varies by pixel when any operand does. A reduction's
        // accumulator may also vary because its default does.
        let pixel = operands.reads.iter().any(|(_, slot)| !slot.is_scalar());
        let destination = match (operands.dst, instruction) {
            (_, Instruction::Branch { .. }) => true,
            (
                Some((_, dst)),
                Instruction::Reduce {
                    start,
                    end,
                    filter,
                    value,
                    ..
                },
            ) => {
                let varies = [start, end, filter, value]
                    .iter()
                    .any(|slot| !slot.is_none() && !slot.is_scalar());
                !matches!(dst.kind(), SlotKind::Input(_)) && (!varies || !dst.is_scalar())
            }
            (Some((_, dst)), Instruction::Move { src, .. }) => {
                !matches!(dst.kind(), SlotKind::Input(_)) && (src.is_scalar() || !dst.is_scalar())
            }
            // Samples and sections read the strip's pixels themselves.
            (Some((_, dst)), _) => {
                let reads_pixels = matches!(
                    instruction,
                    Instruction::Sample { .. }
                        | Instruction::SectionCount { .. }
                        | Instruction::SectionIndex { .. }
                        | Instruction::SectionPosition { .. }
                );
                !matches!(dst.kind(), SlotKind::Input(_))
                    && (pixel || reads_pixels) == !dst.is_scalar()
            }
            (None, _) => false,
        };
        let bank = |kind: ParameterKind, bank: u16| usize::from(bank) < (self.bank_count)(kind);
        let specific = match *instruction {
            Instruction::FloatParam { bank: index, .. } => bank(ParameterKind::Float, index),
            Instruction::IntParam { bank: index, .. } => bank(ParameterKind::Int, index),
            Instruction::BoolParam { bank: index, .. } => bank(ParameterKind::Bool, index),
            Instruction::ColorParam { bank: index, .. } => bank(ParameterKind::Color, index),
            Instruction::EnumParam { bank: index, .. } => bank(ParameterKind::Enum, index),
            Instruction::ResourceParam {
                kind, bank: index, ..
            } => bank(ParameterKind::for_resource(kind), index),
            Instruction::ResourceConst { kind, index, .. } => {
                usize::from(index)
                    < match kind {
                        Resource::Curve => program.curves.len(),
                        Resource::Gradient => program.gradients.len(),
                        Resource::Marks => program.marks.len(),
                        Resource::Array => program.arrays.len(),
                    }
            }
            Instruction::Pick { items, .. } => {
                items.len != 0 && program.operands.get(items.range()).is_some()
            }
            Instruction::Equal { bank, .. } => bank != Bank::Resource,
            Instruction::Clock { .. } => matches!(self.context, ProgramContext::Operator { .. }),
            Instruction::Sample {
                input, frame_cache, ..
            } => {
                matches!(self.context, ProgramContext::Operator { inputs } if usize::from(input) < inputs)
                    && (frame_cache == NO_FRAME_CACHE || frame_cache < program.frame_caches)
            }
            Instruction::Reduce {
                reducer,
                bank,
                index,
                start,
                end,
                ..
            } => {
                let combines = match reducer {
                    Reducer::Max | Reducer::Sum => {
                        matches!(bank, Bank::Float | Bank::Int | Bank::Color)
                    }
                    Reducer::Min => matches!(bank, Bank::Float | Bank::Int),
                    Reducer::Any | Reducer::All => bank == Bank::Bool,
                    Reducer::First | Reducer::Last => true,
                };
                let bounds_pixel = !start.is_scalar() || !end.is_scalar();
                combines
                    && program.slot_fits(Bank::Int, index)
                    && !matches!(index.kind(), SlotKind::Input(_))
                    && bounds_pixel == !index.is_scalar()
            }
            _ => true,
        };
        slots_fit && destination && specific
    }
}
