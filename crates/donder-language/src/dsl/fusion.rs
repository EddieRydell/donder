//! Host-only composition of compatible signal programs. Source clocks remain
//! explicit instructions; playback needs no call stack or graph-fusion tables.
use super::bytecode::{
    ContextRead, FloatSlot, Instruction, NumberSlot, SignalPixel, SlotLayout, ValueSlot,
};
use super::{OperatorDefinition, OperatorInvocation, OperatorProgram};

impl OperatorInvocation {
    /// Inline one current-pixel source instruction. The result's inputs are this
    /// invocation's other inputs in order, followed by the source's inputs.
    /// Both invocations must use the same sequence duration, pixel domain and
    /// section context. The caller must preserve graph sharing and must not retain
    /// the replaced source node. Source automation retains a query-time boundary.
    /// Refuse code duplication and loss of existing block execution.
    pub fn fuse_input(&self, input: usize, source: &Self) -> Option<Self> {
        if !source.automation().is_empty() {
            return None;
        }
        let (mut caller, inputs, mut types) = self.program().as_ref().clone().into_parts();
        if input >= inputs {
            return None;
        }
        let (mut callee, source_inputs, source_types) =
            source.program().as_ref().clone().into_parts();
        let mut calls = caller
            .instructions
            .iter()
            .enumerate()
            .filter_map(|(ip, op)| match *op {
                Instruction::SignalSample {
                    dst,
                    input: source,
                    seconds,
                    pixel,
                    ..
                } if source == input => Some((ip, dst, seconds, pixel)),
                _ => None,
            });
        let (call, dst, seconds, SignalPixel::Current) = calls.next()? else {
            return None;
        };
        if calls.next().is_some()
            || callee.instructions.iter().any(|op| {
                matches!(
                    op,
                    Instruction::ContextRead {
                        dst: NumberSlot::Int(_),
                        read: ContextRead::Seconds | ContextRead::Progress
                    }
                )
            })
            || super::optimize::reads_initial_registers(
                &callee.instructions,
                &mut callee.value_operands,
            )
        {
            return None;
        }

        let registers = caller.layout;
        caller.layout = add_layout(registers, callee.layout)?;
        let query_seconds = FloatSlot(caller.layout.floats);
        caller.layout.floats = caller.layout.floats.checked_add(1)?;
        let loops = caller.loop_count;
        caller.loop_count = caller.loop_count.checked_add(callee.loop_count)?;
        let param_offset = types.len();
        let mut params = SlotLayout::default();
        for ty in types.iter() {
            ValueSlot::for_type(ty, &mut params);
        }
        let enum_offset = caller.enums.len();
        let curve_offset = caller.curves.len();
        let gradient_offset = caller.gradients.len();
        let array_offset = caller.array_constants.len();
        caller.enums = [caller.enums.as_ref(), callee.enums.as_ref()]
            .concat()
            .into();
        caller.curves = [caller.curves.as_ref(), callee.curves.as_ref()]
            .concat()
            .into();
        caller.gradients = [caller.gradients.as_ref(), callee.gradients.as_ref()]
            .concat()
            .into();
        caller.array_constants = [
            caller.array_constants.as_ref(),
            callee.array_constants.as_ref(),
        ]
        .concat()
        .into();
        caller.enum_types = [caller.enum_types.as_ref(), callee.enum_types.as_ref()]
            .concat()
            .into();
        caller.array_types = [caller.array_types.as_ref(), callee.array_types.as_ref()]
            .concat()
            .into();
        let mut operands = caller.value_operands.into_vec();
        let mut body = Vec::new();
        // Invalid queries return black without evaluating the source, including
        // source-independent constants and any source errors.
        body.push(Instruction::QuerySeconds {
            dst: query_seconds,
            seconds,
        });
        body.push(Instruction::LoadColorConst {
            dst,
            value: crate::values::Color::BLACK,
        });
        body.push(Instruction::FloatJumpEqual {
            left: query_seconds,
            right: query_seconds,
            when: false,
            target: usize::MAX,
        });
        let mut offsets = Vec::with_capacity(callee.instructions.len() + 1);
        for mut op in callee.instructions.into_vec() {
            offsets.push(body.len());
            use Instruction::*;
            // Make operand spans private before visiting them, even for a
            // hand-built admitted program with shared/overlapping spans.
            if let MakeArray { items, .. } | Select { items, .. } = &mut op {
                let values = &callee.value_operands[items.range()];
                items.start = u32::try_from(operands.len()).ok()?;
                operands.extend_from_slice(values);
            }
            super::optimize::slots(&mut op, &mut operands, |slot, _| {
                offset_slot(slot, registers)
            });
            remap_params(&mut op, param_offset, params);
            match &mut op {
                LoadEnumConst { constant, .. } | EnumParamEqualConst { constant, .. } => {
                    *constant += enum_offset
                }
                LoadCurveConst { constant, .. } => *constant += curve_offset,
                LoadGradientConst { constant, .. } => *constant += gradient_offset,
                LoadArrayConst { constant, .. } => *constant += array_offset,
                LoopRangeStart { id, .. } | LoopMarksStart { id, .. } | LoopEnd { id, .. } => {
                    *id += loops
                }
                SignalSample { input, .. } => *input += inputs - 1,
                _ => {}
            }
            match op {
                ContextRead {
                    dst: NumberSlot::Float(dst),
                    read: self::ContextRead::Seconds,
                } => body.push(Move {
                    dst: ValueSlot::Float(dst),
                    src: query_seconds.0,
                }),
                ContextRead {
                    dst: NumberSlot::Float(dst),
                    read: self::ContextRead::Progress,
                } => body.push(QueryProgress { dst, seconds }),
                ReturnColor(value) => {
                    body.push(Move {
                        dst: ValueSlot::Color(dst),
                        src: value.0,
                    });
                    body.push(Jump(usize::MAX));
                }
                op => body.push(op),
            }
        }
        offsets.push(body.len());
        let continuation = call + body.len();
        for op in &mut body {
            if let Some(target) = op.jump_target_mut() {
                *target = if *target == usize::MAX {
                    continuation
                } else {
                    call + offsets[*target]
                };
            }
        }
        let added = body.len() - 1;
        let mut code = caller.instructions.into_vec();
        for op in &mut code {
            if let Some(target) = op.jump_target_mut()
                && *target > call
            {
                *target += added;
            }
            if let Instruction::SignalSample { input: source, .. } = op
                && *source > input
            {
                *source -= 1;
            }
        }
        code.splice(call..=call, body);
        caller.instructions = code.into();
        caller.value_operands = operands.into();
        types = [types.as_ref(), source_types.as_ref()].concat().into();
        let program = OperatorProgram::admit(
            super::specialize::optimize_program(caller),
            inputs - 1 + source_inputs,
            types,
        )
        .unwrap_or_else(|| unreachable!("signal inlining preserves admitted program invariants"));
        if self.program().supports_blocks() && !program.supports_blocks() {
            return None;
        }
        let values = self
            .params()
            .iter_values()
            .chain(source.params().iter_values())
            .collect();
        Some(
            OperatorDefinition::new(program)
                .bind(values)
                .unwrap_or_else(|_| unreachable!("inlining concatenates parameter schemas"))
                .with_automation(self.automation().into())
                .unwrap_or_else(|_| unreachable!("caller automation addresses stay unchanged")),
        )
    }
}

fn add_layout(a: SlotLayout, b: SlotLayout) -> Option<SlotLayout> {
    Some(SlotLayout {
        ints: a.ints.checked_add(b.ints)?,
        floats: a.floats.checked_add(b.floats)?,
        bools: a.bools.checked_add(b.bools)?,
        colors: a.colors.checked_add(b.colors)?,
        arrays: a.arrays.checked_add(b.arrays)?,
        enums: a.enums.checked_add(b.enums)?,
        marks: a.marks.checked_add(b.marks)?,
        curves: a.curves.checked_add(b.curves)?,
        gradients: a.gradients.checked_add(b.gradients)?,
    })
}

fn offset_slot(slot: ValueSlot, base: SlotLayout) -> ValueSlot {
    slot.with_index(
        slot.index()
            + match slot {
                ValueSlot::Void => 0,
                ValueSlot::Int(_) => base.ints,
                ValueSlot::Float(_) => base.floats,
                ValueSlot::Bool(_) => base.bools,
                ValueSlot::Color(_) => base.colors,
                ValueSlot::Array(_) => base.arrays,
                ValueSlot::Enum(_) => base.enums,
                ValueSlot::Marks(_) => base.marks,
                ValueSlot::Curve(_) => base.curves,
                ValueSlot::Gradient(_) => base.gradients,
            },
    )
}

fn remap_params(op: &mut Instruction, offset: usize, banks: SlotLayout) {
    use Instruction::*;
    match op {
        LoadIntParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.ints;
        }
        LoadFloatParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.floats;
        }
        LoadBoolParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.bools;
        }
        LoadColorParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.colors;
        }
        LoadArrayParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.arrays;
        }
        LoadMarksParam { param, source, .. } => {
            *param += offset;
            source.0 += banks.marks;
        }
        LoadEnumParam { param, source, .. } | EnumParamEqualConst { param, source, .. } => {
            *param += offset;
            source.0 += banks.enums;
        }
        LoadCurveParam { param, source, .. }
        | CurveParamSample { param, source, .. }
        | CurveParamFloatClamped { param, source, .. }
        | CurveParamCrossing { param, source, .. } => {
            *param += offset;
            source.0 += banks.curves;
        }
        LoadGradientParam { param, source, .. }
        | GradientParamSample { param, source, .. }
        | GradientParamColorScaled { param, source, .. } => {
            *param += offset;
            source.0 += banks.gradients;
        }
        _ => {}
    }
}
