//! Framewise beat and downbeat logits from the embedded Beat This! model.
//!
//! Songs are longer than the 30-second excerpts the model was trained on, so
//! the spectrogram is split into overlapping chunks whose edge frames are
//! discarded, exactly as the reference implementation does. Chunks are
//! independent and run on parallel threads.

use std::sync::Arc;

use tract_onnx::prelude::*;

use crate::AnalysisError;
use crate::spectrogram::MEL_BANDS;

/// Beat This! `small0`, the reduced model trained on all data except GTZAN, exported
/// from the official checkpoint with `torch.onnx.export` (opset 17, dynamic time axis).
const MODEL: &[u8] = include_bytes!("../models/beat_this_small0.onnx");
/// Frames per model input: the training excerpt length.
const CHUNK_FRAMES: usize = 1500;
/// Frames at each chunk edge that the model was not trained to predict.
const BORDER_FRAMES: usize = 6;
/// Logit for frames no chunk predicts, so they never become peaks.
const NO_PREDICTION: f32 = -1000.0;

type Plan = Arc<TypedRunnableModel>;

struct ChunkPrediction {
    start: isize,
    beat: Vec<f32>,
    downbeat: Vec<f32>,
}

/// Beat and downbeat logits for every spectrogram frame.
pub(crate) fn predict(spectrogram: &[f32]) -> Result<(Vec<f32>, Vec<f32>), AnalysisError> {
    let frames = spectrogram.len() / MEL_BANDS;
    let starts = chunk_starts(frames);
    let Some(&first) = starts.first() else {
        return Ok((Vec::new(), Vec::new()));
    };
    // Every chunk of a song has the same length; a song shorter than one chunk is a single chunk.
    let chunk_frames = chunk_range(first, frames).len;
    let plan = load(chunk_frames)?;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(starts.len());
    let predictions: Vec<Result<Vec<ChunkPrediction>, AnalysisError>> =
        std::thread::scope(|scope| {
            let handles: Vec<_> = (0..workers)
                .map(|worker| {
                    let (plan, starts) = (&plan, &starts);
                    scope.spawn(move || {
                        starts
                            .iter()
                            .skip(worker)
                            .step_by(workers)
                            .map(|&start| {
                                let (beat, downbeat) = run(plan, spectrogram, frames, start)?;
                                Ok(ChunkPrediction {
                                    start,
                                    beat,
                                    downbeat,
                                })
                            })
                            .collect()
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| {
                    handle.join().unwrap_or_else(|_| {
                        Err(AnalysisError::Model("an inference thread panicked".into()))
                    })
                })
                .collect()
        });
    let mut chunks = Vec::with_capacity(starts.len());
    for worker in predictions {
        chunks.extend(worker?);
    }
    // Earlier chunks win where chunks overlap: write later ones first.
    chunks.sort_by_key(|chunk| std::cmp::Reverse(chunk.start));
    let mut beat = vec![NO_PREDICTION; frames];
    let mut downbeat = vec![NO_PREDICTION; frames];
    for chunk in chunks {
        for index in BORDER_FRAMES..chunk.beat.len() - BORDER_FRAMES {
            let frame = chunk.start + index as isize;
            if (0..frames as isize).contains(&frame) {
                beat[frame as usize] = chunk.beat[index];
                downbeat[frame as usize] = chunk.downbeat[index];
            }
        }
    }
    Ok((beat, downbeat))
}

fn load(chunk_frames: usize) -> Result<Plan, AnalysisError> {
    let error = |error: TractError| AnalysisError::Model(error.to_string());
    tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(MODEL))
        .map_err(error)?
        .with_input_fact(0, f32::fact([1, chunk_frames, MEL_BANDS]).into())
        .map_err(error)?
        .into_optimized()
        .map_err(error)?
        .into_runnable()
        .map_err(error)
}

/// Chunk start frames; the first starts `BORDER_FRAMES` before the song, and
/// the last is moved back to end exactly at the song's end.
fn chunk_starts(frames: usize) -> Vec<isize> {
    let step = CHUNK_FRAMES - 2 * BORDER_FRAMES;
    let border = BORDER_FRAMES as isize;
    let mut starts: Vec<isize> = (0..)
        .map(|index| (index * step) as isize - border)
        .take_while(|&start| start < frames as isize - border)
        .collect();
    if frames > step
        && let Some(last) = starts.last_mut()
    {
        *last = frames as isize - (CHUNK_FRAMES - BORDER_FRAMES) as isize;
    }
    starts
}

/// The spectrogram frames a chunk covers and the zero frames padding it.
struct ChunkRange {
    left_padding: usize,
    from: usize,
    to: usize,
    len: usize,
}

fn chunk_range(start: isize, frames: usize) -> ChunkRange {
    let end = start + CHUNK_FRAMES as isize;
    let left_padding = (-start).max(0) as usize;
    let from = start.max(0) as usize;
    let to = (end.max(0) as usize).min(frames);
    let right_padding = BORDER_FRAMES.min((end - frames as isize).max(0) as usize);
    ChunkRange {
        left_padding,
        from,
        to,
        len: left_padding + (to - from) + right_padding,
    }
}

fn run(
    plan: &Plan,
    spectrogram: &[f32],
    frames: usize,
    start: isize,
) -> Result<(Vec<f32>, Vec<f32>), AnalysisError> {
    let error = |error: TractError| AnalysisError::Model(error.to_string());
    let range = chunk_range(start, frames);
    let mut input = vec![0.0; range.len * MEL_BANDS];
    let covered =
        range.left_padding * MEL_BANDS..(range.left_padding + range.to - range.from) * MEL_BANDS;
    input[covered].copy_from_slice(&spectrogram[range.from * MEL_BANDS..range.to * MEL_BANDS]);
    let input = tract_ndarray::Array3::from_shape_vec((1, range.len, MEL_BANDS), input)
        .map_err(|shape| AnalysisError::Model(shape.to_string()))?
        .into_tensor();
    let outputs = plan.run(tvec!(input.into())).map_err(error)?;
    let logits = |index: usize| -> Result<Vec<f32>, AnalysisError> {
        let output = outputs
            .get(index)
            .ok_or_else(|| AnalysisError::Model("missing model output".into()))?;
        Ok(output
            .to_plain_array_view::<f32>()
            .map_err(error)?
            .iter()
            .copied()
            .collect())
    };
    Ok((logits(0)?, logits(1)?))
}
