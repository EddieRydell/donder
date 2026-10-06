use super::types::{Identifier, Type, Value};
use crate::Shared as Arc;
use alloc::{boxed::Box, collections::BTreeSet, vec, vec::Vec};

fn slot_key(slot: ValueSlot) -> (u8, u32) {
    match slot {
        ValueSlot::Void => (9, 0),
        ValueSlot::Int(slot) => (0, slot.0),
        ValueSlot::Float(slot) => (1, slot.0),
        ValueSlot::Bool(slot) => (2, slot.0),
        ValueSlot::Color(slot) => (3, slot.0),
        ValueSlot::Array(slot) => (4, slot.0),
        ValueSlot::Enum(slot) => (8, slot.0),
        ValueSlot::Marks(slot) => (5, slot.0),
        ValueSlot::Curve(slot) => (6, slot.0),
        ValueSlot::Gradient(slot) => (7, slot.0),
    }
}

pub type ConstantId = usize;
pub type LocalId = ValueSlot;
pub type ParamId = usize;
pub type Target = usize;

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramContext {
    Effect,
    Operator { inputs: usize },
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
            Type::Signal => Self::Void,
            Type::Array(_) => Self::Array,
        }
    }
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

#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct BytecodeProgram<C = ContextRead, S = (), A = ColorSlot> {
    pub instructions: Box<[Instruction<C, S, A>]>,
    pub array_constants: Box<[Arc<[Value]>]>,
    pub enums: Box<[Identifier]>,
    pub enum_types: Box<[EnumSlotType]>,
    /// Typed resource pools let constant loads preserve ownership without
    /// inspecting a dynamically tagged Value during execution.
    pub curves: Box<[Arc<crate::values::Curve>]>,
    pub gradients: Box<[Arc<crate::values::Gradient>]>,
    pub value_operands: Box<[ValueSlot]>,
    /// Compiler-owned type of each array register, in register order.
    pub array_types: Box<[Type]>,
    pub layout: SlotLayout,
    /// Compiler-proven dependency on pixel geometry or an upstream signal.
    pub uses_pixel_context: bool,
    /// First pixel-dependent instruction, following query and target initialization.
    pub pixel_entry: u32,
    /// Conservative live calculated-array bound, including construction space.
    pub array_capacity: u32,
    pub array_width: u32,
    /// Number of private counted-loop states reserved by this program.
    pub loop_count: u32,
}

impl<C, S, A> BytecodeProgram<C, S, A> {
    pub(super) fn try_map_execution<R, T, U, E>(
        self,
        mut read: impl FnMut(C) -> Result<R, E>,
        mut signal: impl FnMut(S) -> Result<T, E>,
        mut color: impl FnMut(A) -> Result<U, E>,
    ) -> Result<BytecodeProgram<R, T, U>, E> {
        let instructions = self
            .instructions
            .into_vec()
            .into_iter()
            .map(|instruction| instruction.try_map_execution(&mut read, &mut signal, &mut color))
            .collect::<Result<_, _>>()?;
        Ok(BytecodeProgram {
            instructions,
            array_constants: self.array_constants,
            enums: self.enums,
            enum_types: self.enum_types,
            curves: self.curves,
            gradients: self.gradients,
            value_operands: self.value_operands,
            array_types: self.array_types,
            layout: self.layout,
            uses_pixel_context: self.uses_pixel_context,
            pixel_entry: self.pixel_entry,
            array_capacity: self.array_capacity,
            array_width: self.array_width,
            loop_count: self.loop_count,
        })
    }
}

impl BytecodeProgram {
    pub fn has_valid_context(&self, context: ProgramContext) -> bool {
        let mut has_return = false;
        for instruction in &self.instructions {
            match instruction {
                Instruction::SignalSample { input, .. } => {
                    if !matches!(context, ProgramContext::Operator { inputs } if *input < inputs) {
                        return false;
                    }
                }
                Instruction::QuerySeconds { .. } | Instruction::QueryProgress { .. } => {
                    if !matches!(context, ProgramContext::Operator { .. }) {
                        return false;
                    }
                }
                Instruction::ReturnColor(_) => {
                    has_return = true;
                }
                _ => {}
            }
        }
        has_return
    }

    /// Check every parameter opcode against the invocation's parameter kinds.
    /// The caller supplies authored types during compilation and admitted bound
    /// values when loading portable bytecode.
    pub fn has_valid_parameter_reads(
        &self,
        kind_at: impl Fn(ParamId) -> Option<ParameterKind>,
    ) -> bool {
        let scalar = |param, source, kind| {
            kind_at(param) == Some(kind)
                && (0..param)
                    .filter(|index| kind_at(*index) == Some(kind))
                    .count()
                    == source
        };
        self.instructions.iter().all(|instruction| {
            use Instruction::*;
            match instruction {
                LoadIntParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Int)
                }
                LoadFloatParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Float)
                }
                LoadBoolParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Bool)
                }
                LoadColorParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Color)
                }
                LoadMarksParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Marks)
                }
                LoadArrayParam { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Array)
                }
                LoadCurveParam { param, source, .. }
                | CurveParamSample { param, source, .. }
                | CurveParamFloatClamped { param, source, .. }
                | CurveParamCrossing { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Curve)
                }
                LoadGradientParam { param, source, .. }
                | GradientParamSample { param, source, .. }
                | GradientParamColorScaled { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Gradient)
                }
                LoadEnumParam { param, source, .. } | EnumParamEqualConst { param, source, .. } => {
                    scalar(*param, source.0 as usize, ParameterKind::Enum)
                }
                _ => true,
            }
        })
    }

    /// Parameter kinds alone do not prove that a reference register contains
    /// the kind expected by an instruction (or even the declared enum/array).
    pub fn has_valid_reference_parameter_reads(
        &self,
        accepts: impl Fn(ParamId, &Type) -> bool,
    ) -> bool {
        self.instructions
            .iter()
            .all(|instruction| match instruction {
                Instruction::LoadEnumParam { dst, param, .. } => self
                    .enum_types
                    .get(dst.0 as usize)
                    .is_some_and(|ty| accepts(*param, ty.ty())),
                Instruction::LoadArrayParam { dst, param, .. } => self
                    .array_types
                    .get(dst.0 as usize)
                    .is_some_and(|ty| accepts(*param, ty)),
                _ => true,
            })
    }

    /// Reject malformed register and instruction references before a portable
    /// program can reach the unchecked register access in the VM.
    pub fn has_valid_structure(&self) -> bool {
        let valid_slot = |slot: ValueSlot| match slot {
            ValueSlot::Void => true,
            ValueSlot::Int(slot) => slot.0 < self.layout.ints,
            ValueSlot::Float(slot) => slot.0 < self.layout.floats,
            ValueSlot::Bool(slot) => slot.0 < self.layout.bools,
            ValueSlot::Color(slot) => slot.0 < self.layout.colors,
            ValueSlot::Array(slot) => slot.0 < self.layout.arrays,
            ValueSlot::Enum(slot) => slot.0 < self.layout.enums,
            ValueSlot::Marks(slot) => slot.0 < self.layout.marks,
            ValueSlot::Curve(slot) => slot.0 < self.layout.curves,
            ValueSlot::Gradient(slot) => slot.0 < self.layout.gradients,
        };
        let valid_pool = |span: PoolSpan| {
            (span.start as usize)
                .checked_add(span.len as usize)
                .and_then(|end| self.value_operands.get(span.start as usize..end))
                .is_some_and(|slots| slots.iter().copied().all(valid_slot))
        };
        let uses_pixel_context = self.reads_pixel_context();
        if self.layout.exceeded_bank().is_some()
            || !self.has_valid_pixel_entry()
            || self.uses_pixel_context != uses_pixel_context
            || !self.value_operands.iter().copied().all(valid_slot)
            || !self.has_valid_reference_types()
            || !self.has_valid_array_storage()
        {
            return false;
        }
        let references_valid = self
            .instructions
            .iter()
            .enumerate()
            .all(|(ip, instruction)| {
                use Instruction::*;
                match instruction {
                    LoadCurveConst { dst, constant } => {
                        valid_slot(ValueSlot::Curve(*dst)) && self.curves.get(*constant).is_some()
                    }
                    LoadGradientConst { dst, constant } => {
                        valid_slot(ValueSlot::Gradient(*dst))
                            && self.gradients.get(*constant).is_some()
                    }
                    LoadCurveParam { dst, .. } => valid_slot(ValueSlot::Curve(*dst)),
                    LoadGradientParam { dst, .. } => valid_slot(ValueSlot::Gradient(*dst)),
                    CurveSample {
                        dst,
                        curve,
                        position,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Curve(*curve))
                            && valid_slot(ValueSlot::Float(*position))
                    }
                    GradientSample {
                        dst,
                        gradient,
                        position,
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Gradient(*gradient))
                            && valid_slot(ValueSlot::Float(*position))
                    }
                    LoadIntConst { dst, .. } => valid_slot(ValueSlot::Int(*dst)),
                    LoadFloatConst { dst, .. } => valid_slot(ValueSlot::Float(*dst)),
                    LoadBoolConst { dst, .. } => valid_slot(ValueSlot::Bool(*dst)),
                    LoadColorConst { dst, .. } => valid_slot(ValueSlot::Color(*dst)),
                    LoadMarksConst { dst, .. } | LoadMarksParam { dst, .. } => {
                        valid_slot(ValueSlot::Marks(*dst))
                    }
                    LoadEnumConst { dst, constant } => {
                        valid_slot(ValueSlot::Enum(*dst)) && self.enums.get(*constant).is_some()
                    }
                    LoadEnumParam { dst, .. } => valid_slot(ValueSlot::Enum(*dst)),
                    LoadArrayConst { dst, constant } => {
                        valid_slot(ValueSlot::Array(*dst))
                            && self.array_constants.get(*constant).is_some()
                    }
                    LoadIntParam { dst, .. } => valid_slot(ValueSlot::Int(*dst)),
                    LoadFloatParam { dst, .. } => valid_slot(ValueSlot::Float(*dst)),
                    LoadBoolParam { dst, .. } => valid_slot(ValueSlot::Bool(*dst)),
                    LoadColorParam { dst, .. } => valid_slot(ValueSlot::Color(*dst)),
                    LoadArrayParam { dst, .. } => valid_slot(ValueSlot::Array(*dst)),
                    ContextRead { dst, .. } => valid_slot(dst.value_slot()),
                    Move { dst, src } => valid_slot(*dst) && valid_slot(dst.with_index(*src)),
                    Choose {
                        dst,
                        condition,
                        when_true,
                        when_false,
                    } => {
                        matches!(
                            dst,
                            ValueSlot::Int(_)
                                | ValueSlot::Float(_)
                                | ValueSlot::Bool(_)
                                | ValueSlot::Color(_)
                        ) && valid_slot(*dst)
                            && valid_slot(ValueSlot::Bool(*condition))
                            && valid_slot(dst.with_index(*when_true))
                            && valid_slot(dst.with_index(*when_false))
                    }
                    MakeArray { dst, items } => {
                        valid_slot(ValueSlot::Array(*dst))
                            && valid_pool(*items)
                            && self.array_capacity != 0
                            && items.len <= self.array_width
                    }
                    Index {
                        dst,
                        target,
                        index,
                        default,
                    } => {
                        valid_slot(*dst)
                            && valid_slot(ValueSlot::Array(*target))
                            && valid_slot(index.value_slot())
                            && valid_slot(dst.with_index(*default))
                    }
                    Select {
                        dst,
                        items,
                        index,
                        default,
                    } => {
                        valid_slot(*dst)
                            && valid_pool(*items)
                            && valid_slot(index.value_slot())
                            && valid_slot(dst.with_index(*default))
                    }
                    CurveParamSample { dst, position, .. } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*position))
                    }
                    GradientParamSample { dst, position, .. } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Float(*position))
                    }
                    SignalSample {
                        dst,
                        seconds,
                        pixel,
                        frame_cache,
                        ..
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Float(*seconds))
                            && pixel
                                .index()
                                .is_none_or(|slot| valid_slot(ValueSlot::Int(*slot)))
                            && (*frame_cache == u32::MAX
                                || (*frame_cache as usize) < self.instructions.len())
                    }
                    IntToFloat { dst, src } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Int(*src))
                    }
                    FloatToInt { dst, src } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Float(*src))
                    }
                    Not { dst, src } => {
                        valid_slot(ValueSlot::Bool(*dst)) && valid_slot(ValueSlot::Bool(*src))
                    }
                    NegInt { dst, src } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Int(*src))
                    }
                    NegFloat { dst, src }
                    | QuerySeconds { dst, seconds: src }
                    | QueryProgress { dst, seconds: src } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*src))
                    }
                    FloatAdd {
                        dst, left, right, ..
                    }
                    | FloatSubtract {
                        dst, left, right, ..
                    }
                    | FloatMultiply {
                        dst, left, right, ..
                    }
                    | FloatDivide {
                        dst, left, right, ..
                    }
                    | FloatRemainder {
                        dst, left, right, ..
                    }
                    | FloatBinary {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*left))
                            && valid_slot(ValueSlot::Float(*right))
                    }
                    FloatAddConst { dst, value, .. }
                    | FloatSubtractConst { dst, value, .. }
                    | FloatMultiplyConst { dst, value, .. }
                    | FloatDivideConst { dst, value, .. }
                    | FloatRemainderConst { dst, value, .. }
                    | FloatSubtractFromConst { dst, value, .. }
                    | FloatDivideIntoConst { dst, value, .. }
                    | FloatRemainderFromConst { dst, value, .. }
                    | FloatUnary { dst, value, .. }
                    | FloatBinaryConst { dst, value, .. }
                    | ClampConst { dst, value, .. } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*value))
                    }
                    IntAdd {
                        dst, left, right, ..
                    }
                    | IntSubtract {
                        dst, left, right, ..
                    }
                    | IntMultiply {
                        dst, left, right, ..
                    }
                    | IntRemainder {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Int(*dst))
                            && valid_slot(ValueSlot::Int(*left))
                            && valid_slot(ValueSlot::Int(*right))
                    }
                    IntCompare {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Bool(*dst))
                            && valid_slot(ValueSlot::Int(*left))
                            && valid_slot(ValueSlot::Int(*right))
                    }
                    FloatCompare {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Bool(*dst))
                            && valid_slot(ValueSlot::Float(*left))
                            && valid_slot(ValueSlot::Float(*right))
                    }
                    FloatCompareConst { dst, value, .. } => {
                        valid_slot(ValueSlot::Bool(*dst)) && valid_slot(ValueSlot::Float(*value))
                    }
                    ValueEqual {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Bool(*dst)) && valid_slot(*left) && valid_slot(*right)
                    }
                    EnumParamEqualConst { dst, constant, .. } => {
                        valid_slot(ValueSlot::Bool(*dst)) && self.enums.get(*constant).is_some()
                    }
                    IntJumpLess {
                        left,
                        right,
                        target,
                        ..
                    }
                    | IntJumpLessEqual {
                        left,
                        right,
                        target,
                        ..
                    }
                    | IntJumpGreater {
                        left,
                        right,
                        target,
                        ..
                    }
                    | IntJumpGreaterEqual {
                        left,
                        right,
                        target,
                        ..
                    }
                    | IntJumpEqual {
                        left,
                        right,
                        target,
                        ..
                    } => {
                        valid_slot(ValueSlot::Int(*left))
                            && valid_slot(ValueSlot::Int(*right))
                            && *target > ip
                            && *target < self.instructions.len()
                    }
                    FloatJumpLess {
                        left,
                        right,
                        target,
                        ..
                    }
                    | FloatJumpLessEqual {
                        left,
                        right,
                        target,
                        ..
                    }
                    | FloatJumpGreater {
                        left,
                        right,
                        target,
                        ..
                    }
                    | FloatJumpGreaterEqual {
                        left,
                        right,
                        target,
                        ..
                    }
                    | FloatJumpEqual {
                        left,
                        right,
                        target,
                        ..
                    } => {
                        valid_slot(ValueSlot::Float(*left))
                            && valid_slot(ValueSlot::Float(*right))
                            && *target > ip
                            && *target < self.instructions.len()
                    }
                    FloatJumpLessConst { value, target, .. }
                    | FloatJumpLessEqualConst { value, target, .. }
                    | FloatJumpGreaterConst { value, target, .. }
                    | FloatJumpGreaterEqualConst { value, target, .. }
                    | FloatJumpEqualConst { value, target, .. } => {
                        valid_slot(ValueSlot::Float(*value))
                            && *target > ip
                            && *target < self.instructions.len()
                    }
                    Jump(target) => *target > ip && *target < self.instructions.len(),
                    JumpIfFalse { condition, target } | JumpIfTrue { condition, target } => {
                        valid_slot(ValueSlot::Bool(*condition))
                            && *target > ip
                            && *target < self.instructions.len()
                    }
                    LoopRangeStart {
                        id,
                        count,
                        cap,
                        end,
                    } => {
                        *id < self.loop_count
                            && valid_slot(ValueSlot::Int(*count))
                            && *cap > 0
                            && *cap as usize <= super::MAX_DSL_LOOP_ITERATIONS
                            && *end > ip
                            && *end < self.instructions.len()
                    }
                    LoopMarksStart { id, marks, end } => {
                        *id < self.loop_count
                            && valid_slot(ValueSlot::Marks(*marks))
                            && *end > ip
                            && *end < self.instructions.len()
                    }
                    LoopEnd { id, start } => {
                        *id < self.loop_count && *start <= ip && *start < self.instructions.len()
                    }
                    SectionPosition {
                        dst,
                        width,
                        inverse,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*width))
                            && valid_slot(ValueSlot::Float(*inverse))
                    }
                    SectionQuery { dst, width, .. } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Int(*width))
                    }
                    Clamp {
                        dst,
                        value,
                        min,
                        max,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*value))
                            && valid_slot(ValueSlot::Float(*min))
                            && valid_slot(ValueSlot::Float(*max))
                    }
                    Smoothstep { dst, value } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*value))
                    }
                    MixFloat {
                        dst,
                        left,
                        right,
                        amount,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*left))
                            && valid_slot(ValueSlot::Float(*right))
                            && valid_slot(ValueSlot::Float(*amount))
                    }
                    MixColor {
                        dst,
                        left,
                        right,
                        amount,
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Color(*left))
                            && valid_slot(ValueSlot::Color(*right))
                            && valid_slot(ValueSlot::Float(*amount))
                    }
                    ColorBinary {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Color(*left))
                            && valid_slot(ValueSlot::Color(*right))
                    }
                    ColorScale { dst, color, scale } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Color(*color))
                            && valid_slot(ValueSlot::Float(*scale))
                    }
                    ColorComponent { dst, color, .. } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Color(*color))
                    }
                    ColorInvert { dst, color } => {
                        valid_slot(ValueSlot::Color(*dst)) && valid_slot(ValueSlot::Color(*color))
                    }
                    Rgb {
                        dst,
                        red,
                        green,
                        blue,
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Float(*red))
                            && valid_slot(ValueSlot::Float(*green))
                            && valid_slot(ValueSlot::Float(*blue))
                    }
                    Hsv {
                        dst,
                        hue,
                        saturation,
                        value,
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Float(*hue))
                            && valid_slot(ValueSlot::Float(*saturation))
                            && valid_slot(ValueSlot::Float(*value))
                    }
                    Rand { dst, seed } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*seed))
                    }
                    CurveFloatClamped {
                        dst,
                        curve,
                        position,
                        min,
                        max,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Curve(*curve))
                            && valid_slot(ValueSlot::Float(*position))
                            && valid_slot(ValueSlot::Float(*min))
                            && valid_slot(ValueSlot::Float(*max))
                    }
                    CurveParamFloatClamped {
                        dst,
                        position,
                        min,
                        max,
                        ..
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*position))
                            && valid_slot(ValueSlot::Float(*min))
                            && valid_slot(ValueSlot::Float(*max))
                    }
                    GradientColorScaled {
                        dst,
                        gradient,
                        position,
                        scale,
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Gradient(*gradient))
                            && valid_slot(ValueSlot::Float(*position))
                            && valid_slot(ValueSlot::Float(*scale))
                    }
                    GradientParamColorScaled {
                        dst,
                        position,
                        scale,
                        ..
                    } => {
                        valid_slot(ValueSlot::Color(*dst))
                            && valid_slot(ValueSlot::Float(*position))
                            && valid_slot(ValueSlot::Float(*scale))
                    }
                    CurveCrossing {
                        dst,
                        curve,
                        value,
                        before,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Curve(*curve))
                            && valid_slot(ValueSlot::Float(*value))
                            && before.is_none_or(|slot| valid_slot(ValueSlot::Float(slot)))
                    }
                    CurveParamCrossing {
                        dst, value, before, ..
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*value))
                            && before.is_none_or(|slot| valid_slot(ValueSlot::Float(slot)))
                    }
                    Len { dst, value } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Array(*value))
                    }
                    Mark { marks, op } => {
                        valid_slot(ValueSlot::Marks(*marks))
                            && valid_slot(op.output())
                            && op
                                .inputs()
                                .into_iter()
                                .flatten()
                                .all(|slot| valid_slot(slot.value_slot()))
                    }
                    ReturnColor(slot) => valid_slot(ValueSlot::Color(*slot)),
                }
            });
        references_valid
            && self.has_valid_loops()
            && self.has_no_fallthrough_path()
            && self.has_initialized_references()
    }

    /// Conservative live calculated-array storage required by the final
    /// instruction stream. Both compilation and archive admission use this proof.
    /// A reused query/target starts after its initialization with scalar registers
    /// carried over. Prove that initialization depends only on its inputs and
    /// earlier immutable scalar results. Changing target count/bounds restarts
    /// the target stage. This is an archive boundary, not an optimizer hint.
    fn has_valid_pixel_entry(&self) -> bool {
        use Instruction::*;

        let entry = self.pixel_entry as usize;
        if entry >= self.instructions.len() {
            return false;
        }
        let mut cached = BTreeSet::new();
        let mut query_uniform = BTreeSet::new();
        let mut references = Vec::new();
        let target_entry = self.target_entry();
        for (ip, instruction) in self.instructions[..entry].iter().enumerate() {
            let mut reads = [None; 4];
            let dst = match instruction {
                LoadIntConst { dst, .. } => ValueSlot::Int(*dst),
                LoadFloatConst { dst, .. } => ValueSlot::Float(*dst),
                LoadBoolConst { dst, .. } => ValueSlot::Bool(*dst),
                LoadColorConst { dst, .. } => ValueSlot::Color(*dst),
                LoadIntParam { dst, .. } => ValueSlot::Int(*dst),
                LoadFloatParam { dst, .. } => ValueSlot::Float(*dst),
                LoadBoolParam { dst, .. } => ValueSlot::Bool(*dst),
                LoadColorParam { dst, .. } => ValueSlot::Color(*dst),
                EnumParamEqualConst { dst, .. } => ValueSlot::Bool(*dst),
                Clamp {
                    dst,
                    value,
                    min,
                    max,
                } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    reads[1] = Some(ValueSlot::Float(*min));
                    reads[2] = Some(ValueSlot::Float(*max));
                    ValueSlot::Float(*dst)
                }
                ClampConst { dst, value, .. }
                | Smoothstep { dst, value }
                | Rand { dst, seed: value } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    ValueSlot::Float(*dst)
                }
                ContextRead {
                    dst,
                    read:
                        self::ContextRead::Progress
                        | self::ContextRead::Seconds
                        | self::ContextRead::Duration
                        | self::ContextRead::PixelCount
                        | self::ContextRead::TargetMinX
                        | self::ContextRead::TargetMinY
                        | self::ContextRead::TargetMaxX
                        | self::ContextRead::TargetMaxY,
                } => dst.value_slot(),
                FloatAdd {
                    dst, left, right, ..
                }
                | FloatSubtract {
                    dst, left, right, ..
                }
                | FloatMultiply {
                    dst, left, right, ..
                }
                | FloatDivide {
                    dst, left, right, ..
                }
                | FloatRemainder {
                    dst, left, right, ..
                }
                | FloatBinary {
                    dst, left, right, ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*left));
                    reads[1] = Some(ValueSlot::Float(*right));
                    ValueSlot::Float(*dst)
                }
                QuerySeconds {
                    dst,
                    seconds: value,
                }
                | QueryProgress {
                    dst,
                    seconds: value,
                }
                | FloatAddConst { dst, value, .. }
                | FloatSubtractConst { dst, value, .. }
                | FloatMultiplyConst { dst, value, .. }
                | FloatDivideConst { dst, value, .. }
                | FloatRemainderConst { dst, value, .. }
                | FloatSubtractFromConst { dst, value, .. }
                | FloatDivideIntoConst { dst, value, .. }
                | FloatRemainderFromConst { dst, value, .. }
                | FloatBinaryConst { dst, value, .. }
                | FloatUnary { dst, value, .. } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    ValueSlot::Float(*dst)
                }
                FloatCompare {
                    dst, left, right, ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*left));
                    reads[1] = Some(ValueSlot::Float(*right));
                    ValueSlot::Bool(*dst)
                }
                IntCompare {
                    dst, left, right, ..
                } => {
                    reads[0] = Some(ValueSlot::Int(*left));
                    reads[1] = Some(ValueSlot::Int(*right));
                    ValueSlot::Bool(*dst)
                }
                FloatCompareConst { dst, value, .. } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    ValueSlot::Bool(*dst)
                }
                IntToFloat { dst, src } => {
                    reads[0] = Some(ValueSlot::Int(*src));
                    ValueSlot::Float(*dst)
                }
                FloatToInt { dst, src } => {
                    reads[0] = Some(ValueSlot::Float(*src));
                    ValueSlot::Int(*dst)
                }
                Not { dst, src } => {
                    reads[0] = Some(ValueSlot::Bool(*src));
                    ValueSlot::Bool(*dst)
                }
                NegFloat { dst, src } => {
                    reads[0] = Some(ValueSlot::Float(*src));
                    ValueSlot::Float(*dst)
                }
                MixFloat {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    reads[0] = Some(ValueSlot::Float(*left));
                    reads[1] = Some(ValueSlot::Float(*right));
                    reads[2] = Some(ValueSlot::Float(*amount));
                    ValueSlot::Float(*dst)
                }
                MixColor {
                    dst,
                    left,
                    right,
                    amount,
                } => {
                    reads[0] = Some(ValueSlot::Color(*left));
                    reads[1] = Some(ValueSlot::Color(*right));
                    reads[2] = Some(ValueSlot::Float(*amount));
                    ValueSlot::Color(*dst)
                }
                ColorBinary {
                    dst, left, right, ..
                } => {
                    reads[0] = Some(ValueSlot::Color(*left));
                    reads[1] = Some(ValueSlot::Color(*right));
                    ValueSlot::Color(*dst)
                }
                ColorScale { dst, color, scale } => {
                    reads[0] = Some(ValueSlot::Color(*color));
                    reads[1] = Some(ValueSlot::Float(*scale));
                    ValueSlot::Color(*dst)
                }
                ColorComponent { dst, color, .. } => {
                    reads[0] = Some(ValueSlot::Color(*color));
                    ValueSlot::Float(*dst)
                }
                ColorInvert { dst, color } => {
                    reads[0] = Some(ValueSlot::Color(*color));
                    ValueSlot::Color(*dst)
                }
                Rgb {
                    dst,
                    red,
                    green,
                    blue,
                } => {
                    reads[0] = Some(ValueSlot::Float(*red));
                    reads[1] = Some(ValueSlot::Float(*green));
                    reads[2] = Some(ValueSlot::Float(*blue));
                    ValueSlot::Color(*dst)
                }
                Hsv {
                    dst,
                    hue,
                    saturation,
                    value,
                } => {
                    reads[0] = Some(ValueSlot::Float(*hue));
                    reads[1] = Some(ValueSlot::Float(*saturation));
                    reads[2] = Some(ValueSlot::Float(*value));
                    ValueSlot::Color(*dst)
                }
                CurveParamSample { dst, position, .. } => {
                    reads[0] = Some(ValueSlot::Float(*position));
                    ValueSlot::Float(*dst)
                }
                GradientParamSample { dst, position, .. } => {
                    reads[0] = Some(ValueSlot::Float(*position));
                    ValueSlot::Color(*dst)
                }
                CurveParamFloatClamped {
                    dst,
                    position,
                    min,
                    max,
                    ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*position));
                    reads[1] = Some(ValueSlot::Float(*min));
                    reads[2] = Some(ValueSlot::Float(*max));
                    ValueSlot::Float(*dst)
                }
                GradientParamColorScaled {
                    dst,
                    position,
                    scale,
                    ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*position));
                    reads[1] = Some(ValueSlot::Float(*scale));
                    ValueSlot::Color(*dst)
                }
                CurveParamCrossing {
                    dst, value, before, ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    reads[1] = before.map(ValueSlot::Float);
                    ValueSlot::Float(*dst)
                }
                Move { dst, src }
                    if matches!(
                        dst,
                        ValueSlot::Int(_)
                            | ValueSlot::Float(_)
                            | ValueSlot::Bool(_)
                            | ValueSlot::Color(_)
                    ) =>
                {
                    reads[0] = Some(dst.with_index(*src));
                    *dst
                }
                Choose {
                    dst,
                    condition,
                    when_true,
                    when_false,
                } => {
                    reads[0] = Some(ValueSlot::Bool(*condition));
                    reads[1] = Some(dst.with_index(*when_true));
                    reads[2] = Some(dst.with_index(*when_false));
                    *dst
                }
                LoadMarksConst { dst, .. } | LoadMarksParam { dst, .. } => ValueSlot::Marks(*dst),
                LoadArrayConst { dst, .. } | LoadArrayParam { dst, .. } => ValueSlot::Array(*dst),
                LoadCurveConst { dst, .. } | LoadCurveParam { dst, .. } => ValueSlot::Curve(*dst),
                LoadGradientConst { dst, .. } | LoadGradientParam { dst, .. } => {
                    ValueSlot::Gradient(*dst)
                }
                Mark { marks, op } => {
                    reads[0] = Some(ValueSlot::Marks(*marks));
                    reads[1] = op.inputs()[0].map(|slot| slot.value_slot());
                    op.output()
                }
                Len { dst, value } => {
                    reads[0] = Some(ValueSlot::Array(*value));
                    ValueSlot::Int(*dst)
                }
                CurveSample {
                    dst,
                    curve,
                    position,
                } => {
                    reads[0] = Some(ValueSlot::Curve(*curve));
                    reads[1] = Some(ValueSlot::Float(*position));
                    ValueSlot::Float(*dst)
                }
                CurveCrossing {
                    dst,
                    curve,
                    value,
                    before,
                } => {
                    reads[0] = Some(ValueSlot::Curve(*curve));
                    reads[1] = Some(ValueSlot::Float(*value));
                    reads[2] = before.map(ValueSlot::Float);
                    ValueSlot::Float(*dst)
                }
                CurveFloatClamped {
                    dst,
                    curve,
                    position,
                    min,
                    max,
                } => {
                    reads[0] = Some(ValueSlot::Curve(*curve));
                    reads[1] = Some(ValueSlot::Float(*position));
                    reads[2] = Some(ValueSlot::Float(*min));
                    reads[3] = Some(ValueSlot::Float(*max));
                    ValueSlot::Float(*dst)
                }
                GradientSample {
                    dst,
                    gradient,
                    position,
                } => {
                    reads[0] = Some(ValueSlot::Gradient(*gradient));
                    reads[1] = Some(ValueSlot::Float(*position));
                    ValueSlot::Color(*dst)
                }
                GradientColorScaled {
                    dst,
                    gradient,
                    position,
                    scale,
                } => {
                    reads[0] = Some(ValueSlot::Gradient(*gradient));
                    reads[1] = Some(ValueSlot::Float(*position));
                    reads[2] = Some(ValueSlot::Float(*scale));
                    ValueSlot::Color(*dst)
                }
                _ => return false,
            };
            if matches!(
                dst,
                ValueSlot::Marks(_)
                    | ValueSlot::Array(_)
                    | ValueSlot::Curve(_)
                    | ValueSlot::Gradient(_)
            ) {
                references.push(dst);
            }
            if !reads
                .into_iter()
                .flatten()
                .all(|slot| cached.contains(&slot_key(slot)))
                || !cached.insert(slot_key(dst))
            {
                return false;
            }
            if ip < target_entry {
                query_uniform.insert(slot_key(dst));
            }
        }
        // Reference registers do not survive an invocation, so a resumed pixel
        // must never read one loaded only by initialization.
        if self.instructions[entry..].iter().any(|instruction| {
            instruction
                .written_slot()
                .is_some_and(|slot| cached.contains(&slot_key(slot)))
                || references
                    .iter()
                    .any(|&slot| self.instruction_reads_ref(instruction, slot))
        }) {
            return false;
        }
        // A full-frame cache is reusable only when its time is unchanged
        // across pixels. Requiring the compiler's dense slot numbering also
        // prevents an untrusted artifact from reserving spurious frame buffers.
        let mut next_cache = 0u32;
        for instruction in &self.instructions {
            let SignalSample {
                seconds,
                frame_cache,
                ..
            } = instruction
            else {
                continue;
            };
            if *frame_cache == u32::MAX {
                continue;
            }
            if *frame_cache != next_cache
                || !query_uniform.contains(&slot_key(ValueSlot::Float(*seconds)))
            {
                return false;
            }
            let Some(next) = next_cache.checked_add(1) else {
                return false;
            };
            next_cache = next;
        }
        true
    }

    /// Array element typing strictly decreases nesting depth on each child edge,
    /// so arrays cannot retain themselves or form same-depth chains. At each
    /// depth, count register roots plus the maximum children of live parents.
    /// One extra node covers MakeArray before its destination releases the old
    /// value. Shared authored/parameter arrays do not consume local arena nodes.
    pub fn required_array_storage(&self) -> Option<(u32, u32)> {
        if !self
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MakeArray { .. }))
        {
            return Some((0, 0));
        }
        let mut roots = vec![0_u32];
        for ty in &self.array_types {
            let depth = array_depth(ty);
            if depth != 0 {
                roots.resize(roots.len().max(depth + 1), 0);
                roots[depth] = roots[depth].checked_add(1)?;
            }
        }
        let mut widths = vec![0_u32];
        for instruction in &self.instructions {
            let Instruction::MakeArray { dst, items } = instruction else {
                continue;
            };
            let ty = self.array_types.get(dst.0 as usize)?;
            let depth = array_depth(ty);
            if depth == 0 {
                return None;
            }
            widths.resize(widths.len().max(depth + 1), 0);
            widths[depth] = widths[depth].max(items.len);
        }
        let width = widths.iter().copied().max().unwrap_or(0);
        if width == 0 {
            return None;
        }
        let mut live = 0_u32;
        let mut capacity = 1_u32;
        for depth in (1..roots.len()).rev() {
            let parent_width = widths.get(depth + 1).copied().unwrap_or(0);
            live = live
                .checked_mul(parent_width)
                .and_then(|value| value.checked_add(roots[depth]))?;
            capacity = capacity.checked_add(live)?;
        }
        Some((capacity, width))
    }

    /// A portable program's stored capacity is an untrusted claim. Require
    /// the compiler-derived dimensions, not an arbitrary larger allocation.
    fn has_valid_array_storage(&self) -> bool {
        match self.required_array_storage() {
            Some((0, 0)) => self.array_capacity == 0 && self.array_width == 0,
            Some((capacity, width)) => self.array_capacity == capacity && self.array_width == width,
            None => false,
        }
    }

    /// Resource registers release their contents after each invocation. Every read
    /// must follow a write on every path, including the zero-iteration loop path.
    /// Once written, instruction typing keeps subsequent values in its type.
    fn has_initialized_references(&self) -> bool {
        for slot in (0..self.layout.arrays)
            .map(|slot| ValueSlot::Array(ArraySlot(slot)))
            .chain((0..self.layout.enums).map(|slot| ValueSlot::Enum(EnumSlot(slot))))
            .chain((0..self.layout.marks).map(|slot| ValueSlot::Marks(MarksSlot(slot))))
            .chain((0..self.layout.curves).map(|slot| ValueSlot::Curve(CurveSlot(slot))))
            .chain((0..self.layout.gradients).map(|slot| ValueSlot::Gradient(GradientSlot(slot))))
        {
            let mut visited = vec![false; self.instructions.len()];
            let mut pending = vec![0usize];
            while let Some(ip) = pending.pop() {
                if visited[ip] {
                    continue;
                }
                visited[ip] = true;
                let instruction = &self.instructions[ip];
                if self.instruction_reads_ref(instruction, slot) {
                    return false;
                }
                if self.instruction_writes_ref(instruction, slot) {
                    continue;
                }
                match instruction {
                    Instruction::ReturnColor(_) => {}
                    Instruction::Jump(target) => pending.push(*target),
                    Instruction::LoopRangeStart { end, .. }
                    | Instruction::LoopMarksStart { end, .. } => {
                        pending.push(end + 1);
                        pending.push(ip + 1);
                    }
                    Instruction::LoopEnd { start, .. } => {
                        pending.push(*start);
                        pending.push(ip + 1);
                    }
                    _ => {
                        if let Some(target) = instruction.conditional_target() {
                            pending.push(target);
                        }
                        pending.push(ip + 1);
                    }
                }
            }
        }
        true
    }

    fn instruction_reads_ref(&self, instruction: &Instruction, slot: ValueSlot) -> bool {
        let is_ref = |value| value == slot;
        let pool_reads = |span| {
            self.value_operands(span)
                .is_some_and(|values| values.iter().copied().any(is_ref))
        };
        match instruction {
            Instruction::Move { dst, src } => is_ref(dst.with_index(*src)),
            Instruction::Choose { .. } => false,
            Instruction::MakeArray { items, .. } => pool_reads(*items),
            Instruction::Select {
                dst,
                items,
                default,
                ..
            } => pool_reads(*items) || is_ref(dst.with_index(*default)),
            Instruction::Index {
                dst,
                target,
                default,
                ..
            } => is_ref(ValueSlot::Array(*target)) || is_ref(dst.with_index(*default)),
            Instruction::ValueEqual { left, right, .. } => is_ref(*left) || is_ref(*right),
            Instruction::CurveFloatClamped { curve, .. }
            | Instruction::CurveSample { curve, .. }
            | Instruction::CurveCrossing { curve, .. } => is_ref(ValueSlot::Curve(*curve)),
            Instruction::GradientColorScaled { gradient, .. }
            | Instruction::GradientSample { gradient, .. } => {
                is_ref(ValueSlot::Gradient(*gradient))
            }
            Instruction::Len { value, .. } => is_ref(ValueSlot::Array(*value)),
            Instruction::LoopMarksStart { marks, .. } => is_ref(ValueSlot::Marks(*marks)),
            Instruction::Mark { marks, .. } => is_ref(ValueSlot::Marks(*marks)),
            _ => false,
        }
    }

    fn instruction_writes_ref(&self, instruction: &Instruction, slot: ValueSlot) -> bool {
        let is_ref = |value| value == slot;
        match instruction {
            Instruction::Move { dst, .. }
            | Instruction::Index { dst, .. }
            | Instruction::Select { dst, .. } => is_ref(*dst),
            Instruction::LoadEnumConst { dst, .. } | Instruction::LoadEnumParam { dst, .. } => {
                is_ref(ValueSlot::Enum(*dst))
            }
            Instruction::LoadArrayConst { dst, .. }
            | Instruction::LoadArrayParam { dst, .. }
            | Instruction::MakeArray { dst, .. } => is_ref(ValueSlot::Array(*dst)),
            Instruction::LoadCurveConst { dst, .. } | Instruction::LoadCurveParam { dst, .. } => {
                is_ref(ValueSlot::Curve(*dst))
            }
            Instruction::LoadGradientConst { dst, .. }
            | Instruction::LoadGradientParam { dst, .. } => is_ref(ValueSlot::Gradient(*dst)),
            Instruction::LoadMarksConst { dst, .. } | Instruction::LoadMarksParam { dst, .. } => {
                is_ref(ValueSlot::Marks(*dst))
            }
            _ => false,
        }
    }

    fn has_valid_reference_types(&self) -> bool {
        if self.enum_types.len() != self.layout.enums as usize
            || !self.enum_types.iter().all(EnumSlotType::is_valid)
            || self.array_types.len() != self.layout.arrays as usize
            || !self.array_types.iter().all(well_formed_ref_type)
        {
            return false;
        }
        let ref_type = |slot: ArraySlot| self.array_types.get(slot.0 as usize);
        let slot_type = |slot: ValueSlot| match slot {
            ValueSlot::Void => Some(&Type::Void),
            ValueSlot::Int(_) => Some(&Type::Int),
            ValueSlot::Float(_) => Some(&Type::Float),
            ValueSlot::Bool(_) => Some(&Type::Bool),
            ValueSlot::Color(_) => Some(&Type::Color),
            ValueSlot::Marks(_) => Some(&Type::Marks),
            ValueSlot::Curve(_) => Some(&Type::Curve),
            ValueSlot::Gradient(_) => Some(&Type::Gradient),
            ValueSlot::Array(slot) => ref_type(slot),
            ValueSlot::Enum(slot) => self.enum_types.get(slot.0 as usize).map(EnumSlotType::ty),
        };
        let accepts_slot = |dst: ValueSlot, src: ValueSlot| {
            slot_type(dst)
                .zip(slot_type(src))
                .is_some_and(|(dst, src)| dst.accepts(src))
        };
        let accepts_value = |dst: ValueSlot, value: &Value| {
            slot_type(dst).is_some_and(|ty| ty.accepts_value(value))
        };
        let operands = |span: PoolSpan| self.value_operands(span);
        self.instructions.iter().all(|instruction| {
            use Instruction::*;
            match instruction {
                LoadEnumConst { dst, constant } => self.enums.get(*constant).is_some_and(|value| {
                    accepts_value(ValueSlot::Enum(*dst), &Value::Enum(value.clone()))
                }),
                LoadArrayConst { dst, constant } => {
                    self.array_constants.get(*constant).is_some_and(|value| {
                        accepts_value(ValueSlot::Array(*dst), &Value::Array(Arc::clone(value)))
                    })
                }
                ContextRead { dst, read } => match read {
                    self::ContextRead::PixelIndex | self::ContextRead::PixelCount => true,
                    _ => matches!(dst, NumberSlot::Float(_)),
                },
                Move { dst, src } => accepts_slot(*dst, dst.with_index(*src)),
                MakeArray { dst, items } => {
                    let Some(Type::Array(item_type)) = ref_type(*dst) else {
                        return false;
                    };
                    operands(*items).is_some_and(|items| {
                        items
                            .iter()
                            .all(|slot| slot_type(*slot).is_some_and(|ty| item_type.accepts(ty)))
                    })
                }
                Index {
                    dst,
                    target,
                    index,
                    default,
                } => {
                    let index_type = slot_type(index.value_slot());
                    let target_ok = match ref_type(*target) {
                        Some(Type::Array(item)) => {
                            index_type.is_some_and(|ty| Type::Int.accepts(ty))
                                && slot_type(*dst).is_some_and(|ty| ty.accepts(item))
                        }
                        _ => false,
                    };
                    target_ok && accepts_slot(*dst, dst.with_index(*default))
                }
                Select {
                    dst,
                    items,
                    default,
                    ..
                } => {
                    operands(*items)
                        .is_some_and(|items| items.iter().all(|slot| accepts_slot(*dst, *slot)))
                        && accepts_slot(*dst, dst.with_index(*default))
                }
                Len { value, .. } => {
                    matches!(ref_type(*value), Some(Type::Array(_)))
                }
                _ => true,
            }
        })
    }

    fn has_valid_loops(&self) -> bool {
        if self.loop_count as usize > self.instructions.len() {
            return false;
        }
        let mut seen = vec![false; self.loop_count as usize];
        let mut stack = Vec::new();
        for (ip, instruction) in self.instructions.iter().enumerate() {
            match instruction {
                Instruction::LoopRangeStart { id, end, .. }
                | Instruction::LoopMarksStart { id, end, .. } => {
                    let Some(used) = seen.get_mut(*id as usize) else {
                        return false;
                    };
                    if *used
                        || !matches!(self.instructions.get(*end), Some(Instruction::LoopEnd { id: end_id, start }) if end_id == id && *start == ip + 1)
                    {
                        return false;
                    }
                    *used = true;
                    stack.push((*id, *end));
                }
                Instruction::LoopEnd { id, .. } if stack.pop() != Some((*id, ip)) => return false,
                _ => {}
            }
        }
        stack.is_empty() && seen.into_iter().all(|used| used)
    }

    fn has_no_fallthrough_path(&self) -> bool {
        let mut visited = vec![false; self.instructions.len()];
        let mut pending = Vec::from([0]);
        while let Some(ip) = pending.pop() {
            let Some(instruction) = self.instructions.get(ip) else {
                return false;
            };
            if visited[ip] {
                continue;
            }
            visited[ip] = true;
            match instruction {
                Instruction::ReturnColor(_) => {}
                Instruction::Jump(target) => pending.push(*target),
                Instruction::LoopRangeStart { end, .. }
                | Instruction::LoopMarksStart { end, .. } => {
                    pending.push(end + 1);
                    pending.push(ip + 1);
                }
                Instruction::LoopEnd { start, .. } => {
                    pending.push(*start);
                    pending.push(ip + 1);
                }
                _ => {
                    if let Some(target) = instruction.conditional_target() {
                        pending.push(target);
                    }
                    pending.push(ip + 1);
                }
            }
        }
        true
    }

    pub fn frame_cache_count(&self) -> usize {
        self.instructions
            .iter()
            .filter_map(|instruction| match instruction {
                Instruction::SignalSample { frame_cache, .. } if *frame_cache != u32::MAX => {
                    Some(*frame_cache as usize + 1)
                }
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    pub fn value_operands(&self, span: PoolSpan) -> Option<&[ValueSlot]> {
        self.value_operands.get(span.range())
    }

    pub fn uses_spatial_context(&self) -> bool {
        self.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::ContextRead {
                    read: ContextRead::PixelX
                        | ContextRead::PixelY
                        | ContextRead::TargetMinX
                        | ContextRead::TargetMinY
                        | ContextRead::TargetMaxX
                        | ContextRead::TargetMaxY,
                    ..
                }
            )
        })
    }

    /// Query initialization precedes the first target-context read. The compiler
    /// groups all target-dependent initialization after this boundary.
    pub fn target_entry(&self) -> usize {
        self.instructions
            .iter()
            .take(self.pixel_entry as usize)
            .position(|op| {
                matches!(
                    op,
                    Instruction::ContextRead {
                        read: ContextRead::PixelCount
                            | ContextRead::TargetMinX
                            | ContextRead::TargetMinY
                            | ContextRead::TargetMaxX
                            | ContextRead::TargetMaxY,
                        ..
                    }
                )
            })
            .unwrap_or(self.pixel_entry as usize)
    }

    pub fn reads_progress(&self) -> bool {
        self.instructions.iter().any(|op| {
            matches!(
                op,
                Instruction::ContextRead {
                    read: ContextRead::Progress,
                    ..
                }
            )
        })
    }

    pub fn reads_pixel_context(&self) -> bool {
        self.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                Instruction::ContextRead {
                    read: ContextRead::PixelIndex
                        | ContextRead::PixelCount
                        | ContextRead::PixelFraction
                        | ContextRead::PixelX
                        | ContextRead::PixelY
                        | ContextRead::TargetMinX
                        | ContextRead::TargetMinY
                        | ContextRead::TargetMaxX
                        | ContextRead::TargetMaxY,
                    ..
                } | Instruction::SectionPosition { .. }
                    | Instruction::SectionQuery { .. }
                    | Instruction::SignalSample { .. }
            )
        })
    }
}

fn well_formed_ref_type(ty: &Type) -> bool {
    match ty {
        Type::Void | Type::Signal => false,
        Type::Array(item) => match item.as_ref() {
            Type::Enum(options) => !options.is_empty(),
            Type::Array(_) => well_formed_ref_type(item),
            _ => true,
        },
        Type::Enum(_) => false,
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::Color
        | Type::Marks
        | Type::Curve
        | Type::Gradient => false,
    }
}

fn array_depth(mut ty: &Type) -> usize {
    let mut depth = 0;
    while let Type::Array(item) = ty {
        depth += 1;
        ty = item;
    }
    depth
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct PoolSpan {
    pub start: u32,
    pub len: u32,
}

impl PoolSpan {
    pub fn range(self) -> core::ops::Range<usize> {
        self.start as usize..self.start.saturating_add(self.len) as usize
    }
}

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
pub struct SlotLayout {
    pub ints: u32,
    pub floats: u32,
    pub bools: u32,
    pub colors: u32,
    pub arrays: u32,
    pub enums: u32,
    pub marks: u32,
    pub curves: u32,
    pub gradients: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PrimitiveBank {
    Float,
    Int,
    Bool,
}

impl SlotLayout {
    /// Fixed primitive bank capacities. The interpreter stores these banks
    /// inline, so each register access is a direct masked index.
    pub const FLOAT_REGISTERS: u32 = 256;
    pub const INT_REGISTERS: u32 = 64;
    pub const BOOL_REGISTERS: u32 = 64;

    /// The first primitive bank that exceeds its fixed capacity, if any.
    pub fn exceeded_bank(&self) -> Option<(PrimitiveBank, u32, u32)> {
        [
            (PrimitiveBank::Float, self.floats, Self::FLOAT_REGISTERS),
            (PrimitiveBank::Int, self.ints, Self::INT_REGISTERS),
            (PrimitiveBank::Bool, self.bools, Self::BOOL_REGISTERS),
        ]
        .into_iter()
        .find(|(_, used, limit)| used > limit)
    }
}

/// Declaration metadata and a valid initialization value for one enum register.
/// The VM never needs to inspect Type when allocating or loading this bank.
#[derive(Clone, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct EnumSlotType {
    ty: Type,
    initial: Identifier,
}

impl EnumSlotType {
    pub fn new(ty: Type) -> Option<Self> {
        let Type::Enum(options) = &ty else {
            return None;
        };
        let initial = options.first()?.clone();
        Some(Self { ty, initial })
    }
    pub fn ty(&self) -> &Type {
        &self.ty
    }
    pub fn initial(&self) -> &Identifier {
        &self.initial
    }
    fn is_valid(&self) -> bool {
        matches!(&self.ty, Type::Enum(options) if options.first() == Some(&self.initial))
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct EnumSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct IntSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct FloatSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct BoolSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct ColorSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct ArraySlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct MarksSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct CurveSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct GradientSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ValueSlot {
    Void,
    Int(IntSlot),
    Float(FloatSlot),
    Bool(BoolSlot),
    Color(ColorSlot),
    Array(ArraySlot),
    Enum(EnumSlot),
    Marks(MarksSlot),
    Curve(CurveSlot),
    Gradient(GradientSlot),
}

impl ValueSlot {
    /// Index within this slot's typed register bank.
    pub fn index(self) -> u32 {
        match self {
            Self::Void => 0,
            Self::Int(slot) => slot.0,
            Self::Float(slot) => slot.0,
            Self::Bool(slot) => slot.0,
            Self::Color(slot) => slot.0,
            Self::Array(slot) => slot.0,
            Self::Enum(slot) => slot.0,
            Self::Marks(slot) => slot.0,
            Self::Curve(slot) => slot.0,
            Self::Gradient(slot) => slot.0,
        }
    }

    /// Another register in the same bank. A copy cannot change scalar types;
    /// numeric conversions have their own instructions.
    pub fn with_index(self, index: u32) -> Self {
        match self {
            Self::Void => Self::Void,
            Self::Int(_) => Self::Int(IntSlot(index)),
            Self::Float(_) => Self::Float(FloatSlot(index)),
            Self::Bool(_) => Self::Bool(BoolSlot(index)),
            Self::Color(_) => Self::Color(ColorSlot(index)),
            Self::Array(_) => Self::Array(ArraySlot(index)),
            Self::Enum(_) => Self::Enum(EnumSlot(index)),
            Self::Marks(_) => Self::Marks(MarksSlot(index)),
            Self::Curve(_) => Self::Curve(CurveSlot(index)),
            Self::Gradient(_) => Self::Gradient(GradientSlot(index)),
        }
    }

    pub fn for_type(ty: &Type, layout: &mut SlotLayout) -> Self {
        match ty {
            Type::Int => {
                let slot = IntSlot(layout.ints);
                layout.ints += 1;
                Self::Int(slot)
            }
            Type::Float => {
                let slot = FloatSlot(layout.floats);
                layout.floats += 1;
                Self::Float(slot)
            }
            Type::Bool => {
                let slot = BoolSlot(layout.bools);
                layout.bools += 1;
                Self::Bool(slot)
            }
            Type::Color => {
                let slot = ColorSlot(layout.colors);
                layout.colors += 1;
                Self::Color(slot)
            }
            Type::Marks => {
                let slot = MarksSlot(layout.marks);
                layout.marks += 1;
                Self::Marks(slot)
            }
            Type::Curve => {
                let slot = CurveSlot(layout.curves);
                layout.curves += 1;
                Self::Curve(slot)
            }
            Type::Gradient => {
                let slot = GradientSlot(layout.gradients);
                layout.gradients += 1;
                Self::Gradient(slot)
            }
            Type::Enum(_) => {
                let slot = EnumSlot(layout.enums);
                layout.enums += 1;
                Self::Enum(slot)
            }
            Type::Void | Type::Signal => Self::Void,
            Type::Array(_) => {
                let slot = ArraySlot(layout.arrays);
                layout.arrays += 1;
                Self::Array(slot)
            }
        }
    }
}

// Signal capabilities change during admission; samples cannot query signals.
// The macro keeps that mechanical map alongside the instruction schema.
macro_rules! instructions {
    ($(
        $(#[$attr:meta])*
        $variant:ident
        $({ $($(#[$field_attr:meta])* $field:ident: $ty:ty),* $(,)? })?
        $(($value:ident: $tuple_ty:ty))?,
    )*) => {
        #[derive(Clone, Debug, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
        pub enum Instruction<C = ContextRead, S = (), A = ColorSlot> {
            $(
                $(#[$attr])*
                $variant
                $({ $($(#[$field_attr])* $field: $ty),* })?
                $(($tuple_ty))?,
            )*
            ReturnColor(A),
            ContextRead { dst: NumberSlot, read: C },
            SignalSample {
                dst: ColorSlot,
                input: usize,
                seconds: FloatSlot,
                pixel: SignalPixel<IntSlot>,
                frame_cache: u32,
                capability: S,
            },
        }

        impl<C, S, A> Instruction<C, S, A> {
            fn try_map_execution<R, T, U, E>(
                self,
                read: impl FnOnce(C) -> Result<R, E>,
                signal: impl FnOnce(S) -> Result<T, E>,
                color: impl FnOnce(A) -> Result<U, E>,
            ) -> Result<Instruction<R, T, U>, E> {
                Ok(match self {
                    Self::ReturnColor(value) => Instruction::ReturnColor(color(value)?),
                    Self::ContextRead { dst, read: value } =>
                        Instruction::ContextRead { dst, read: read(value)? },
                    Self::SignalSample { dst, input, seconds, pixel, frame_cache, capability } =>
                        Instruction::SignalSample {
                            dst, input, seconds, pixel, frame_cache,
                            capability: signal(capability)?,
                        },
                    $(Self::$variant $({ $($field),* })? $(($value))? =>
                        Instruction::$variant $({ $($field),* })? $(($value))?,)*
                })
            }
        }
    };
}

instructions! {
    /// Quantize an inlined source query using the runtime clock. Invalid or
    /// out-of-sequence queries yield NaN so their source body can be skipped.
    QuerySeconds { dst: FloatSlot, seconds: FloatSlot },
    /// Progress at the same original query, before any seconds round trip.
    QueryProgress { dst: FloatSlot, seconds: FloatSlot },
    LoadCurveConst {
        dst: CurveSlot,
        constant: ConstantId,
    },
    LoadGradientConst {
        dst: GradientSlot,
        constant: ConstantId,
    },
    LoadCurveParam {
        dst: CurveSlot,
        param: ParamId,
        source: CurveSlot,
    },
    LoadGradientParam {
        dst: GradientSlot,
        param: ParamId,
        source: GradientSlot,
    },
    CurveSample {
        dst: FloatSlot,
        curve: CurveSlot,
        position: FloatSlot,
    },
    GradientSample {
        dst: ColorSlot,
        gradient: GradientSlot,
        position: FloatSlot,
    },
    LoadMarksConst {
        dst: MarksSlot,
        value: Arc<crate::values::Marks>,
    },
    LoadMarksParam {
        dst: MarksSlot,
        param: ParamId,
        source: MarksSlot,
    },
    LoadIntConst {
        dst: IntSlot,
        value: i32,
    },
    /// Store IEEE bits so hashing and serialization preserve NaNs and signed zero.
    LoadFloatConst {
        dst: FloatSlot,
        bits: u32,
    },
    LoadBoolConst {
        dst: BoolSlot,
        value: bool,
    },
    LoadColorConst {
        dst: ColorSlot,
        value: crate::values::Color,
    },
    LoadEnumConst {
        dst: EnumSlot,
        constant: ConstantId,
    },
    LoadEnumParam {
        dst: EnumSlot,
        param: ParamId,
        source: EnumSlot,
    },
    LoadArrayConst {
        dst: ArraySlot,
        constant: ConstantId,
    },
    LoadIntParam {
        dst: IntSlot,
        param: ParamId,
        /// Address in the bound parameter bank, checked against `param` at admission.
        source: IntSlot,
    },
    LoadFloatParam {
        dst: FloatSlot,
        param: ParamId,
        source: FloatSlot,
    },
    LoadBoolParam {
        dst: BoolSlot,
        param: ParamId,
        source: BoolSlot,
    },
    LoadColorParam {
        dst: ColorSlot,
        param: ParamId,
        source: ColorSlot,
    },
    LoadArrayParam {
        dst: ArraySlot,
        param: ParamId,
        source: ArraySlot,
    },
    Move {
        dst: ValueSlot,
        /// Source index in the destination's register bank.
        src: u32,
    },
    /// Branch-free choice between two primitive registers of the destination's
    /// bank. If-conversion emits it so conditional assignments stay hoistable.
    Choose {
        dst: ValueSlot,
        condition: BoolSlot,
        when_true: u32,
        when_false: u32,
    },
    MakeArray {
        dst: ArraySlot,
        items: PoolSpan,
    },
    Index {
        dst: ValueSlot,
        target: ArraySlot,
        index: NumberSlot,
        /// Slot index in the destination's bank, holding the empty-array result.
        default: u32,
    },
    /// Index an immutable array snapshot lowered to existing value slots.
    Select {
        dst: ValueSlot,
        items: PoolSpan,
        index: NumberSlot,
        /// Slot index in the destination's bank, holding the empty-array result.
        default: u32,
    },
    CurveParamSample {
        dst: FloatSlot,
        param: ParamId,
        source: CurveSlot,
        position: FloatSlot,
    },
    GradientParamSample {
        dst: ColorSlot,
        param: ParamId,
        source: GradientSlot,
        position: FloatSlot,
    },
    IntToFloat {
        dst: FloatSlot,
        src: IntSlot,
    },
    /// Truncates toward zero, saturating at the int range; NaN becomes zero.
    FloatToInt {
        dst: IntSlot,
        src: FloatSlot,
    },
    Not {
        dst: BoolSlot,
        src: BoolSlot,
    },
    NegInt {
        dst: IntSlot,
        src: IntSlot,
    },
    NegFloat {
        dst: FloatSlot,
        src: FloatSlot,
    },
    FloatAdd { dst: FloatSlot, left: FloatSlot, right: FloatSlot },
    FloatSubtract { dst: FloatSlot, left: FloatSlot, right: FloatSlot },
    FloatMultiply { dst: FloatSlot, left: FloatSlot, right: FloatSlot },
    /// Separate multiply and add semantics in one interpreter dispatch.
    FloatDivide { dst: FloatSlot, left: FloatSlot, right: FloatSlot },
    FloatRemainder { dst: FloatSlot, left: FloatSlot, right: FloatSlot },
    IntAdd { dst: IntSlot, left: IntSlot, right: IntSlot },
    IntSubtract { dst: IntSlot, left: IntSlot, right: IntSlot },
    IntMultiply { dst: IntSlot, left: IntSlot, right: IntSlot },
    IntRemainder { dst: IntSlot, left: IntSlot, right: IntSlot },
    FloatAddConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatSubtractConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatMultiplyConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatDivideConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatRemainderConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatSubtractFromConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatDivideIntoConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    FloatRemainderFromConst { dst: FloatSlot, value: FloatSlot, constant_bits: u32 },
    IntCompare {
        dst: BoolSlot,
        op: CompareOp,
        left: IntSlot,
        right: IntSlot,
    },
    FloatCompare {
        dst: BoolSlot,
        op: CompareOp,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatCompareConst {
        dst: BoolSlot,
        op: CompareOp,
        value: FloatSlot,
        constant_bits: u32,
        constant_left: bool,
    },
    ValueEqual {
        dst: BoolSlot,
        negate: bool,
        left: ValueSlot,
        right: ValueSlot,
    },
    EnumParamEqualConst {
        dst: BoolSlot,
        param: ParamId,
        source: EnumSlot,
        constant: ConstantId,
        negate: bool,
    },
    IntJumpLess { left: IntSlot, right: IntSlot, when: bool, target: Target },
    IntJumpLessEqual { left: IntSlot, right: IntSlot, when: bool, target: Target },
    IntJumpGreater { left: IntSlot, right: IntSlot, when: bool, target: Target },
    IntJumpGreaterEqual { left: IntSlot, right: IntSlot, when: bool, target: Target },
    IntJumpEqual { left: IntSlot, right: IntSlot, when: bool, target: Target },
    FloatJumpLess { left: FloatSlot, right: FloatSlot, when: bool, target: Target },
    FloatJumpLessEqual { left: FloatSlot, right: FloatSlot, when: bool, target: Target },
    FloatJumpGreater { left: FloatSlot, right: FloatSlot, when: bool, target: Target },
    FloatJumpGreaterEqual { left: FloatSlot, right: FloatSlot, when: bool, target: Target },
    FloatJumpEqual { left: FloatSlot, right: FloatSlot, when: bool, target: Target },
    FloatJumpLessConst { value: FloatSlot, constant_bits: u32, when: bool, target: Target },
    FloatJumpLessEqualConst { value: FloatSlot, constant_bits: u32, when: bool, target: Target },
    FloatJumpGreaterConst { value: FloatSlot, constant_bits: u32, when: bool, target: Target },
    FloatJumpGreaterEqualConst { value: FloatSlot, constant_bits: u32, when: bool, target: Target },
    FloatJumpEqualConst { value: FloatSlot, constant_bits: u32, when: bool, target: Target },
    Jump(value: Target),
    JumpIfFalse {
        condition: BoolSlot,
        target: Target,
    },
    JumpIfTrue {
        condition: BoolSlot,
        target: Target,
    },
    /// A numeric range with a language-bounded literal cap.
    LoopRangeStart {
        id: u32,
        count: IntSlot,
        cap: i32,
        end: Target,
    },
    /// Iterate one snapshotted Marks collection, never an arbitrary integer.
    LoopMarksStart {
        id: u32,
        marks: MarksSlot,
        end: Target,
    },
    LoopEnd {
        id: u32,
        start: Target,
    },
    SectionPosition {
        dst: FloatSlot,
        /// Normalized width and its reciprocal, shared at their dependency scope.
        width: FloatSlot,
        inverse: FloatSlot,
    },
    SectionQuery {
        dst: IntSlot,
        width: IntSlot,
        index: bool,
    },
    FloatUnary {
        dst: FloatSlot,
        op: FloatUnary,
        value: FloatSlot,
    },
    FloatBinary {
        dst: FloatSlot,
        op: FloatBinary,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatBinaryConst {
        dst: FloatSlot,
        op: FloatBinary,
        value: FloatSlot,
        constant_bits: u32,
    },
    Clamp {
        dst: FloatSlot,
        value: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    ClampConst {
        dst: FloatSlot,
        value: FloatSlot,
        min_bits: u32,
        max_bits: u32,
    },
    /// Clamped cubic interpolation of an already normalized position. Edge
    /// normalization uses ordinary arithmetic so invariant work can be hoisted.
    Smoothstep {
        dst: FloatSlot,
        value: FloatSlot,
    },
    MixFloat {
        dst: FloatSlot,
        left: FloatSlot,
        right: FloatSlot,
        amount: FloatSlot,
    },
    MixColor {
        dst: ColorSlot,
        left: ColorSlot,
        right: ColorSlot,
        amount: FloatSlot,
    },
    ColorBinary {
        dst: ColorSlot,
        op: ColorBinary,
        left: ColorSlot,
        right: ColorSlot,
    },
    ColorScale {
        dst: ColorSlot,
        color: ColorSlot,
        scale: FloatSlot,
    },
    ColorComponent {
        dst: FloatSlot,
        op: ColorComponent,
        color: ColorSlot,
    },
    ColorInvert {
        dst: ColorSlot,
        color: ColorSlot,
    },
    Rgb {
        dst: ColorSlot,
        red: FloatSlot,
        green: FloatSlot,
        blue: FloatSlot,
    },
    Hsv {
        dst: ColorSlot,
        hue: FloatSlot,
        saturation: FloatSlot,
        value: FloatSlot,
    },
    Rand {
        dst: FloatSlot,
        seed: FloatSlot,
    },
    CurveFloatClamped {
        dst: FloatSlot,
        curve: CurveSlot,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    CurveParamFloatClamped {
        dst: FloatSlot,
        param: ParamId,
        source: CurveSlot,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    GradientColorScaled {
        dst: ColorSlot,
        gradient: GradientSlot,
        position: FloatSlot,
        scale: FloatSlot,
    },
    GradientParamColorScaled {
        dst: ColorSlot,
        param: ParamId,
        source: GradientSlot,
        position: FloatSlot,
        scale: FloatSlot,
    },
    CurveCrossing {
        dst: FloatSlot,
        curve: CurveSlot,
        value: FloatSlot,
        /// None selects the first crossing; Some selects the last crossing
        /// at or before the supplied curve position.
        before: Option<FloatSlot>,
    },
    CurveParamCrossing {
        dst: FloatSlot,
        param: ParamId,
        source: CurveSlot,
        value: FloatSlot,
        /// None selects the first crossing; Some selects the last crossing
        /// at or before the supplied curve position.
        before: Option<FloatSlot>,
    },
    Len {
        dst: IntSlot,
        value: ArraySlot,
    },
    Mark {
        marks: MarksSlot,
        op: MarkOp,
    },
}

impl Instruction {
    pub fn conditional_target(&self) -> Option<Target> {
        match self {
            Self::JumpIfFalse { target, .. }
            | Self::JumpIfTrue { target, .. }
            | Self::IntJumpLess { target, .. }
            | Self::IntJumpLessEqual { target, .. }
            | Self::IntJumpGreater { target, .. }
            | Self::IntJumpGreaterEqual { target, .. }
            | Self::IntJumpEqual { target, .. }
            | Self::FloatJumpLess { target, .. }
            | Self::FloatJumpLessEqual { target, .. }
            | Self::FloatJumpGreater { target, .. }
            | Self::FloatJumpGreaterEqual { target, .. }
            | Self::FloatJumpEqual { target, .. }
            | Self::FloatJumpLessConst { target, .. }
            | Self::FloatJumpLessEqualConst { target, .. }
            | Self::FloatJumpGreaterConst { target, .. }
            | Self::FloatJumpGreaterEqualConst { target, .. }
            | Self::FloatJumpEqualConst { target, .. } => Some(*target),
            _ => None,
        }
    }
    pub fn jump_target(&self) -> Option<Target> {
        match self {
            Self::Jump(target)
            | Self::LoopRangeStart { end: target, .. }
            | Self::LoopMarksStart { end: target, .. }
            | Self::LoopEnd { start: target, .. } => Some(*target),
            _ => self.conditional_target(),
        }
    }
    pub fn jump_target_mut(&mut self) -> Option<&mut Target> {
        match self {
            Self::Jump(target)
            | Self::LoopRangeStart { end: target, .. }
            | Self::LoopMarksStart { end: target, .. }
            | Self::LoopEnd { start: target, .. }
            | Self::JumpIfFalse { target, .. }
            | Self::JumpIfTrue { target, .. }
            | Self::IntJumpLess { target, .. }
            | Self::IntJumpLessEqual { target, .. }
            | Self::IntJumpGreater { target, .. }
            | Self::IntJumpGreaterEqual { target, .. }
            | Self::IntJumpEqual { target, .. }
            | Self::FloatJumpLess { target, .. }
            | Self::FloatJumpLessEqual { target, .. }
            | Self::FloatJumpGreater { target, .. }
            | Self::FloatJumpGreaterEqual { target, .. }
            | Self::FloatJumpEqual { target, .. }
            | Self::FloatJumpLessConst { target, .. }
            | Self::FloatJumpLessEqualConst { target, .. }
            | Self::FloatJumpGreaterConst { target, .. }
            | Self::FloatJumpGreaterEqualConst { target, .. }
            | Self::FloatJumpEqualConst { target, .. } => Some(target),
            _ => None,
        }
    }
}

impl Instruction {
    pub(super) fn written_slot(&self) -> Option<ValueSlot> {
        use Instruction::*;
        Some(match self {
            LoadCurveConst { dst, .. } | LoadCurveParam { dst, .. } => ValueSlot::Curve(*dst),
            LoadGradientConst { dst, .. } | LoadGradientParam { dst, .. } => {
                ValueSlot::Gradient(*dst)
            }
            CurveSample { dst, .. } => ValueSlot::Float(*dst),
            GradientSample { dst, .. } => ValueSlot::Color(*dst),
            Move { dst, .. } | Choose { dst, .. } | Index { dst, .. } | Select { dst, .. } => *dst,
            ContextRead { dst, .. } => dst.value_slot(),
            LoadMarksConst { dst, .. } | LoadMarksParam { dst, .. } => ValueSlot::Marks(*dst),
            Mark { op, .. } => op.output(),
            LoadIntConst { dst, .. }
            | LoadIntParam { dst, .. }
            | NegInt { dst, .. }
            | IntAdd { dst, .. }
            | IntSubtract { dst, .. }
            | IntMultiply { dst, .. }
            | IntRemainder { dst, .. }
            | SectionQuery { dst, .. }
            | Len { dst, .. }
            | FloatToInt { dst, .. } => ValueSlot::Int(*dst),
            QuerySeconds { dst, .. }
            | QueryProgress { dst, .. }
            | LoadFloatConst { dst, .. }
            | LoadFloatParam { dst, .. }
            | CurveParamSample { dst, .. }
            | IntToFloat { dst, .. }
            | NegFloat { dst, .. }
            | FloatAdd { dst, .. }
            | FloatSubtract { dst, .. }
            | FloatMultiply { dst, .. }
            | FloatDivide { dst, .. }
            | FloatRemainder { dst, .. }
            | FloatAddConst { dst, .. }
            | FloatSubtractConst { dst, .. }
            | FloatMultiplyConst { dst, .. }
            | FloatDivideConst { dst, .. }
            | FloatRemainderConst { dst, .. }
            | FloatSubtractFromConst { dst, .. }
            | FloatDivideIntoConst { dst, .. }
            | FloatRemainderFromConst { dst, .. }
            | SectionPosition { dst, .. }
            | FloatUnary { dst, .. }
            | FloatBinary { dst, .. }
            | FloatBinaryConst { dst, .. }
            | Clamp { dst, .. }
            | ClampConst { dst, .. }
            | Smoothstep { dst, .. }
            | MixFloat { dst, .. }
            | ColorComponent { dst, .. }
            | Rand { dst, .. }
            | CurveFloatClamped { dst, .. }
            | CurveParamFloatClamped { dst, .. }
            | CurveCrossing { dst, .. }
            | CurveParamCrossing { dst, .. } => ValueSlot::Float(*dst),
            LoadBoolConst { dst, .. }
            | LoadBoolParam { dst, .. }
            | Not { dst, .. }
            | IntCompare { dst, .. }
            | FloatCompare { dst, .. }
            | FloatCompareConst { dst, .. }
            | ValueEqual { dst, .. }
            | EnumParamEqualConst { dst, .. } => ValueSlot::Bool(*dst),
            LoadColorConst { dst, .. }
            | LoadColorParam { dst, .. }
            | GradientParamSample { dst, .. }
            | SignalSample { dst, .. }
            | MixColor { dst, .. }
            | ColorBinary { dst, .. }
            | ColorScale { dst, .. }
            | ColorInvert { dst, .. }
            | Rgb { dst, .. }
            | Hsv { dst, .. }
            | GradientColorScaled { dst, .. }
            | GradientParamColorScaled { dst, .. } => ValueSlot::Color(*dst),
            LoadEnumConst { dst, .. } | LoadEnumParam { dst, .. } => ValueSlot::Enum(*dst),
            LoadArrayConst { dst, .. } | LoadArrayParam { dst, .. } | MakeArray { dst, .. } => {
                ValueSlot::Array(*dst)
            }
            IntJumpLess { .. }
            | IntJumpLessEqual { .. }
            | IntJumpGreater { .. }
            | IntJumpGreaterEqual { .. }
            | IntJumpEqual { .. }
            | FloatJumpLess { .. }
            | FloatJumpLessEqual { .. }
            | FloatJumpGreater { .. }
            | FloatJumpGreaterEqual { .. }
            | FloatJumpEqual { .. }
            | FloatJumpLessConst { .. }
            | FloatJumpLessEqualConst { .. }
            | FloatJumpGreaterConst { .. }
            | FloatJumpGreaterEqualConst { .. }
            | FloatJumpEqualConst { .. }
            | Jump(_)
            | JumpIfFalse { .. }
            | JumpIfTrue { .. }
            | LoopRangeStart { .. }
            | LoopMarksStart { .. }
            | LoopEnd { .. }
            | ReturnColor(_) => return None,
        })
    }
}

#[cfg(test)]
mod representation_tests {
    use super::{
        BytecodeProgram, ColorSlot, CurveSlot, Instruction, ParameterKind, PoolSpan,
        ProgramContext, SignalPixel, SlotLayout, ValueSlot,
    };
    use alloc::{boxed::Box, vec};

    #[test]
    fn bytecode_headers_stay_compact() {
        assert!(size_of::<Instruction>() <= 32);
        // Nine register banks and typed curve/gradient constant pools.
        assert!(
            size_of::<BytecodeProgram>() <= 256,
            "{}",
            size_of::<BytecodeProgram>()
        );
    }

    #[test]
    fn malformed_bytecode_references_are_rejected_before_execution() {
        let mut program = BytecodeProgram {
            instructions: vec![Instruction::ReturnColor(ColorSlot(0))].into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());

        program.instructions = vec![Instruction::ReturnColor(ColorSlot(1))].into_boxed_slice();
        assert!(!program.has_valid_structure());

        program.instructions = vec![Instruction::Jump(2)].into_boxed_slice();
        assert!(!program.has_valid_structure());

        program.instructions = vec![Instruction::Rand {
            dst: super::FloatSlot(0),
            seed: super::FloatSlot(1),
        }]
        .into_boxed_slice();
        program.layout.floats = 1;
        program.value_operands = vec![ValueSlot::Float(super::FloatSlot(0))].into_boxed_slice();
        assert!(!program.has_valid_structure());

        program.instructions = vec![
            Instruction::Select {
                dst: ValueSlot::Float(super::FloatSlot(0)),
                items: PoolSpan { start: 1, len: 1 },
                index: super::NumberSlot::Float(super::FloatSlot(0)),
                default: 0,
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]
        .into_boxed_slice();
        assert!(!program.has_valid_structure());
        if let Instruction::Select { items, .. } = &mut program.instructions[0] {
            items.start = 0;
        }
        assert!(program.has_valid_structure());

        program.instructions = vec![
            Instruction::LoadFloatParam {
                dst: super::FloatSlot(0),
                param: 0,
                source: super::FloatSlot(0),
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]
        .into_boxed_slice();
        assert!(program.has_valid_structure());
        assert!(program.has_valid_parameter_reads(|_| Some(ParameterKind::Float)));
        assert!(!program.has_valid_parameter_reads(|_| Some(ParameterKind::Int)));
        assert!(!program.has_valid_parameter_reads(|_| Some(ParameterKind::Bool)));
        assert!(!program.has_valid_parameter_reads(|_| None));
        if let Instruction::LoadFloatParam { source, .. } = &mut program.instructions[0] {
            source.0 = 1;
        }
        assert!(!program.has_valid_parameter_reads(|_| Some(ParameterKind::Float)));

        program.layout.ints = 2;
        program.layout.bools = 1;
        program.instructions = vec![
            Instruction::Move {
                dst: ValueSlot::Bool(super::BoolSlot(0)),
                src: 1,
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]
        .into_boxed_slice();
        // Index 1 exists in the int bank, but a boolean copy can only address
        // the boolean bank. Its source has no independently selectable type.
        assert!(!program.has_valid_structure());
        program.instructions[0] = Instruction::Move {
            dst: ValueSlot::Bool(super::BoolSlot(0)),
            src: 0,
        };
        assert!(program.has_valid_structure());
    }

    #[test]
    fn pixel_entry_requires_an_immutable_frame_uniform_prefix() {
        use super::{ContextRead, FloatSlot};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadFloatParam {
                    dst: FloatSlot(0),
                    param: 0,
                    source: FloatSlot(0),
                },
                Instruction::ContextRead {
                    dst: super::NumberSlot::Float(FloatSlot(1)),
                    read: ContextRead::PixelFraction,
                },
                Instruction::Rgb {
                    dst: ColorSlot(0),
                    red: FloatSlot(0),
                    green: FloatSlot(1),
                    blue: FloatSlot(0),
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                floats: 2,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: true,
            pixel_entry: 1,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());

        program.pixel_entry = 2;
        assert!(!program.has_valid_structure());

        program.pixel_entry = 1;
        program.instructions[1] = Instruction::LoadFloatParam {
            dst: FloatSlot(0),
            param: 1,
            source: FloatSlot(1),
        };
        program.uses_pixel_context = false;
        assert!(!program.has_valid_structure());

        program.instructions[1] = Instruction::FloatAdd {
            dst: FloatSlot(1),
            left: FloatSlot(0),
            right: FloatSlot(1),
        };
        program.pixel_entry = 2;
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn full_frame_signal_cache_requires_a_uniform_time() {
        use super::{ContextRead, FloatSlot};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadFloatParam {
                    dst: FloatSlot(0),
                    param: 0,
                    source: FloatSlot(0),
                },
                Instruction::ContextRead {
                    dst: super::NumberSlot::Float(FloatSlot(1)),
                    read: ContextRead::PixelFraction,
                },
                Instruction::SignalSample {
                    capability: (),
                    dst: ColorSlot(0),
                    input: 0,
                    seconds: FloatSlot(1),
                    pixel: SignalPixel::Current,
                    frame_cache: 0,
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                floats: 2,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: true,
            pixel_entry: 1,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(!program.has_valid_structure());

        if let Instruction::SignalSample { seconds, .. } = &mut program.instructions[2] {
            *seconds = FloatSlot(0);
        }
        assert!(program.has_valid_structure());
        assert!(program.has_valid_context(ProgramContext::Operator { inputs: 1 }));

        if let Instruction::SignalSample { frame_cache, .. } = &mut program.instructions[2] {
            *frame_cache = 2;
        }
        assert!(!program.has_valid_structure());
        if let Instruction::SignalSample { frame_cache, .. } = &mut program.instructions[2] {
            *frame_cache = u32::MAX;
        }
        assert!(program.has_valid_structure());
    }

    #[test]
    fn reference_register_types_reject_wrong_values_and_opcode_operands() {
        use super::{ArraySlot, Type, Value};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadArrayConst {
                    dst: ArraySlot(0),
                    constant: 0,
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![vec![Value::Int(3)].into()].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![Type::array(Type::Int)].into_boxed_slice(),
            layout: SlotLayout {
                arrays: 1,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());

        program.array_types[0] = Type::Curve;
        assert!(!program.has_valid_structure());
        program.array_types[0] = Type::array(Type::Int);
        program.instructions[0] = Instruction::CurveFloatClamped {
            dst: super::FloatSlot(0),
            curve: CurveSlot(0),
            position: super::FloatSlot(0),
            min: super::FloatSlot(0),
            max: super::FloatSlot(0),
        };
        program.layout.floats = 1;
        assert!(!program.has_valid_structure());

        program.instructions[0] = Instruction::LoadArrayParam {
            dst: ArraySlot(0),
            param: 0,
            source: ArraySlot(0),
        };
        assert!(program.has_valid_structure());
        assert!(program.has_valid_parameter_reads(|_| Some(ParameterKind::Array)));
        assert!(
            !program.has_valid_reference_parameter_reads(|_, expected| {
                expected.accepts(&Type::Marks)
            })
        );
        assert!(program.has_valid_reference_parameter_reads(|_, expected| {
            expected.accepts(&Type::array(Type::Int))
        }));

        program.instructions[0] = Instruction::ContextRead {
            dst: super::NumberSlot::Int(super::IntSlot(0)),
            read: super::ContextRead::Seconds,
        };
        program.layout.ints = 1;
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn indexing_checks_the_numeric_bank_and_target_index_type() {
        use super::{ArraySlot, FloatSlot, IntSlot, NumberSlot, Type};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadArrayParam {
                    dst: ArraySlot(0),
                    param: 0,
                    source: ArraySlot(0),
                },
                Instruction::Index {
                    dst: ValueSlot::Float(FloatSlot(0)),
                    target: ArraySlot(0),
                    index: NumberSlot::Int(IntSlot(0)),
                    default: 0,
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![Type::array(Type::Float)].into_boxed_slice(),
            layout: SlotLayout {
                ints: 1,
                floats: 2,
                arrays: 1,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());
        for (target, index, valid) in [
            (Type::array(Type::Float), NumberSlot::Int(IntSlot(1)), false),
            (
                Type::array(Type::Float),
                NumberSlot::Float(FloatSlot(1)),
                false,
            ),
        ] {
            program.array_types[0] = target;
            if let Instruction::Index { index: operand, .. } = &mut program.instructions[1] {
                *operand = index;
            }
            assert_eq!(program.has_valid_structure(), valid);
        }
        program.array_types = Box::new([]);
        program.layout.arrays = 0;
        program.layout.curves = 1;
        program.instructions[0] = Instruction::LoadCurveParam {
            dst: CurveSlot(0),
            param: 0,
            source: CurveSlot(0),
        };
        program.instructions[1] = Instruction::CurveSample {
            dst: FloatSlot(0),
            curve: CurveSlot(0),
            position: FloatSlot(1),
        };
        assert!(program.has_valid_structure());
        program.instructions[0] = Instruction::LoadCurveConst {
            dst: CurveSlot(0),
            constant: 0,
        };
        assert!(!program.has_valid_structure());
        program.curves = Box::new([super::Arc::new(crate::values::Curve { points: vec![] })]);
        assert!(program.has_valid_structure());
        if let Instruction::CurveSample { position, .. } = &mut program.instructions[1] {
            position.0 = 2;
        }
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn only_paired_counted_loops_can_jump_backward() {
        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadIntConst {
                    dst: super::IntSlot(0),
                    value: 2,
                },
                Instruction::LoopRangeStart {
                    id: 0,
                    count: super::IntSlot(0),
                    cap: 3,
                    end: 2,
                },
                Instruction::LoopEnd { id: 0, start: 2 },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                ints: 1,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 1,
        };
        assert!(program.has_valid_structure());

        program.instructions[2] = Instruction::LoopEnd { id: 0, start: 1 };
        assert!(!program.has_valid_structure());
        program.instructions[2] = Instruction::LoopEnd { id: 0, start: 2 };
        program.instructions[1] = Instruction::Jump(0);
        assert!(!program.has_valid_structure());
        program.instructions[1] = Instruction::LoopRangeStart {
            id: 0,
            count: super::IntSlot(0),
            cap: 0,
            end: 2,
        };
        assert!(!program.has_valid_structure());
        program.instructions[1] = Instruction::LoopRangeStart {
            id: 0,
            count: super::IntSlot(0),
            cap: super::super::MAX_DSL_LOOP_ITERATIONS as i32 + 1,
            end: 2,
        };
        assert!(!program.has_valid_structure());
        program.instructions[1] = Instruction::LoopRangeStart {
            id: 0,
            count: super::IntSlot(0),
            cap: 3,
            end: 2,
        };
        program.loop_count = 2;
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn marks_loop_bound_must_come_from_initialized_marks() {
        use super::MarksSlot;

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadMarksParam {
                    dst: MarksSlot(0),
                    param: 0,
                    source: MarksSlot(0),
                },
                Instruction::LoopMarksStart {
                    id: 0,
                    marks: MarksSlot(0),
                    end: 2,
                },
                Instruction::LoopEnd { id: 0, start: 2 },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                marks: 1,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 1,
        };
        assert!(program.has_valid_structure());

        assert!(program.has_valid_parameter_reads(|_| Some(ParameterKind::Marks)));
        assert!(!program.has_valid_parameter_reads(|_| Some(ParameterKind::Curve)));
        program.instructions[0] = Instruction::Jump(1);
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn playback_context_rejects_wrong_returns_and_signal_inputs() {
        let mut program = BytecodeProgram {
            instructions: vec![Instruction::ReturnColor(ColorSlot(0))].into_boxed_slice(),
            curves: Box::new([]),
            gradients: Box::new([]),
            enums: Box::new([]),
            enum_types: Box::new([]),
            array_constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            array_types: vec![].into_boxed_slice(),
            layout: SlotLayout {
                floats: 1,
                colors: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 0,
            array_width: 0,
            loop_count: 0,
        };
        assert!(program.has_valid_context(ProgramContext::Effect));

        program.instructions = vec![
            Instruction::SignalSample {
                capability: (),
                dst: ColorSlot(0),
                input: 0,
                seconds: super::FloatSlot(0),
                pixel: SignalPixel::Current,
                frame_cache: u32::MAX,
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]
        .into_boxed_slice();
        program.uses_pixel_context = true;
        assert!(program.has_valid_structure());
        assert!(program.has_valid_context(ProgramContext::Operator { inputs: 1 }));
        assert!(!program.has_valid_context(ProgramContext::Operator { inputs: 0 }));
        assert!(!program.has_valid_context(ProgramContext::Effect));
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ContextRead {
    PixelX,
    PixelY,
    TargetMinX,
    TargetMinY,
    TargetMaxX,
    TargetMaxY,

    Progress,
    Seconds,
    Duration,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum FloatUnary {
    Sin,
    Cos,
    Abs,
    Floor,
    Ceil,
    Trunc,
    Sqrt,
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
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum FloatBinary {
    Min,
    Max,
    /// Preserve the left operand unless it is NaN; otherwise use the right.
    ValueOr,
    /// Angle in radians: left is y, right is x.
    Atan2,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum MarkOp {
    Count { dst: IntSlot },
    At { dst: FloatSlot, index: IntSlot },
    Last { dst: FloatSlot, seconds: FloatSlot },
    LastIndex { dst: IntSlot, seconds: FloatSlot },
}

impl MarkOp {
    fn output(self) -> ValueSlot {
        match self {
            Self::Count { dst } | Self::LastIndex { dst, .. } => ValueSlot::Int(dst),
            Self::At { dst, .. } | Self::Last { dst, .. } => ValueSlot::Float(dst),
        }
    }

    fn inputs(self) -> [Option<NumberSlot>; 1] {
        match self {
            Self::Count { .. } => [None],
            Self::At { index, .. } => [Some(NumberSlot::Int(index))],
            Self::Last { seconds, .. } | Self::LastIndex { seconds, .. } => {
                [Some(NumberSlot::Float(seconds))]
            }
        }
    }
}

/// A numeric builtin operand. Keeping the original integer form avoids
/// rounding an index through f32 before converting it back to an integer.
#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum NumberSlot {
    Int(IntSlot),
    Float(FloatSlot),
}

impl NumberSlot {
    pub fn value_slot(self) -> ValueSlot {
        match self {
            Self::Int(slot) => ValueSlot::Int(slot),
            Self::Float(slot) => ValueSlot::Float(slot),
        }
    }
}

/// One exhaustive register-operand description, shared by compilation and
/// batch planning.
/// Operand spans are unique per instruction in compiler output.
pub(super) fn slots(
    op: &mut Instruction,
    operands: &mut [ValueSlot],
    mut visit: impl FnMut(ValueSlot, bool) -> ValueSlot,
) {
    macro_rules! typed {
        ($write:expr, $kind:ident, $($slot:ident),+) => {{$(
            let ValueSlot::$kind(mapped) = visit(ValueSlot::$kind(*$slot), $write) else { unreachable!("compiler register remapping preserves types") };
            *$slot = mapped;
        )+}};
    }
    macro_rules! number {
        ($operand:expr) => {
            match $operand {
                NumberSlot::Int(slot) => typed!(false, Int, slot),
                NumberSlot::Float(slot) => typed!(false, Float, slot),
            }
        };
    }
    match op {
        Instruction::LoadCurveConst { dst, .. } | Instruction::LoadCurveParam { dst, .. } => {
            typed!(true, Curve, dst)
        }
        Instruction::LoadGradientConst { dst, .. } | Instruction::LoadGradientParam { dst, .. } => {
            typed!(true, Gradient, dst)
        }
        Instruction::CurveSample {
            dst,
            curve,
            position,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, position);
            typed!(true, Float, dst);
        }
        Instruction::GradientSample {
            dst,
            gradient,
            position,
        } => {
            typed!(false, Gradient, gradient);
            typed!(false, Float, position);
            typed!(true, Color, dst);
        }
        Instruction::ContextRead { dst, .. } => match dst {
            NumberSlot::Int(slot) => typed!(true, Int, slot),
            NumberSlot::Float(slot) => typed!(true, Float, slot),
        },
        Instruction::LoadIntConst { dst, .. } | Instruction::LoadIntParam { dst, .. } => {
            typed!(true, Int, dst)
        }
        Instruction::LoadFloatConst { dst, .. } | Instruction::LoadFloatParam { dst, .. } => {
            typed!(true, Float, dst)
        }
        Instruction::LoadBoolConst { dst, .. }
        | Instruction::LoadBoolParam { dst, .. }
        | Instruction::EnumParamEqualConst { dst, .. } => {
            typed!(true, Bool, dst)
        }
        Instruction::LoadColorConst { dst, .. } | Instruction::LoadColorParam { dst, .. } => {
            typed!(true, Color, dst)
        }
        Instruction::LoadEnumConst { dst, .. } | Instruction::LoadEnumParam { dst, .. } => {
            typed!(true, Enum, dst);
        }
        Instruction::LoadArrayConst { dst, .. } | Instruction::LoadArrayParam { dst, .. } => {
            typed!(true, Array, dst)
        }
        Instruction::LoadMarksConst { dst, .. } | Instruction::LoadMarksParam { dst, .. } => {
            typed!(true, Marks, dst)
        }
        Instruction::Move { dst, src } => {
            *src = visit(dst.with_index(*src), false).index();
            *dst = visit(*dst, true);
        }
        Instruction::Choose {
            dst,
            condition,
            when_true,
            when_false,
        } => {
            typed!(false, Bool, condition);
            *when_true = visit(dst.with_index(*when_true), false).index();
            *when_false = visit(dst.with_index(*when_false), false).index();
            *dst = visit(*dst, true);
        }
        Instruction::MakeArray { dst, items } => {
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            typed!(true, Array, dst);
        }
        Instruction::Index {
            dst,
            target,
            index,
            default,
        } => {
            *default = visit(dst.with_index(*default), false).index();
            typed!(false, Array, target);
            number!(index);
            *dst = visit(*dst, true);
        }
        Instruction::Select {
            dst,
            items,
            index,
            default,
        } => {
            *default = visit(dst.with_index(*default), false).index();
            for slot in &mut operands[items.start as usize..(items.start + items.len) as usize] {
                *slot = visit(*slot, false);
            }
            number!(index);
            *dst = visit(*dst, true);
        }
        Instruction::CurveParamSample { dst, position, .. } => {
            typed!(false, Float, position);
            typed!(true, Float, dst);
        }
        Instruction::GradientParamSample { dst, position, .. } => {
            typed!(false, Float, position);
            typed!(true, Color, dst);
        }
        Instruction::SignalSample {
            dst,
            seconds,
            pixel,
            ..
        } => {
            typed!(false, Float, seconds);
            *pixel = pixel.map(|mut slot| {
                let index = &mut slot;
                typed!(false, Int, index);
                slot
            });
            typed!(true, Color, dst);
        }
        Instruction::IntToFloat { dst, src } => {
            typed!(false, Int, src);
            typed!(true, Float, dst);
        }
        Instruction::FloatToInt { dst, src } => {
            typed!(false, Float, src);
            typed!(true, Int, dst);
        }
        Instruction::Not { dst, src } => {
            typed!(false, Bool, src);
            typed!(true, Bool, dst);
        }
        Instruction::NegInt { dst, src } => {
            typed!(false, Int, src);
            typed!(true, Int, dst);
        }
        Instruction::NegFloat { dst, src } => {
            typed!(false, Float, src);
            typed!(true, Float, dst);
        }
        Instruction::FloatAdd {
            dst, left, right, ..
        }
        | Instruction::FloatSubtract {
            dst, left, right, ..
        }
        | Instruction::FloatMultiply {
            dst, left, right, ..
        }
        | Instruction::FloatDivide {
            dst, left, right, ..
        }
        | Instruction::FloatRemainder {
            dst, left, right, ..
        }
        | Instruction::FloatBinary {
            dst, left, right, ..
        } => {
            typed!(false, Float, left, right);
            typed!(true, Float, dst);
        }
        Instruction::IntAdd {
            dst, left, right, ..
        }
        | Instruction::IntSubtract {
            dst, left, right, ..
        }
        | Instruction::IntMultiply {
            dst, left, right, ..
        }
        | Instruction::IntRemainder {
            dst, left, right, ..
        } => {
            typed!(false, Int, left, right);
            typed!(true, Int, dst);
        }
        Instruction::FloatCompare {
            dst, left, right, ..
        } => {
            typed!(false, Float, left, right);
            typed!(true, Bool, dst);
        }
        Instruction::IntCompare {
            dst, left, right, ..
        } => {
            typed!(false, Int, left, right);
            typed!(true, Bool, dst);
        }
        Instruction::FloatCompareConst { dst, value, .. } => {
            typed!(false, Float, value);
            typed!(true, Bool, dst);
        }
        Instruction::ValueEqual {
            dst, left, right, ..
        } => {
            *left = visit(*left, false);
            *right = visit(*right, false);
            typed!(true, Bool, dst);
        }
        Instruction::IntJumpLess { left, right, .. }
        | Instruction::IntJumpLessEqual { left, right, .. }
        | Instruction::IntJumpGreater { left, right, .. }
        | Instruction::IntJumpGreaterEqual { left, right, .. }
        | Instruction::IntJumpEqual { left, right, .. } => {
            typed!(false, Int, left, right);
        }
        Instruction::FloatJumpLess { left, right, .. }
        | Instruction::FloatJumpLessEqual { left, right, .. }
        | Instruction::FloatJumpGreater { left, right, .. }
        | Instruction::FloatJumpGreaterEqual { left, right, .. }
        | Instruction::FloatJumpEqual { left, right, .. } => {
            typed!(false, Float, left, right);
        }
        Instruction::FloatJumpLessConst { value, .. }
        | Instruction::FloatJumpLessEqualConst { value, .. }
        | Instruction::FloatJumpGreaterConst { value, .. }
        | Instruction::FloatJumpGreaterEqualConst { value, .. }
        | Instruction::FloatJumpEqualConst { value, .. } => {
            typed!(false, Float, value);
        }
        Instruction::JumpIfFalse { condition, .. } | Instruction::JumpIfTrue { condition, .. } => {
            typed!(false, Bool, condition)
        }
        Instruction::LoopRangeStart { count, .. } => typed!(false, Int, count),
        Instruction::LoopMarksStart { marks, .. } => typed!(false, Marks, marks),
        Instruction::SectionQuery { dst, width, .. } => {
            typed!(false, Int, width);
            typed!(true, Int, dst);
        }
        Instruction::SectionPosition {
            dst,
            width,
            inverse,
        } => {
            typed!(false, Float, width, inverse);
            typed!(true, Float, dst);
        }
        Instruction::QuerySeconds {
            dst,
            seconds: value,
        }
        | Instruction::QueryProgress {
            dst,
            seconds: value,
        }
        | Instruction::FloatAddConst { dst, value, .. }
        | Instruction::FloatSubtractConst { dst, value, .. }
        | Instruction::FloatMultiplyConst { dst, value, .. }
        | Instruction::FloatDivideConst { dst, value, .. }
        | Instruction::FloatRemainderConst { dst, value, .. }
        | Instruction::FloatSubtractFromConst { dst, value, .. }
        | Instruction::FloatDivideIntoConst { dst, value, .. }
        | Instruction::FloatRemainderFromConst { dst, value, .. }
        | Instruction::FloatUnary { dst, value, .. }
        | Instruction::FloatBinaryConst { dst, value, .. }
        | Instruction::ClampConst { dst, value, .. } => {
            typed!(false, Float, value);
            typed!(true, Float, dst);
        }
        Instruction::Clamp {
            dst,
            value,
            min,
            max,
        } => {
            typed!(false, Float, value, min, max);
            typed!(true, Float, dst);
        }
        Instruction::Smoothstep { dst, value } => {
            typed!(false, Float, value);
            typed!(true, Float, dst);
        }
        Instruction::MixFloat {
            dst,
            left,
            right,
            amount,
        } => {
            typed!(false, Float, left, right, amount);
            typed!(true, Float, dst);
        }
        Instruction::MixColor {
            dst,
            left,
            right,
            amount,
        } => {
            typed!(false, Color, left, right);
            typed!(false, Float, amount);
            typed!(true, Color, dst);
        }
        Instruction::ColorBinary {
            dst, left, right, ..
        } => {
            typed!(false, Color, left, right);
            typed!(true, Color, dst);
        }
        Instruction::ColorScale { dst, color, scale } => {
            typed!(false, Color, color);
            typed!(false, Float, scale);
            typed!(true, Color, dst);
        }
        Instruction::ColorComponent { dst, color, .. } => {
            typed!(false, Color, color);
            typed!(true, Float, dst);
        }
        Instruction::ColorInvert { dst, color } => {
            typed!(false, Color, color);
            typed!(true, Color, dst);
        }
        Instruction::Rgb {
            dst,
            red,
            green,
            blue,
        } => {
            typed!(false, Float, red, green, blue);
            typed!(true, Color, dst);
        }
        Instruction::Hsv {
            dst,
            hue,
            saturation,
            value,
        } => {
            typed!(false, Float, hue, saturation, value);
            typed!(true, Color, dst);
        }
        Instruction::Rand { dst, seed } => {
            typed!(false, Float, seed);
            typed!(true, Float, dst);
        }
        Instruction::CurveFloatClamped {
            dst,
            curve,
            position,
            min,
            max,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, position, min, max);
            typed!(true, Float, dst);
        }
        Instruction::CurveParamFloatClamped {
            dst,
            position,
            min,
            max,
            ..
        } => {
            typed!(false, Float, position, min, max);
            typed!(true, Float, dst);
        }
        Instruction::GradientColorScaled {
            dst,
            gradient,
            position,
            scale,
        } => {
            typed!(false, Gradient, gradient);
            typed!(false, Float, position, scale);
            typed!(true, Color, dst);
        }
        Instruction::GradientParamColorScaled {
            dst,
            position,
            scale,
            ..
        } => {
            typed!(false, Float, position, scale);
            typed!(true, Color, dst);
        }
        Instruction::CurveCrossing {
            dst,
            curve,
            value,
            before,
        } => {
            typed!(false, Curve, curve);
            typed!(false, Float, value);
            if let Some(before) = before {
                typed!(false, Float, before);
            }
            typed!(true, Float, dst);
        }
        Instruction::CurveParamCrossing {
            dst, value, before, ..
        } => {
            typed!(false, Float, value);
            if let Some(before) = before {
                typed!(false, Float, before);
            }
            typed!(true, Float, dst);
        }
        Instruction::Len { dst, value } => {
            typed!(false, Array, value);
            typed!(true, Int, dst);
        }
        Instruction::Mark { marks, op } => {
            typed!(false, Marks, marks);
            match op {
                MarkOp::Count { dst } => typed!(true, Int, dst),
                MarkOp::At { dst, index } => {
                    typed!(false, Int, index);
                    typed!(true, Float, dst);
                }
                MarkOp::Last { dst, seconds } => {
                    typed!(false, Float, seconds);
                    typed!(true, Float, dst);
                }
                MarkOp::LastIndex { dst, seconds } => {
                    typed!(false, Float, seconds);
                    typed!(true, Int, dst);
                }
            }
        }
        Instruction::ReturnColor(value) => typed!(false, Color, value),
        Instruction::Jump(_) | Instruction::LoopEnd { .. } => {}
    }
}
