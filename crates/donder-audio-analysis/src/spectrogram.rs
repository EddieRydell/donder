//! The model's input: torchaudio's `MelSpectrogram` with Beat This!'s settings,
//! reproduced exactly so the model sees what it was trained on.

use realfft::RealFftPlanner;
use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{
    Async, FixedAsync, Resampler, SincInterpolationParameters, SincInterpolationType,
    WindowFunction,
};

use crate::AnalysisError;

pub(crate) const SAMPLE_RATE: u32 = 22_050;
pub(crate) const MEL_BANDS: usize = 128;
/// Spectrogram frames per second: the sample rate over the hop.
pub(crate) const FRAMES_PER_SECOND: f64 = 50.0;
const FFT_SIZE: usize = 1024;
const HOP: usize = 441;
const MIN_HZ: f64 = 30.0;
const MAX_HZ: f64 = 11_000.0;
const LOG_MULTIPLIER: f32 = 1000.0;
const SINC_LENGTH: usize = 128;
const SINC_OVERSAMPLING: usize = 256;
const RESAMPLE_CHUNK: usize = 1024;

pub(crate) fn resample(mono: &[f64], rate: u32) -> Result<Vec<f32>, AnalysisError> {
    if rate == SAMPLE_RATE {
        return Ok(mono.iter().map(|&sample| sample as f32).collect());
    }
    let error = |error: &dyn std::fmt::Display| AnalysisError::Resample(error.to_string());
    let ratio = f64::from(SAMPLE_RATE) / f64::from(rate);
    let parameters = SincInterpolationParameters::new(SINC_LENGTH, WindowFunction::Blackman2)
        .oversampling_factor(SINC_OVERSAMPLING)
        .interpolation(SincInterpolationType::Quadratic);
    let mut resampler = Async::<f64>::new_sinc(
        ratio,
        1.0,
        &parameters,
        RESAMPLE_CHUNK,
        1,
        FixedAsync::Input,
    )
    .map_err(|e| error(&e))?;
    let input = InterleavedSlice::new(mono, 1, mono.len()).map_err(|e| error(&e))?;
    let capacity = resampler.process_all_needed_output_len(mono.len());
    let mut resampled = vec![0.0; capacity];
    let mut output =
        InterleavedSlice::new_mut(&mut resampled, 1, capacity).map_err(|e| error(&e))?;
    let (_, produced) = resampler
        .process_all_into_buffer(&input, &mut output, mono.len(), None)
        .map_err(|e| error(&e))?;
    resampled.truncate(produced);
    Ok(resampled.into_iter().map(|sample| sample as f32).collect())
}

/// Slaney mel scale: linear below 1 kHz, logarithmic above.
fn hz_to_mel(hz: f64) -> f64 {
    if hz >= SLANEY_MIN_LOG_HZ {
        SLANEY_MIN_LOG_MEL + (hz / SLANEY_MIN_LOG_HZ).ln() / slaney_log_step()
    } else {
        hz / SLANEY_LINEAR_HZ_PER_MEL
    }
}

fn mel_to_hz(mel: f64) -> f64 {
    if mel >= SLANEY_MIN_LOG_MEL {
        SLANEY_MIN_LOG_HZ * (slaney_log_step() * (mel - SLANEY_MIN_LOG_MEL)).exp()
    } else {
        SLANEY_LINEAR_HZ_PER_MEL * mel
    }
}

const SLANEY_LINEAR_HZ_PER_MEL: f64 = 200.0 / 3.0;
const SLANEY_MIN_LOG_HZ: f64 = 1000.0;
const SLANEY_MIN_LOG_MEL: f64 = 15.0;

fn slaney_log_step() -> f64 {
    6.4f64.ln() / 27.0
}

/// Unnormalized triangular filters over the one-sided FFT bins, `[bin][band]`.
fn mel_filterbank() -> Vec<f32> {
    let bins = FFT_SIZE / 2 + 1;
    let (low, high) = (hz_to_mel(MIN_HZ), hz_to_mel(MAX_HZ));
    let edges: Vec<f64> = (0..MEL_BANDS + 2)
        .map(|index| mel_to_hz(low + (high - low) * index as f64 / (MEL_BANDS + 1) as f64))
        .collect();
    let mut bank = vec![0.0; bins * MEL_BANDS];
    for bin in 0..bins {
        let hz = f64::from(SAMPLE_RATE) / 2.0 * bin as f64 / (bins - 1) as f64;
        for band in 0..MEL_BANDS {
            let rising = (hz - edges[band]) / (edges[band + 1] - edges[band]);
            let falling = (edges[band + 2] - hz) / (edges[band + 2] - edges[band + 1]);
            bank[bin * MEL_BANDS + band] = rising.min(falling).max(0.0) as f32;
        }
    }
    bank
}

/// Centered frames with reflect padding and a periodic Hann window,
/// magnitudes scaled by `1/sqrt(FFT_SIZE)`, mel-filtered, then
/// `ln(1 + 1000x)`. Row-major `[frame][band]`.
pub(crate) fn log_mel(signal: &[f32]) -> Result<Vec<f32>, AnalysisError> {
    if signal.is_empty() {
        return Ok(Vec::new());
    }
    let bins = FFT_SIZE / 2 + 1;
    let bank = mel_filterbank();
    let window: Vec<f32> = (0..FFT_SIZE)
        .map(|index| {
            let phase = 2.0 * std::f64::consts::PI * index as f64 / FFT_SIZE as f64;
            (0.5 - 0.5 * phase.cos()) as f32
        })
        .collect();
    let pad = FFT_SIZE / 2;
    let last = signal.len() as isize - 1;
    let padded: Vec<f32> = (0..signal.len() + 2 * pad)
        .map(|index| {
            let offset = index as isize - pad as isize;
            let reflected = if offset < 0 {
                -offset
            } else if offset > last {
                2 * last - offset
            } else {
                offset
            };
            signal[reflected.clamp(0, last) as usize]
        })
        .collect();
    let frames = 1 + signal.len() / HOP;
    let fft = RealFftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let mut input = fft.make_input_vec();
    let mut spectrum = fft.make_output_vec();
    let scale = 1.0 / (FFT_SIZE as f32).sqrt();
    let mut output = vec![0.0; frames * MEL_BANDS];
    for (frame, row) in output.as_chunks_mut::<MEL_BANDS>().0.iter_mut().enumerate() {
        let start = frame * HOP;
        for ((sample, &value), &weight) in input
            .iter_mut()
            .zip(&padded[start..start + FFT_SIZE])
            .zip(&window)
        {
            *sample = value * weight;
        }
        fft.process(&mut input, &mut spectrum)
            .map_err(|error| AnalysisError::Spectrogram(error.to_string()))?;
        for (bin, value) in spectrum.iter().enumerate().take(bins) {
            let magnitude = value.norm() * scale;
            if magnitude == 0.0 {
                continue;
            }
            let filters = &bank[bin * MEL_BANDS..(bin + 1) * MEL_BANDS];
            for (band, &filter) in row.iter_mut().zip(filters) {
                *band += magnitude * filter;
            }
        }
        for band in row.iter_mut() {
            *band = (LOG_MULTIPLIER * *band).ln_1p();
        }
    }
    Ok(output)
}
