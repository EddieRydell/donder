//! The key: a pitch-class profile of what was played, fading as it ages,
//! correlated with the Krumhansl-Kessler profiles of the 24 major and minor
//! keys. A new key must lead clearly for a moment before the key changes.

use crate::PitchClass;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Mode {
    Major,
    Minor,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct Key {
    pub tonic: PitchClass,
    pub mode: Mode,
}

impl Key {
    /// Whether `class` belongs to the key's scale; a minor key's raised
    /// seventh does.
    pub fn contains(self, class: PitchClass) -> bool {
        let degree = (class.0 + 12 - self.tonic.0) % 12;
        match self.mode {
            Mode::Major => [0, 2, 4, 5, 7, 9, 11].contains(&degree),
            Mode::Minor => [0, 2, 3, 5, 7, 8, 10, 11].contains(&degree),
        }
    }
}

const MAJOR_PROFILE: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
const MINOR_PROFILE: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// How long the profile takes to fade to about a third, in seconds.
const MEMORY: f64 = 10.0;
/// The profile weight, about sixteen notes, before the listener names a key.
const FIRST_KEY: f64 = 18.0;
/// How long a key must have held before a major chord falling home to its
/// tonic counts as a resolution, in seconds.
const SETTLED: f64 = 4.0;
/// How long a key must have held for a new one to be a change of key, in
/// seconds; earlier, the listener is still finding the first key.
const ESTABLISHED: f64 = 8.0;
/// How much better a new key must correlate, and for how long, to change.
const CHANGE_MARGIN: f64 = 0.1;
const CHANGE_HOLD: f64 = 2.5;

#[derive(Clone, Debug, Default)]
pub(crate) struct KeyFinder {
    profile: [f64; 12],
    at: f64,
    key: Option<Key>,
    /// When the key was last named.
    since: f64,
    /// A key that leads, and since when.
    candidate: Option<(Key, f64)>,
    change: Option<KeyChange>,
}

/// The key the listener names, from the key it named before.
#[derive(Clone, Copy, Debug)]
pub(crate) struct KeyChange {
    pub(crate) from: Option<Key>,
    pub(crate) to: Key,
    /// The key before had held long enough for this to be a change of key;
    /// otherwise the listener was still finding the first key.
    pub(crate) established: bool,
}

impl KeyFinder {
    pub(crate) fn key(&self) -> Option<Key> {
        self.key
    }

    /// The key, once it has held long enough to hear a cadence in it.
    pub(crate) fn settled(&self, time: f64) -> Option<Key> {
        self.key.filter(|_| time - self.since >= SETTLED)
    }

    pub(crate) fn hear(&mut self, note: u8, velocity: f32, time: f64) {
        self.fade(time);
        self.profile[usize::from(note % 12)] += 0.5 + f64::from(velocity);
        self.settle(time);
    }

    /// Change the key when another has led long enough.
    pub(crate) fn settle(&mut self, time: f64) {
        self.fade(time);
        let total = self.profile.iter().sum::<f64>();
        let Some(current) = self.key else {
            if total >= FIRST_KEY
                && let Some((best, _)) = self.best()
            {
                self.key = Some(best);
                self.since = time;
                self.change = Some(KeyChange {
                    from: None,
                    to: best,
                    established: false,
                });
            }
            return;
        };
        let Some((best, best_score)) = self.best() else {
            return;
        };
        if best == current || best_score - self.correlation(current) < CHANGE_MARGIN {
            self.candidate = None;
            return;
        }
        match self.candidate {
            Some((candidate, since)) if candidate == best => {
                if time - since >= CHANGE_HOLD {
                    self.change = Some(KeyChange {
                        from: Some(current),
                        to: best,
                        established: time - self.since >= ESTABLISHED,
                    });
                    self.key = Some(best);
                    self.since = time;
                    self.candidate = None;
                }
            }
            _ => self.candidate = Some((best, time)),
        }
    }

    /// The key change since the last call.
    pub(crate) fn take_change(&mut self) -> Option<KeyChange> {
        self.change.take()
    }

    fn fade(&mut self, time: f64) {
        let elapsed = (time - self.at).max(0.0);
        if elapsed > 0.0 {
            let fade = (-elapsed / MEMORY).exp();
            for weight in &mut self.profile {
                *weight *= fade;
            }
            self.at = time;
        }
    }

    fn best(&self) -> Option<(Key, f64)> {
        let mut best: Option<(Key, f64)> = None;
        for tonic in 0..12 {
            for mode in [Mode::Major, Mode::Minor] {
                let key = Key {
                    tonic: PitchClass(tonic),
                    mode,
                };
                let score = self.correlation(key);
                if best.is_none_or(|(_, best)| score > best) {
                    best = Some((key, score));
                }
            }
        }
        best.filter(|(_, score)| score.is_finite())
    }

    /// The Pearson correlation of the profile with `key`'s.
    fn correlation(&self, key: Key) -> f64 {
        let template = match key.mode {
            Mode::Major => &MAJOR_PROFILE,
            Mode::Minor => &MINOR_PROFILE,
        };
        let rotated = |class: usize| template[(class + 12 - usize::from(key.tonic.0)) % 12];
        let mean_profile = self.profile.iter().sum::<f64>() / 12.0;
        let mean_template = template.iter().sum::<f64>() / 12.0;
        let (mut product, mut profile_square, mut template_square) = (0.0, 0.0, 0.0);
        for class in 0..12 {
            let profile = self.profile[class] - mean_profile;
            let template = rotated(class) - mean_template;
            product += profile * template;
            profile_square += profile * profile;
            template_square += template * template;
        }
        product / (profile_square * template_square).sqrt()
    }
}
