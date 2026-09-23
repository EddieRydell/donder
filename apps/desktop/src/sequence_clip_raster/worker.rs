use super::*;

pub(super) struct RasterJob {
    pub(super) request_id: u32,
    pub(super) document_key: GuiDocumentRequest,
    pub(super) project: Arc<ProjectSession>,
    pub(super) setup_id: SetupId,
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
    let mut renderer_cache = HashMap::<String, PreparedEffectRasterRenderer>::new();
    let mut job = match receiver.recv() {
        Ok(job) => job,
        Err(_) => return,
    };
    loop {
        prune_worker_raster_cache(&mut raster_cache, &job.work_items);
        prune_prepared_renderer_cache(&mut renderer_cache, &job.work_items);
        let mut completed = true;
        let work_items = std::mem::take(&mut job.work_items);
        let (mut prepare_batch, prepare_batch_error) = match EffectRasterPrepareBatch::prepare(
            &job.project.project,
            &job.setup_id,
            &job.sequence_id,
        ) {
            Ok(batch) => (Some(batch), None),
            Err(error) => (None, Some(format!("{error:?}"))),
        };
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
                    let renderer = match renderer_cache.get(&item.signature_key).cloned() {
                        Some(renderer) => Ok(renderer),
                        None => match prepare_batch.as_mut() {
                            Some(batch) => {
                                match batch.prepare_effect(&EffectInstId(item.effect_id)) {
                                    Ok(renderer) => {
                                        renderer_cache
                                            .insert(item.signature_key.clone(), renderer.clone());
                                        Ok(renderer)
                                    }
                                    Err(error) => Err(format!("{error:?}")),
                                }
                            }
                            None => match &prepare_batch_error {
                                Some(message) => Err(message.clone()),
                                None => Err("raster prepare batch unavailable".to_string()),
                            },
                        },
                    };
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

pub(super) fn prune_prepared_renderer_cache(
    cache: &mut HashMap<String, PreparedEffectRasterRenderer>,
    active_items: &[RasterWorkItem],
) {
    let active = active_items
        .iter()
        .map(|item| item.signature_key.clone())
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
