use super::types::{Type, Value};
use alloc::{boxed::Box, collections::BTreeSet, vec, vec::Vec};

fn slot_key(slot: ValueSlot) -> (u8, u32) {
    match slot {
        ValueSlot::Int(slot) => (0, slot.0),
        ValueSlot::Float(slot) => (1, slot.0),
        ValueSlot::Bool(slot) => (2, slot.0),
        ValueSlot::Color(slot) => (3, slot.0),
        ValueSlot::Ref(slot) => (4, slot.0),
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
    Curve,
    Gradient,
    Enum,
    Reference,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProgramContext {
    Effect,
    Operator { inputs: usize },
    Calculation,
}

impl ParameterKind {
    pub fn for_type(ty: &Type) -> Self {
        match ty {
            Type::Void => Self::Void,
            Type::Int => Self::Int,
            Type::Float => Self::Float,
            Type::Bool => Self::Bool,
            Type::Color => Self::Color,
            Type::Curve => Self::Curve,
            Type::Gradient => Self::Gradient,
            Type::Enum(_) => Self::Enum,
            Type::Signal
            | Type::Marks
            | Type::Timeline
            | Type::Target
            | Type::TargetItems
            | Type::TargetItem
            | Type::Array(_) => Self::Reference,
        }
    }

    fn is_reference(self) -> bool {
        matches!(
            self,
            Self::Curve | Self::Gradient | Self::Enum | Self::Reference
        )
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
pub struct BytecodeProgram {
    pub instructions: Box<[Instruction]>,
    pub constants: Box<[Value]>,
    pub value_operands: Box<[ValueSlot]>,
    /// Compiler-owned type of each reference register, in register order.
    pub ref_types: Box<[Type]>,
    pub layout: SlotLayout,
    /// Compiler-proven dependency on pixel geometry or an upstream signal.
    pub uses_pixel_context: bool,
    /// First pixel-dependent instruction, following pure frame initialization.
    pub pixel_entry: u32,
    /// Conservative live calculated-array bound, including construction space.
    pub array_capacity: u32,
    pub array_width: u32,
    /// Number of private counted-loop states reserved by this program.
    pub loop_count: u32,
}

impl BytecodeProgram {
    pub fn has_valid_context(&self, context: ProgramContext) -> bool {
        let mut has_return = false;
        for instruction in &self.instructions {
            match instruction {
                Instruction::LoadGeneratorContext { .. } | Instruction::Emit { .. } => {
                    return false;
                }
                Instruction::SignalSample { input, .. } => {
                    if !matches!(context, ProgramContext::Operator { inputs } if *input < inputs) {
                        return false;
                    }
                }
                Instruction::Return(_) => {
                    if context != ProgramContext::Calculation {
                        return false;
                    }
                    let Instruction::Return(ValueSlot::Ref(slot)) = instruction else {
                        return false;
                    };
                    if !matches!(self.ref_types.get(slot.0 as usize), Some(Type::Array(_))) {
                        return false;
                    }
                    has_return = true;
                }
                Instruction::ReturnColor(_) => {
                    if context == ProgramContext::Calculation {
                        return false;
                    }
                    has_return = true;
                }
                _ => {}
            }
        }
        has_return && (context != ProgramContext::Calculation || !self.uses_pixel_context)
    }

    /// Retained calculations return one internal tuple. Its element types are
    /// erased to `array<void>` in the register layout, so the surrounding
    /// parameter environment must prove their actual types before admission.
    pub fn has_valid_calculation_outputs(&self, outputs: &[Type]) -> bool {
        let Some(Instruction::Return(ValueSlot::Ref(result))) = self.instructions.last() else {
            return false;
        };
        if !matches!(self.ref_types.get(result.0 as usize), Some(Type::Array(item)) if **item == Type::Void)
        {
            return false;
        }
        let return_ip = self.instructions.len() - 1;
        if return_ip == 0
            || self.instructions[..return_ip].iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Return(_) | Instruction::ReturnColor(_)
                )
            })
        {
            return false;
        }
        if self.instructions[..return_ip]
            .iter()
            .any(|instruction| match instruction {
                Instruction::Jump(target)
                | Instruction::JumpIfFalse { target, .. }
                | Instruction::JumpIfTrue { target, .. } => *target == return_ip,
                _ => false,
            })
        {
            return false;
        }
        match &self.instructions[return_ip - 1] {
            Instruction::MakeArray { dst, items } if dst == result => {
                self.value_operands(*items).is_some_and(|items| {
                    items.len() == outputs.len()
                        && items.iter().zip(outputs).all(|(slot, expected)| {
                            self.slot_type(*slot)
                                .is_some_and(|actual| expected.accepts(actual))
                        })
                })
            }
            Instruction::LoadConst {
                dst: ValueSlot::Ref(dst),
                constant,
            } if dst == result => {
                matches!(self.constants.get(*constant), Some(Value::Array(items)) if items.len() == outputs.len() && items.iter().zip(outputs).all(|(item, expected)| expected.accepts_value(item)))
            }
            _ => false,
        }
    }

    fn slot_type(&self, slot: ValueSlot) -> Option<&Type> {
        match slot {
            ValueSlot::Int(_) => Some(&Type::Int),
            ValueSlot::Float(_) => Some(&Type::Float),
            ValueSlot::Bool(_) => Some(&Type::Bool),
            ValueSlot::Color(_) => Some(&Type::Color),
            ValueSlot::Ref(slot) => self.ref_types.get(slot.0 as usize),
        }
    }

    /// Check every parameter opcode against the invocation's parameter kinds.
    /// The caller supplies authored types during compilation and admitted bound
    /// values or retained-environment types when loading portable bytecode.
    pub fn has_valid_parameter_reads(
        &self,
        kind_at: impl Fn(ParamId) -> Option<ParameterKind>,
    ) -> bool {
        self.instructions.iter().all(|instruction| {
            use Instruction::*;
            match instruction {
                LoadIntParam { param, .. } | LoadFloatParam { param, .. } => matches!(
                    kind_at(*param),
                    Some(ParameterKind::Int | ParameterKind::Float)
                ),
                LoadBoolParam { param, .. } => kind_at(*param) == Some(ParameterKind::Bool),
                LoadColorParam { param, .. } => kind_at(*param) == Some(ParameterKind::Color),
                LoadRefParam { param, .. } => {
                    kind_at(*param).is_some_and(ParameterKind::is_reference)
                }
                CurveParamSample { param, .. }
                | CurveParamFloatClamped { param, .. }
                | CurveParamCrossing { param, .. } => kind_at(*param) == Some(ParameterKind::Curve),
                GradientParamSample { param, .. } | GradientParamColorScaled { param, .. } => {
                    kind_at(*param) == Some(ParameterKind::Gradient)
                }
                EnumParamEqualConst { param, .. } => kind_at(*param) == Some(ParameterKind::Enum),
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
                Instruction::LoadRefParam { dst, param } => self
                    .ref_types
                    .get(dst.0 as usize)
                    .is_some_and(|ty| accepts(*param, ty)),
                _ => true,
            })
    }

    /// Reject malformed register and instruction references before a portable
    /// program can reach the unchecked register access in the VM.
    pub fn has_valid_structure(&self) -> bool {
        let valid_slot = |slot: ValueSlot| match slot {
            ValueSlot::Int(slot) => slot.0 < self.layout.ints,
            ValueSlot::Float(slot) => slot.0 < self.layout.floats,
            ValueSlot::Bool(slot) => slot.0 < self.layout.bools,
            ValueSlot::Color(slot) => slot.0 < self.layout.colors,
            ValueSlot::Ref(slot) => slot.0 < self.layout.refs,
        };
        let valid_pool = |span: PoolSpan| {
            (span.start as usize)
                .checked_add(span.len as usize)
                .and_then(|end| self.value_operands.get(span.start as usize..end))
                .is_some_and(|slots| slots.iter().copied().all(valid_slot))
        };
        let uses_pixel_context = self.instructions.iter().any(|instruction| {
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
                    | Instruction::SignalSample { .. }
            )
        });
        if !self.has_valid_pixel_entry()
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
                    LoadConst { dst, constant } => {
                        valid_slot(*dst)
                            && self.constants.get(*constant).is_some_and(|value| {
                                matches!(
                                    (dst, value),
                                    (ValueSlot::Int(_), Value::Int(_))
                                        | (ValueSlot::Float(_), Value::Float(_) | Value::Int(_))
                                        | (ValueSlot::Bool(_), Value::Bool(_))
                                        | (ValueSlot::Color(_), Value::Color(_))
                                        | (ValueSlot::Ref(_), _)
                                )
                            })
                    }
                    LoadIntParam { dst, .. } => valid_slot(ValueSlot::Int(*dst)),
                    LoadFloatParam { dst, .. } => valid_slot(ValueSlot::Float(*dst)),
                    LoadBoolParam { dst, .. } => valid_slot(ValueSlot::Bool(*dst)),
                    LoadColorParam { dst, .. } => valid_slot(ValueSlot::Color(*dst)),
                    LoadRefParam { dst, .. } => valid_slot(ValueSlot::Ref(*dst)),
                    LoadGeneratorContext { dst, .. } | ContextRead { dst, .. } => valid_slot(*dst),
                    Move { dst, src } => {
                        valid_slot(*dst)
                            && valid_slot(*src)
                            && matches!(
                                (dst, src),
                                (
                                    ValueSlot::Int(_) | ValueSlot::Float(_),
                                    ValueSlot::Int(_) | ValueSlot::Float(_)
                                ) | (ValueSlot::Bool(_), ValueSlot::Bool(_))
                                    | (ValueSlot::Color(_), ValueSlot::Color(_))
                                    | (ValueSlot::Ref(_), ValueSlot::Ref(_))
                            )
                    }
                    MakeArray { dst, items } => {
                        valid_slot(ValueSlot::Ref(*dst))
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
                            && valid_slot(ValueSlot::Ref(*target))
                            && valid_slot(*index)
                            && self.constants.get(*default as usize).is_some()
                    }
                    Select {
                        dst,
                        items,
                        index,
                        default,
                    } => {
                        valid_slot(*dst)
                            && valid_pool(*items)
                            && valid_slot(*index)
                            && self.constants.get(*default as usize).is_some()
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
                    Member { dst, target, .. } => {
                        valid_slot(*dst) && valid_slot(ValueSlot::Ref(*target))
                    }
                    IntToFloat { dst, src } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Int(*src))
                    }
                    Not { dst, src } => {
                        valid_slot(ValueSlot::Bool(*dst)) && valid_slot(ValueSlot::Bool(*src))
                    }
                    NegInt { dst, src } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Int(*src))
                    }
                    NegFloat { dst, src } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*src))
                    }
                    FloatArithmetic {
                        dst, left, right, ..
                    }
                    | FloatBinary {
                        dst, left, right, ..
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*left))
                            && valid_slot(ValueSlot::Float(*right))
                    }
                    FloatArithmeticConst { dst, value, .. }
                    | FloatUnary { dst, value, .. }
                    | FloatBinaryConst { dst, value, .. }
                    | ClampConst { dst, value, .. } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*value))
                    }
                    IntArithmetic {
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
                        valid_slot(ValueSlot::Bool(*dst))
                            && matches!(self.constants.get(*constant), Some(Value::Enum(_)))
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
                            && valid_slot(ValueSlot::Ref(*marks))
                            && *end > ip
                            && *end < self.instructions.len()
                    }
                    LoopEnd { id, start } => {
                        *id < self.loop_count && *start <= ip && *start < self.instructions.len()
                    }
                    SectionPosition { dst, width } => {
                        valid_slot(ValueSlot::Float(*dst)) && valid_slot(ValueSlot::Float(*width))
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
                    Smoothstep {
                        dst,
                        edge0,
                        edge1,
                        value,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*edge0))
                            && valid_slot(ValueSlot::Float(*edge1))
                            && valid_slot(ValueSlot::Float(*value))
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
                    Rand { dst, args } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_pool(*args)
                            && self.value_operands(*args).is_some_and(|slots| {
                                slots.iter().all(|slot| matches!(slot, ValueSlot::Float(_)))
                            })
                    }
                    CurveFloatClamped {
                        dst,
                        curve,
                        position,
                        min,
                        max,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Ref(*curve))
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
                            && valid_slot(ValueSlot::Ref(*gradient))
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
                        fallback,
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Ref(*curve))
                            && valid_slot(ValueSlot::Float(*value))
                            && fallback.is_none_or(|slot| valid_slot(ValueSlot::Float(slot)))
                    }
                    CurveParamCrossing {
                        dst,
                        value,
                        fallback,
                        ..
                    } => {
                        valid_slot(ValueSlot::Float(*dst))
                            && valid_slot(ValueSlot::Float(*value))
                            && fallback.is_none_or(|slot| valid_slot(ValueSlot::Float(slot)))
                    }
                    Len { dst, value } => {
                        valid_slot(ValueSlot::Int(*dst)) && valid_slot(ValueSlot::Ref(*value))
                    }
                    Mark { dst, args, .. } | TargetItems { dst, args, .. } => {
                        valid_slot(*dst) && valid_pool(*args)
                    }
                    Emit { .. } => true,
                    Return(slot) => valid_slot(*slot),
                    ReturnColor(slot) => valid_slot(ValueSlot::Color(*slot)),
                }
            });
        references_valid
            && self.has_valid_loops()
            && self.has_no_fallthrough_path()
            && self.has_initialized_references()
    }

    /// Conservative live calculated-array storage required by the final
    /// instruction stream. Both compilation and wire admission use this proof.
    /// A reused pixel starts after this prefix with only its scalar registers
    /// carried over. Prove the skipped instructions depend only on values
    /// produced earlier in the prefix and that the pixel body cannot mutate
    /// those cached values. This is a wire boundary, not an optimizer hint.
    fn has_valid_pixel_entry(&self) -> bool {
        use Instruction::*;

        let entry = self.pixel_entry as usize;
        if entry >= self.instructions.len() {
            return false;
        }
        let mut cached = BTreeSet::new();
        for instruction in &self.instructions[..entry] {
            let mut reads = [None; 4];
            let dst = match instruction {
                LoadConst { dst, .. } if !matches!(dst, ValueSlot::Ref(_)) => *dst,
                LoadIntParam { dst, .. } => ValueSlot::Int(*dst),
                LoadFloatParam { dst, .. } => ValueSlot::Float(*dst),
                LoadBoolParam { dst, .. } => ValueSlot::Bool(*dst),
                LoadColorParam { dst, .. } => ValueSlot::Color(*dst),
                ContextRead {
                    dst,
                    read:
                        self::ContextRead::Progress
                        | self::ContextRead::Seconds
                        | self::ContextRead::Duration,
                } if !matches!(dst, ValueSlot::Ref(_)) => *dst,
                FloatArithmetic {
                    dst, left, right, ..
                }
                | FloatBinary {
                    dst, left, right, ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*left));
                    reads[1] = Some(ValueSlot::Float(*right));
                    ValueSlot::Float(*dst)
                }
                FloatArithmeticConst { dst, value, .. }
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
                    dst,
                    value,
                    fallback,
                    ..
                } => {
                    reads[0] = Some(ValueSlot::Float(*value));
                    reads[1] = fallback.map(ValueSlot::Float);
                    ValueSlot::Float(*dst)
                }
                _ => return false,
            };
            if !reads
                .into_iter()
                .flatten()
                .all(|slot| cached.contains(&slot_key(slot)))
                || !cached.insert(slot_key(dst))
            {
                return false;
            }
        }
        if self.instructions[entry..].iter().any(|instruction| {
            instruction
                .written_slot()
                .is_some_and(|slot| cached.contains(&slot_key(slot)))
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
            if *frame_cache != next_cache || !cached.contains(&slot_key(ValueSlot::Float(*seconds)))
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

    pub fn required_array_storage(&self) -> Option<(u32, u32)> {
        if !self
            .instructions
            .iter()
            .any(|instruction| matches!(instruction, Instruction::MakeArray { .. }))
        {
            return Some((0, 0));
        }
        let mut roots = vec![0_u32];
        for ty in &self.ref_types {
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
            let ty = self.ref_types.get(dst.0 as usize)?;
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

    /// A reference register starts as Void. Every read must be preceded by a
    /// write on every path, including the zero-iteration path through a loop.
    /// Once written, instruction typing keeps subsequent values in its type.
    fn has_initialized_references(&self) -> bool {
        for slot in 0..self.layout.refs {
            let slot = RefSlot(slot);
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
                    Instruction::Return(_) | Instruction::ReturnColor(_) => {}
                    Instruction::Jump(target) => pending.push(*target),
                    Instruction::JumpIfFalse { target, .. }
                    | Instruction::JumpIfTrue { target, .. } => {
                        pending.push(*target);
                        pending.push(ip + 1);
                    }
                    Instruction::LoopRangeStart { end, .. }
                    | Instruction::LoopMarksStart { end, .. } => {
                        pending.push(end + 1);
                        pending.push(ip + 1);
                    }
                    Instruction::LoopEnd { start, .. } => {
                        pending.push(*start);
                        pending.push(ip + 1);
                    }
                    _ => pending.push(ip + 1),
                }
            }
        }
        true
    }

    fn instruction_reads_ref(&self, instruction: &Instruction, slot: RefSlot) -> bool {
        let is_ref = |value| value == ValueSlot::Ref(slot);
        let pool_reads = |span| {
            self.value_operands(span)
                .is_some_and(|values| values.iter().copied().any(is_ref))
        };
        match instruction {
            Instruction::Move { src, .. } => is_ref(*src),
            Instruction::MakeArray { items, .. } | Instruction::Select { items, .. } => {
                pool_reads(*items)
            }
            Instruction::Index { target, index, .. } => *target == slot || is_ref(*index),
            Instruction::Member { target, .. } => *target == slot,
            Instruction::ValueEqual { left, right, .. } => is_ref(*left) || is_ref(*right),
            Instruction::CurveFloatClamped { curve, .. }
            | Instruction::CurveCrossing { curve, .. } => *curve == slot,
            Instruction::GradientColorScaled { gradient, .. } => *gradient == slot,
            Instruction::Len { value, .. } => *value == slot,
            Instruction::LoopMarksStart { marks, .. } => *marks == slot,
            Instruction::Mark { args, .. } | Instruction::TargetItems { args, .. } => {
                pool_reads(*args)
            }
            Instruction::Return(value) => is_ref(*value),
            _ => false,
        }
    }

    fn instruction_writes_ref(&self, instruction: &Instruction, slot: RefSlot) -> bool {
        let is_ref = |value| value == ValueSlot::Ref(slot);
        match instruction {
            Instruction::LoadConst { dst, .. }
            | Instruction::LoadGeneratorContext { dst, .. }
            | Instruction::ContextRead { dst, .. }
            | Instruction::Move { dst, .. }
            | Instruction::Index { dst, .. }
            | Instruction::Select { dst, .. }
            | Instruction::Member { dst, .. }
            | Instruction::Mark { dst, .. }
            | Instruction::TargetItems { dst, .. } => is_ref(*dst),
            Instruction::LoadRefParam { dst, .. } | Instruction::MakeArray { dst, .. } => {
                *dst == slot
            }
            _ => false,
        }
    }

    fn has_valid_reference_types(&self) -> bool {
        if self.ref_types.len() != self.layout.refs as usize
            || !self.ref_types.iter().all(well_formed_ref_type)
        {
            return false;
        }
        let ref_type = |slot: RefSlot| self.ref_types.get(slot.0 as usize);
        let slot_type = |slot: ValueSlot| match slot {
            ValueSlot::Int(_) => Some(&Type::Int),
            ValueSlot::Float(_) => Some(&Type::Float),
            ValueSlot::Bool(_) => Some(&Type::Bool),
            ValueSlot::Color(_) => Some(&Type::Color),
            ValueSlot::Ref(slot) => ref_type(slot),
        };
        let accepts_slot = |dst: ValueSlot, src: ValueSlot| {
            slot_type(dst)
                .zip(slot_type(src))
                .is_some_and(|(dst, src)| dst.accepts(src))
        };
        let accepts_value = |dst: ValueSlot, value: &Value| {
            slot_type(dst).is_some_and(|ty| ty.accepts_value(value))
        };
        let terminal_tuple = |ip: usize, dst: RefSlot| {
            ip.checked_add(2) == Some(self.instructions.len())
                && matches!(
                    self.instructions.get(ip + 1),
                    Some(Instruction::Return(ValueSlot::Ref(result))) if *result == dst
                )
        };
        let operands = |span: PoolSpan| self.value_operands(span);
        self.instructions.iter().enumerate().all(|(ip, instruction)| {
            use Instruction::*;
            match instruction {
                LoadConst { dst, constant } => self.constants.get(*constant).is_some_and(|value| {
                    accepts_value(*dst, value)
                        || matches!((dst, value), (ValueSlot::Ref(slot), Value::Array(_))
                            if matches!(ref_type(*slot), Some(Type::Array(item)) if **item == Type::Void)
                                && terminal_tuple(ip, *slot))
                }),
                LoadGeneratorContext { dst, slot } => match slot {
                    GeneratorContextId::Timeline => slot_type(*dst) == Some(&Type::Timeline),
                    GeneratorContextId::Target => slot_type(*dst) == Some(&Type::Target),
                    GeneratorContextId::Duration => slot_type(*dst) == Some(&Type::Float),
                },
                ContextRead { dst, read } => match read {
                    self::ContextRead::PixelIndex | self::ContextRead::PixelCount => {
                        matches!(dst, ValueSlot::Int(_) | ValueSlot::Float(_))
                    }
                    _ => matches!(dst, ValueSlot::Float(_)),
                },
                Move { dst, src } => accepts_slot(*dst, *src),
                MakeArray { dst, items } => {
                    let Some(Type::Array(item_type)) = ref_type(*dst) else {
                        return false;
                    };
                    operands(*items).is_some_and(|items| {
                        items.iter().all(|slot| {
                            // Only the final calculation result may use
                            // array<void> as a heterogeneous output tuple.
                            slot_type(*slot).is_some_and(|ty| {
                                item_type.accepts(ty)
                                    || (**item_type == Type::Void && terminal_tuple(ip, *dst))
                            })
                        })
                    })
                }
                Index {
                    dst,
                    target,
                    index,
                    default,
                } => {
                    let index_type = slot_type(*index);
                    let target_ok = match ref_type(*target) {
                        Some(Type::Array(item)) => {
                            index_type.is_some_and(|ty| Type::Int.accepts(ty))
                                && slot_type(*dst).is_some_and(|ty| ty.accepts(item))
                        }
                        Some(Type::TargetItems) => {
                            index_type.is_some_and(|ty| Type::Int.accepts(ty))
                                && slot_type(*dst) == Some(&Type::TargetItem)
                        }
                        Some(Type::Curve) => {
                            index_type.is_some_and(|ty| Type::Float.accepts(ty))
                                && slot_type(*dst) == Some(&Type::Float)
                        }
                        Some(Type::Gradient) => {
                            index_type.is_some_and(|ty| Type::Float.accepts(ty))
                                && slot_type(*dst) == Some(&Type::Color)
                        }
                        _ => false,
                    };
                    target_ok
                        && self
                            .constants
                            .get(*default as usize)
                            .is_some_and(|value| accepts_value(*dst, value))
                }
                Select {
                    dst,
                    items,
                    index,
                    default,
                } => {
                    matches!(index, ValueSlot::Int(_) | ValueSlot::Float(_))
                        && operands(*items)
                            .is_some_and(|items| items.iter().all(|slot| accepts_slot(*dst, *slot)))
                        && self
                            .constants
                            .get(*default as usize)
                            .is_some_and(|value| accepts_value(*dst, value))
                }
                Member {
                    dst,
                    target,
                    member,
                } => {
                    ref_type(*target) == Some(&Type::TargetItem)
                        && slot_type(*dst)
                            == Some(match member {
                                TargetMember::PixelFraction => &Type::Float,
                                _ => &Type::Int,
                            })
                }
                CurveFloatClamped { curve, .. } | CurveCrossing { curve, .. } => {
                    ref_type(*curve) == Some(&Type::Curve)
                }
                GradientColorScaled { gradient, .. } => {
                    ref_type(*gradient) == Some(&Type::Gradient)
                }
                Len { value, .. } => {
                    matches!(ref_type(*value), Some(Type::Array(_) | Type::Marks))
                }
                LoopMarksStart { marks, .. } => ref_type(*marks) == Some(&Type::Marks),
                Mark { dst, op, args } => {
                    let Some(args) = operands(*args) else {
                        return false;
                    };
                    let Some(ValueSlot::Ref(marks)) = args.first() else {
                        return false;
                    };
                    if ref_type(*marks) != Some(&Type::Marks) {
                        return false;
                    }
                    let numeric =
                        |slot: &ValueSlot| matches!(slot, ValueSlot::Int(_) | ValueSlot::Float(_));
                    match op {
                        MarkOp::Count => args.len() == 1 && matches!(dst, ValueSlot::Int(_)),
                        MarkOp::At => {
                            (2..=3).contains(&args.len())
                                && numeric(&args[1])
                                && args.get(2).is_none_or(numeric)
                                && matches!(dst, ValueSlot::Float(_))
                        }
                        MarkOp::PrevIndex | MarkOp::NextIndex => {
                            (1..=2).contains(&args.len())
                                && args.get(1).is_none_or(numeric)
                                && matches!(dst, ValueSlot::Int(_))
                        }
                        MarkOp::Prev | MarkOp::Elapsed | MarkOp::Phase => {
                            (1..=3).contains(&args.len())
                                && args.iter().skip(1).all(numeric)
                                && matches!(dst, ValueSlot::Float(_))
                        }
                    }
                }
                TargetItems { dst, op, args } => {
                    let Some(args) = operands(*args) else {
                        return false;
                    };
                    let Some(ValueSlot::Ref(source)) = args.first() else {
                        return false;
                    };
                    let source_ty = ref_type(*source);
                    match op {
                        TargetItemsOp::Fixtures | TargetItemsOp::Pixels => {
                            args.len() == 1
                                && matches!(
                                    source_ty,
                                    Some(Type::Target | Type::TargetItems | Type::TargetItem)
                                )
                                && slot_type(*dst) == Some(&Type::TargetItems)
                        }
                        TargetItemsOp::Sections => {
                            args.len() == 2
                                && matches!(
                                    source_ty,
                                    Some(Type::Target | Type::TargetItems | Type::TargetItem)
                                )
                                && matches!(args[1], ValueSlot::Int(_) | ValueSlot::Float(_))
                                && slot_type(*dst) == Some(&Type::TargetItems)
                        }
                        TargetItemsOp::Count => {
                            args.len() == 1
                                && source_ty == Some(&Type::TargetItems)
                                && matches!(dst, ValueSlot::Int(_))
                        }
                        TargetItemsOp::Pick => {
                            args.len() == 2
                                && source_ty == Some(&Type::TargetItems)
                                && matches!(args[1], ValueSlot::Int(_) | ValueSlot::Float(_))
                                && slot_type(*dst) == Some(&Type::TargetItem)
                        }
                    }
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
                Instruction::Return(_) | Instruction::ReturnColor(_) => {}
                Instruction::Jump(target) => pending.push(*target),
                Instruction::JumpIfFalse { target, .. }
                | Instruction::JumpIfTrue { target, .. } => {
                    pending.push(*target);
                    pending.push(ip + 1);
                }
                Instruction::LoopRangeStart { end, .. }
                | Instruction::LoopMarksStart { end, .. } => {
                    pending.push(end + 1);
                    pending.push(ip + 1);
                }
                Instruction::LoopEnd { start, .. } => {
                    pending.push(*start);
                    pending.push(ip + 1);
                }
                _ => pending.push(ip + 1),
            }
        }
        true
    }

    /// Evaluate retained generator calculations into preallocated typed slots.
    pub fn evaluate_bindings(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        workspace: &mut super::VmWorkspace,
        output: &mut super::BoundParams,
        types: &[Type],
    ) -> Result<(), super::RuntimeError> {
        super::vm::evaluate_bindings(self, params, context, workspace, output, types)
    }

    /// Host preparation evaluates typed expressions through the same VM as
    /// playback. Returning owned arrays is a preparation operation.
    pub fn evaluate_value(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        workspace: &mut super::VmWorkspace,
    ) -> Result<Value, super::RuntimeError> {
        super::vm::evaluate_value(self, params, context, workspace)
    }

    pub(crate) fn frame_cache_count(&self) -> usize {
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

    pub fn sample_effect(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        workspace: &mut super::VmWorkspace,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        self.sample_effect_from(params, context, workspace, false)
    }

    pub(crate) fn sample_effect_from(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        workspace: &mut super::VmWorkspace,
        reuse_uniform: bool,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        super::vm::run_sample_program(
            self,
            params,
            context,
            workspace,
            if reuse_uniform {
                self.pixel_entry as usize
            } else {
                0
            },
        )
    }

    pub fn sample_spatial_effect(
        &self,
        params: &super::BoundParams,
        context: &super::RunContext,
        spatial: Option<&super::SpatialContext>,
        workspace: &mut super::VmWorkspace,
        reuse_uniform: bool,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        super::vm::run_spatial_sample_program(
            self,
            params,
            context,
            workspace,
            if reuse_uniform {
                self.pixel_entry as usize
            } else {
                0
            },
            spatial,
        )
    }

    pub fn sample_operator(
        &self,
        params: &super::BoundParams,
        context: &super::OperatorRunContext,
        sampler: &mut dyn super::SignalSampler,
        workspace: &mut super::VmWorkspace,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        self.sample_operator_from(params, context, sampler, workspace, false, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sample_operator_from(
        &self,
        params: &super::BoundParams,
        context: &super::OperatorRunContext,
        sampler: &mut dyn super::SignalSampler,
        workspace: &mut super::VmWorkspace,
        reuse_uniform: bool,
        spatial: Option<&super::SpatialContext>,
    ) -> Result<crate::values::Color, super::RuntimeError> {
        super::vm::run_operator_program(
            self,
            params,
            context,
            sampler,
            workspace,
            if reuse_uniform {
                self.pixel_entry as usize
            } else {
                0
            },
            spatial,
        )
    }
}

fn well_formed_ref_type(ty: &Type) -> bool {
    match ty {
        Type::Void
        | Type::Signal
        | Type::Marks
        | Type::Timeline
        | Type::Target
        | Type::TargetItems
        | Type::TargetItem
        | Type::Curve
        | Type::Gradient => true,
        Type::Array(item) => match item.as_ref() {
            Type::Enum(options) => !options.is_empty(),
            Type::Array(_) => well_formed_ref_type(item),
            _ => true,
        },
        Type::Enum(options) => !options.is_empty(),
        Type::Int | Type::Float | Type::Bool | Type::Color => false,
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
    pub(super) fn range(self) -> core::ops::Range<usize> {
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
    pub refs: u32,
}

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
pub struct RefSlot(pub u32);

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum ValueSlot {
    Int(IntSlot),
    Float(FloatSlot),
    Bool(BoolSlot),
    Color(ColorSlot),
    Ref(RefSlot),
}

impl ValueSlot {
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
            Type::Void
            | Type::Signal
            | Type::Marks
            | Type::Timeline
            | Type::Target
            | Type::TargetItems
            | Type::TargetItem
            | Type::Curve
            | Type::Gradient
            | Type::Array(_)
            | Type::Enum(_) => {
                let slot = RefSlot(layout.refs);
                layout.refs += 1;
                Self::Ref(slot)
            }
        }
    }
}

#[derive(Clone, Debug, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum Instruction {
    LoadConst {
        dst: ValueSlot,
        constant: ConstantId,
    },
    LoadIntParam {
        dst: IntSlot,
        param: ParamId,
    },
    LoadFloatParam {
        dst: FloatSlot,
        param: ParamId,
    },
    LoadBoolParam {
        dst: BoolSlot,
        param: ParamId,
    },
    LoadColorParam {
        dst: ColorSlot,
        param: ParamId,
    },
    LoadRefParam {
        dst: RefSlot,
        param: ParamId,
    },
    LoadGeneratorContext {
        dst: ValueSlot,
        slot: GeneratorContextId,
    },
    Move {
        dst: ValueSlot,
        src: ValueSlot,
    },
    MakeArray {
        dst: RefSlot,
        items: PoolSpan,
    },
    Index {
        dst: ValueSlot,
        target: RefSlot,
        index: ValueSlot,
        default: u32,
    },
    /// Index an immutable array snapshot lowered to existing value slots.
    Select {
        dst: ValueSlot,
        items: PoolSpan,
        index: ValueSlot,
        default: u32,
    },
    CurveParamSample {
        dst: FloatSlot,
        param: ParamId,
        position: FloatSlot,
    },
    GradientParamSample {
        dst: ColorSlot,
        param: ParamId,
        position: FloatSlot,
    },
    SignalSample {
        dst: ColorSlot,
        input: usize,
        seconds: FloatSlot,
        pixel: SignalPixel<IntSlot>,
        frame_cache: u32,
    },
    Member {
        dst: ValueSlot,
        target: RefSlot,
        member: TargetMember,
    },
    IntToFloat {
        dst: FloatSlot,
        src: IntSlot,
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
    FloatArithmetic {
        dst: FloatSlot,
        op: ArithmeticOp,
        left: FloatSlot,
        right: FloatSlot,
    },
    FloatArithmeticConst {
        dst: FloatSlot,
        op: ArithmeticOp,
        value: FloatSlot,
        constant_bits: u32,
        constant_left: bool,
    },
    IntArithmetic {
        dst: IntSlot,
        op: IntArithmeticOp,
        left: IntSlot,
        right: IntSlot,
    },
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
        constant: ConstantId,
        negate: bool,
    },
    Jump(Target),
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
        marks: RefSlot,
        end: Target,
    },
    LoopEnd {
        id: u32,
        start: Target,
    },
    ContextRead {
        dst: ValueSlot,
        read: ContextRead,
    },
    SectionPosition {
        dst: FloatSlot,
        width: FloatSlot,
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
    Smoothstep {
        dst: FloatSlot,
        edge0: FloatSlot,
        edge1: FloatSlot,
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
        args: PoolSpan,
    },
    CurveFloatClamped {
        dst: FloatSlot,
        curve: RefSlot,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    CurveParamFloatClamped {
        dst: FloatSlot,
        param: ParamId,
        position: FloatSlot,
        min: FloatSlot,
        max: FloatSlot,
    },
    GradientColorScaled {
        dst: ColorSlot,
        gradient: RefSlot,
        position: FloatSlot,
        scale: FloatSlot,
    },
    GradientParamColorScaled {
        dst: ColorSlot,
        param: ParamId,
        position: FloatSlot,
        scale: FloatSlot,
    },
    CurveCrossing {
        dst: FloatSlot,
        curve: RefSlot,
        value: FloatSlot,
        fallback: Option<FloatSlot>,
    },
    CurveParamCrossing {
        dst: FloatSlot,
        param: ParamId,
        value: FloatSlot,
        fallback: Option<FloatSlot>,
    },
    Len {
        dst: IntSlot,
        value: RefSlot,
    },
    Mark {
        dst: ValueSlot,
        op: MarkOp,
        args: PoolSpan,
    },
    TargetItems {
        dst: ValueSlot,
        op: TargetItemsOp,
        args: PoolSpan,
    },
    Emit {
        effect: super::GeneratedEffectSlot,
        fields: PoolSpan,
    },
    Return(ValueSlot),
    ReturnColor(ColorSlot),
}

impl Instruction {
    fn written_slot(&self) -> Option<ValueSlot> {
        use Instruction::*;
        Some(match self {
            LoadConst { dst, .. }
            | LoadGeneratorContext { dst, .. }
            | Move { dst, .. }
            | Index { dst, .. }
            | Select { dst, .. }
            | Member { dst, .. }
            | ContextRead { dst, .. }
            | Mark { dst, .. }
            | TargetItems { dst, .. } => *dst,
            LoadIntParam { dst, .. }
            | NegInt { dst, .. }
            | IntArithmetic { dst, .. }
            | Len { dst, .. } => ValueSlot::Int(*dst),
            LoadFloatParam { dst, .. }
            | CurveParamSample { dst, .. }
            | IntToFloat { dst, .. }
            | NegFloat { dst, .. }
            | FloatArithmetic { dst, .. }
            | FloatArithmeticConst { dst, .. }
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
            LoadBoolParam { dst, .. }
            | Not { dst, .. }
            | IntCompare { dst, .. }
            | FloatCompare { dst, .. }
            | FloatCompareConst { dst, .. }
            | ValueEqual { dst, .. }
            | EnumParamEqualConst { dst, .. } => ValueSlot::Bool(*dst),
            LoadColorParam { dst, .. }
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
            LoadRefParam { dst, .. } | MakeArray { dst, .. } => ValueSlot::Ref(*dst),
            Jump(_)
            | JumpIfFalse { .. }
            | JumpIfTrue { .. }
            | LoopRangeStart { .. }
            | LoopMarksStart { .. }
            | LoopEnd { .. }
            | Emit { .. }
            | Return(_)
            | ReturnColor(_) => return None,
        })
    }
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum TargetMember {
    FixtureIndex,
    FixturePixelIndex,
    PixelIndex,
    PixelCount,
    PixelFraction,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum GeneratorContextId {
    Timeline,
    Target,
    Duration,
}

#[cfg(test)]
mod representation_tests {
    use super::{
        BytecodeProgram, ColorSlot, Instruction, ParameterKind, PoolSpan, ProgramContext,
        SignalPixel, SlotLayout, ValueSlot,
    };
    use alloc::vec;

    #[test]
    fn bytecode_headers_stay_compact() {
        assert!(size_of::<Instruction>() <= 32);
        assert!(size_of::<BytecodeProgram>() <= 104);
    }

    #[test]
    fn malformed_bytecode_references_are_rejected_before_execution() {
        let mut program = BytecodeProgram {
            instructions: vec![Instruction::ReturnColor(ColorSlot(0))].into_boxed_slice(),
            constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![].into_boxed_slice(),
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
            args: PoolSpan { start: 1, len: 1 },
        }]
        .into_boxed_slice();
        program.layout.floats = 1;
        program.value_operands = vec![ValueSlot::Float(super::FloatSlot(0))].into_boxed_slice();
        assert!(!program.has_valid_structure());

        program.instructions = vec![
            Instruction::LoadFloatParam {
                dst: super::FloatSlot(0),
                param: 0,
            },
            Instruction::ReturnColor(ColorSlot(0)),
        ]
        .into_boxed_slice();
        assert!(program.has_valid_structure());
        assert!(program.has_valid_parameter_reads(|_| Some(ParameterKind::Int)));
        assert!(!program.has_valid_parameter_reads(|_| Some(ParameterKind::Bool)));
        assert!(!program.has_valid_parameter_reads(|_| None));
    }

    #[test]
    fn pixel_entry_requires_an_immutable_frame_uniform_prefix() {
        use super::{ContextRead, FloatSlot};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadFloatParam {
                    dst: FloatSlot(0),
                    param: 0,
                },
                Instruction::ContextRead {
                    dst: ValueSlot::Float(FloatSlot(1)),
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
            constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![].into_boxed_slice(),
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
        };
        program.uses_pixel_context = false;
        assert!(!program.has_valid_structure());

        program.instructions[1] = Instruction::FloatArithmetic {
            dst: FloatSlot(1),
            op: super::ArithmeticOp::Add,
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
                },
                Instruction::ContextRead {
                    dst: ValueSlot::Float(FloatSlot(1)),
                    read: ContextRead::PixelFraction,
                },
                Instruction::SignalSample {
                    dst: ColorSlot(0),
                    input: 0,
                    seconds: FloatSlot(1),
                    pixel: SignalPixel::Current,
                    frame_cache: 0,
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![].into_boxed_slice(),
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
        use super::{RefSlot, Type, Value};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadConst {
                    dst: ValueSlot::Ref(RefSlot(0)),
                    constant: 0,
                },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            constants: vec![Value::Array(vec![Value::Int(3)].into())].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![Type::array(Type::Int)].into_boxed_slice(),
            layout: SlotLayout {
                refs: 1,
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

        program.ref_types[0] = Type::Curve;
        assert!(!program.has_valid_structure());
        program.ref_types[0] = Type::array(Type::Int);
        program.instructions[0] = Instruction::CurveFloatClamped {
            dst: super::FloatSlot(0),
            curve: RefSlot(0),
            position: super::FloatSlot(0),
            min: super::FloatSlot(0),
            max: super::FloatSlot(0),
        };
        program.layout.floats = 1;
        assert!(!program.has_valid_structure());

        program.instructions[0] = Instruction::LoadRefParam {
            dst: RefSlot(0),
            param: 0,
        };
        assert!(program.has_valid_structure());
        assert!(program.has_valid_parameter_reads(|_| Some(ParameterKind::Reference)));
        assert!(
            !program.has_valid_reference_parameter_reads(|_, expected| {
                expected.accepts(&Type::Marks)
            })
        );
        assert!(program.has_valid_reference_parameter_reads(|_, expected| {
            expected.accepts(&Type::array(Type::Int))
        }));

        program.instructions[0] = Instruction::ContextRead {
            dst: ValueSlot::Ref(RefSlot(0)),
            read: super::ContextRead::Seconds,
        };
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn calculation_tuple_must_match_declared_outputs_on_every_return_path() {
        use super::{FloatSlot, RefSlot, Type, Value};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadConst {
                    dst: ValueSlot::Float(FloatSlot(0)),
                    constant: 0,
                },
                Instruction::MakeArray {
                    dst: RefSlot(0),
                    items: PoolSpan { start: 0, len: 1 },
                },
                Instruction::Return(ValueSlot::Ref(RefSlot(0))),
            ]
            .into_boxed_slice(),
            constants: vec![Value::Float(1.0)].into_boxed_slice(),
            value_operands: vec![ValueSlot::Float(FloatSlot(0))].into_boxed_slice(),
            ref_types: vec![Type::array(Type::Void)].into_boxed_slice(),
            layout: SlotLayout {
                floats: 1,
                refs: 1,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 2,
            array_width: 1,
            loop_count: 0,
        };
        assert!(program.has_valid_structure());
        program.array_capacity = 1;
        assert!(!program.has_valid_structure());
        program.array_capacity = 3;
        assert!(!program.has_valid_structure());
        program.array_capacity = 2;
        program.array_width = 0;
        assert!(!program.has_valid_structure());
        program.array_width = 2;
        assert!(!program.has_valid_structure());
        program.array_width = 1;
        assert!(program.has_valid_context(ProgramContext::Calculation));
        assert!(program.has_valid_calculation_outputs(&[Type::Float]));
        assert!(!program.has_valid_calculation_outputs(&[Type::Int]));
        assert!(!program.has_valid_calculation_outputs(&[]));

        program.instructions[0] = Instruction::Jump(2);
        assert!(!program.has_valid_structure());
        assert!(!program.has_valid_calculation_outputs(&[Type::Float]));
    }

    #[test]
    fn calculation_tuple_cannot_chain_arrays_at_the_same_depth() {
        use super::{IntSlot, RefSlot, Type, Value};

        let program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadConst {
                    dst: ValueSlot::Int(IntSlot(0)),
                    constant: 0,
                },
                Instruction::MakeArray {
                    dst: RefSlot(0),
                    items: PoolSpan { start: 0, len: 1 },
                },
                Instruction::LoopRangeStart {
                    id: 0,
                    count: IntSlot(0),
                    cap: 10,
                    end: 5,
                },
                Instruction::MakeArray {
                    dst: RefSlot(1),
                    items: PoolSpan { start: 1, len: 1 },
                },
                Instruction::Move {
                    dst: ValueSlot::Ref(RefSlot(0)),
                    src: ValueSlot::Ref(RefSlot(1)),
                },
                Instruction::LoopEnd { id: 0, start: 3 },
                Instruction::MakeArray {
                    dst: RefSlot(2),
                    items: PoolSpan { start: 0, len: 1 },
                },
                Instruction::Return(ValueSlot::Ref(RefSlot(2))),
            ]
            .into_boxed_slice(),
            constants: vec![Value::Int(10)].into_boxed_slice(),
            value_operands: vec![ValueSlot::Int(IntSlot(0)), ValueSlot::Ref(RefSlot(0))]
                .into_boxed_slice(),
            ref_types: vec![Type::array(Type::Void); 3].into_boxed_slice(),
            layout: SlotLayout {
                ints: 1,
                refs: 3,
                ..SlotLayout::default()
            },
            uses_pixel_context: false,
            pixel_entry: 0,
            array_capacity: 4,
            array_width: 1,
            loop_count: 1,
        };
        assert!(!program.has_valid_structure());
        assert!(program.has_valid_context(ProgramContext::Calculation));
        assert!(program.has_valid_calculation_outputs(&[Type::Int]));
    }

    #[test]
    fn only_paired_counted_loops_can_jump_backward() {
        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadConst {
                    dst: ValueSlot::Int(super::IntSlot(0)),
                    constant: 0,
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
            constants: vec![super::Value::Int(2)].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![].into_boxed_slice(),
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
        use super::{RefSlot, Type};

        let mut program = BytecodeProgram {
            instructions: vec![
                Instruction::LoadRefParam {
                    dst: RefSlot(0),
                    param: 0,
                },
                Instruction::LoopMarksStart {
                    id: 0,
                    marks: RefSlot(0),
                    end: 2,
                },
                Instruction::LoopEnd { id: 0, start: 2 },
                Instruction::ReturnColor(ColorSlot(0)),
            ]
            .into_boxed_slice(),
            constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![Type::Marks].into_boxed_slice(),
            layout: SlotLayout {
                refs: 1,
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

        program.ref_types[0] = Type::Curve;
        assert!(!program.has_valid_structure());
        program.ref_types[0] = Type::Marks;
        program.instructions[0] = Instruction::Jump(1);
        assert!(!program.has_valid_structure());
    }

    #[test]
    fn playback_context_rejects_wrong_returns_and_signal_inputs() {
        let mut program = BytecodeProgram {
            instructions: vec![Instruction::ReturnColor(ColorSlot(0))].into_boxed_slice(),
            constants: vec![].into_boxed_slice(),
            value_operands: vec![].into_boxed_slice(),
            ref_types: vec![].into_boxed_slice(),
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
        assert!(!program.has_valid_context(ProgramContext::Calculation));

        program.instructions[0] = Instruction::Return(ValueSlot::Color(ColorSlot(0)));
        assert!(program.has_valid_structure());
        assert!(!program.has_valid_context(ProgramContext::Effect));
        assert!(!program.has_valid_context(ProgramContext::Calculation));

        program.instructions = vec![
            Instruction::SignalSample {
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
pub enum ArithmeticOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum IntArithmeticOp {
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
pub enum FloatBinary {
    Min,
    Max,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum MarkOp {
    Count,
    At,
    Prev,
    PrevIndex,
    NextIndex,
    Elapsed,
    Phase,
}

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub enum TargetItemsOp {
    Fixtures,
    Pixels,
    Sections,
    Count,
    Pick,
}
