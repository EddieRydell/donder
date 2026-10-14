#![deny(unsafe_code)]
#![deny(unreachable_pub)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

use camino::{Utf8Path, Utf8PathBuf};
use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::compiler::{compile_effects, compile_operators};
use donder_language::{Distance, DistanceSpan, Point3};
use donder_model::SequenceId;
use donder_model::SourceIdentity;
use donder_model::ValueSource;
use donder_model::{DonderProject, ProjectEdit};
use donder_model::{EffectDefinition, EffectDefinitionId};
use donder_model::{
    FixtureDefinition, FixtureElement, FixtureElementId, FixtureShape, FixtureTransform,
};
use donder_model::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_project_io::{MemoryProject, ProjectSession};
use donder_runtime::SequencePlayback;
use donder_runtime_types::sample_time_from_seconds_f32;
use donder_sequence_api::{
    BrowserCompileDiagnostic as DiagnosticView, BrowserCompileResult as CompileView,
    BrowserPageNode, BrowserSessionConfig,
};
use serde::Serialize;
use std::sync::Arc;
use wasm_bindgen::prelude::*;

mod editing;
mod language_server;
mod page_layout;
mod rasters;
mod sources;

fn js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::json_compatible())
        .map_err(|error| {
            JsValue::from_str(&format!(
                "Could not serialize browser API response: {error}"
            ))
        })
}

fn diagnostics_view(
    diagnostics: Vec<donder_language::compiler::Diagnostic>,
) -> Result<Vec<DiagnosticView>, JsValue> {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            Ok(DiagnosticView {
                start: u32::try_from(diagnostic.span.start).map_err(|_| {
                    JsValue::from_str("Diagnostic offset exceeds the browser limit.")
                })?,
                end: u32::try_from(diagnostic.span.end).map_err(|_| {
                    JsValue::from_str("Diagnostic offset exceeds the browser limit.")
                })?,
                message: diagnostic.message,
            })
        })
        .collect()
}

fn prepare_playback(
    project: &DonderProject,
    sequence_id: &SequenceId,
) -> Result<SequencePlayback, JsValue> {
    Ok(prepare(project, sequence_id, PrepareOutputs::All)
        .ok_or_else(|| JsValue::from_str("The demo sequence could not be prepared."))?
        .into_playback())
}

/// An in-memory project built from the website's page, sources, and audio.
/// No project files or server-side runtime are involved.
#[wasm_bindgen]
pub struct BrowserSession {
    session: Arc<ProjectSession>,
    past: Vec<Arc<ProjectSession>>,
    future: Vec<Arc<ProjectSession>>,
    clipboard: Option<donder_editor::SequenceClipboard>,
    sequence_id: SequenceId,
    layout_id: LayoutId,
    page: Vec<BrowserPageNode>,
    duration_seconds: f32,
    revision: u32,
    playback: SequencePlayback,
    /// While replaying saved operations, playback is prepared once at the end.
    replaying: bool,
}

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(constructor)]
    pub fn new(config: JsValue) -> Result<BrowserSession, JsValue> {
        let config: BrowserSessionConfig = serde_wasm_bindgen::from_value(config)
            .map_err(|error| JsValue::from_str(&format!("Invalid session config: {error}")))?;
        page_layout::validate_page(&config.page)?;
        let project = MemoryProject {
            documents: config
                .project
                .documents
                .into_iter()
                .map(|document| (Utf8PathBuf::from(document.path), document.text))
                .collect(),
            assets: config
                .project
                .assets
                .into_iter()
                .map(|asset| (Utf8PathBuf::from(asset.path), Utf8PathBuf::from(asset.url)))
                .collect(),
        };
        let mut session =
            donder_project_io::load_memory_project(Utf8Path::new(BROWSER_PROJECT_ROOT), &project)
                .map_err(|error| {
                JsValue::from_str(&format!(
                    "The show does not load:
{error}"
                ))
            })?;
        let root = session.project.root();
        let sequence_id = root
            .sequences
            .first()
            .ok_or_else(|| JsValue::from_str("The show has no sequence."))?
            .id()
            .clone();
        let layout_id = session
            .project
            .setup(root.setup.id())
            .ok_or_else(|| JsValue::from_str("The show's setup was not found."))?
            .layout
            .id()
            .clone();
        page_layout::bind_page_layout(
            &mut session.project,
            &layout_id,
            &sequence_id,
            &config.page,
        )?;
        let playback = prepare_playback(&session.project, &sequence_id)?;
        let duration_seconds = session
            .project
            .sequence(&sequence_id)
            .ok_or_else(|| JsValue::from_str("The show's sequence was not found."))?
            .duration
            .as_seconds_f32();
        Ok(Self {
            session: Arc::new(session),
            past: Vec::new(),
            future: Vec::new(),
            clipboard: None,
            sequence_id,
            layout_id,
            page: config.page,
            duration_seconds,
            revision: 0,
            playback,
            replaying: false,
        })
    }

    /// Evaluate the prepared sequence at an absolute playback time. Returns RGB
    /// bytes in depth-first page order.
    #[wasm_bindgen(js_name = render)]
    pub fn render(&mut self, seconds: f32) -> Result<Vec<u8>, JsValue> {
        let sample_time = sample_time_from_seconds_f32(seconds).map_err(|_| {
            JsValue::from_str("Playback time must be a finite non-negative supported value.")
        })?;
        let frame = self.playback.evaluate(sample_time);
        Ok(frame
            .colors()
            .iter()
            .flat_map(|color| [color.red, color.green, color.blue])
            .collect())
    }

    /// Replace the measured page. Effects and automation rows on removed page
    /// nodes are deleted. Page changes are not history entries.
    #[wasm_bindgen(js_name = setPageLayout)]
    pub fn set_page_layout(&mut self, page: JsValue) -> Result<(), JsValue> {
        let page: Vec<BrowserPageNode> = serde_wasm_bindgen::from_value(page)
            .map_err(|error| JsValue::from_str(&format!("Invalid page layout: {error}")))?;
        page_layout::validate_page(&page)?;
        let mut candidate = (*self.session).clone();
        page_layout::apply_page_layout(
            &mut candidate.project,
            &self.layout_id,
            &self.sequence_id,
            &page,
        )?;
        let playback = prepare_playback(&candidate.project, &self.sequence_id)?;
        self.revision = self.next_revision()?;
        self.session = Arc::new(candidate);
        self.playback = playback;
        self.page = page;
        Ok(())
    }

    #[wasm_bindgen(js_name = revision)]
    pub fn revision(&self) -> u32 {
        self.revision
    }

    #[wasm_bindgen(js_name = frameRate)]
    pub fn frame_rate(&self) -> u32 {
        self.playback.sequence().frame_rate()
    }

    #[wasm_bindgen(js_name = durationSeconds)]
    pub fn duration_seconds(&self) -> f32 {
        self.duration_seconds
    }
}

/// Where the show's documents stand in for a project directory.
const BROWSER_PROJECT_ROOT: &str = "/show";

impl BrowserSession {
    fn next_revision(&self) -> Result<u32, JsValue> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))
    }
}
