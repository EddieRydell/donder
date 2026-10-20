//! Chords: the template of chord tones that best explains the sounding pitch
//! classes, with the bass note as a hint to the root.

use crate::PitchClass;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ChordQuality {
    /// Major triads and major sevenths.
    Major,
    /// Minor triads and minor sevenths.
    Minor,
    /// Dominant sevenths, which want to resolve.
    Dominant,
    /// Diminished and half-diminished chords.
    Diminished,
    Augmented,
    /// Suspended seconds and fourths.
    Suspended,
}

impl ChordQuality {
    /// How far a chord of this quality is from rest, before the key is
    /// considered.
    pub(crate) fn tension(self) -> f32 {
        match self {
            Self::Major => 0.1,
            Self::Minor => 0.25,
            Self::Suspended => 0.45,
            Self::Dominant => 0.6,
            Self::Augmented => 0.75,
            Self::Diminished => 0.85,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Chord {
    pub root: PitchClass,
    pub quality: ChordQuality,
}

impl Chord {
    /// Whether this chord resolves `previous`, as V goes to I: the root
    /// falls a fifth from a dominant seventh, or from a major chord to the
    /// tonic of `key`.
    pub fn resolves(self, previous: Chord, key: Option<crate::Key>) -> bool {
        previous.root.above(5) == self.root
            && matches!(self.quality, ChordQuality::Major | ChordQuality::Minor)
            && match previous.quality {
                ChordQuality::Dominant => true,
                ChordQuality::Major => key.is_some_and(|key| key.tonic == self.root),
                _ => false,
            }
    }
}

/// A chord shape: its tones above the root, each with how much it says about
/// the chord.
struct Template {
    quality: ChordQuality,
    tones: &'static [(u8, f32)],
}

const TEMPLATES: &[Template] = &[
    Template {
        quality: ChordQuality::Major,
        tones: &[(0, 1.0), (4, 1.0), (7, 0.7)],
    },
    Template {
        quality: ChordQuality::Major,
        tones: &[(0, 1.0), (4, 1.0), (7, 0.7), (11, 0.8)],
    },
    Template {
        quality: ChordQuality::Minor,
        tones: &[(0, 1.0), (3, 1.0), (7, 0.7)],
    },
    Template {
        quality: ChordQuality::Minor,
        tones: &[(0, 1.0), (3, 1.0), (7, 0.7), (10, 0.8)],
    },
    Template {
        quality: ChordQuality::Dominant,
        tones: &[(0, 1.0), (4, 1.0), (7, 0.6), (10, 1.0)],
    },
    Template {
        quality: ChordQuality::Diminished,
        tones: &[(0, 1.0), (3, 1.0), (6, 1.0)],
    },
    Template {
        quality: ChordQuality::Diminished,
        tones: &[(0, 1.0), (3, 1.0), (6, 1.0), (9, 0.8)],
    },
    Template {
        quality: ChordQuality::Diminished,
        tones: &[(0, 1.0), (3, 1.0), (6, 1.0), (10, 0.8)],
    },
    Template {
        quality: ChordQuality::Augmented,
        tones: &[(0, 1.0), (4, 1.0), (8, 1.0)],
    },
    Template {
        quality: ChordQuality::Suspended,
        tones: &[(0, 1.0), (5, 1.0), (7, 0.65)],
    },
    Template {
        quality: ChordQuality::Suspended,
        tones: &[(0, 1.0), (2, 1.0), (7, 0.65)],
    },
];

/// How much better a new chord must explain the notes than the current one
/// before the listener hears a change.
const HYSTERESIS: f32 = 0.12;
/// The share of the sounding weight a pitch class needs to count as present.
const PRESENT: f32 = 0.06;

/// The chord `weights`, the sounding weight of each pitch class, best
/// explain, keeping `current` unless another explains them clearly better.
/// `None` when too little sounds to name a chord: a single note leaves the
/// chord as it was.
pub(crate) fn recognise(
    weights: &[f32; 12],
    bass: Option<u8>,
    current: Option<Chord>,
) -> Option<Chord> {
    let total = weights.iter().sum::<f32>();
    if total <= f32::EPSILON {
        return None;
    }
    let present = weights
        .iter()
        .filter(|weight| **weight > total * PRESENT)
        .count();
    if present < 2 {
        return None;
    }
    let bass = bass.map(|note| PitchClass(note % 12));
    let score = |chord: Chord, template: &Template| {
        let mut covered = [false; 12];
        let mut fit = 0.0;
        for &(interval, weight) in template.tones {
            let class = chord.root.above(interval).0 as usize;
            covered[class] = true;
            fit += weights[class] * weight;
        }
        let stray = (0..12)
            .filter(|class| !covered[*class])
            .map(|class| weights[class])
            .sum::<f32>();
        // A chord of more tones explains more, so each tone it names but
        // nobody plays counts against it.
        let missing = template
            .tones
            .iter()
            .filter(|(interval, _)| {
                weights[chord.root.above(*interval).0 as usize] <= total * PRESENT
            })
            .count() as f32;
        let bass_bonus = if bass == Some(chord.root) { 0.25 } else { 0.0 };
        (fit - 0.8 * stray) / total - 0.2 * missing + bass_bonus
    };
    let mut best: Option<(Chord, f32)> = None;
    let mut current_score = f32::NEG_INFINITY;
    for root in 0..12 {
        for template in TEMPLATES {
            let chord = Chord {
                root: PitchClass(root),
                quality: template.quality,
            };
            let score = score(chord, template);
            if Some(chord) == current {
                current_score = current_score.max(score);
            }
            if best.is_none_or(|(_, best)| score > best) {
                best = Some((chord, score));
            }
        }
    }
    let (chord, score) = best?;
    if score < 0.3 {
        return None;
    }
    if current.is_some() && score - current_score < HYSTERESIS {
        return current;
    }
    Some(chord)
}
