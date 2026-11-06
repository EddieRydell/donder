use super::render::{RasterRenderFailure, RenderedRaster, render_clip_raster};
use crate::state_tasks::LatestScheduler;
use donder_editor::dto::ClipRasterSettings;
use donder_elaboration::{PreparationCache, Retain, prepare_cached};
use donder_model::{Clip, SequenceId};
use donder_project_io::ProjectSession;
use donder_runtime::PreparedShow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, mpsc};

/// Clips to render, in order, from one project snapshot. A newer job
/// replaces it at the next clip boundary.
pub(super) struct RasterJob {
    generation: u64,
    project: Arc<ProjectSession>,
    sequence_id: SequenceId,
    settings: ClipRasterSettings,
    clips: Vec<(u32, Arc<Clip>)>,
}

/// A finished clip. `clip` is the allocation it was rendered from, so the
/// service can tell whether the clip changed while it rendered.
pub(super) struct RasterOutcome {
    pub(super) clip_id: u32,
    pub(super) clip: Arc<Clip>,
    pub(super) result: Result<RenderedRaster, String>,
}

/// Renders on the desktop's latest-request scheduler: a submitted job
/// replaces a pending one, and the generation stops a running one at its next
/// clip boundary.
pub(super) struct RasterWorker {
    scheduler: LatestScheduler<RasterJob>,
    results: mpsc::Receiver<RasterOutcome>,
    latest: Arc<AtomicU64>,
}

impl RasterWorker {
    pub(super) fn new() -> Self {
        let (result_sender, results) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        // Unchanged clips keep their lowering across project snapshots.
        let mut preparation = PreparationCache::default();
        let mut prepared: Option<(Arc<ProjectSession>, SequenceId, Option<PreparedShow>)> = None;
        let scheduler = LatestScheduler::new({
            let latest = Arc::clone(&latest);
            move |job| {
                render(
                    job,
                    &result_sender,
                    &latest,
                    &mut preparation,
                    &mut prepared,
                )
            }
        });
        Self {
            scheduler,
            results,
            latest,
        }
    }

    /// Render `clips` in order, replacing any earlier job.
    pub(super) fn submit(
        &self,
        project: Arc<ProjectSession>,
        sequence_id: SequenceId,
        settings: ClipRasterSettings,
        clips: Vec<(u32, Arc<Clip>)>,
    ) {
        let generation = self.latest.fetch_add(1, Ordering::Relaxed) + 1;
        self.scheduler.schedule(RasterJob {
            generation,
            project,
            sequence_id,
            settings,
            clips,
        });
    }

    pub(super) fn cancel(&self) {
        self.latest.fetch_add(1, Ordering::Relaxed);
        self.scheduler.invalidate_pending();
    }

    pub(super) fn drain(&self) -> impl Iterator<Item = RasterOutcome> + '_ {
        self.results.try_iter()
    }
}

fn render(
    mut job: RasterJob,
    results: &mpsc::Sender<RasterOutcome>,
    latest: &AtomicU64,
    preparation: &mut PreparationCache,
    prepared: &mut Option<(Arc<ProjectSession>, SequenceId, Option<PreparedShow>)>,
) {
    let current = || latest.load(Ordering::Relaxed) == job.generation;
    if !current() {
        return;
    }
    if !prepared.as_ref().is_some_and(|(project, sequence, _)| {
        Arc::ptr_eq(project, &job.project) && sequence == &job.sequence_id
    }) {
        let sequence = prepare_cached(
            &job.project.project,
            &job.sequence_id,
            Retain::All,
            preparation,
        );
        *prepared = Some((Arc::clone(&job.project), job.sequence_id, sequence));
    }
    let Some((_, _, Some(sequence))) = prepared.as_ref() else {
        return;
    };
    for (clip_id, clip) in job.clips.drain(..) {
        if !current() {
            break;
        }
        let result = match render_clip_raster(sequence, clip_id, &job.settings, &current) {
            Ok(raster) => Ok(raster),
            Err(RasterRenderFailure::Cancelled) => break,
            Err(RasterRenderFailure::Error(message)) => Err(message),
        };
        // A closed receiver means the service is gone; nothing reads results.
        if results
            .send(RasterOutcome {
                clip_id,
                clip,
                result,
            })
            .is_err()
        {
            return;
        }
    }
}
