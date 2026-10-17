//! Instructions as fixed-size words, the form an image stores them in. A word
//! is twelve 16-bit fields: the opcode, a byte for each of up to two small
//! enum operands, then the instruction's slots, lengths and constants in
//! declaration order. Decoding checks every field, so a decoded word is a
//! well-typed [`Instruction`]; admission then checks the program around it.
use crate::bytecode::{
    Bank, ColorBinary, ColorComponent, CompareOp, ContextRead, Direction, Edges, FloatBinary,
    FloatUnary, Instruction, IntBinary, MarkOp, Reducer, Resource, SignalPixel, Slot, Span,
};
use crate::values::Color;

/// One encoded instruction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub struct InstructionWord(pub [u16; 12]);

/// A fieldless enum stored as a byte: its index among its variants.
trait Wire: Sized + Copy + 'static {
    const ALL: &'static [Self];
    fn to_wire(self) -> u16;
    fn from_wire(value: u16) -> Option<Self> {
        Self::ALL.get(usize::from(value)).copied()
    }
}

macro_rules! wire {
    ($($ty:ident { $($variant:ident),+ $(,)? })+) => {$(
        impl Wire for $ty {
            const ALL: &'static [Self] = &[$($ty::$variant),+];
            fn to_wire(self) -> u16 {
                // Every variant is listed: a missing one is not matched.
                match self {
                    $($ty::$variant)|+ => self as u16,
                }
            }
        }
        // Listed in declaration order, so a variant's index is its number.
        const _: () = {
            let all = <$ty as Wire>::ALL;
            let mut index = 0;
            while index < all.len() {
                assert!(all[index] as usize == index);
                index += 1;
            }
        };
    )+};
}

wire! {
    FloatUnary { Negate, Sin, Cos, Tan, Exp, Log, Abs, Floor, Ceil, Trunc, Sqrt, Smoothstep, Rand }
    FloatBinary { Add, Subtract, Multiply, Divide, Remainder, Min, Max, ValueOr, Atan2, Power }
    IntBinary { Add, Subtract, Multiply, Remainder, FloorDivide, Min, Max }
    CompareOp { Less, LessEqual, Greater, GreaterEqual }
    ColorBinary { Add, Multiply, Max }
    ColorComponent { Hue, Saturation, Intensity, Red, Green, Blue }
    MarkOp { Count, At, Last, LastIndex }
    Resource { Curve, Gradient, Marks, Array }
    Reducer { Max, Min, Sum, Any, All, First, Last }
    Bank { Float, Int, Bool, Color, Resource }
    ContextRead {
        Seconds, Progress, Duration, FrameRate, PixelCount, TargetMinX, TargetMinY, TargetMaxX,
        TargetMaxY
    }
    Direction { Forward, Backward, Time }
    Edges { Skip, Extend, Mirror }
}

/// Writes fields in order after the opcode and the two enum bytes.
struct Writer {
    word: [u16; 12],
    at: usize,
}

impl Writer {
    fn new(opcode: u16, first: u16, second: u16) -> Self {
        let mut word = [0; 12];
        word[0] = opcode;
        word[1] = first | (second << 8);
        Self { word, at: 2 }
    }
    fn field(mut self, value: u16) -> Self {
        self.word[self.at] = value;
        self.at += 1;
        self
    }
    fn slot(self, slot: Slot) -> Self {
        self.field(slot.bits())
    }
    fn wide(self, value: u32) -> Self {
        self.field(value as u16).field((value >> 16) as u16)
    }
    fn done(self) -> InstructionWord {
        InstructionWord(self.word)
    }
}

/// Reads fields in the order [`Writer`] wrote them.
struct Reader<'w> {
    word: &'w [u16; 12],
    at: usize,
}

impl Reader<'_> {
    fn field(&mut self) -> u16 {
        let value = self.word[self.at];
        self.at += 1;
        value
    }
    fn slot(&mut self) -> Slot {
        Slot::from_bits(self.field())
    }
    fn wide(&mut self) -> u32 {
        u32::from(self.field()) | (u32::from(self.field()) << 16)
    }
    fn flag(&mut self) -> Option<bool> {
        match self.field() {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
    fn first<T: Wire>(&self) -> Option<T> {
        T::from_wire(self.word[1] & 0xff)
    }
    fn second<T: Wire>(&self) -> Option<T> {
        T::from_wire(self.word[1] >> 8)
    }
}

/// Opcodes, in [`Instruction`] declaration order.
mod op {
    pub(super) const FLOAT_CONST: u16 = 0;
    pub(super) const INT_CONST: u16 = 1;
    pub(super) const BOOL_CONST: u16 = 2;
    pub(super) const COLOR_CONST: u16 = 3;
    pub(super) const RESOURCE_CONST: u16 = 4;
    pub(super) const FLOAT_PARAM: u16 = 5;
    pub(super) const INT_PARAM: u16 = 6;
    pub(super) const BOOL_PARAM: u16 = 7;
    pub(super) const COLOR_PARAM: u16 = 8;
    pub(super) const RESOURCE_PARAM: u16 = 9;
    pub(super) const CONTEXT: u16 = 10;
    pub(super) const FLOAT_UNARY: u16 = 11;
    pub(super) const FLOAT_BINARY: u16 = 12;
    pub(super) const CLAMP: u16 = 13;
    pub(super) const MIX: u16 = 14;
    pub(super) const POWER: u16 = 15;
    pub(super) const CLOCK: u16 = 16;
    pub(super) const INT_NEGATE: u16 = 17;
    pub(super) const INT_BINARY: u16 = 18;
    pub(super) const INT_TO_FLOAT: u16 = 19;
    pub(super) const FLOAT_TO_INT: u16 = 20;
    pub(super) const NOT: u16 = 21;
    pub(super) const FLOAT_COMPARE: u16 = 22;
    pub(super) const INT_COMPARE: u16 = 23;
    pub(super) const EQUAL: u16 = 24;
    pub(super) const COLOR_BINARY: u16 = 25;
    pub(super) const COLOR_SCALE: u16 = 26;
    pub(super) const MIX_COLOR: u16 = 27;
    pub(super) const COLOR_COMPONENT: u16 = 28;
    pub(super) const INVERT: u16 = 29;
    pub(super) const RGB: u16 = 30;
    pub(super) const HSV: u16 = 31;
    pub(super) const RECOLOR: u16 = 32;
    pub(super) const CURVE_SAMPLE: u16 = 33;
    pub(super) const CURVE_CLAMPED: u16 = 34;
    pub(super) const CURVE_INTEGRAL: u16 = 35;
    pub(super) const CURVE_CROSSING: u16 = 36;
    pub(super) const GRADIENT_SAMPLE: u16 = 37;
    pub(super) const GRADIENT_SCALED: u16 = 38;
    pub(super) const MARK: u16 = 39;
    pub(super) const LEN: u16 = 40;
    pub(super) const INDEX: u16 = 41;
    pub(super) const PICK: u16 = 42;
    pub(super) const SELECT: u16 = 43;
    pub(super) const MOVE: u16 = 44;
    pub(super) const SECTION_COUNT: u16 = 45;
    pub(super) const SECTION_INDEX: u16 = 46;
    pub(super) const SECTION_POSITION: u16 = 47;
    pub(super) const SAMPLE: u16 = 48;
    pub(super) const BRANCH: u16 = 49;
    pub(super) const REDUCE: u16 = 50;
    pub(super) const STENCIL: u16 = 51;
    pub(super) const SCAN: u16 = 52;
}

/// How a sample addresses its pixel, as its first enum byte.
const PIXEL_CURRENT: u16 = 0;
const PIXEL_LOCAL: u16 = 1;
const PIXEL_SHIFTED: u16 = 2;

impl InstructionWord {
    pub fn encode(instruction: &Instruction) -> Self {
        use Instruction as I;
        let w = Writer::new;
        match *instruction {
            I::FloatConst { dst, bits } => w(op::FLOAT_CONST, 0, 0).slot(dst).wide(bits).done(),
            I::IntConst { dst, value } => {
                w(op::INT_CONST, 0, 0).slot(dst).wide(value as u32).done()
            }
            I::BoolConst { dst, value } => w(op::BOOL_CONST, 0, 0)
                .slot(dst)
                .field(u16::from(value))
                .done(),
            I::ColorConst { dst, value } => w(op::COLOR_CONST, 0, 0)
                .slot(dst)
                .field(u16::from(value.red))
                .field(u16::from(value.green))
                .field(u16::from(value.blue))
                .done(),
            I::ResourceConst { dst, kind, index } => w(op::RESOURCE_CONST, kind.to_wire(), 0)
                .slot(dst)
                .field(index)
                .done(),
            I::FloatParam { dst, bank } => w(op::FLOAT_PARAM, 0, 0).slot(dst).field(bank).done(),
            I::IntParam { dst, bank } => w(op::INT_PARAM, 0, 0).slot(dst).field(bank).done(),
            I::BoolParam { dst, bank } => w(op::BOOL_PARAM, 0, 0).slot(dst).field(bank).done(),
            I::ColorParam { dst, bank } => w(op::COLOR_PARAM, 0, 0).slot(dst).field(bank).done(),
            I::ResourceParam { dst, kind, bank } => w(op::RESOURCE_PARAM, kind.to_wire(), 0)
                .slot(dst)
                .field(bank)
                .done(),
            I::Context { dst, read } => w(op::CONTEXT, read.to_wire(), 0).slot(dst).done(),
            I::FloatUnary { op, dst, a } => {
                w(op::FLOAT_UNARY, op.to_wire(), 0).slot(dst).slot(a).done()
            }
            I::FloatBinary { op, dst, a, b } => w(op::FLOAT_BINARY, op.to_wire(), 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::Clamp {
                dst,
                value,
                min,
                max,
            } => w(op::CLAMP, 0, 0)
                .slot(dst)
                .slot(value)
                .slot(min)
                .slot(max)
                .done(),
            I::Mix { dst, a, b, amount } => w(op::MIX, 0, 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .slot(amount)
                .done(),
            I::Power { dst, base, count } => {
                w(op::POWER, 0, 0).slot(dst).slot(base).slot(count).done()
            }
            I::Clock {
                progress,
                dst,
                seconds,
            } => w(op::CLOCK, u16::from(progress), 0)
                .slot(dst)
                .slot(seconds)
                .done(),
            I::IntNegate { dst, a } => w(op::INT_NEGATE, 0, 0).slot(dst).slot(a).done(),
            I::IntBinary { op, dst, a, b } => w(op::INT_BINARY, op.to_wire(), 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::IntToFloat { dst, a } => w(op::INT_TO_FLOAT, 0, 0).slot(dst).slot(a).done(),
            I::FloatToInt { dst, a } => w(op::FLOAT_TO_INT, 0, 0).slot(dst).slot(a).done(),
            I::Not { dst, a } => w(op::NOT, 0, 0).slot(dst).slot(a).done(),
            I::FloatCompare { op, dst, a, b } => w(op::FLOAT_COMPARE, op.to_wire(), 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::IntCompare { op, dst, a, b } => w(op::INT_COMPARE, op.to_wire(), 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::Equal {
                bank,
                negate,
                dst,
                a,
                b,
            } => w(op::EQUAL, bank.to_wire(), u16::from(negate))
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::ColorBinary { op, dst, a, b } => w(op::COLOR_BINARY, op.to_wire(), 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .done(),
            I::ColorScale { dst, color, scale } => w(op::COLOR_SCALE, 0, 0)
                .slot(dst)
                .slot(color)
                .slot(scale)
                .done(),
            I::MixColor { dst, a, b, amount } => w(op::MIX_COLOR, 0, 0)
                .slot(dst)
                .slot(a)
                .slot(b)
                .slot(amount)
                .done(),
            I::ColorComponent { op, dst, color } => w(op::COLOR_COMPONENT, op.to_wire(), 0)
                .slot(dst)
                .slot(color)
                .done(),
            I::Invert { dst, color } => w(op::INVERT, 0, 0).slot(dst).slot(color).done(),
            I::Rgb {
                dst,
                red,
                green,
                blue,
            } => w(op::RGB, 0, 0)
                .slot(dst)
                .slot(red)
                .slot(green)
                .slot(blue)
                .done(),
            I::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => w(op::HSV, 0, 0)
                .slot(dst)
                .slot(hue)
                .slot(saturation)
                .slot(value)
                .done(),
            I::Recolor {
                shift,
                dst,
                color,
                hue,
            } => w(op::RECOLOR, u16::from(shift), 0)
                .slot(dst)
                .slot(color)
                .slot(hue)
                .done(),
            I::CurveSample {
                dst,
                curve,
                position,
            } => w(op::CURVE_SAMPLE, 0, 0)
                .slot(dst)
                .slot(curve)
                .slot(position)
                .done(),
            I::CurveClamped {
                dst,
                curve,
                position,
                min,
                max,
            } => w(op::CURVE_CLAMPED, 0, 0)
                .slot(dst)
                .slot(curve)
                .slot(position)
                .slot(min)
                .slot(max)
                .done(),
            I::CurveIntegral {
                dst,
                curve,
                position,
            } => w(op::CURVE_INTEGRAL, 0, 0)
                .slot(dst)
                .slot(curve)
                .slot(position)
                .done(),
            I::CurveCrossing {
                dst,
                curve,
                value,
                before,
            } => w(op::CURVE_CROSSING, 0, 0)
                .slot(dst)
                .slot(curve)
                .slot(value)
                .slot(before)
                .done(),
            I::GradientSample {
                dst,
                gradient,
                position,
            } => w(op::GRADIENT_SAMPLE, 0, 0)
                .slot(dst)
                .slot(gradient)
                .slot(position)
                .done(),
            I::GradientScaled {
                dst,
                gradient,
                position,
                scale,
            } => w(op::GRADIENT_SCALED, 0, 0)
                .slot(dst)
                .slot(gradient)
                .slot(position)
                .slot(scale)
                .done(),
            I::Mark {
                op,
                dst,
                marks,
                operand,
            } => w(op::MARK, op.to_wire(), 0)
                .slot(dst)
                .slot(marks)
                .slot(operand)
                .done(),
            I::Len { dst, array } => w(op::LEN, 0, 0).slot(dst).slot(array).done(),
            I::Index {
                bank,
                dst,
                array,
                index,
                default,
            } => w(op::INDEX, bank.to_wire(), 0)
                .slot(dst)
                .slot(array)
                .slot(index)
                .slot(default)
                .done(),
            I::Pick {
                bank,
                dst,
                index,
                items,
            } => w(op::PICK, bank.to_wire(), 0)
                .slot(dst)
                .slot(index)
                .field(items.start)
                .field(items.len)
                .done(),
            I::Select {
                bank,
                dst,
                condition,
                yes,
                no,
            } => w(op::SELECT, bank.to_wire(), 0)
                .slot(dst)
                .slot(condition)
                .slot(yes)
                .slot(no)
                .done(),
            I::Move { bank, dst, src } => w(op::MOVE, bank.to_wire(), 0).slot(dst).slot(src).done(),
            I::SectionCount { dst, width } => {
                w(op::SECTION_COUNT, 0, 0).slot(dst).slot(width).done()
            }
            I::SectionIndex { dst, width } => {
                w(op::SECTION_INDEX, 0, 0).slot(dst).slot(width).done()
            }
            I::SectionPosition {
                dst,
                width,
                inverse,
            } => w(op::SECTION_POSITION, 0, 0)
                .slot(dst)
                .slot(width)
                .slot(inverse)
                .done(),
            I::Sample {
                dst,
                input,
                seconds,
                pixel,
                frame_cache,
            } => {
                let (kind, edges, index) = match pixel {
                    SignalPixel::Current => (PIXEL_CURRENT, 0, Slot::NONE),
                    SignalPixel::Local(index) => (PIXEL_LOCAL, 0, index),
                    SignalPixel::Shifted(offset, edges) => (PIXEL_SHIFTED, edges.to_wire(), offset),
                };
                w(op::SAMPLE, kind, edges)
                    .slot(dst)
                    .field(input)
                    .slot(seconds)
                    .slot(index)
                    .field(frame_cache)
                    .done()
            }
            I::Branch {
                condition,
                then_len,
                else_len,
            } => w(op::BRANCH, 0, 0)
                .slot(condition)
                .field(then_len)
                .field(else_len)
                .done(),
            I::Reduce {
                reducer,
                bank,
                acc,
                index,
                start,
                end,
                filter,
                value,
                loop_len,
                contribute_len,
            } => w(op::REDUCE, reducer.to_wire(), bank.to_wire())
                .slot(acc)
                .slot(index)
                .slot(start)
                .slot(end)
                .slot(filter)
                .slot(value)
                .field(loop_len)
                .field(contribute_len)
                .done(),
            I::Stencil {
                reducer,
                edges,
                acc,
                index,
                start,
                end,
                weight,
                scale,
                pixel,
                source_len,
                tap_len,
            } => w(op::STENCIL, reducer.to_wire(), edges.to_wire())
                .slot(acc)
                .slot(index)
                .slot(start)
                .slot(end)
                .slot(weight)
                .slot(scale)
                .slot(pixel)
                .field(source_len)
                .field(tap_len)
                .done(),
            I::Scan {
                direction,
                dst,
                decay,
                weight,
                cache,
                source_len,
            } => w(op::SCAN, direction.to_wire(), 0)
                .slot(dst)
                .slot(decay)
                .slot(weight)
                .field(cache)
                .field(source_len)
                .done(),
        }
    }

    /// The instruction this word encodes, or `None` for a word no
    /// instruction encodes to.
    pub fn decode(&self) -> Option<Instruction> {
        let (instruction, used) = self.fields::<false>()?;
        // Every field after the instruction's own is zero, so each
        // instruction has exactly one word.
        (self.0[used..].iter().all(|&field| field == 0) && Self::encode(&instruction) == *self)
            .then_some(instruction)
    }

    /// The instruction a word admission already decoded encodes.
    #[inline(always)]
    pub fn decode_trusted(&self) -> Option<Instruction> {
        self.fields::<true>().map(|(instruction, _)| instruction)
    }

    /// The instruction and how many fields it uses. A `TRUSTED` word was
    /// decoded with checks at admission, so its unused fields are not
    /// checked again.
    #[inline(always)]
    fn fields<const TRUSTED: bool>(&self) -> Option<(Instruction, usize)> {
        use Instruction as I;
        let mut r = Reader {
            word: &self.0,
            at: 2,
        };
        let opcode = self.0[0];
        let unused_enums = TRUSTED || self.0[1] == 0;
        let instruction = match opcode {
            op::FLOAT_CONST if unused_enums => I::FloatConst {
                dst: r.slot(),
                bits: r.wide(),
            },
            op::INT_CONST if unused_enums => I::IntConst {
                dst: r.slot(),
                value: r.wide() as i32,
            },
            op::BOOL_CONST if unused_enums => I::BoolConst {
                dst: r.slot(),
                value: r.flag()?,
            },
            op::COLOR_CONST if unused_enums => {
                let dst = r.slot();
                let mut channel = || u8::try_from(r.field()).ok();
                I::ColorConst {
                    dst,
                    value: Color {
                        red: channel()?,
                        green: channel()?,
                        blue: channel()?,
                    },
                }
            }
            op::RESOURCE_CONST => I::ResourceConst {
                kind: r.first()?,
                dst: r.slot(),
                index: r.field(),
            },
            op::FLOAT_PARAM if unused_enums => I::FloatParam {
                dst: r.slot(),
                bank: r.field(),
            },
            op::INT_PARAM if unused_enums => I::IntParam {
                dst: r.slot(),
                bank: r.field(),
            },
            op::BOOL_PARAM if unused_enums => I::BoolParam {
                dst: r.slot(),
                bank: r.field(),
            },
            op::COLOR_PARAM if unused_enums => I::ColorParam {
                dst: r.slot(),
                bank: r.field(),
            },
            op::RESOURCE_PARAM => I::ResourceParam {
                kind: r.first()?,
                dst: r.slot(),
                bank: r.field(),
            },
            op::CONTEXT => I::Context {
                read: r.first()?,
                dst: r.slot(),
            },
            op::FLOAT_UNARY => I::FloatUnary {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
            },
            op::FLOAT_BINARY => I::FloatBinary {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::CLAMP if unused_enums => I::Clamp {
                dst: r.slot(),
                value: r.slot(),
                min: r.slot(),
                max: r.slot(),
            },
            op::MIX if unused_enums => I::Mix {
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
                amount: r.slot(),
            },
            op::POWER if unused_enums => I::Power {
                dst: r.slot(),
                base: r.slot(),
                count: r.slot(),
            },
            op::CLOCK => I::Clock {
                progress: match self.0[1] {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
                dst: r.slot(),
                seconds: r.slot(),
            },
            op::INT_NEGATE if unused_enums => I::IntNegate {
                dst: r.slot(),
                a: r.slot(),
            },
            op::INT_BINARY => I::IntBinary {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::INT_TO_FLOAT if unused_enums => I::IntToFloat {
                dst: r.slot(),
                a: r.slot(),
            },
            op::FLOAT_TO_INT if unused_enums => I::FloatToInt {
                dst: r.slot(),
                a: r.slot(),
            },
            op::NOT if unused_enums => I::Not {
                dst: r.slot(),
                a: r.slot(),
            },
            op::FLOAT_COMPARE => I::FloatCompare {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::INT_COMPARE => I::IntCompare {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::EQUAL => I::Equal {
                bank: r.first()?,
                negate: match self.0[1] >> 8 {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::COLOR_BINARY => I::ColorBinary {
                op: r.first()?,
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
            },
            op::COLOR_SCALE if unused_enums => I::ColorScale {
                dst: r.slot(),
                color: r.slot(),
                scale: r.slot(),
            },
            op::MIX_COLOR if unused_enums => I::MixColor {
                dst: r.slot(),
                a: r.slot(),
                b: r.slot(),
                amount: r.slot(),
            },
            op::COLOR_COMPONENT => I::ColorComponent {
                op: r.first()?,
                dst: r.slot(),
                color: r.slot(),
            },
            op::INVERT if unused_enums => I::Invert {
                dst: r.slot(),
                color: r.slot(),
            },
            op::RGB if unused_enums => I::Rgb {
                dst: r.slot(),
                red: r.slot(),
                green: r.slot(),
                blue: r.slot(),
            },
            op::HSV if unused_enums => I::Hsv {
                dst: r.slot(),
                hue: r.slot(),
                saturation: r.slot(),
                value: r.slot(),
            },
            op::RECOLOR => I::Recolor {
                shift: match self.0[1] {
                    0 => false,
                    1 => true,
                    _ => return None,
                },
                dst: r.slot(),
                color: r.slot(),
                hue: r.slot(),
            },
            op::CURVE_SAMPLE if unused_enums => I::CurveSample {
                dst: r.slot(),
                curve: r.slot(),
                position: r.slot(),
            },
            op::CURVE_CLAMPED if unused_enums => I::CurveClamped {
                dst: r.slot(),
                curve: r.slot(),
                position: r.slot(),
                min: r.slot(),
                max: r.slot(),
            },
            op::CURVE_INTEGRAL if unused_enums => I::CurveIntegral {
                dst: r.slot(),
                curve: r.slot(),
                position: r.slot(),
            },
            op::CURVE_CROSSING if unused_enums => I::CurveCrossing {
                dst: r.slot(),
                curve: r.slot(),
                value: r.slot(),
                before: r.slot(),
            },
            op::GRADIENT_SAMPLE if unused_enums => I::GradientSample {
                dst: r.slot(),
                gradient: r.slot(),
                position: r.slot(),
            },
            op::GRADIENT_SCALED if unused_enums => I::GradientScaled {
                dst: r.slot(),
                gradient: r.slot(),
                position: r.slot(),
                scale: r.slot(),
            },
            op::MARK => I::Mark {
                op: r.first()?,
                dst: r.slot(),
                marks: r.slot(),
                operand: r.slot(),
            },
            op::LEN if unused_enums => I::Len {
                dst: r.slot(),
                array: r.slot(),
            },
            op::INDEX => I::Index {
                bank: r.first()?,
                dst: r.slot(),
                array: r.slot(),
                index: r.slot(),
                default: r.slot(),
            },
            op::PICK => I::Pick {
                bank: r.first()?,
                dst: r.slot(),
                index: r.slot(),
                items: Span {
                    start: r.field(),
                    len: r.field(),
                },
            },
            op::SELECT => I::Select {
                bank: r.first()?,
                dst: r.slot(),
                condition: r.slot(),
                yes: r.slot(),
                no: r.slot(),
            },
            op::MOVE => I::Move {
                bank: r.first()?,
                dst: r.slot(),
                src: r.slot(),
            },
            op::SECTION_COUNT if unused_enums => I::SectionCount {
                dst: r.slot(),
                width: r.slot(),
            },
            op::SECTION_INDEX if unused_enums => I::SectionIndex {
                dst: r.slot(),
                width: r.slot(),
            },
            op::SECTION_POSITION if unused_enums => I::SectionPosition {
                dst: r.slot(),
                width: r.slot(),
                inverse: r.slot(),
            },
            op::SAMPLE => {
                let (kind, edges) = (self.0[1] & 0xff, self.0[1] >> 8);
                let dst = r.slot();
                let input = r.field();
                let seconds = r.slot();
                let index = r.slot();
                let pixel = match kind {
                    PIXEL_CURRENT if edges == 0 && index == Slot::NONE => SignalPixel::Current,
                    PIXEL_LOCAL if edges == 0 => SignalPixel::Local(index),
                    PIXEL_SHIFTED => SignalPixel::Shifted(index, r.second()?),
                    _ => return None,
                };
                I::Sample {
                    dst,
                    input,
                    seconds,
                    pixel,
                    frame_cache: r.field(),
                }
            }
            op::BRANCH if unused_enums => I::Branch {
                condition: r.slot(),
                then_len: r.field(),
                else_len: r.field(),
            },
            op::REDUCE => I::Reduce {
                reducer: r.first()?,
                bank: r.second()?,
                acc: r.slot(),
                index: r.slot(),
                start: r.slot(),
                end: r.slot(),
                filter: r.slot(),
                value: r.slot(),
                loop_len: r.field(),
                contribute_len: r.field(),
            },
            op::STENCIL => I::Stencil {
                reducer: r.first()?,
                edges: r.second()?,
                acc: r.slot(),
                index: r.slot(),
                start: r.slot(),
                end: r.slot(),
                weight: r.slot(),
                scale: r.slot(),
                pixel: r.slot(),
                source_len: r.field(),
                tap_len: r.field(),
            },
            op::SCAN => I::Scan {
                direction: r.first()?,
                dst: r.slot(),
                decay: r.slot(),
                weight: r.slot(),
                cache: r.field(),
                source_len: r.field(),
            },
            _ => return None,
        };
        Some((instruction, r.at))
    }
}
