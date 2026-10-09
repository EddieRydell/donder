//! Beat This!'s minimal postprocessing: local maxima above probability 0.5.

use crate::BeatTimes;
use crate::spectrogram::FRAMES_PER_SECOND;

/// Frames on each side a peak must dominate: ±70 ms.
const PEAK_RADIUS: usize = 3;

pub(crate) fn beat_times(beat: &[f32], downbeat: &[f32]) -> BeatTimes {
    let beats = peak_times(beat);
    let mut downbeats: Vec<f64> = peak_times(downbeat)
        .into_iter()
        .filter_map(|time| nearest(&beats, time))
        .collect();
    downbeats.dedup();
    BeatTimes { beats, downbeats }
}

/// Positive logits that are the maximum of their neighborhood, with runs of
/// adjacent peak frames merged into their mean.
fn peak_times(logits: &[f32]) -> Vec<f64> {
    let mut times = Vec::new();
    let mut run: Option<(f64, f64)> = None;
    for (frame, &value) in logits.iter().enumerate() {
        let neighborhood =
            &logits[frame.saturating_sub(PEAK_RADIUS)..(frame + PEAK_RADIUS + 1).min(logits.len())];
        if value <= 0.0 || neighborhood.iter().any(|&other| other > value) {
            continue;
        }
        let frame = frame as f64;
        run = Some(match run {
            Some((mean, count)) if frame - mean <= 1.0 => {
                let count = count + 1.0;
                (mean + (frame - mean) / count, count)
            }
            Some((mean, _)) => {
                times.push(mean / FRAMES_PER_SECOND);
                (frame, 1.0)
            }
            None => (frame, 1.0),
        });
    }
    if let Some((mean, _)) = run {
        times.push(mean / FRAMES_PER_SECOND);
    }
    times
}

/// Downbeats are moved onto the closest beat.
fn nearest(beats: &[f64], time: f64) -> Option<f64> {
    beats
        .iter()
        .copied()
        .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()))
}
