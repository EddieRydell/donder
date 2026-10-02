use super::*;

pub(super) struct RasterJob {
    pub(super) request_id: u32,
    pub(super) document_key: GuiDocumentRequest,
    pub(super) project: Arc<ProjectSession>,
    pub(super) sequence_id: SequenceId,
    pub(super) settings: EffectRasterSettings,
    pub(super) work_items: Vec<RasterWorkItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RasterWorkItem {
    pub(super) cache_key: RasterCacheKey,
    pub(super) signature: RenderInputSignature,
    pub(super) signature_key: String,
    pub(super) effect_id: u32,
}

pub(super) enum RasterResultEntry {
    Ready {
        request_id: u32,
        signature: String,
        payload: CachedRasterPayload,
    },
    Unavailable {
        request_id: u32,
        effect_id: u32,
        signature: String,
    },
    Error {
        request_id: u32,
        signature: String,
        error: SequenceClipRasterError,
    },
    Complete {
        request_id: u32,
    },
}

pub(super) enum RasterWorkerResult {
    Raster {
        request_id: u32,
        document_key: GuiDocumentRequest,
        cache_key: RasterCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        payload: CachedRasterPayload,
    },
    Error {
        request_id: u32,
        document_key: GuiDocumentRequest,
        cache_key: RasterCacheKey,
        signature: RenderInputSignature,
        signature_key: String,
        error: SequenceClipRasterError,
    },
    Complete {
        request_id: u32,
        document_key: GuiDocumentRequest,
    },
}

pub(super) fn raster_worker(
    receiver: mpsc::Receiver<RasterJob>,
    sender: mpsc::Sender<RasterWorkerResult>,
    latest_request_id: Arc<AtomicU64>,
) {
    let mut raster_cache = HashMap::<RasterRenderCacheKey, CachedRasterValue>::new();
    let mut prepared_cache: Option<PreparedRasterSequence> = None;
    let mut job = match receiver.recv() {
        Ok(job) => job,
        Err(_) => return,
    };
    loop {
        prune_worker_raster_cache(&mut raster_cache, &job.work_items);
        if prepared_cache.as_ref().is_some_and(|cached| {
            !Arc::ptr_eq(&cached.project, &job.project) || cached.sequence_id != job.sequence_id
        }) {
            prepared_cache = None;
        }
        let mut completed = true;
        let work_items = std::mem::take(&mut job.work_items);
        for item in work_items {
            if latest_request_id.load(Ordering::Relaxed) != u64::from(job.request_id) {
                completed = false;
                break;
            }
            let render_cache_key = RasterRenderCacheKey::new(&item);
            let result = match raster_cache.get(&render_cache_key).cloned() {
                Some(CachedRasterValue::Raster(raster)) => RasterWorkerResult::Raster {
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                    cache_key: item.cache_key,
                    signature: item.signature,
                    signature_key: item.signature_key,
                    payload: raster,
                },
                Some(CachedRasterValue::Error(error)) => RasterWorkerResult::Error {
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                    cache_key: item.cache_key,
                    signature: item.signature,
                    signature_key: item.signature_key,
                    error,
                },
                None => {
                    let should_continue =
                        || latest_request_id.load(Ordering::Relaxed) == u64::from(job.request_id);
                    // One preparation per immutable project snapshot and sequence,
                    // shared by every clip and every raster size requested for it.
                    let prepared = prepared_cache.get_or_insert_with(|| PreparedRasterSequence {
                        project: Arc::clone(&job.project),
                        sequence_id: job.sequence_id.clone(),
                        sequence: prepare(
                            &job.project.project,
                            &job.sequence_id,
                            PrepareOutputs::All,
                        )
                        .map(Arc::new),
                    });
                    let renderer = prepared
                        .sequence
                        .as_ref()
                        .cloned()
                        .ok_or_else(|| "raster sequence selection is unavailable".to_string());
                    match match renderer {
                        Ok(renderer) => render_effect_raster(RasterRenderRequest {
                            renderer,
                            effect_id: item.effect_id,
                            signature_key: &item.signature_key,
                            cache_key: &item.cache_key,
                            display_column_count: item.cache_key.display_column_count,
                            display_row_count: item.cache_key.display_row_count,
                            settings: &job.settings,
                            should_continue: &should_continue,
                        }),
                        Err(message) => Err(RasterRenderFailure::Error(message)),
                    } {
                        Ok(raster) => {
                            raster_cache.insert(
                                render_cache_key,
                                CachedRasterValue::Raster(raster.clone()),
                            );
                            RasterWorkerResult::Raster {
                                request_id: job.request_id,
                                document_key: job.document_key.clone(),
                                cache_key: item.cache_key,
                                signature: item.signature,
                                signature_key: item.signature_key,
                                payload: raster,
                            }
                        }
                        Err(RasterRenderFailure::Cancelled) => {
                            completed = false;
                            break;
                        }
                        Err(RasterRenderFailure::Error(message)) => {
                            let error = SequenceClipRasterError {
                                request_id: job.request_id,
                                effect_id: item.effect_id,
                                signature: item.signature_key.clone(),
                                message,
                            };
                            raster_cache
                                .insert(render_cache_key, CachedRasterValue::Error(error.clone()));
                            RasterWorkerResult::Error {
                                request_id: job.request_id,
                                document_key: job.document_key.clone(),
                                cache_key: item.cache_key,
                                signature: item.signature,
                                signature_key: item.signature_key,
                                error,
                            }
                        }
                    }
                }
            };
            if sender.send(result).is_err() {
                return;
            }
            if let Some(next) = newest_queued_job(&receiver) {
                job = next;
                completed = false;
                break;
            }
        }
        if completed
            && sender
                .send(RasterWorkerResult::Complete {
                    request_id: job.request_id,
                    document_key: job.document_key.clone(),
                })
                .is_err()
        {
            return;
        }
        if completed {
            job = match receiver.recv() {
                Ok(job) => job,
                Err(_) => return,
            };
        } else if let Some(next) = newest_queued_job(&receiver) {
            job = next;
        } else {
            job = match receiver.recv() {
                Ok(job) => job,
                Err(_) => return,
            };
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct RasterRenderCacheKey {
    signature: String,
    display_column_count: u32,
    display_row_count: u32,
    settings: EffectRasterSettings,
}

impl Eq for RasterRenderCacheKey {}

impl Hash for RasterRenderCacheKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.signature.hash(state);
        self.display_column_count.hash(state);
        self.display_row_count.hash(state);
        hash_effect_raster_settings(&self.settings, state);
    }
}

impl RasterRenderCacheKey {
    fn new(item: &RasterWorkItem) -> Self {
        Self {
            signature: item.signature_key.clone(),
            display_column_count: item.cache_key.display_column_count,
            display_row_count: item.cache_key.display_row_count,
            settings: item.cache_key.settings.clone(),
        }
    }
}

pub(super) fn prune_worker_raster_cache(
    cache: &mut HashMap<RasterRenderCacheKey, CachedRasterValue>,
    active_items: &[RasterWorkItem],
) {
    let active = active_items
        .iter()
        .map(RasterRenderCacheKey::new)
        .collect::<HashSet<_>>();
    cache.retain(|key, _| active.contains(key));
}

pub(super) fn newest_queued_job(receiver: &mpsc::Receiver<RasterJob>) -> Option<RasterJob> {
    let mut newest = None;
    while let Ok(job) = receiver.try_recv() {
        newest = Some(job);
    }
    newest
}

pub(super) fn ordered_existing_effect_ids(
    effects: &[donder_language::effect::EffectInst],
    ordered_effect_ids: &[u32],
) -> Vec<u32> {
    let existing = effects
        .iter()
        .map(|effect| effect.id.0)
        .collect::<HashSet<_>>();
    let mut ids = Vec::new();
    for id in ordered_effect_ids {
        if existing.contains(id) && !ids.contains(id) {
            ids.push(*id);
        }
    }
    ids
}

struct PreparedRasterSequence {
    project: Arc<ProjectSession>,
    sequence_id: SequenceId,
    sequence: Option<Arc<PreparedSequence>>,
}
