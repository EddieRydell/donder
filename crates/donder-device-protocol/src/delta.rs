//! A show delta turns the show a controller holds into a new one without
//! sending it whole. It is little-endian 32-bit words: a [`Header`], the
//! operation count, one word per operation, then the inserted words in
//! operation order. Each operation keeps, drops or inserts a run of words,
//! front to back through both shows.

/// Words before the operations: the header and the operation count.
const HEADER_WORDS: usize = 5;
const KIND_SHIFT: u32 = 30;
const MAX_RUN: u32 = (1 << KIND_SHIFT) - 1;
const KEEP: u32 = 0;
const DROP: u32 = 1;
const INSERT: u32 = 2;

/// The show a delta applies to and the show it makes, by identity and length
/// in bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub base_id: u32,
    pub base_bytes: u32,
    pub target_id: u32,
    pub target_bytes: u32,
}

/// A delta whose operations account for every word of both shows.
pub struct Delta<'a> {
    pub header: Header,
    operations: &'a [u32],
    inserted: &'a [u32],
}

#[derive(Clone, Copy, Debug)]
enum Operation {
    Keep(usize),
    Drop(usize),
    Insert(usize),
}

fn operation(word: u32) -> Option<Operation> {
    let run = (word & MAX_RUN) as usize;
    match word >> KIND_SHIFT {
        KEEP => Some(Operation::Keep(run)),
        DROP => Some(Operation::Drop(run)),
        INSERT => Some(Operation::Insert(run)),
        _ => None,
    }
}

impl<'a> Delta<'a> {
    /// Read a delta, or `None` when it is malformed.
    pub fn decode(words: &'a [u32]) -> Option<Self> {
        let (head, rest) = words.split_at_checked(HEADER_WORDS)?;
        let header = Header {
            base_id: head[0],
            base_bytes: head[1],
            target_id: head[2],
            target_bytes: head[3],
        };
        let (operations, inserted) = rest.split_at_checked(head[4] as usize)?;
        let (mut base, mut target, mut insert) = (0usize, 0usize, 0usize);
        for &word in operations {
            match operation(word)? {
                Operation::Keep(run) => {
                    base += run;
                    target += run;
                }
                Operation::Drop(run) => base += run,
                Operation::Insert(run) => {
                    target += run;
                    insert += run;
                }
            }
        }
        (base * 4 == header.base_bytes as usize
            && target * 4 == header.target_bytes as usize
            && insert == inserted.len())
        .then_some(Self {
            header,
            operations,
            inserted,
        })
    }

    /// Words the base and the target both need while the delta applies.
    pub fn room_words(&self) -> usize {
        self.header.base_bytes.max(self.header.target_bytes) as usize / 4
    }

    /// Turn the base show at the start of `words` into the target, in place.
    /// `words` holds at least [`Self::room_words`]. Kept runs that move
    /// toward the start go first, front to back, then those that move toward
    /// the end, back to front, so no run is overwritten before it moves;
    /// inserted runs fill the gaps last.
    pub fn apply(&self, words: &mut [u32]) {
        let (mut base, mut target) = (0, 0);
        for &word in self.operations {
            match operation(word) {
                Some(Operation::Keep(run)) => {
                    if target <= base {
                        words.copy_within(base..base + run, target);
                    }
                    base += run;
                    target += run;
                }
                Some(Operation::Drop(run)) => base += run,
                Some(Operation::Insert(run)) => target += run,
                None => unreachable!("decoding checked every operation"),
            }
        }
        for &word in self.operations.iter().rev() {
            match operation(word) {
                Some(Operation::Keep(run)) => {
                    base -= run;
                    target -= run;
                    if target > base {
                        words.copy_within(base..base + run, target);
                    }
                }
                Some(Operation::Drop(run)) => base -= run,
                Some(Operation::Insert(run)) => target -= run,
                None => unreachable!("decoding checked every operation"),
            }
        }
        let mut inserted = self.inserted;
        for &word in self.operations {
            match operation(word) {
                Some(Operation::Keep(run)) => target += run,
                Some(Operation::Drop(_)) => {}
                Some(Operation::Insert(run)) => {
                    let (run_words, rest) = inserted.split_at(run);
                    words[target..target + run].copy_from_slice(run_words);
                    inserted = rest;
                    target += run;
                }
                None => unreachable!("decoding checked every operation"),
            }
        }
    }
}

/// The delta that turns `base` into `target`, two encoded shows, as bytes.
#[cfg(feature = "diff")]
pub fn diff(base: &[u8], base_id: u32, target: &[u8], target_id: u32) -> alloc::vec::Vec<u8> {
    use alloc::vec::Vec;
    use similar::{Algorithm, DiffOp, capture_diff_slices};
    let words = |bytes: &[u8]| -> Vec<u32> {
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|word| u32::from_le_bytes(*word))
            .collect()
    };
    let (old, new) = (words(base), words(target));
    let mut operations = Vec::new();
    let mut inserted = Vec::new();
    let mut push = |kind: u32, mut run: usize| {
        while run > 0 {
            let part = run.min(MAX_RUN as usize);
            operations.push(kind << KIND_SHIFT | part as u32);
            run -= part;
        }
    };
    for op in capture_diff_slices(Algorithm::Myers, &old, &new) {
        match op {
            DiffOp::Equal { len, .. } => push(KEEP, len),
            DiffOp::Delete { old_len, .. } => push(DROP, old_len),
            DiffOp::Insert {
                new_index, new_len, ..
            } => {
                push(INSERT, new_len);
                inserted.extend_from_slice(&new[new_index..new_index + new_len]);
            }
            DiffOp::Replace {
                old_len,
                new_index,
                new_len,
                ..
            } => {
                push(DROP, old_len);
                push(INSERT, new_len);
                inserted.extend_from_slice(&new[new_index..new_index + new_len]);
            }
        }
    }
    [
        base_id,
        base.len() as u32,
        target_id,
        target.len() as u32,
        operations.len() as u32,
    ]
    .into_iter()
    .chain(operations)
    .chain(inserted)
    .flat_map(u32::to_le_bytes)
    .collect()
}
