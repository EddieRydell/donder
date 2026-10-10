use super::render::{RasterRenderFailure, RenderedRaster, render_effect_raster};
use donder_elaboration::{PreparationCache, PrepareOutputs, prepare_cached};
use donder_model::{EffectInst, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedSequence;
use donder_sequence_api::EffectRasterSettings;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;

/// Clips to render, in order, from one project snapshot. A newer job
/// replaces it at the next clip boundary.
pub(super) struct RasterJob {
    generation: u64,
    project: Arc<ProjectSession>,
    sequence_id: SequenceId,
    settings: EffectRasterSettings,
    clips: Vec<(u32, Arc<EffectInst>)>,
}

/// A finished clip. `clip` is the allocation it was rendered from, so the
/// service can tell whether the clip changed while it rendered.
pub(super) struct RasterOutcome {
    pub(super) effect_id: u32,
    pub(super) clip: Arc<EffectInst>,
    pub(super) result: Result<RenderedRaster, String>,
}

pub(super) struct RasterWorker {
    jobs: mpsc::Sender<RasterJob>,
    results: mpsc::Receiver<RasterOutcome>,
    latest: Arc<AtomicU64>,
}

impl RasterWorker {
    pub(super) fn new() -> Self {
        let (jobs, job_receiver) = mpsc::channel();
        let (result_sender, results) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        thread::spawn({
            let latest = Arc::clone(&latest);
            move || run(job_receiver, result_sender, latest)
        });
        Self {
            jobs,
            results,
            latest,
        }
    }

    /// Render `clips` in order, replacing any earlier job.
    pub(super) fn submit(
        &self,
        project: Arc<ProjectSession>,
        sequence_id: SequenceId,
        settings: EffectRasterSettings,
        clips: Vec<(u32, Arc<EffectInst>)>,
    ) {
        let generation = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        // A closed channel means the worker thread is gone; nothing renders.
        let _ = self.jobs.send(RasterJob {
            generation,
            project,
            sequence_id,
            settings,
            clips,
        });
    }

    pub(super) fn cancel(&self) {
        self.latest.fetch_add(1, Ordering::Relaxed);
    }

    pub(super) fn drain(&self) -> impl Iterator<Item = RasterOutcome> + '_ {
        self.results.try_iter()
    }
}

fn run(
    jobs: mpsc::Receiver<RasterJob>,
    results: mpsc::Sender<RasterOutcome>,
    latest: Arc<AtomicU64>,
) {
    // Unchanged clips keep their lowering across project snapshots.
    let mut preparation = PreparationCache::default();
    let mut prepared: Option<(Arc<ProjectSession>, SequenceId, Option<PreparedSequence>)> = None;
    while let Ok(mut job) = jobs.recv() {
        while let Ok(newer) = jobs.try_recv() {
            job = newer;
        }
        let current = || latest.load(Ordering::Relaxed) == job.generation;
        if !current() {
            continue;
        }
        if !prepared.as_ref().is_some_and(|(project, sequence, _)| {
            Arc::ptr_eq(project, &job.project) && sequence == &job.sequence_id
        }) {
            let sequence = prepare_cached(
                &job.project.project,
                &job.sequence_id,
                PrepareOutputs::All,
                &mut preparation,
            );
            prepared = Some((Arc::clone(&job.project), job.sequence_id.clone(), sequence));
        }
        let Some((_, _, Some(sequence))) = &prepared else {
            continue;
        };
        for (effect_id, clip) in job.clips.drain(..) {
            if !current() {
                break;
            }
            let result = match render_effect_raster(sequence, effect_id, &job.settings, &current) {
                Ok(raster) => Ok(raster),
                Err(RasterRenderFailure::Cancelled) => break,
                Err(RasterRenderFailure::Error(message)) => Err(message),
            };
            if results
                .send(RasterOutcome {
                    effect_id,
                    clip,
                    result,
                })
                .is_err()
            {
                return;
            }
        }
    }
}
