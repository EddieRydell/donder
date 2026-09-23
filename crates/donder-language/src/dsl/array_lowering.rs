use std::collections::{HashMap, HashSet};

use super::bytecode::{Instruction, PoolSpan, RefSlot, ValueSlot};
use super::types::Value;

/// Resolve array reads within a basic block. Generic cleanup removes unused
/// array objects. Never substitute a mutable register for an array snapshot.
pub(super) fn lower_arrays(
    code: &mut [Instruction],
    constants: &mut Vec<Value>,
    operands: &mut Vec<ValueSlot>,
) {
    if !code
        .iter()
        .any(|op| matches!(op, Instruction::MakeArray { .. }))
    {
        return;
    }
    // The compiler creates fresh destinations for expressions. Only Move writes
    // a previously bound local (including parameter assignment).
    let mutable = code
        .iter()
        .filter_map(|op| match op {
            Instruction::Move { dst, .. } => Some(*dst),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let integers = code
        .iter()
        .filter_map(|op| match op {
            Instruction::LoadConst { dst, constant } if !mutable.contains(dst) => {
                match constants[*constant] {
                    Value::Int(value) => Some((*dst, value)),
                    _ => None,
                }
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let targets = code
        .iter()
        .filter_map(|op| match op {
            Instruction::Jump(target)
            | Instruction::JumpIfFalse { target, .. }
            | Instruction::JumpIfTrue { target, .. } => Some(*target),
            _ => None,
        })
        .collect::<HashSet<_>>();
    let mut arrays = HashMap::<RefSlot, Vec<ValueSlot>>::new();
    for (offset, op) in code.iter_mut().enumerate() {
        if targets.contains(&offset) {
            arrays.clear();
        }
        match op {
            Instruction::MakeArray { dst, items } => {
                let values = &operands[items.start as usize..(items.start + items.len) as usize];
                if values.iter().all(|value| !mutable.contains(value)) {
                    arrays.insert(*dst, values.to_vec());
                }
            }
            Instruction::Move {
                dst: ValueSlot::Ref(dst),
                src: ValueSlot::Ref(src),
            } => {
                let values = arrays.get(src).cloned();
                arrays.remove(dst);
                if let Some(values) = values {
                    arrays.insert(*dst, values);
                }
            }
            Instruction::Index { dst, target, index } => {
                if let Some(value) = integers
                    .get(index)
                    .and_then(|index| usize::try_from(*index).ok())
                    .and_then(|index| arrays.get(target)?.get(index))
                    .copied()
                {
                    // Ref results may themselves denote a known nested array.
                    if let (ValueSlot::Ref(dst), ValueSlot::Ref(src)) = (*dst, value)
                        && let Some(values) = arrays.get(&src).cloned()
                    {
                        arrays.insert(dst, values);
                    }
                    *op = Instruction::Move {
                        dst: *dst,
                        src: value,
                    };
                } else if let Some(values) = arrays.get(target) {
                    let items = PoolSpan {
                        start: operands.len() as u32,
                        len: values.len() as u32,
                    };
                    operands.extend_from_slice(values);
                    *op = Instruction::Select {
                        dst: *dst,
                        items,
                        index: *index,
                    };
                }
            }
            Instruction::Len { dst, value } => {
                if let Some(len) = arrays
                    .get(value)
                    .and_then(|values| i32::try_from(values.len()).ok())
                {
                    let constant = constants.len();
                    constants.push(Value::Int(len));
                    *op = Instruction::LoadConst {
                        dst: ValueSlot::Int(*dst),
                        constant,
                    };
                }
            }
            Instruction::Jump(_)
            | Instruction::JumpIfFalse { .. }
            | Instruction::JumpIfTrue { .. }
            | Instruction::Return(_)
            | Instruction::ReturnColor(_) => arrays.clear(),
            _ => {}
        }
    }
}
