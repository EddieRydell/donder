//! Instructions as fixed-size 8-byte words, the form programs store them in.
//! A word is its opcode, a byte of small enum operands, then the
//! instruction's slots, one byte each, and its constants and lengths, in a
//! layout each opcode fixes. A reduction and a stencil continue into a second
//! word. Words are trusted: the compiler wrote them, so decoding reads each
//! field as its instruction declares it.
use crate::bytecode::{
    Bank, ColorComponent, Colors, CompareOp, ContextRead, CurveOp, Direction, Edges, FloatBinary,
    FloatUnary, Instruction, IntBinary, IntUnary, MarkOp, Reducer, Resource, SignalPixel, Slot,
    SlotKind,
};

/// One encoded instruction word.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    Hash,
    bytemuck::Pod,
    bytemuck::Zeroable,
    rkyv::Archive,
    rkyv::Serialize,
    rkyv::Deserialize,
)]
#[repr(C)]
pub struct InstructionWord(pub [u8; 8]);

/// A fieldless enum stored in bits of a byte: its index among its variants.
trait Wire: Sized + Copy + 'static {
    const ALL: &'static [Self];
    fn to_wire(self) -> u8;
    fn from_wire(value: u8) -> Self {
        Self::ALL[usize::from(value)]
    }
}

macro_rules! wire {
    ($($ty:ident { $($variant:ident),+ $(,)? })+) => {$(
        impl Wire for $ty {
            const ALL: &'static [Self] = &[$($ty::$variant),+];
            fn to_wire(self) -> u8 {
                // Every variant is listed: a missing one is not matched.
                match self {
                    $($ty::$variant)|+ => self as u8,
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
    IntUnary { Negate, Abs }
    IntBinary { Add, Subtract, Multiply, Remainder, FloorDivide, Min, Max }
    CompareOp { Less, LessEqual, Greater, GreaterEqual, Equal, NotEqual }
    ColorComponent { Hue, Saturation, Intensity }
    CurveOp { Sample, Integral, Crossing, LastCrossing }
    MarkOp { At, Last, LastIndex }
    Resource { Curve, Gradient, Marks, Array }
    Reducer { Max, Min, Sum, Any, All, First, Last }
    Bank { Float, Int, Bool, Color, Resource }
    ContextRead {
        Seconds, Progress, Duration, FrameRate, FramesElapsed, PixelCount, TargetMinX, TargetMinY, TargetMaxX,
        TargetMaxY
    }
    Direction { Forward, Backward }
    Edges { Skip, Extend, Mirror }
}

/// Opcodes.
mod op {
    pub(super) const CONST: u8 = 0;
    pub(super) const PARAM: u8 = 1;
    pub(super) const CONTEXT: u8 = 2;
    pub(super) const FLOAT_UNARY: u8 = 3;
    pub(super) const FLOAT_BINARY: u8 = 4;
    pub(super) const CLAMP: u8 = 5;
    pub(super) const MIX: u8 = 6;
    pub(super) const INT_UNARY: u8 = 7;
    pub(super) const INT_BINARY: u8 = 8;
    pub(super) const INT_TO_FLOAT: u8 = 9;
    pub(super) const FLOAT_TO_INT: u8 = 10;
    pub(super) const COMPARE: u8 = 11;
    pub(super) const COLOR_COMPONENT: u8 = 12;
    pub(super) const HSV: u8 = 13;
    pub(super) const RECOLOR: u8 = 14;
    pub(super) const CURVE: u8 = 15;
    pub(super) const GRADIENT: u8 = 16;
    pub(super) const MARK: u8 = 17;
    pub(super) const LENGTH: u8 = 18;
    pub(super) const INDEX: u8 = 19;
    pub(super) const SELECT: u8 = 20;
    pub(super) const MOVE: u8 = 21;
    pub(super) const SECTION: u8 = 22;
    pub(super) const SAMPLE: u8 = 23;
    pub(super) const BRANCH: u8 = 24;
    pub(super) const REDUCE: u8 = 25;
    pub(super) const STENCIL: u8 = 26;
    pub(super) const SCAN: u8 = 27;
    pub(super) const TABULATE: u8 = 29;
    pub(super) const LOOKUP: u8 = 30;
    /// The second word of a reduction or a stencil.
    pub(super) const CONTINUED: u8 = 28;
}

/// How a sample addresses its pixel, in the low bits of its enum byte.
const PIXEL_CURRENT: u8 = 0;
const PIXEL_LOCAL: u8 = 1;
const PIXEL_SHIFTED: u8 = 2;

/// A slot in a byte: scalars from 0, [`Slot::NONE`] as 127, rows from 128
/// and the pixel inputs as the last four.
const NONE_BYTE: u8 = 0x7f;
const ROW_BYTE: u8 = 0x80;
const INPUT_BYTE: u8 = 0xfc;

fn pack(slot: Slot) -> u8 {
    if slot.is_none() {
        return NONE_BYTE;
    }
    match slot.kind() {
        SlotKind::Scalar(index) => {
            debug_assert!(index < Slot::SCALARS, "allocation bounds scalars");
            index as u8
        }
        SlotKind::Row(index) => {
            debug_assert!(index < Slot::ROWS, "allocation bounds rows");
            ROW_BYTE | index as u8
        }
        SlotKind::Input(input) => INPUT_BYTE + input.number() as u8,
    }
}

#[inline(always)]
fn unpack(byte: u8) -> Slot {
    match byte {
        NONE_BYTE => Slot::NONE,
        INPUT_BYTE.. => Slot::input(crate::bytecode::Input::ALL[usize::from(byte - INPUT_BYTE)]),
        ROW_BYTE.. => Slot::row(u16::from(byte & !ROW_BYTE)),
        _ => Slot::scalar(u16::from(byte)),
    }
}

/// Fills a word: the opcode, its enum byte, then fields by byte position.
struct Writer([u8; 8]);

impl Writer {
    fn new(opcode: u8, enums: u8) -> Self {
        let mut word = [0; 8];
        word[0] = opcode;
        word[1] = enums;
        Self(word)
    }
    /// Slots from byte 2 on.
    fn slots(mut self, slots: &[Slot]) -> Self {
        for (at, &slot) in slots.iter().enumerate() {
            self.0[2 + at] = pack(slot);
        }
        self
    }
    fn half(mut self, at: usize, value: u16) -> Self {
        self.0[at..at + 2].copy_from_slice(&value.to_le_bytes());
        self
    }
    fn wide(mut self, value: u32) -> Self {
        self.0[4..8].copy_from_slice(&value.to_le_bytes());
        self
    }
    fn done(self) -> InstructionWord {
        InstructionWord(self.0)
    }
}

impl InstructionWord {
    #[inline(always)]
    fn slot(&self, at: usize) -> Slot {
        unpack(self.0[at])
    }

    #[inline(always)]
    fn half(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }

    #[inline(always)]
    fn wide(&self) -> u32 {
        u32::from_le_bytes([self.0[4], self.0[5], self.0[6], self.0[7]])
    }

    /// The enum operand in the low or high four bits of the enum byte.
    #[inline(always)]
    fn low<T: Wire>(&self) -> T {
        T::from_wire(self.0[1] & 0xf)
    }

    #[inline(always)]
    fn high<T: Wire>(&self) -> T {
        T::from_wire(self.0[1] >> 4)
    }

    /// Append the words of `instruction` to `code`.
    pub fn encode(instruction: &Instruction, code: &mut alloc::vec::Vec<Self>) {
        use Instruction as I;
        let w = Writer::new;
        let both = |low: u8, high: u8| low | (high << 4);
        match *instruction {
            I::Const { bank, dst, bits } => {
                code.push(w(op::CONST, bank.to_wire()).slots(&[dst]).wide(bits).done());
            }
            I::Param { bank, dst, index } => code.push(
                w(op::PARAM, bank.to_wire())
                    .slots(&[dst])
                    .half(4, index)
                    .done(),
            ),
            I::Context { dst, read } => {
                code.push(w(op::CONTEXT, read.to_wire()).slots(&[dst]).done());
            }
            I::FloatUnary { op, dst, a } => {
                code.push(w(op::FLOAT_UNARY, op.to_wire()).slots(&[dst, a]).done());
            }
            I::FloatBinary {
                op,
                colors,
                dst,
                a,
                b,
            } => code.push(
                w(op::FLOAT_BINARY, both(op.to_wire(), colors.bits()))
                    .slots(&[dst, a, b])
                    .done(),
            ),
            I::Clamp {
                dst,
                value,
                min,
                max,
            } => code.push(w(op::CLAMP, 0).slots(&[dst, value, min, max]).done()),
            I::Mix {
                colors,
                dst,
                a,
                b,
                amount,
            } => code.push(w(op::MIX, colors.bits()).slots(&[dst, a, b, amount]).done()),
            I::IntUnary { op, dst, a } => {
                code.push(w(op::INT_UNARY, op.to_wire()).slots(&[dst, a]).done());
            }
            I::IntBinary { op, dst, a, b } => {
                code.push(w(op::INT_BINARY, op.to_wire()).slots(&[dst, a, b]).done());
            }
            I::IntToFloat { dst, a } => code.push(w(op::INT_TO_FLOAT, 0).slots(&[dst, a]).done()),
            I::FloatToInt { dst, a } => code.push(w(op::FLOAT_TO_INT, 0).slots(&[dst, a]).done()),
            I::Compare {
                op,
                bank,
                dst,
                a,
                b,
            } => code.push(
                w(op::COMPARE, both(op.to_wire(), bank.to_wire()))
                    .slots(&[dst, a, b])
                    .done(),
            ),
            I::ColorComponent { op, dst, color } => code.push(
                w(op::COLOR_COMPONENT, op.to_wire())
                    .slots(&[dst, color])
                    .done(),
            ),
            I::Hsv {
                dst,
                hue,
                saturation,
                value,
            } => code.push(w(op::HSV, 0).slots(&[dst, hue, saturation, value]).done()),
            I::Recolor {
                shift,
                dst,
                color,
                hue,
            } => code.push(
                w(op::RECOLOR, u8::from(shift))
                    .slots(&[dst, color, hue])
                    .done(),
            ),
            I::Curve {
                op,
                dst,
                curve,
                value,
                before,
            } => code.push(
                w(op::CURVE, op.to_wire())
                    .slots(&[dst, curve, value, before])
                    .done(),
            ),
            I::Gradient {
                dst,
                gradient,
                position,
                scale,
            } => code.push(
                w(op::GRADIENT, 0)
                    .slots(&[dst, gradient, position, scale])
                    .done(),
            ),
            I::Mark {
                op,
                dst,
                marks,
                operand,
            } => code.push(
                w(op::MARK, op.to_wire())
                    .slots(&[dst, marks, operand])
                    .done(),
            ),
            I::Length {
                kind,
                dst,
                resource,
            } => code.push(w(op::LENGTH, kind.to_wire()).slots(&[dst, resource]).done()),
            I::Index {
                bank,
                dst,
                array,
                index,
                default,
            } => code.push(
                w(op::INDEX, bank.to_wire())
                    .slots(&[dst, array, index, default])
                    .done(),
            ),
            I::Select {
                bank,
                dst,
                condition,
                yes,
                no,
            } => code.push(
                w(op::SELECT, bank.to_wire())
                    .slots(&[dst, condition, yes, no])
                    .done(),
            ),
            I::Move { bank, dst, src } => {
                code.push(w(op::MOVE, bank.to_wire()).slots(&[dst, src]).done());
            }
            I::Section { index, dst, width } => {
                code.push(w(op::SECTION, u8::from(index)).slots(&[dst, width]).done());
            }
            I::Sample { dst, input, pixel } => {
                let (kind, edges, index) = match pixel {
                    SignalPixel::Current => (PIXEL_CURRENT, 0, Slot::NONE),
                    SignalPixel::Local(index) => (PIXEL_LOCAL, 0, index),
                    SignalPixel::Shifted(offset, edges) => (PIXEL_SHIFTED, edges.to_wire(), offset),
                };
                code.push(
                    w(op::SAMPLE, both(kind, edges))
                        .slots(&[dst, index])
                        .half(4, input)
                        .done(),
                );
            }
            I::Branch {
                condition,
                then_len,
                else_len,
            } => code.push(
                w(op::BRANCH, 0)
                    .slots(&[condition])
                    .half(4, then_len)
                    .half(6, else_len)
                    .done(),
            ),
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
            } => {
                code.push(
                    w(op::REDUCE, both(reducer.to_wire(), bank.to_wire()))
                        .slots(&[acc, index, start, end])
                        .half(6, loop_len)
                        .done(),
                );
                code.push(
                    w(op::CONTINUED, 0)
                        .slots(&[filter, value])
                        .half(6, contribute_len)
                        .done(),
                );
            }
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
            } => {
                code.push(
                    w(op::STENCIL, both(reducer.to_wire(), edges.to_wire()))
                        .slots(&[acc, index, start, end])
                        .half(6, source_len)
                        .done(),
                );
                code.push(
                    w(op::CONTINUED, 0)
                        .slots(&[weight, scale, pixel])
                        .half(6, tap_len)
                        .done(),
                );
            }
            I::Tabulate {
                bank,
                prefix,
                dst,
                index,
                start,
                end,
                value,
                len,
            } => {
                let mut word = w(op::TABULATE, both(bank.to_wire(), u8::from(prefix)))
                    .slots(&[dst, index, start, end, value]);
                word.0[7] = len;
                code.push(word.done());
            }
            I::Lookup {
                bank,
                dst,
                table,
                index,
                start,
            } => code.push(
                w(op::LOOKUP, bank.to_wire())
                    .slots(&[dst, table, index, start])
                    .done(),
            ),
            I::Scan {
                direction,
                dst,
                decay,
                weight,
                cache,
                source_len,
            } => {
                let mut word = w(op::SCAN, direction.to_wire())
                    .slots(&[dst, decay, weight])
                    .half(6, source_len);
                word.0[5] = cache;
                code.push(word.done());
            }
        }
    }

    /// The instruction the words at the start of `code` encode. Only the
    /// compiler writes words, so they encode one.
    #[inline(always)]
    pub fn decode(code: &[Self]) -> Instruction {
        use Instruction as I;
        let word = &code[0];
        let s = |at| word.slot(at);
        match word.0[0] {
            op::CONST => I::Const {
                bank: word.low(),
                dst: s(2),
                bits: word.wide(),
            },
            op::PARAM => I::Param {
                bank: word.low(),
                dst: s(2),
                index: word.half(4),
            },
            op::CONTEXT => I::Context {
                read: word.low(),
                dst: s(2),
            },
            op::FLOAT_UNARY => I::FloatUnary {
                op: word.low(),
                dst: s(2),
                a: s(3),
            },
            op::FLOAT_BINARY => I::FloatBinary {
                op: word.low(),
                colors: Colors::from_bits(word.0[1] >> 4),
                dst: s(2),
                a: s(3),
                b: s(4),
            },
            op::CLAMP => I::Clamp {
                dst: s(2),
                value: s(3),
                min: s(4),
                max: s(5),
            },
            op::MIX => I::Mix {
                colors: Colors::from_bits(word.0[1]),
                dst: s(2),
                a: s(3),
                b: s(4),
                amount: s(5),
            },
            op::INT_UNARY => I::IntUnary {
                op: word.low(),
                dst: s(2),
                a: s(3),
            },
            op::INT_BINARY => I::IntBinary {
                op: word.low(),
                dst: s(2),
                a: s(3),
                b: s(4),
            },
            op::INT_TO_FLOAT => I::IntToFloat { dst: s(2), a: s(3) },
            op::FLOAT_TO_INT => I::FloatToInt { dst: s(2), a: s(3) },
            op::COMPARE => I::Compare {
                op: word.low(),
                bank: word.high(),
                dst: s(2),
                a: s(3),
                b: s(4),
            },
            op::COLOR_COMPONENT => I::ColorComponent {
                op: word.low(),
                dst: s(2),
                color: s(3),
            },
            op::HSV => I::Hsv {
                dst: s(2),
                hue: s(3),
                saturation: s(4),
                value: s(5),
            },
            op::RECOLOR => I::Recolor {
                shift: word.0[1] != 0,
                dst: s(2),
                color: s(3),
                hue: s(4),
            },
            op::CURVE => I::Curve {
                op: word.low(),
                dst: s(2),
                curve: s(3),
                value: s(4),
                before: s(5),
            },
            op::GRADIENT => I::Gradient {
                dst: s(2),
                gradient: s(3),
                position: s(4),
                scale: s(5),
            },
            op::MARK => I::Mark {
                op: word.low(),
                dst: s(2),
                marks: s(3),
                operand: s(4),
            },
            op::LENGTH => I::Length {
                kind: word.low(),
                dst: s(2),
                resource: s(3),
            },
            op::INDEX => I::Index {
                bank: word.low(),
                dst: s(2),
                array: s(3),
                index: s(4),
                default: s(5),
            },
            op::SELECT => I::Select {
                bank: word.low(),
                dst: s(2),
                condition: s(3),
                yes: s(4),
                no: s(5),
            },
            op::MOVE => I::Move {
                bank: word.low(),
                dst: s(2),
                src: s(3),
            },
            op::SECTION => I::Section {
                index: word.0[1] != 0,
                dst: s(2),
                width: s(3),
            },
            op::SAMPLE => {
                let index = s(3);
                let pixel = match word.0[1] & 0xf {
                    PIXEL_CURRENT => SignalPixel::Current,
                    PIXEL_LOCAL => SignalPixel::Local(index),
                    _ => SignalPixel::Shifted(index, word.high()),
                };
                I::Sample {
                    dst: s(2),
                    input: word.half(4),
                    pixel,
                }
            }
            op::BRANCH => I::Branch {
                condition: s(2),
                then_len: word.half(4),
                else_len: word.half(6),
            },
            op::REDUCE => {
                let next = &code[1];
                I::Reduce {
                    reducer: word.low(),
                    bank: word.high(),
                    acc: s(2),
                    index: s(3),
                    start: s(4),
                    end: s(5),
                    loop_len: word.half(6),
                    filter: next.slot(2),
                    value: next.slot(3),
                    contribute_len: next.half(6),
                }
            }
            op::STENCIL => {
                let next = &code[1];
                I::Stencil {
                    reducer: word.low(),
                    edges: word.high(),
                    acc: s(2),
                    index: s(3),
                    start: s(4),
                    end: s(5),
                    source_len: word.half(6),
                    weight: next.slot(2),
                    scale: next.slot(3),
                    pixel: next.slot(4),
                    tap_len: next.half(6),
                }
            }
            op::SCAN => I::Scan {
                direction: word.low(),
                dst: s(2),
                decay: s(3),
                weight: s(4),
                cache: word.0[5],
                source_len: word.half(6),
            },
            op::TABULATE => I::Tabulate {
                bank: word.low(),
                prefix: word.0[1] >> 4 != 0,
                dst: s(2),
                index: s(3),
                start: s(4),
                end: s(5),
                value: s(6),
                len: word.0[7],
            },
            op::LOOKUP => I::Lookup {
                bank: word.low(),
                dst: s(2),
                table: s(3),
                index: s(4),
                start: s(5),
            },
            _ => unreachable!("only the compiler writes instruction words"),
        }
    }
}
