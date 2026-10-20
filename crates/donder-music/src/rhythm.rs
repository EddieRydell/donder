//! The beat: a period that best explains the intervals between recent
//! onsets, with a preference for moderate tempos, and a phase that snaps to
//! the onsets landing near a beat. Beats keep coming between notes while the
//! player plays, and stop soon after they stop.

use std::collections::VecDeque;

/// The onsets the tempo is estimated from, in seconds.
pub(crate) const WINDOW: f64 = 8.0;
/// Onsets this close together are one, as the notes of a chord are.
const MERGE: f64 = 0.05;
/// Beat periods considered: 200 down to 60 beats a minute.
const SHORTEST: f64 = 0.3;
const LONGEST: f64 = 1.0;
const STEP: f64 = 0.005;
/// The period a listener leans towards: about 110 beats a minute.
const PREFERRED: f64 = 0.55;
/// Onsets needed before the listener keeps a beat.
const ENOUGH: usize = 6;
/// How often at most the period is estimated again, in seconds.
const ESTIMATE_INTERVAL: f64 = 0.25;
/// Beats stop this long after the last onset, in seconds.
const STOP_AFTER: f64 = 3.0;
/// An onset within this share of a period of a beat sets the phase.
const SNAP: f64 = 0.2;

#[derive(Clone, Debug, Default)]
pub(crate) struct BeatTracker {
    /// Onsets with their weight, merged within `MERGE`.
    onsets: VecDeque<(f64, f32)>,
    period: Option<f64>,
    estimated_at: Option<f64>,
    /// A beat the phase is counted from.
    anchor: Option<f64>,
    /// The last beat reported.
    reported: Option<f64>,
}

impl BeatTracker {
    pub(crate) fn period(&self) -> Option<f64> {
        self.period
    }

    pub(crate) fn hear(&mut self, time: f64, velocity: f32) {
        match self.onsets.back_mut() {
            Some((at, weight)) if time - *at <= MERGE => *weight = weight.max(velocity),
            _ => self.onsets.push_back((time, velocity)),
        }
        while self
            .onsets
            .front()
            .is_some_and(|(at, _)| time - at > WINDOW)
        {
            self.onsets.pop_front();
        }
        if self.onsets.len() >= ENOUGH
            && self
                .estimated_at
                .is_none_or(|at| time - at >= ESTIMATE_INTERVAL)
        {
            self.estimated_at = Some(time);
            self.period = self.estimate().or(self.period);
        }
        let Some(period) = self.period else {
            return;
        };
        match self.anchor {
            None => self.anchor = Some(time),
            Some(anchor) => {
                let beats = ((time - anchor) / period).round();
                let expected = anchor + beats * period;
                if (time - expected).abs() <= SNAP * period {
                    self.anchor = Some(time);
                }
            }
        }
    }

    /// Whether a beat falls due by `time`. Each beat is due once.
    pub(crate) fn due(&mut self, time: f64) -> bool {
        let (Some(period), Some(anchor)) = (self.period, self.anchor) else {
            return false;
        };
        let playing = self
            .onsets
            .back()
            .is_some_and(|(at, _)| time - at <= STOP_AFTER);
        if !playing {
            return false;
        }
        // The latest beat by now, counted from the anchor, unless it was
        // reported already; beats missed between ticks are skipped.
        let latest = ((time - anchor) / period).floor();
        let unreported = self.reported.map_or(f64::NEG_INFINITY, |beat| {
            ((beat + 0.5 * period - anchor) / period).ceil()
        });
        if latest < unreported {
            return false;
        }
        self.reported = Some(anchor + latest * period);
        true
    }

    /// The period that best explains the recent onsets.
    fn estimate(&self) -> Option<f64> {
        let onsets = self.onsets.iter().copied().collect::<Vec<_>>();
        let mut best: Option<(f64, f64)> = None;
        let mut period = SHORTEST;
        while period <= LONGEST {
            let spread = 0.03 + 0.04 * period;
            let mut score = 0.0;
            for (index, &(earlier, weight)) in onsets.iter().enumerate() {
                for &(later, other) in &onsets[index + 1..] {
                    let interval = later - earlier;
                    if interval > 4.0 * period + spread {
                        break;
                    }
                    let pair = f64::from(weight * other);
                    for multiple in 1..=4 {
                        let error = (interval - f64::from(multiple) * period) / spread;
                        score += pair * (-0.5 * error * error).exp() / f64::from(multiple);
                    }
                }
            }
            let lean = (period / PREFERRED).log2() / 0.8;
            score *= (-0.5 * lean * lean).exp();
            if best.is_none_or(|(_, best)| score > best) {
                best = Some((period, score));
            }
            period += STEP;
        }
        best.filter(|(_, score)| *score > 0.0)
            .map(|(period, _)| period)
    }
}
