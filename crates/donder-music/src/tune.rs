//! Tunes: a melody recognised in any key and any octave, with a wrong note
//! now and then forgiven.

use std::collections::VecDeque;

/// The most notes a tune has.
pub const MAX_TUNE_NOTES: usize = 24;
/// The fewest notes a tune has: fewer match too much by chance.
pub const MIN_TUNE_NOTES: usize = 5;

/// A melody, as the MIDI notes of its opening, in any key.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Melody {
    notes: [u8; MAX_TUNE_NOTES],
    len: u8,
}

impl Melody {
    /// `None` when there are too few or too many notes, or a note MIDI does
    /// not have.
    pub fn new(notes: &[u8]) -> Option<Self> {
        if !(MIN_TUNE_NOTES..=MAX_TUNE_NOTES).contains(&notes.len())
            || notes.iter().any(|note| *note > 127)
        {
            return None;
        }
        let mut melody = Self {
            notes: [0; MAX_TUNE_NOTES],
            len: notes.len() as u8,
        };
        melody.notes[..notes.len()].copy_from_slice(notes);
        Some(melody)
    }

    pub fn notes(&self) -> &[u8] {
        &self.notes[..usize::from(self.len)]
    }
}

/// Notes struck this close together are one chord, whose top note carries
/// the melody.
const CHORD_SPREAD: f64 = 0.05;
/// Notes below this are accompaniment, not melody: G3.
const LOWEST_MELODY: u8 = 55;

/// The melody line: the top note of each strike above the accompaniment.
#[derive(Clone, Debug, Default)]
pub(crate) struct MelodyLine {
    notes: VecDeque<(f64, u8)>,
    /// Counts notes added, so a tune is heard once each time it is played.
    added: u64,
}

impl MelodyLine {
    /// Hear a note; `true` when it adds to or changes the line.
    pub(crate) fn hear(&mut self, note: u8, time: f64) -> bool {
        if note < LOWEST_MELODY {
            return false;
        }
        match self.notes.back_mut() {
            Some((at, top)) if time - *at <= CHORD_SPREAD => {
                if note <= *top {
                    return false;
                }
                *top = note;
            }
            _ => {
                self.notes.push_back((time, note));
                self.added += 1;
                if self.notes.len() > MAX_TUNE_NOTES {
                    self.notes.pop_front();
                }
            }
        }
        true
    }

    pub(crate) fn added(&self) -> u64 {
        self.added
    }

    /// Whether the line ends with `melody` in some key: its notes moved by
    /// one transposition, each in any octave, with about one wrong note in
    /// six forgiven.
    pub(crate) fn ends_with(&self, melody: &Melody) -> bool {
        let wanted = melody.notes();
        if self.notes.len() < wanted.len() {
            return false;
        }
        let played = self.notes.iter().skip(self.notes.len() - wanted.len());
        // How many notes each transposition, in semitones up, explains.
        let mut fits = [0usize; 12];
        for ((_, played), wanted) in played.zip(wanted) {
            fits[usize::from((u16::from(*played) + 12 * 11 - u16::from(*wanted)) % 12)] += 1;
        }
        let best = fits.iter().copied().max().unwrap_or(0);
        wanted.len() - best <= wanted.len() / 6
    }
}
