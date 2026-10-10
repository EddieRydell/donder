use super::worker::RasterWorker;
use donder_model::{AutomationTarget, EffectInst, Sequence, SequenceId};
use donder_project_io::ProjectSession;
use donder_sequence_api::{
    EffectRasterSettings, GuiDocumentRequest, SequenceClipRaster, SequenceClipRasterError,
    SequenceClipRasterRequest, SequenceClipRasterResponse, SequenceClipRasterResultBatch,
};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// Past this many bytes of rasters, the ones off screen longest are dropped
/// and render again when they come back into view.
const RASTER_BYTE_BUDGET: usize = 256 * 1024 * 1024;

pub(crate) struct SequenceClipRasterService {
    worker: RasterWorker,
    open: Option<OpenSequence>,
    /// Raster revisions only grow, across sequences too, so a client never
    /// mistakes a new raster for one it already has.
    revision: u32,
}

/// The rasters of the sequence the editor shows.
struct OpenSequence {
    path: String,
    object_key: Option<String>,
    sequence_id: SequenceId,
    project: Arc<ProjectSession>,
    settings: EffectRasterSettings,
    clips: HashMap<u32, ClipRaster>,
    /// Clip order in the sequence, for rendering clips off screen.
    order: Vec<u32>,
    visible: Vec<u32>,
    bytes: usize,
    /// Increases with each request, to find the clips off screen longest.
    tick: u64,
}

struct ClipRaster {
    clip: Arc<EffectInst>,
    state: RasterState,
    revision: u32,
    seen: u64,
}

enum RasterState {
    Pending,
    /// Dropped for the byte budget; renders again once visible.
    Evicted,
    Ready {
        columns: u32,
        rows: u32,
        pixels_rgba: Arc<[u8]>,
    },
    Failed(String),
}

impl SequenceClipRasterService {
    pub(crate) fn new() -> Self {
        Self {
            worker: RasterWorker::new(),
            open: None,
            revision: 0,
        }
    }

    /// Show `request`'s sequence from `project`: clips the snapshot changed
    /// render again, visible clips first.
    pub(crate) fn request(
        &mut self,
        settings: EffectRasterSettings,
        project: Option<Arc<ProjectSession>>,
        sequence_id: Option<SequenceId>,
        request: SequenceClipRasterRequest,
    ) -> SequenceClipRasterResponse {
        let (Some(project), Some(sequence_id)) = (project, sequence_id) else {
            self.close();
            return SequenceClipRasterResponse { pending: 0 };
        };
        let Some(sequence) = project.project.sequence(&sequence_id) else {
            self.close();
            return SequenceClipRasterResponse { pending: 0 };
        };
        let reopen = !self.open.as_ref().is_some_and(|open| {
            open.path == request.document.path
                && open.object_key == request.document.object_key
                && open.sequence_id == sequence_id
                && open.settings == settings
        });
        if reopen {
            self.open = None;
        }
        let open = self.open.get_or_insert_with(|| OpenSequence {
            path: request.document.path.clone(),
            object_key: request.document.object_key.clone(),
            sequence_id: sequence_id.clone(),
            project: Arc::clone(&project),
            settings: settings.clone(),
            clips: HashMap::new(),
            order: Vec::new(),
            visible: Vec::new(),
            bytes: 0,
            tick: 0,
        });
        let previous = std::mem::replace(&mut open.project, Arc::clone(&project));
        let previous_sequence = previous.project.sequence(&sequence_id);
        open.sync(
            &project,
            sequence,
            previous_sequence.filter(|_| !reopen),
            &previous,
        );
        open.tick += 1;
        open.visible = request.visible_effect_ids;
        for id in &open.visible {
            if let Some(clip) = open.clips.get_mut(id) {
                clip.seen = open.tick;
            }
        }
        let work = open.work();
        let pending = work.len() as u32;
        if work.is_empty() {
            self.worker.cancel();
        } else {
            self.worker.submit(project, sequence_id, settings, work);
        }
        SequenceClipRasterResponse { pending }
    }

    /// The rasters rendered or failed after revision `since`.
    pub(crate) fn take_results(
        &mut self,
        request: GuiDocumentRequest,
        since: u32,
    ) -> SequenceClipRasterResultBatch {
        for outcome in self.worker.drain() {
            let Some(open) = self.open.as_mut() else {
                continue;
            };
            let Some(entry) = open.clips.get_mut(&outcome.effect_id) else {
                continue;
            };
            if !Arc::ptr_eq(&entry.clip, &outcome.clip)
                || !matches!(entry.state, RasterState::Pending | RasterState::Evicted)
            {
                continue;
            }
            self.revision = self.revision.wrapping_add(1);
            entry.revision = self.revision;
            entry.state = match outcome.result {
                Ok(raster) => {
                    open.bytes += raster.pixels_rgba.len();
                    RasterState::Ready {
                        columns: raster.columns,
                        rows: raster.rows,
                        pixels_rgba: raster.pixels_rgba,
                    }
                }
                Err(message) => RasterState::Failed(message),
            };
        }
        let mut batch = SequenceClipRasterResultBatch {
            revision: self.revision,
            rasters: Vec::new(),
            errors: Vec::new(),
            pending: 0,
        };
        let Some(open) = self
            .open
            .as_mut()
            .filter(|open| open.path == request.path && open.object_key == request.object_key)
        else {
            return batch;
        };
        open.enforce_budget();
        for (&effect_id, entry) in &open.clips {
            match &entry.state {
                RasterState::Pending => batch.pending += 1,
                RasterState::Evicted => {}
                RasterState::Ready { columns, rows, .. } if entry.revision > since => {
                    batch.rasters.push(SequenceClipRaster {
                        effect_id,
                        revision: entry.revision,
                        columns: *columns,
                        rows: *rows,
                        pixels_rgba_token: format!("{effect_id}-{}", entry.revision),
                    })
                }
                RasterState::Failed(message) if entry.revision > since => {
                    batch.errors.push(SequenceClipRasterError {
                        effect_id,
                        revision: entry.revision,
                        message: message.clone(),
                    })
                }
                RasterState::Ready { .. } | RasterState::Failed(_) => {}
            }
        }
        batch
    }

    /// The pixels named by a token of [`Self::take_results`].
    pub(crate) fn pixels_rgba_for_token(&self, token: &str) -> Option<Vec<u8>> {
        let (effect_id, revision) = token.split_once('-')?;
        let (effect_id, revision) = (
            effect_id.parse::<u32>().ok()?,
            revision.parse::<u32>().ok()?,
        );
        let entry = self.open.as_ref()?.clips.get(&effect_id)?;
        match &entry.state {
            RasterState::Ready { pixels_rgba, .. } if entry.revision == revision => {
                Some(pixels_rgba.to_vec())
            }
            _ => None,
        }
    }

    fn close(&mut self) {
        self.open = None;
        self.worker.cancel();
    }
}

impl OpenSequence {
    /// Keep the rasters of clips `previous` shared with `sequence` in an
    /// unchanged context; every other clip renders again.
    fn sync(
        &mut self,
        project: &ProjectSession,
        sequence: &Sequence,
        previous: Option<&Sequence>,
        previous_project: &ProjectSession,
    ) {
        let context = previous.filter(|previous| {
            project
                .project
                .only_sequences_differ_from(&previous_project.project)
                && previous.frame_rate == sequence.frame_rate
                && previous.mark_collections == sequence.mark_collections
        });
        // Automation drives the parameters of the clips it binds.
        let automated: HashSet<_> = context
            .filter(|previous| previous.automation_clips != sequence.automation_clips)
            .map(|previous| {
                automated_clips(previous)
                    .chain(automated_clips(sequence))
                    .collect()
            })
            .unwrap_or_default();
        if context.is_none() {
            self.clips.clear();
            self.bytes = 0;
        }
        let mut clips = HashMap::with_capacity(sequence.effects.len());
        for effect in &sequence.effects {
            let id = effect.id.0;
            let entry = match self.clips.remove(&id) {
                Some(entry) if Arc::ptr_eq(&entry.clip, effect) && !automated.contains(&id) => {
                    entry
                }
                stale => {
                    if let Some(RasterState::Ready { pixels_rgba, .. }) =
                        stale.map(|entry| entry.state)
                    {
                        self.bytes -= pixels_rgba.len();
                    }
                    ClipRaster {
                        clip: Arc::clone(effect),
                        state: RasterState::Pending,
                        revision: 0,
                        seen: 0,
                    }
                }
            };
            clips.insert(id, entry);
        }
        for removed in self.clips.drain() {
            if let (
                _,
                ClipRaster {
                    state: RasterState::Ready { pixels_rgba, .. },
                    ..
                },
            ) = removed
            {
                self.bytes -= pixels_rgba.len();
            }
        }
        self.clips = clips;
        self.order = sequence.effects.iter().map(|effect| effect.id.0).collect();
    }

    /// Clips still to render: visible ones in the editor's order, then the
    /// rest of the sequence. Evicted clips render again only when visible.
    fn work(&self) -> Vec<(u32, Arc<EffectInst>)> {
        let visible = self.visible.iter().copied().collect::<HashSet<_>>();
        let wanted = |id: &u32, include_evicted: bool| {
            self.clips.get(id).filter(|entry| match entry.state {
                RasterState::Pending => true,
                RasterState::Evicted => include_evicted,
                RasterState::Ready { .. } | RasterState::Failed(_) => false,
            })
        };
        self.visible
            .iter()
            .filter_map(|id| wanted(id, true).map(|entry| (*id, Arc::clone(&entry.clip))))
            .chain(
                self.order
                    .iter()
                    .filter(|id| !visible.contains(id))
                    .filter_map(|id| wanted(id, false).map(|entry| (*id, Arc::clone(&entry.clip)))),
            )
            .collect()
    }

    /// Drop the rasters off screen longest until the rest fit the budget.
    fn enforce_budget(&mut self) {
        if self.bytes <= RASTER_BYTE_BUDGET {
            return;
        }
        let visible = self.visible.iter().copied().collect::<HashSet<_>>();
        let mut candidates = self
            .clips
            .iter()
            .filter(|(id, entry)| {
                !visible.contains(id) && matches!(entry.state, RasterState::Ready { .. })
            })
            .map(|(id, entry)| (entry.seen, *id))
            .collect::<Vec<_>>();
        candidates.sort_unstable();
        for (_, id) in candidates {
            if self.bytes <= RASTER_BYTE_BUDGET {
                break;
            }
            if let Some(entry) = self.clips.get_mut(&id)
                && let RasterState::Ready { pixels_rgba, .. } =
                    std::mem::replace(&mut entry.state, RasterState::Evicted)
            {
                self.bytes -= pixels_rgba.len();
            }
        }
    }
}

fn automated_clips(sequence: &Sequence) -> impl Iterator<Item = u32> + '_ {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| clip.bindings.iter())
        .filter_map(|binding| match &binding.target {
            AutomationTarget::EffectParam { effect_id, .. } => Some(effect_id.0),
            AutomationTarget::CompositionNodeParam { .. } => None,
        })
}
