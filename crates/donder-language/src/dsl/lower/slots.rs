//! Slot allocation. Emission gives every value its own virtual slot; liveness
//! over the structured code then packs them into few slots of each bank and
//! kind. A value read inside a reduction but defined before it stays live
//! through every iteration. Values of the query and target blocks persist
//! across strips and are never shared.
use crate::dsl::bytecode::{Bank, Banks, Instruction, Slot, SlotKind};
use std::collections::{BTreeSet, HashMap};

/// A virtual slot: its bank, whether it is a row, and its index.
type Key = (Bank, bool, u16);

#[derive(Clone, Copy)]
struct Interval {
    start: usize,
    end: usize,
}

fn key(bank: Bank, slot: Slot) -> Option<Key> {
    match slot.kind() {
        SlotKind::Scalar(index) => Some((bank, false, index)),
        SlotKind::Row(index) => Some((bank, true, index)),
        SlotKind::Input(_) => None,
    }
}

fn place(row: bool, index: u16) -> Slot {
    if row {
        Slot::row(index)
    } else {
        Slot::scalar(index)
    }
}

/// Map virtual slots onto physical ones. Returns the scalar and row counts.
pub(super) fn allocate(
    code: &mut [Instruction],
    pool: &mut [Slot],
    result: &mut Slot,
    prefix_end: usize,
) -> (Banks, Banks) {
    let end = code.len();
    let mut intervals: HashMap<Key, Interval> = HashMap::new();
    let mut touch = |bank, slot: Slot, at: usize| {
        if let Some(key) = key(bank, slot) {
            let interval = intervals
                .entry(key)
                .or_insert(Interval { start: at, end: at });
            interval.start = interval.start.min(at);
            interval.end = interval.end.max(at);
        }
    };
    // Reductions, as their instruction and last nested position.
    let mut loops = Vec::new();
    for (at, instruction) in code.iter().enumerate() {
        let mut copy = instruction.clone();
        match *instruction {
            Instruction::Reduce {
                bank,
                acc,
                index,
                start,
                end,
                filter,
                value,
                loop_len,
                contribute_len,
                ..
            } => {
                // Bounds are read by every iteration and the index written
                // there; the filter after the loop part; the value and the
                // accumulator at the end of each iteration.
                let filter_at = at + usize::from(loop_len);
                let last = filter_at + usize::from(contribute_len);
                loops.push((at, last));
                touch(Bank::Int, start, last);
                touch(Bank::Int, end, last);
                touch(Bank::Int, index, at);
                if !filter.is_none() {
                    touch(Bank::Bool, filter, filter_at);
                }
                touch(bank, value, last);
                touch(bank, acc, last);
            }
            Instruction::Pick { bank, items, .. } => {
                copy.visit_slots(&mut |bank, slot, _| touch(bank, *slot, at));
                for &item in &pool[items.range()] {
                    touch(bank, item, at);
                }
            }
            _ => copy.visit_slots(&mut |bank, slot, _| touch(bank, *slot, at)),
        }
    }
    touch(Bank::Color, *result, end);
    for interval in intervals.values_mut() {
        if interval.start < prefix_end {
            interval.end = end;
        }
        for &(start, last) in &loops {
            if interval.start <= start && interval.end > start {
                interval.end = interval.end.max(last);
            }
        }
    }

    // Linear scan per bank and kind, lowest free slot first. A slot whose
    // last use is an instruction is still busy while that instruction writes.
    let mut order: Vec<(Key, Interval)> = intervals.into_iter().collect();
    order.sort_by_key(|&(key, interval)| (interval.start, interval.end, key.1, key.2));
    let mut assigned: HashMap<Key, u16> = HashMap::new();
    let mut active: HashMap<(Bank, bool), Vec<(usize, u16)>> = HashMap::new();
    let mut free: HashMap<(Bank, bool), BTreeSet<u16>> = HashMap::new();
    let mut counts: HashMap<(Bank, bool), u16> = HashMap::new();
    for ((bank, row, index), interval) in order {
        let group = (bank, row);
        let busy = active.entry(group).or_default();
        let available = free.entry(group).or_default();
        busy.retain(|&(until, slot)| {
            let expired = until < interval.start;
            if expired {
                available.insert(slot);
            }
            !expired
        });
        let slot = match available.pop_first() {
            Some(slot) => slot,
            None => {
                let count = counts.entry(group).or_insert(0);
                *count += 1;
                *count - 1
            }
        };
        busy.push((interval.end, slot));
        assigned.insert((bank, row, index), slot);
    }

    let rename = |bank, slot: &mut Slot| {
        if let Some(key @ (_, row, _)) = key(bank, *slot) {
            *slot = place(row, assigned[&key]);
        }
    };
    for instruction in code.iter_mut() {
        if let Instruction::Pick { bank, items, .. } = *instruction {
            for item in &mut pool[items.range()] {
                rename(bank, item);
            }
        }
        instruction.visit_slots(&mut |bank, slot, _| rename(bank, slot));
    }
    rename(Bank::Color, result);
    let mut scalars = Banks::default();
    let mut rows = Banks::default();
    for ((bank, row), count) in counts {
        *if row {
            rows.get_mut(bank)
        } else {
            scalars.get_mut(bank)
        } = count;
    }
    (scalars, rows)
}
