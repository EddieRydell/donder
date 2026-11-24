//! The analyses effects read: a level, spectrum bands or the waveform,
//! sampled at a fixed rate as bytes playback interpolates.
use std::sync::Arc;

use realfft::RealFftPlanner;

use crate::{AnalysisError, DecodedAudio};

/// Samples a second of a level or a spectrum.
pub const LEVEL_RATE: u32 = 50;
/// Samples a second of a waveform.
pub const WAVE_RATE: u32 = 2000;
/// The bands of a spectrum span these frequencies, in hertz.
pub const SPECTRUM_LOW: f64 = 30.0;
pub const SPECTRUM_HIGH: f64 = 16000.0;
/// The most bands a spectrum has.
pub const MAX_BANDS: u32 = 128;
/// Decibels below the loudest band that a band's 0 stands for.
const SPECTRUM_FLOOR: f64 = -60.0;
/// Shortest window a level averages over, in seconds.
const LEVEL_WINDOW: f64 = 0.02;
/// Finest frequency resolution a spectrum resolves, in hertz.
const SPECTRUM_RESOLUTION: f64 = 10.0;

/// What an analysis measures. Bands are rounded and held to `1..=128`, and
/// negative times count as zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AudioFeature {
    /// Loudness between `low` and `high` hertz, rising over `attack` seconds
    /// and falling over `decay`.
    Level {
        low: f64,
        high: f64,
        attack: f64,
        decay: f64,
    },
    Spectrum {
        bands: f64,
        attack: f64,
        decay: f64,
    },
    Wave,
}

/// An analysis's bytes: `channels` per sample at `rate` a second, each 0..1,
/// or -1..1 when `signed`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioTrack {
    pub samples: Arc<[u8]>,
    pub channels: u16,
    pub signed: bool,
    pub rate: u32,
}

/// Analyze `audio` as `feature` asks.
pub fn analyze_audio(
    audio: &DecodedAudio,
    feature: &AudioFeature,
) -> Result<AudioTrack, AnalysisError> {
    let sample_rate = f64::from(audio.sample_rate);
    let signal = audio.mono();
    let (rate, channels, signed, values) = match *feature {
        AudioFeature::Level {
            low,
            high,
            attack,
            decay,
        } => {
            let hop = sample_rate / f64::from(LEVEL_RATE);
            let mut signal = signal;
            if low > 0.0 {
                signal = Biquad::high_pass(low, sample_rate).run(&signal);
            }
            if high < sample_rate / 2.0 {
                signal = Biquad::low_pass(high, sample_rate).run(&signal);
            }
            let window = hop.max(LEVEL_WINDOW * sample_rate);
            let mut values: Vec<f64> = (0..count(&signal, hop))
                .map(|index| rms(&signal, index as f64 * hop, window))
                .collect();
            follow(&mut values, 1, attack, decay, LEVEL_RATE);
            normalize(&mut values);
            (LEVEL_RATE, 1, false, values)
        }
        AudioFeature::Spectrum {
            bands,
            attack,
            decay,
        } => {
            let bands = bands.round().clamp(1.0, f64::from(MAX_BANDS)) as usize;
            let hop = sample_rate / f64::from(LEVEL_RATE);
            let mut values = spectrum(&signal, sample_rate, hop, bands)?;
            follow(&mut values, bands, attack, decay, LEVEL_RATE);
            let loudest = values
                .iter()
                .fold(0.0_f64, |loudest, value| loudest.max(*value));
            for value in &mut values {
                let decibels = 20.0 * (*value / loudest).max(1e-12).log10();
                *value = (decibels - SPECTRUM_FLOOR) / -SPECTRUM_FLOOR;
            }
            (LEVEL_RATE, bands, false, values)
        }
        AudioFeature::Wave => {
            let hop = sample_rate / f64::from(WAVE_RATE);
            let mut values: Vec<f64> = (0..count(&signal, hop))
                .map(|index| {
                    let start = ((index as f64 * hop) as usize).min(signal.len());
                    let end = (((index + 1) as f64 * hop) as usize).clamp(start, signal.len());
                    let samples = &signal[start..end];
                    samples.iter().sum::<f64>() / samples.len().max(1) as f64
                })
                .collect();
            normalize(&mut values);
            (WAVE_RATE, 1, true, values)
        }
    };
    let samples = values
        .iter()
        .map(|&value| {
            let unit = if signed {
                (value.clamp(-1.0, 1.0) + 1.0) / 2.0
            } else {
                value.clamp(0.0, 1.0)
            };
            (unit * 255.0).round() as u8
        })
        .collect();
    Ok(AudioTrack {
        samples,
        channels: channels as u16,
        signed,
        rate,
    })
}

/// Samples of `signal` taken every `hop` of its samples.
fn count(signal: &[f64], hop: f64) -> usize {
    (signal.len() as f64 / hop).ceil() as usize
}

/// Scale so the largest magnitude is 1.
fn normalize(values: &mut [f64]) {
    let peak = values
        .iter()
        .fold(0.0_f64, |peak, value| peak.max(value.abs()));
    if peak > 0.0 {
        for value in values {
            *value /= peak;
        }
    }
}

/// Root mean square over `window` samples centered at `center`.
fn rms(signal: &[f64], center: f64, window: f64) -> f64 {
    let start = (center - window / 2.0).max(0.0) as usize;
    let end = ((center + window / 2.0) as usize).min(signal.len());
    if end <= start {
        return 0.0;
    }
    let power = signal[start..end]
        .iter()
        .map(|sample| sample * sample)
        .sum::<f64>()
        / (end - start) as f64;
    power.sqrt()
}

/// Attack and decay ballistics over interleaved `channels`, sampled at
/// `rate`.
fn follow(values: &mut [f64], channels: usize, attack: f64, decay: f64, rate: u32) {
    let coefficient = |seconds: f64| {
        if seconds > 0.0 {
            (-1.0 / (seconds * f64::from(rate))).exp()
        } else {
            0.0
        }
    };
    let (attack, decay) = (coefficient(attack), coefficient(decay));
    for channel in 0..channels {
        let mut held = 0.0;
        for value in values.iter_mut().skip(channel).step_by(channels) {
            let coefficient = if *value > held { attack } else { decay };
            held = *value + (held - *value) * coefficient;
            *value = held;
        }
    }
}

/// Each sample's band amplitudes, interleaved: the loudest frequency of each
/// band, from a Hann-windowed transform centered on the sample.
fn spectrum(
    signal: &[f64],
    sample_rate: f64,
    hop: f64,
    bands: usize,
) -> Result<Vec<f64>, AnalysisError> {
    let size = ((sample_rate / SPECTRUM_RESOLUTION) as usize)
        .max((2.0 * hop) as usize)
        .next_power_of_two();
    let fft = RealFftPlanner::<f64>::new().plan_fft_forward(size);
    let window: Vec<f64> = (0..size)
        .map(|index| 0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / size as f64).cos())
        .collect();
    let bin_width = sample_rate / size as f64;
    // Each band's bins; a band narrower than a bin reads the bin at its middle.
    let ranges: Vec<(usize, usize)> = (0..bands)
        .map(|band| {
            let edge = |at: f64| {
                SPECTRUM_LOW * (SPECTRUM_HIGH / SPECTRUM_LOW).powf(at / bands as f64) / bin_width
            };
            let (start, end) = (edge(band as f64), edge(band as f64 + 1.0));
            let (first, last) = (start.ceil() as usize, end.ceil() as usize);
            if last > first {
                (first, last)
            } else {
                let middle = ((start + end) / 2.0).round() as usize;
                (middle, middle + 1)
            }
        })
        .collect();
    let mut input = fft.make_input_vec();
    let mut output = fft.make_output_vec();
    let samples = count(signal, hop);
    let mut values = Vec::with_capacity(samples * bands);
    for index in 0..samples {
        let start = index as f64 * hop - size as f64 / 2.0;
        for (offset, slot) in input.iter_mut().enumerate() {
            let at = start + offset as f64;
            let sample = if at >= 0.0 {
                signal.get(at as usize).copied().unwrap_or(0.0)
            } else {
                0.0
            };
            *slot = sample * window[offset];
        }
        fft.process(&mut input, &mut output)
            .map_err(|error| AnalysisError::Spectrogram(error.to_string()))?;
        for &(first, last) in &ranges {
            let loudest = output
                .get(first.min(output.len())..last.min(output.len()))
                .unwrap_or(&[])
                .iter()
                .fold(0.0_f64, |loudest, bin| loudest.max(bin.norm()));
            values.push(loudest);
        }
    }
    Ok(values)
}

/// A second-order section, from the RBJ audio EQ cookbook, Butterworth Q.
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
}

impl Biquad {
    fn low_pass(cutoff: f64, rate: f64) -> Self {
        Self::new(cutoff, rate, |cos| {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        })
    }

    fn high_pass(cutoff: f64, rate: f64) -> Self {
        Self::new(cutoff, rate, |cos| {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        })
    }

    fn new(cutoff: f64, rate: f64, numerator: impl Fn(f64) -> [f64; 3]) -> Self {
        let omega = std::f64::consts::TAU * cutoff / rate;
        let alpha = omega.sin() / std::f64::consts::SQRT_2;
        let cos = omega.cos();
        let a0 = 1.0 + alpha;
        let b = numerator(cos).map(|coefficient| coefficient / a0);
        Self {
            b,
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
        }
    }

    fn run(&self, signal: &[f64]) -> Vec<f64> {
        let (mut x1, mut x2, mut y1, mut y2) = (0.0, 0.0, 0.0, 0.0);
        signal
            .iter()
            .map(|&x| {
                let y = self.b[0] * x + self.b[1] * x1 + self.b[2] * x2
                    - self.a[0] * y1
                    - self.a[1] * y2;
                (x2, x1, y2, y1) = (x1, x, y1, y);
                y
            })
            .collect()
    }
}
