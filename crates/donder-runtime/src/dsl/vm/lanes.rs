//! Fixed-width control flow for numeric pixel blocks. Active lanes share an
//! instruction pointer; suspended lanes retain only their next instruction.
//! Straight-line execution checks the next join without scanning per-lane PCs.
use super::COLOR_BLOCK_WIDTH;

#[derive(Clone, Copy, Debug)]
pub(super) struct Mask {
    bits: u32,
}

impl PartialEq for Mask {
    fn eq(&self, other: &Self) -> bool {
        self.bits == other.bits
    }
}

impl Eq for Mask {}

impl Mask {
    const EMPTY: Self = Self { bits: 0 };
    pub(super) const FIRST: Self = Self { bits: 1 };

    pub(super) fn full(width: usize) -> Self {
        debug_assert!((1..=COLOR_BLOCK_WIDTH).contains(&width));
        Self {
            bits: u32::MAX >> (32 - width),
        }
    }

    fn insert(&mut self, lane: usize) {
        debug_assert!(lane < COLOR_BLOCK_WIDTH && self.bits & (1 << lane) == 0);
        self.bits |= 1 << lane;
    }

    #[cfg(test)]
    fn from_bits(bits: u32) -> Self {
        let mut result = Self::EMPTY;
        for lane in 0..COLOR_BLOCK_WIDTH {
            if bits & (1 << lane) != 0 {
                result.insert(lane);
            }
        }
        result
    }
}

impl IntoIterator for Mask {
    type Item = usize;
    type IntoIter = ActiveLanes;

    fn into_iter(self) -> ActiveLanes {
        ActiveLanes {
            bits: self.bits,
            next: 0,
        }
    }
}

pub(super) struct ActiveLanes {
    bits: u32,
    next: usize,
}

impl Iterator for ActiveLanes {
    type Item = usize;

    fn next(&mut self) -> Option<usize> {
        if self.bits == 0 {
            return None;
        }
        if self.bits & 1 == 0 {
            let skip = skip_inactive(self.bits);
            self.bits >>= skip;
            self.next += skip as usize;
        }
        let lane = self.next;
        self.bits >>= 1;
        self.next += 1;
        Some(lane)
    }
}

// The classic ESP32 lowers trailing_zeros to software. Keep that code shared;
// consecutive active lanes never need this call.
#[inline(never)]
fn skip_inactive(bits: u32) -> u32 {
    bits.trailing_zeros()
}

pub(super) struct Flow {
    pub(super) active: Mask,
    waiting: [usize; COLOR_BLOCK_WIDTH],
    width: usize,
    join: usize,
}

/// Resolve register-bank addresses once per instruction. The lane loop borrows
/// only its bank, so writes cannot make the compiler reload VM state per lane.
/// Fixed rows bound lane addresses once. Each lane reads its operands before
/// writing, preserving aliases without copying input rows.
#[inline(always)]
pub(super) fn binary<T: Copy>(
    bank: &mut [T],
    mask: Mask,
    stride: usize,
    dst: u32,
    left: u32,
    right: u32,
    op: impl Fn(T, T) -> T,
) {
    if stride == 1 {
        bank[dst as usize] = op(bank[left as usize], bank[right as usize]);
        return;
    }
    debug_assert_eq!(stride, COLOR_BLOCK_WIDTH);
    let (dst, left, right) = (dst as usize, left as usize, right as usize);
    let rows = checked_rows(bank, [dst, left, right]);
    for lane in mask {
        rows[dst][lane] = op(rows[left][lane], rows[right][lane]);
    }
}

#[inline(always)]
pub(super) fn unary<T: Copy>(
    bank: &mut [T],
    mask: Mask,
    stride: usize,
    dst: u32,
    src: u32,
    op: impl Fn(T) -> T,
) {
    if stride == 1 {
        bank[dst as usize] = op(bank[src as usize]);
        return;
    }
    debug_assert_eq!(stride, COLOR_BLOCK_WIDTH);
    let (dst, src) = (dst as usize, src as usize);
    let rows = checked_rows(bank, [dst, src]);
    for lane in mask {
        rows[dst][lane] = op(rows[src][lane]);
    }
}

#[inline(always)]
fn checked_rows<T, const N: usize>(
    bank: &mut [T],
    slots: [usize; N],
) -> &mut [[T; COLOR_BLOCK_WIDTH]] {
    let (rows, remainder) = bank.as_chunks_mut::<COLOR_BLOCK_WIDTH>();
    debug_assert!(remainder.is_empty());
    for slot in slots {
        assert!(slot < rows.len(), "numeric register row is in bounds");
    }
    rows
}

#[inline(always)]
pub(super) fn ternary<T: Copy>(
    bank: &mut [T],
    mask: Mask,
    stride: usize,
    dst: u32,
    sources: [u32; 3],
    op: impl Fn(T, T, T) -> T,
) {
    let [a, b, c] = sources.map(|slot| slot as usize);
    let dst = dst as usize;
    if stride == 1 {
        bank[dst] = op(bank[a], bank[b], bank[c]);
        return;
    }
    debug_assert_eq!(stride, COLOR_BLOCK_WIDTH);
    let rows = checked_rows(bank, [dst, a, b, c]);
    for lane in mask {
        rows[dst][lane] = op(rows[a][lane], rows[b][lane], rows[c][lane]);
    }
}

impl Flow {
    pub(super) fn new(width: usize) -> Self {
        Self {
            active: Mask::full(width),
            waiting: [usize::MAX; COLOR_BLOCK_WIDTH],
            width,
            join: usize::MAX,
        }
    }

    pub(super) fn redirect(&mut self, lane: usize, instruction: usize) {
        self.waiting[lane] = instruction;
    }

    pub(super) fn next_join_remaining(&self, program_len: usize) -> usize {
        program_len.checked_sub(self.join).unwrap_or(usize::MAX)
    }

    /// Select the earliest pending instruction after a branch or return. This
    /// also handles backward edges and lanes with different bounded loop counts.
    #[inline(never)]
    pub(super) fn resume(&mut self) -> Option<usize> {
        let first = *self.waiting[..self.width].iter().min()?;
        if first == usize::MAX {
            self.active = Mask::EMPTY;
            return None;
        }
        self.active = Mask::EMPTY;
        self.join = first;
        self.merge(first);
        Some(first)
    }

    #[inline]
    pub(super) fn merge(&mut self, instruction: usize) {
        if instruction == self.join {
            self.merge_waiting(instruction);
        }
    }

    #[inline(never)]
    fn merge_waiting(&mut self, instruction: usize) {
        self.join = usize::MAX;
        for (lane, next) in self.waiting[..self.width].iter_mut().enumerate() {
            if *next == instruction {
                self.active.insert(lane);
                *next = usize::MAX;
            } else {
                self.join = self.join.min(*next);
            }
        }
    }
}

// Expand one dispatch around the existing instruction bodies. Numeric arms
// execute their body for the active lanes; wide color operations already own
// their lane loop. Branches retain each lane's destination before reconverging.
// The body remains the single description of each instruction's scalar meaning.
macro_rules! dispatch {
    ($vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident; $($arms:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [] $($arms)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]) => {
        match $op { $($out)* }
    };
    (@parse [$($args:tt)*] [$($out:tt)*] , $($rest:tt)*) => {
        dispatch!(@parse [$($args)*] [$($out)*] $($rest)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]
        @once $pattern:pat => $body:block $($rest:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [$($out)* $pattern => $body,] $($rest)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]
        @once $pattern:pat => $body:expr, $($rest:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [$($out)* $pattern => { $body; },] $($rest)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]
        @branch $pattern:pat => $body:block $($rest:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [$($out)*
            $pattern => {
                if $vm.lanes.is_some() {
                    let fallthrough = $cursor;
                    for lane in $vm.numeric_mask() {
                        $vm.lane = lane;
                        $cursor = fallthrough;
                        $body
                        if let Some(flow) = &mut $vm.lanes {
                            flow.redirect(lane, $code.len() - $cursor.len());
                        }
                    }
                    if let Some(flow) = &mut $vm.lanes {
                        let Some(next) = flow.resume() else {
                            unreachable!("a branch retains its active lanes")
                        };
                        $vm.active = flow.active;
                        $event = flow.next_join_remaining($code.len());
                        $cursor = &$code[next..];
                    }
                } else {
                    $body
                }
            },
        ] $($rest)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]
        $pattern:pat => $body:block $($rest:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [$($out)*
            $pattern => {
                for lane in $vm.numeric_mask() {
                    $vm.lane = lane;
                    $body
                }
            },
        ] $($rest)*)
    };
    (@parse [$vm:ident, $code:ident, $cursor:ident, $op:ident, $event:ident] [$($out:tt)*]
        $pattern:pat => $body:expr, $($rest:tt)*) => {
        dispatch!(@parse [$vm, $code, $cursor, $op, $event] [$($out)*
            $pattern => {
                for lane in $vm.numeric_mask() {
                    $vm.lane = lane;
                    $body;
                }
            },
        ] $($rest)*)
    };
}

pub(super) use dispatch;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[test]
    fn dense_and_sparse_masks_include_the_highest_lane() {
        for width in 1..=COLOR_BLOCK_WIDTH {
            assert_eq!(
                Mask::full(width).into_iter().collect::<Vec<_>>(),
                (0..width).collect::<Vec<_>>()
            );
        }
        let mut flow = Flow::new(COLOR_BLOCK_WIDTH);
        for lane in flow.active {
            flow.redirect(lane, if lane % 2 == 0 { 3 } else { 7 });
        }
        assert_eq!(flow.resume(), Some(3));
        assert_eq!(
            flow.active.into_iter().collect::<Vec<_>>(),
            (0..COLOR_BLOCK_WIDTH).step_by(2).collect::<Vec<_>>()
        );
        flow.merge(7);
        assert_eq!(
            flow.active.into_iter().collect::<Vec<_>>(),
            (0..COLOR_BLOCK_WIDTH).collect::<Vec<_>>()
        );
    }

    #[test]
    fn arithmetic_preserves_register_aliases_and_inactive_lanes() {
        let original: [i32; 4 * COLOR_BLOCK_WIDTH] = core::array::from_fn(|i| i as i32 * 7 - 100);
        for bits in (0..=u8::MAX as u32).chain([u32::MAX >> (32 - COLOR_BLOCK_WIDTH)]) {
            let mask = Mask::from_bits(bits);
            for dst in 0..4 {
                for left in 0..4 {
                    for right in 0..4 {
                        let mut actual = original;
                        binary(
                            &mut actual,
                            mask,
                            COLOR_BLOCK_WIDTH,
                            dst,
                            left,
                            right,
                            |a, b| a * 3 - b,
                        );
                        for (index, value) in actual.into_iter().enumerate() {
                            let lane = index % COLOR_BLOCK_WIDTH;
                            let expected = if index / COLOR_BLOCK_WIDTH == dst as usize
                                && bits & (1 << lane) != 0
                            {
                                original[left as usize * COLOR_BLOCK_WIDTH + lane] * 3
                                    - original[right as usize * COLOR_BLOCK_WIDTH + lane]
                            } else {
                                original[index]
                            };
                            assert_eq!(
                                value, expected,
                                "bits={bits} dst={dst} left={left} right={right} index={index}"
                            );
                        }
                    }
                    let mut actual = original;
                    unary(&mut actual, mask, COLOR_BLOCK_WIDTH, dst, left, |a| -a + 1);
                    for (index, value) in actual.into_iter().enumerate() {
                        let lane = index % COLOR_BLOCK_WIDTH;
                        let expected = if index / COLOR_BLOCK_WIDTH == dst as usize
                            && bits & (1 << lane) != 0
                        {
                            -original[left as usize * COLOR_BLOCK_WIDTH + lane] + 1
                        } else {
                            original[index]
                        };
                        assert_eq!(
                            value, expected,
                            "bits={bits} dst={dst} source={left} index={index}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn divergent_paths_rejoin_and_return_without_reviving_finished_lanes() {
        let mut flow = Flow::new(3);
        flow.redirect(0, 10);
        flow.redirect(1, 4);
        flow.redirect(2, 10);
        assert_eq!(flow.resume(), Some(4));
        assert_eq!(flow.active, Mask::from_bits(0b010));
        flow.merge(7);
        assert_eq!(flow.active, Mask::from_bits(0b010));
        flow.merge(10);
        assert_eq!(flow.active, Mask::from_bits(0b111));
        flow.redirect(0, 20);
        flow.redirect(1, 12);
        flow.redirect(2, 20);
        assert_eq!(flow.resume(), Some(12));
        // Lane 1 returns. Only its suspended peers continue.
        assert_eq!(flow.resume(), Some(20));
        assert_eq!(flow.active, Mask::from_bits(0b101));
        assert_eq!(flow.resume(), None);
    }

    #[test]
    fn backward_edges_keep_early_exiting_lanes_suspended_until_loop_exit() {
        let mut flow = Flow::new(8);
        for lane in flow.active {
            flow.redirect(lane, if lane == 0 { 30 } else { 5 });
        }
        assert_eq!(flow.resume(), Some(5));
        assert_eq!(flow.active, Mask::from_bits(0xfe));
        for lane in flow.active {
            flow.redirect(lane, if lane < 4 { 30 } else { 5 });
        }
        assert_eq!(flow.resume(), Some(5));
        assert_eq!(flow.active, Mask::from_bits(0xf0));
        flow.merge(29);
        assert_eq!(flow.active, Mask::from_bits(0xf0));
        flow.merge(30);
        assert_eq!(flow.active, Mask::full(8));
    }
}
