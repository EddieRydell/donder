use super::*;
use donder_sequence_api::{
    AppSettings, BrowserEditorState, BrowserSelectionResult, DocumentViewId, GuiDocument,
    GuiDocumentRequest, GuiEditCommand, SequenceGuiEdit, SequenceSelectionEdit,
};

impl BrowserSession {
    fn request(&self) -> GuiDocumentRequest {
        let identity = &self.sequence_id.0;
        GuiDocumentRequest {
            project_revision: self.revision,
            path: identity.root_source().document().to_string(),
            object_key: Some(identity.root_source().object().to_string()),
            owned_path: identity.owned_path().iter().map(Into::into).collect(),
            view: DocumentViewId::Sequence,
        }
    }

    fn editor_state_view(&self) -> BrowserEditorState {
        let request = self.request();
        let mut document = donder_editor::project_gui_document(Some(&self.session), &request);
        // Projection checks the filesystem; browser audio is a URL the website serves.
        if let GuiDocument::Sequence { document } = &mut document
            && let Some(audio) = &mut document.audio
        {
            audio.exists = true;
        }
        BrowserEditorState {
            revision: self.revision,
            document,
            request,
            settings: AppSettings::default(),
            can_undo: !self.past.is_empty(),
            can_redo: !self.future.is_empty(),
        }
    }

    pub(super) fn accept(&mut self, candidate: ProjectSession) -> Result<(), JsValue> {
        let playback = prepare_playback(&candidate.project, &self.sequence_id)?;
        let revision = self.next_revision()?;
        self.past.push(Arc::clone(&self.session));
        if self.past.len() > 100 {
            self.past.remove(0);
        }
        self.future.clear();
        self.duration_seconds = candidate
            .project
            .sequence(&self.sequence_id)
            .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?
            .duration
            .as_seconds_f32();
        self.session = Arc::new(candidate);
        self.playback = playback;
        self.revision = revision;
        Ok(())
    }

    /// History holds project snapshots; the current page always wins.
    fn restore(&mut self, snapshot: Arc<ProjectSession>) -> Result<(), JsValue> {
        let mut candidate = (*snapshot).clone();
        page_layout::apply_page_layout(
            &mut candidate.project,
            &self.layout_id,
            &self.sequence_id,
            &self.page,
        )?;
        let playback = prepare_playback(&candidate.project, &self.sequence_id)?;
        let revision = self.next_revision()?;
        self.duration_seconds = candidate
            .project
            .sequence(&self.sequence_id)
            .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?
            .duration
            .as_seconds_f32();
        self.session = Arc::new(candidate);
        self.playback = playback;
        self.revision = revision;
        Ok(())
    }
}

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(js_name = editorState)]
    pub fn editor_state(&self) -> Result<JsValue, JsValue> {
        js_value(&self.editor_state_view())
    }

    #[wasm_bindgen(js_name = applyEdit)]
    pub fn apply_edit(&mut self, edit: JsValue) -> Result<JsValue, JsValue> {
        let edit: SequenceGuiEdit = serde_wasm_bindgen::from_value(edit)
            .map_err(|error| JsValue::from_str(&format!("Invalid sequence edit: {error}")))?;
        if matches!(
            &edit,
            SequenceGuiEdit::SetAudio {
                import_path: Some(_)
            }
        ) {
            return Err(JsValue::from_str(
                "Audio file imports require a desktop host.",
            ));
        }
        let mut candidate = (*self.session).clone();
        donder_editor::apply_edit(
            &mut candidate,
            &self.request(),
            GuiEditCommand::Sequence { edit },
        )
        .map_err(|error| JsValue::from_str(error.message()))?;
        self.accept(candidate)?;
        self.editor_state()
    }

    #[wasm_bindgen(js_name = applySelectionEdit)]
    pub fn apply_selection_edit(&mut self, edit: JsValue) -> Result<JsValue, JsValue> {
        let edit: SequenceSelectionEdit = serde_wasm_bindgen::from_value(edit)
            .map_err(|error| JsValue::from_str(&format!("Invalid selection edit: {error}")))?;
        let mutation = if let SequenceSelectionEdit::Copy { selection } = edit {
            let (clipboard, copied_count, skipped_count) = donder_editor::copy_sequence_selection(
                &self.session,
                &self.sequence_id,
                &selection,
            )
            .map_err(|error| JsValue::from_str(error.message()))?;
            self.clipboard = clipboard;
            donder_editor::SequenceSelectionMutation {
                selection: Some(selection),
                copied_count,
                skipped_count,
            }
        } else {
            let mut candidate = (*self.session).clone();
            let mut clipboard = self.clipboard.clone();
            let mutation = donder_editor::apply_sequence_selection_edit(
                &mut candidate,
                &self.request(),
                edit,
                &mut clipboard,
            )
            .map_err(|error| JsValue::from_str(error.message()))?;
            self.accept(candidate)?;
            self.clipboard = clipboard;
            mutation
        };
        js_value(&BrowserSelectionResult {
            state: self.editor_state_view(),
            selection: mutation.selection,
            copied_count: mutation.copied_count,
            skipped_count: mutation.skipped_count,
        })
    }

    #[wasm_bindgen(js_name = undo)]
    pub fn undo(&mut self) -> Result<JsValue, JsValue> {
        let snapshot = self
            .past
            .last()
            .cloned()
            .ok_or_else(|| JsValue::from_str("Nothing to undo."))?;
        let current = Arc::clone(&self.session);
        self.restore(snapshot)?;
        self.past.pop();
        self.future.push(current);
        self.editor_state()
    }

    #[wasm_bindgen(js_name = redo)]
    pub fn redo(&mut self) -> Result<JsValue, JsValue> {
        let snapshot = self
            .future
            .last()
            .cloned()
            .ok_or_else(|| JsValue::from_str("Nothing to redo."))?;
        let current = Arc::clone(&self.session);
        self.restore(snapshot)?;
        self.future.pop();
        self.past.push(current);
        self.editor_state()
    }
}
