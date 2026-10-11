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

use donder_elaboration::{PrepareOutputs, prepare};
use donder_language::compiler::{compile_effects, compile_operators};
use donder_language::{Distance, DistanceSpan, DonderDuration, Point3};
use donder_model::ControllerId;
use donder_model::SequenceId;
use donder_model::ValueSource;
use donder_model::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
use donder_model::{
    DonderProject, ProjectData, ProjectDefinitionStores, ProjectEdit, ProjectId, ProjectRoot,
};
use donder_model::{EffectDefinition, EffectDefinitionId};
use donder_model::{
    FixtureDefinition, FixtureElement, FixtureElementId, FixtureShape, FixtureTransform,
};
use donder_model::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_model::{Patch, PatchId};
use donder_model::{Setup, SetupId};
use donder_project_io::ProjectSession;
use donder_runtime::SequencePlayback;
use donder_runtime_types::{Color, sample_time_from_seconds_f32};
use donder_sequence_api::{
    BrowserCompileDiagnostic as DiagnosticView, BrowserCompileResult as CompileView,
    BrowserPageNode, BrowserSessionConfig,
};
use indexmap::IndexMap;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use wasm_bindgen::prelude::*;

mod audio;
mod editing;
mod language_server;
mod page_layout;
mod rasters;
mod sources;
pub use sources::declaration_sources;

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

/// Every browser session is the same module, so saved operations, which name
/// definitions by module, still resolve when a later session replays them.
const BROWSER_PROJECT_MODULE: uuid::Uuid =
    uuid::Uuid::from_u128(0x0d0d_e000_b0b0_4000_8000_0000_0000_0001);

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
        validate_session_values(config.frame_rate, config.duration_seconds)?;
        page_layout::validate_page(&config.page)?;
        let document = DocumentId::new(BROWSER_PROJECT_MODULE, "browser-demo.data.donder".into());
        let project_source = SourceIdentity::from_document(document, "demo".into());
        let project_object = ObjectIdentity::from(project_source.clone());
        let setup_object = project_object.owned(OwnedObjectSlot::Setup);
        let layout_id = LayoutId(setup_object.owned(OwnedObjectSlot::Layout));
        let setup = Setup {
            description: None,
            id: SetupId(setup_object.clone()),
            layout: ValueSource::Inline(Box::new(page_layout::page_layout(
                layout_id.clone(),
                &config.page,
            ))),
            patch: ValueSource::Inline(Box::new(Patch {
                description: None,
                id: PatchId(setup_object.owned(OwnedObjectSlot::Patch)),
                routes: Vec::new(),
            })),
            controllers: Vec::<ValueSource<Box<donder_model::Controller>, ControllerId>>::new(),
        };
        let data = ProjectData {
            root: ProjectRoot {
                description: None,
                id: ProjectId(project_source.clone()),
                setup: ValueSource::Inline(Box::new(setup)),
                sequences: Vec::new(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions: ProjectDefinitionStores::default(),
        };
        let mut project = DonderProject::try_new(data).map_err(|error| {
            JsValue::from_str(&format!("Invalid browser demo project: {error}"))
        })?;
        let duration = DonderDuration(
            Duration::try_from_secs_f32(config.duration_seconds).map_err(|_| {
                JsValue::from_str("Sequence duration is outside the supported range.")
            })?,
        );
        let sequence_id =
            donder_model::add_sequence(&mut project, duration, config.frame_rate, Color::BLACK)
                .map_err(|error| JsValue::from_str(&error))?;
        let mut session = sources::initial_session(project, &project_source)?;
        for source in &config.sources {
            sources::install_source(
                &mut session,
                &sequence_id,
                &source.path,
                source.kind.clone(),
                &source.source,
                sources::SourceInstall::Create,
            )?
            .into_result(&source.path)?;
        }
        if let Some(url) = &config.audio_url {
            audio::set_audio(&mut session, &sequence_id, url)?;
        }
        set_mark_collections(&mut session.project, &sequence_id, &config.mark_collections)?;
        let playback = prepare_playback(&session.project, &sequence_id)?;
        Ok(Self {
            session: Arc::new(session),
            past: Vec::new(),
            future: Vec::new(),
            clipboard: None,
            sequence_id,
            layout_id,
            page: config.page,
            duration_seconds: config.duration_seconds,
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

impl BrowserSession {
    fn next_revision(&self) -> Result<u32, JsValue> {
        self.revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))
    }
}

fn set_mark_collections(
    project: &mut DonderProject,
    sequence_id: &SequenceId,
    collections: &[donder_sequence_api::SequenceMarkCollection],
) -> Result<(), JsValue> {
    let mut sequence = project
        .sequence(sequence_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
    sequence.mark_collections = collections
        .iter()
        .map(|collection| {
            Ok(donder_model::MarkCollection {
                key: donder_model::MarkCollectionKey {
                    name: donder_language::object_name(&collection.key),
                },
                description: None,
                display_color: Color::from_hex(&collection.color).ok_or_else(|| {
                    JsValue::from_str(&format!(
                        "Mark color {} is not a hex color.",
                        collection.color
                    ))
                })?,
                marks: collection
                    .marks_seconds
                    .iter()
                    .zip(&collection.mark_labels)
                    .map(|(&seconds, label)| {
                        let time = donder_language::DonderTime::try_from_seconds_f32(seconds)
                            .map_err(|_| {
                                JsValue::from_str("Mark times must be non-negative seconds.")
                            })?;
                        Ok(donder_model::Mark {
                            time,
                            label: label.clone(),
                        })
                    })
                    .collect::<Result<_, JsValue>>()?,
            })
        })
        .collect::<Result<_, JsValue>>()?;
    project
        .replace_sequence(sequence_id, sequence)
        .map_err(|error| JsValue::from_str(&error))
}

fn validate_session_values(frame_rate: u32, duration_seconds: f32) -> Result<(), JsValue> {
    if frame_rate == 0 || frame_rate > 1_000 {
        return Err(JsValue::from_str("Frame rate must be between 1 and 1000."));
    }
    if !duration_seconds.is_finite() || duration_seconds <= 0.0 || duration_seconds > 3_600.0 {
        return Err(JsValue::from_str(
            "Duration must be greater than zero and at most one hour.",
        ));
    }
    Ok(())
}
