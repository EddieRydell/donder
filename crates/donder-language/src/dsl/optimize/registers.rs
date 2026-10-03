//! Allocate primitive temporaries at preparation time. Reference banks retain
//! their ownership and initialization contracts. Prefix storage persists across
//! pixel invocations, so it cannot alias any pixel-body temporary.
use super::{Instruction, SlotLayout, ValueSlot, dataflow, slots};
use std::collections::{HashMap, HashSet};

pub(in crate::dsl) fn reuse(
    code: &mut [Instruction],
    operands: &mut [ValueSlot],
    layout: &mut SlotLayout,
    pixel_entry: u32,
) {
    let mut reads = Vec::with_capacity(code.len());
    let mut writes = Vec::with_capacity(code.len());
    let mut pinned = HashSet::new();
    for (ip, op) in code.iter().enumerate() {
        let mut input = HashSet::new();
        let mut output = HashSet::new();
        slots(&mut op.clone(), operands, |slot, write| {
            if ip < pixel_entry as usize {
                pinned.insert(slot);
            }
            if write {
                output.insert(slot);
            } else {
                input.insert(slot);
            }
            slot
        });
        reads.push(input);
        writes.push(output);
    }
    // Unlike dead-code liveness, every executed operand counts, even when the
    // instruction's result is unused. Include backward edges and both branches.
    let successors: Vec<_> = (0..code.len())
        .map(|ip| dataflow::successors(code, ip))
        .collect();
    let mut before = vec![HashSet::new(); code.len()];
    let mut after = before.clone();
    loop {
        let mut changed = false;
        for ip in (0..code.len()).rev() {
            let mut live = HashSet::new();
            for &next in &successors[ip] {
                live.extend(before[next].iter().copied());
            }
            after[ip] = live.clone();
            live.retain(|slot| !writes[ip].contains(slot));
            live.extend(reads[ip].iter().copied());
            if before[ip] != live {
                before[ip] = live;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    if let Some(initial) = before.first() {
        pinned.extend(initial.iter().copied());
    }
    let mut ranges: HashMap<ValueSlot, (usize, usize)> = HashMap::new();
    for ip in 0..code.len() {
        for &slot in before[ip]
            .iter()
            .chain(&after[ip])
            .chain(&reads[ip])
            .chain(&writes[ip])
        {
            let range = ranges.entry(slot).or_insert((ip, ip));
            range.1 = ip;
        }
    }
    let mut mapping = HashMap::new();
    for bank in [
        ValueSlot::Int(super::IntSlot(0)),
        ValueSlot::Float(super::FloatSlot(0)),
        ValueSlot::Bool(super::super::bytecode::BoolSlot(0)),
        ValueSlot::Color(super::ColorSlot(0)),
    ] {
        let mut intervals: Vec<_> = ranges
            .iter()
            .filter(|(slot, _)| core::mem::discriminant(*slot) == core::mem::discriminant(&bank))
            .map(|(&slot, &(start, end))| (slot, start, end))
            .collect();
        intervals.sort_unstable_by_key(|&(slot, start, _)| {
            (!pinned.contains(&slot), start, slot.index())
        });
        let mut ends = Vec::new();
        for (slot, start, end) in intervals {
            let index = if pinned.contains(&slot) {
                None
            } else {
                ends.iter().position(|&last| last < start)
            }
            .unwrap_or(ends.len());
            let end = if pinned.contains(&slot) {
                usize::MAX
            } else {
                end
            };
            if index == ends.len() {
                ends.push(end);
            } else {
                ends[index] = end;
            }
            mapping.insert(slot, slot.with_index(index as u32));
        }
        let count = ends.len() as u32;
        match bank {
            ValueSlot::Int(_) => layout.ints = count,
            ValueSlot::Float(_) => layout.floats = count,
            ValueSlot::Bool(_) => layout.bools = count,
            ValueSlot::Color(_) => layout.colors = count,
            _ => unreachable!(),
        }
    }
    for op in code {
        slots(op, operands, |slot, _| {
            mapping.get(&slot).copied().unwrap_or(slot)
        });
    }
}
