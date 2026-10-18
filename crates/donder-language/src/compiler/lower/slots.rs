//! Slot allocation. Emission gives every value its own virtual slots; liveness
//! over the structured code then packs them into few slots of each storage
//! bank and kind. A color's three floats stay together. A value read inside a
//! reduction but defined before it stays live through every iteration. Values
//! of the query and target blocks read after their block persist across
//! strips and are never shared.
use donder_runtime_types::bytecode::{Bank, Banks, Instruction, Slot, SlotKind};
use std::collections::HashMap;

/// A virtual slot: its storage bank, whether it is a row, and its index.
type Key = (Bank, bool, u16);

/// Slots held until a position: its last use, first slot and width.
type Held = (usize, u16, u16);

#[derive(Clone, Copy)]
struct Interval {
    start: usize,
    end: usize,
}

fn key(bank: Bank, slot: Slot) -> Option<Key> {
    match slot.kind() {
        SlotKind::Scalar(index) => Some((bank.storage(), false, index)),
        SlotKind::Row(index) => Some((bank.storage(), true, index)),
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

/// Map virtual slots onto physical ones. Positions are word offsets;
/// `blocks` end the query and the target blocks. Returns the scalar and row
/// counts.
pub(super) fn allocate(
    code: &mut [Instruction],
    result: &mut Slot,
    blocks: [usize; 2],
) -> (Banks, Banks) {
    let mut at = 0;
    let positions: Vec<usize> = code
        .iter()
        .map(|instruction| {
            let start = at;
            at += instruction.width();
            start
        })
        .collect();
    let end = at;

    // Colors: each color's first virtual float slot, whose next two belong
    // to it.
    let mut owner: HashMap<Key, Key> = HashMap::new();
    let mut colors = |bank: Bank, slot: Slot| {
        if bank == Bank::Color
            && let Some((storage, row, index)) = key(bank, slot)
        {
            for channel in 0..3 {
                owner.insert((storage, row, index + channel), (storage, row, index));
            }
        }
    };
    for instruction in code.iter() {
        let mut copy = instruction.clone();
        copy.visit_slots(&mut |bank, slot, _| colors(bank, *slot));
    }
    colors(Bank::Color, *result);
    let unit = |key: Key| owner.get(&key).copied().unwrap_or(key);
    let width = |key: Key| if owner.contains_key(&key) { 3 } else { 1 };

    let mut intervals: HashMap<Key, Interval> = HashMap::new();
    let mut touch = |bank, slot: Slot, at: usize| {
        if let Some(key) = key(bank, slot) {
            let interval = intervals
                .entry(unit(key))
                .or_insert(Interval { start: at, end: at });
            interval.start = interval.start.min(at);
            interval.end = interval.end.max(at);
        }
    };
    // Reductions, as their position and last nested position.
    let mut loops = Vec::new();
    for (instruction, &at) in code.iter().zip(&positions) {
        let mut copy = instruction.clone();
        // The word before the nested code.
        let header = at + instruction.width() - 1;
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
                let filter_at = header + usize::from(loop_len);
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
            Instruction::Stencil {
                acc,
                index,
                start,
                end,
                weight,
                scale,
                pixel,
                source_len,
                tap_len,
                ..
            } => {
                // The source block runs once per strip of the neighborhood and
                // the weight is read after it; the offset block runs once per
                // offset, and the rest is read with every offset's term.
                let source_end = header + usize::from(source_len);
                let last = source_end + usize::from(tap_len);
                loops.push((at, last));
                touch(Bank::Int, start, last);
                touch(Bank::Int, end, last);
                touch(Bank::Int, index, at);
                touch(Bank::Float, weight, source_end);
                touch(Bank::Float, scale, last);
                touch(Bank::Float, pixel, last);
                touch(Bank::Color, acc, last);
            }
            Instruction::Scan {
                dst,
                decay,
                weight,
                source_len,
                ..
            } => {
                // The source block runs once per strip of the frame; the scan
                // reads the decay and the weight after each and writes its
                // destination last.
                let last = header + usize::from(source_len);
                loops.push((at, last));
                touch(Bank::Float, decay, last);
                touch(Bank::Float, weight, last);
                touch(Bank::Color, dst, last);
            }
            Instruction::Tabulate {
                bank,
                dst,
                index,
                start,
                end,
                value,
                len,
            } => {
                // The block runs once per index; the bounds and the value are
                // read after each, and the table written last.
                let last = header + usize::from(len);
                loops.push((at, last));
                touch(Bank::Int, start, last);
                touch(Bank::Int, end, last);
                touch(Bank::Int, index, at);
                touch(bank, value, last);
                touch(bank, dst, last);
            }
            _ => copy.visit_slots(&mut |bank, slot, _| touch(bank, *slot, at)),
        }
    }
    touch(Bank::Color, *result, end);
    for interval in intervals.values_mut() {
        for &(start, last) in &loops {
            if interval.start <= start && interval.end > start {
                interval.end = interval.end.max(last);
            }
        }
        // A query value read after its block, or a target value read in the
        // body, persists: its block does not run again before every read.
        if let Some(&block_end) = blocks.iter().find(|&&block_end| interval.start < block_end)
            && interval.end >= block_end
        {
            interval.end = end;
        }
    }

    // Linear scan per storage bank and kind, lowest free run first. A slot
    // whose last use is an instruction is still busy while that instruction
    // writes.
    let mut order: Vec<(Key, Interval)> = intervals.into_iter().collect();
    order.sort_by_key(|&(key, interval)| (interval.start, interval.end, key.1, key.2));
    let mut assigned: HashMap<Key, u16> = HashMap::new();
    let mut active: HashMap<(Bank, bool), Vec<Held>> = HashMap::new();
    let mut used: HashMap<(Bank, bool), Vec<bool>> = HashMap::new();
    for (key @ (bank, row, _), interval) in order {
        let group = (bank, row);
        let busy = active.entry(group).or_default();
        let taken = used.entry(group).or_default();
        busy.retain(|&(until, slot, width)| {
            let expired = until < interval.start;
            if expired {
                for slot in slot..slot + width {
                    taken[usize::from(slot)] = false;
                }
            }
            !expired
        });
        let width = width(key);
        let first = (0..)
            .find(|&slot: &u16| {
                (slot..slot + width)
                    .all(|slot| !taken.get(usize::from(slot)).copied().unwrap_or(false))
            })
            .unwrap_or_else(|| unreachable!("slots are unbounded"));
        let needed = usize::from(first + width);
        if taken.len() < needed {
            taken.resize(needed, false);
        }
        for slot in first..first + width {
            taken[usize::from(slot)] = true;
        }
        busy.push((interval.end, first, width));
        assigned.insert(key, first);
    }

    let rename = |bank, slot: &mut Slot| {
        if let Some(key @ (_, row, index)) = key(bank, *slot) {
            let base = unit(key);
            *slot = place(row, assigned[&base] + (index - base.2));
        }
    };
    for instruction in code.iter_mut() {
        instruction.visit_slots(&mut |bank, slot, _| rename(bank, slot));
    }
    rename(Bank::Color, result);
    let mut scalars = Banks::default();
    let mut rows = Banks::default();
    for ((bank, row), taken) in used {
        *if row {
            rows.get_mut(bank)
        } else {
            scalars.get_mut(bank)
        } = taken.len() as u16;
    }
    (scalars, rows)
}
