use super::{HashMap, HashSet, Instruction, ValueSlot, slots};

/// Fuse adjacent arithmetic after uniform staging. Never absorb an initialized
/// query value into pixel work, cross a branch entry, or remove a live result.
pub(in crate::dsl) fn fuse(code: &mut Vec<Instruction>, operands: &mut [ValueSlot], entry: u32) {
    let targets: HashSet<_> = code.iter().filter_map(Instruction::jump_target).collect();
    let mut uses = HashMap::<ValueSlot, usize>::new();
    for op in code.iter() {
        slots(&mut op.clone(), operands, |slot, write| {
            if !write {
                *uses.entry(slot).or_default() += 1;
            }
            slot
        });
    }
    let mut offsets = vec![0; code.len() + 1];
    let mut output = Vec::with_capacity(code.len());
    let mut ip = 0;
    while ip < code.len() {
        offsets[ip] = output.len();
        let first = &code[ip];
        let replacement = if ip >= entry as usize && !targets.contains(&(ip + 1)) {
            code.get(ip + 1)
                .and_then(|second| pair(first, second, &uses))
        } else {
            None
        };
        if let Some(op) = replacement {
            offsets[ip + 1] = output.len();
            output.push(op);
            ip += 2;
        } else {
            output.push(first.clone());
            ip += 1;
        }
    }
    offsets[code.len()] = output.len();
    for op in &mut output {
        if let Some(target) = op.jump_target_mut() {
            *target = offsets[*target];
        }
    }
    *code = output;
}

fn pair(
    first: &Instruction,
    second: &Instruction,
    uses: &HashMap<ValueSlot, usize>,
) -> Option<Instruction> {
    use Instruction::*;
    let temp = first.written_slot()?;
    if uses.get(&temp) != Some(&1) {
        return None;
    }
    match (first, second) {
        (
            FloatMultiply {
                dst: product,
                left,
                right,
            },
            Smoothstep { dst, value },
        ) if product == value => Some(FloatMultiplySmoothstep {
            dst: *dst,
            left: *left,
            right: *right,
        }),
        (
            FloatMultiply {
                dst: product,
                left,
                right,
            },
            FloatAdd {
                dst,
                left: a,
                right: b,
            },
        ) if product == a || product == b => Some(FloatMultiplyAdd {
            dst: *dst,
            left: *left,
            right: *right,
            addend: if product == a { *b } else { *a },
        }),
        (
            FloatMultiplyConst {
                dst: product,
                value,
                constant_bits,
            },
            FloatAdd { dst, left, right },
        ) if product == left || product == right => Some(FloatMultiplyAddConst {
            dst: *dst,
            value: *value,
            constant_bits: *constant_bits,
            addend: if product == left { *right } else { *left },
        }),
        _ => None,
    }
}
