use super::*;

pub(crate) struct SequenceClipRasterService {
    sender: mpsc::Sender<RasterJob>,
    receiver: mpsc::Receiver<RasterWorkerResult>,
    cache: HashMap<RasterCacheKey, CachedRasterResult>,
    cache_bytes: usize,
    next_cache_access: u64,
    pixels_by_token: HashMap<String, Arc<Vec<u8>>>,
    pending_results: HashMap<GuiDocumentRequest, VecDeque<RasterResultEntry>>,
    active: Option<ActiveRasterJob>,
    next_request_id: u32,
    latest_request_id: Arc<AtomicU64>,
}

impl SequenceClipRasterService {
    pub(crate) fn new() -> Self {
        let (request_sender, request_receiver) = mpsc::channel();
        let (result_sender, result_receiver) = mpsc::channel();
        let latest_request_id = Arc::new(AtomicU64::new(0));
        thread::spawn({
            let latest_request_id = Arc::clone(&latest_request_id);
            move || raster_worker(request_receiver, result_sender, latest_request_id)
        });
        Self {
            sender: request_sender,
            receiver: result_receiver,
            cache: HashMap::new(),
            cache_bytes: 0,
            next_cache_access: 1,
            pixels_by_token: HashMap::new(),
            pending_results: HashMap::new(),
            active: None,
            next_request_id: 1,
            latest_request_id,
        }
    }

    pub fn request(
        &mut self,
        project_revision: u32,
        settings: EffectRasterSettings,
        project: Option<Arc<ProjectSession>>,
        setup_id: Option<SetupId>,
        sequence_id: Option<SequenceId>,
        request: SequenceClipRasterRequest,
    ) -> SequenceClipRasterResponse {
        self.drain_results();
        self.prepare_response_and_schedule(
            project_revision,
            settings,
            project,
            setup_id,
            sequence_id,
            request,
        )
    }

    fn drain_results(&mut self) {
        while let Ok(result) = self.receiver.try_recv() {
            match result {
                RasterWorkerResult::Raster {
                    request_id,
                    document_key,
                    cache_key,
                    signature,
                    signature_key,
                    payload,
                } => {
                    let result_payload = payload.clone();
                    self.pixels_by_token
                        .insert(payload.token.clone(), Arc::clone(&payload.pixels_rgba));
                    let last_used = self.next_cache_access();
                    self.insert_cache_entry(
                        cache_key,
                        CachedRasterResult {
                            signature: signature.clone(),
                            byte_len: payload.byte_len(),
                            last_used,
                            result: CachedRasterValue::Raster(payload),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            RasterResultEntry::Ready {
                                request_id,
                                signature: signature_key,
                                payload: result_payload,
                            },
                        );
                    }
                }
                RasterWorkerResult::Error {
                    request_id,
                    document_key,
                    cache_key,
                    signature,
                    signature_key,
                    error,
                } => {
                    let result_error = error.clone();
                    let last_used = self.next_cache_access();
                    self.insert_cache_entry(
                        cache_key,
                        CachedRasterResult {
                            signature: signature.clone(),
                            byte_len: 0,
                            last_used,
                            result: CachedRasterValue::Error(error),
                        },
                    );
                    if self.is_current_request(&document_key, request_id) {
                        self.push_result(
                            document_key,
                            RasterResultEntry::Error {
                                request_id,
                                signature: signature_key,
                                error: result_error,
                            },
                        );
                    }
                }
                RasterWorkerResult::Complete {
                    request_id,
                    document_key,
                } => {
                    let current_request = self.is_current_request(&document_key, request_id);
                    if self
                        .active
                        .as_ref()
                        .is_some_and(|active| active.request_id == request_id)
                    {
                        self.active = None;
                    }
                    if current_request {
                        self.push_result(document_key, RasterResultEntry::Complete { request_id });
                    }
                }
            }
        }
    }

    pub fn take_results(
        &mut self,
        project_revision: u32,
        request: GuiDocumentRequest,
        request_id: u32,
    ) -> SequenceClipRasterResultBatch {
        self.drain_results();
        let document_key = request;
        let mut ready = Vec::new();
        let mut unavailable = Vec::new();
        let mut errors = Vec::new();
        let mut complete = self
            .active
            .as_ref()
            .is_none_or(|active| active.request_id != request_id);

        if let Some(entries) = self.pending_results.get_mut(&document_key) {
            let mut retained = VecDeque::new();
            while let Some(entry) = entries.pop_front() {
                match entry {
                    RasterResultEntry::Ready {
                        request_id: entry_request_id,
                        signature,
                        payload,
                    } if entry_request_id == request_id => {
                        let mut raster = payload.raster;
                        raster.request_id = entry_request_id;
                        raster.signature = signature;
                        ready.push(raster);
                    }
                    RasterResultEntry::Unavailable {
                        request_id: entry_request_id,
                        effect_id,
                        signature,
                    } if entry_request_id == request_id => {
                        unavailable.push(SequenceClipRasterUnavailable {
                            request_id: entry_request_id,
                            effect_id,
                            signature,
                        });
                    }
                    RasterResultEntry::Error {
                        request_id: entry_request_id,
                        signature,
                        mut error,
                    } if entry_request_id == request_id => {
                        error.request_id = entry_request_id;
                        error.signature = signature;
                        errors.push(error);
                    }
                    RasterResultEntry::Complete {
                        request_id: entry_request_id,
                    } if entry_request_id == request_id => {
                        complete = true;
                    }
                    entry => retained.push_back(entry),
                }
            }
            *entries = retained;
        }
        self.pending_results
            .retain(|_, entries| !entries.is_empty());

        SequenceClipRasterResultBatch {
            project_revision,
            request_id,
            ready,
            unavailable,
            errors,
            complete,
        }
    }

    fn prepare_response_and_schedule(
        &mut self,
        project_revision: u32,
        settings: EffectRasterSettings,
        project: Option<Arc<ProjectSession>>,
        setup_id: Option<SetupId>,
        sequence_id: Option<SequenceId>,
        request: SequenceClipRasterRequest,
    ) -> SequenceClipRasterResponse {
        let document_key = request.document.clone();
        let ordered_items = request.items;
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let Some(project) = project else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };
        let (Some(setup_id), Some(sequence_id)) = (setup_id, sequence_id) else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };

        let Some(sequence) = project.project.sequence(&sequence_id) else {
            self.active = None;
            return empty_response(project_revision, request_id, true);
        };

        let mut work_items = Vec::new();
        let mut requested_cache_keys = Vec::new();
        let requested_ids = ordered_items
            .iter()
            .map(|item| item.effect_id)
            .collect::<Vec<_>>();
        let ordered_ids = ordered_existing_effect_ids(&sequence.effects, &requested_ids);
        let client_signatures = ordered_items
            .iter()
            .map(|item| (item.effect_id, item.signature.clone()))
            .collect::<HashMap<_, _>>();
        let current_ids = sequence
            .effects
            .iter()
            .map(|effect| effect.id.0)
            .collect::<HashSet<_>>();
        let stale_keys = self
            .cache
            .keys()
            .filter(|key| {
                key.matches_request(&request.document, request.display_row_count, &settings)
                    && !current_ids.contains(&key.effect_id)
            })
            .cloned()
            .collect::<Vec<_>>();
        for key in stale_keys {
            self.remove_cache_entry(&key);
        }
        self.retain_active_pixel_tokens();
        self.pending_results
            .insert(document_key.clone(), VecDeque::new());
        for effect_id in requested_ids {
            if !current_ids.contains(&effect_id) {
                self.push_result(
                    document_key.clone(),
                    RasterResultEntry::Unavailable {
                        request_id,
                        effect_id,
                        signature: signature_key(&RenderInputSignature::Invalid {
                            message: "effect unavailable".to_string(),
                        }),
                    },
                );
            }
        }

        for effect_id in ordered_ids {
            let Some(effect) = sequence
                .effects
                .iter()
                .find(|effect| effect.id == EffectInstId(effect_id))
            else {
                continue;
            };
            let Some(request_item) = ordered_items
                .iter()
                .find(|item| item.effect_id == effect_id)
            else {
                continue;
            };
            let cache_key = RasterCacheKey::new(
                &request.document,
                request.display_row_count,
                &settings,
                effect_id,
                request_item.display_column_count,
            );
            let signature = match render_signature(&project.project, &setup_id, sequence, effect) {
                Ok(signature) => signature,
                Err(message) => RenderInputSignature::Invalid { message },
            };
            let signature_key = signature_key(&signature);
            requested_cache_keys.push(cache_key.clone());
            if client_signatures
                .get(&effect_id)
                .and_then(|signature| signature.as_ref())
                .is_some_and(|client_signature| client_signature == &signature_key)
            {
                continue;
            }
            match self.cache_entry(&cache_key) {
                Some(entry) if entry.signature == signature => match entry.result {
                    CachedRasterValue::Raster(payload) => self.push_result(
                        document_key.clone(),
                        RasterResultEntry::Ready {
                            request_id,
                            signature: signature_key,
                            payload,
                        },
                    ),
                    CachedRasterValue::Error(error) => self.push_result(
                        document_key.clone(),
                        RasterResultEntry::Error {
                            request_id,
                            signature: signature_key,
                            error,
                        },
                    ),
                },
                Some(_) => {
                    self.remove_cache_entry(&cache_key);
                    work_items.push(RasterWorkItem {
                        cache_key,
                        signature,
                        signature_key,
                        effect_id,
                    });
                }
                None => work_items.push(RasterWorkItem {
                    cache_key,
                    signature,
                    signature_key,
                    effect_id,
                }),
            }
        }

        let protected_keys = requested_cache_keys.into_iter().collect::<HashSet<_>>();
        self.evict_cache_over_budget(&protected_keys);
        if work_items.is_empty() {
            self.active = None;
            self.push_result(document_key, RasterResultEntry::Complete { request_id });
        } else {
            let job = RasterJob {
                request_id,
                document_key: document_key.clone(),
                project,
                setup_id,
                sequence_id,
                settings,
                work_items,
            };
            if self.sender.send(job).is_ok() {
                self.latest_request_id
                    .store(u64::from(request_id), Ordering::Relaxed);
                self.active = Some(ActiveRasterJob {
                    request_id,
                    document_key,
                });
            } else {
                self.active = None;
                self.push_result(document_key, RasterResultEntry::Complete { request_id });
            }
        }
        SequenceClipRasterResponse {
            project_revision,
            request_id,
            complete: self.active.is_none(),
        }
    }

    fn push_result(&mut self, document_key: GuiDocumentRequest, result: RasterResultEntry) {
        self.pending_results
            .entry(document_key)
            .or_default()
            .push_back(result);
    }

    fn is_current_request(&self, document_key: &GuiDocumentRequest, request_id: u32) -> bool {
        self.active.as_ref().is_some_and(|active| {
            &active.document_key == document_key && active.request_id == request_id
        })
    }

    pub fn pixels_rgba_for_token(&mut self, token: &str) -> Option<Vec<u8>> {
        self.drain_results();
        self.pixels_by_token
            .get(token)
            .map(|pixels| pixels.as_ref().clone())
    }

    fn cache_entry(&mut self, key: &RasterCacheKey) -> Option<CachedRasterResult> {
        let last_used = self.next_cache_access();
        self.cache.get_mut(key).map(|entry| {
            entry.last_used = last_used;
            entry.clone()
        })
    }

    fn insert_cache_entry(&mut self, key: RasterCacheKey, entry: CachedRasterResult) {
        if let Some(previous) = self.cache.insert(key, entry.clone()) {
            self.cache_bytes = self.cache_bytes.saturating_sub(previous.byte_len);
        }
        self.cache_bytes = self.cache_bytes.saturating_add(entry.byte_len);
        self.evict_cache_over_budget(&HashSet::new());
        self.retain_active_pixel_tokens();
    }

    fn remove_cache_entry(&mut self, key: &RasterCacheKey) {
        if let Some(previous) = self.cache.remove(key) {
            self.cache_bytes = self.cache_bytes.saturating_sub(previous.byte_len);
        }
    }

    fn evict_cache_over_budget(&mut self, protected_keys: &HashSet<RasterCacheKey>) {
        while self.cache_bytes > RASTER_CACHE_BYTE_BUDGET {
            let Some(key) = self
                .cache
                .iter()
                .filter(|(key, _)| !protected_keys.contains(*key))
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            self.remove_cache_entry(&key);
        }
    }

    fn next_cache_access(&mut self) -> u64 {
        let access = self.next_cache_access;
        self.next_cache_access = self.next_cache_access.saturating_add(1);
        access
    }

    fn retain_active_pixel_tokens(&mut self) {
        let active_tokens = self
            .cache
            .values()
            .filter_map(|entry| match &entry.result {
                CachedRasterValue::Raster(payload) => Some(payload.token.clone()),
                CachedRasterValue::Error(_) => None,
            })
            .collect::<HashSet<_>>();
        self.pixels_by_token
            .retain(|token, _| active_tokens.contains(token));
    }
}

pub(super) fn empty_response(
    project_revision: u32,
    request_id: u32,
    complete: bool,
) -> SequenceClipRasterResponse {
    SequenceClipRasterResponse {
        project_revision,
        request_id,
        complete,
    }
}
