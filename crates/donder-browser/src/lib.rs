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
use donder_language::controller::ControllerId;
use donder_language::dsl::{compile_effects, compile_operators};
use donder_language::effect::{
    EffectDefinition, EffectDefinitionId, EffectInst, EffectInstId, EffectRef, EffectScope,
};
use donder_language::fixture::{
    FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
    FixtureTransform,
};
use donder_language::identity::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
use donder_language::layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutFixture, LayoutFixtureKind, LayoutId,
};
use donder_language::model::{
    DonderProject, ProjectData, ProjectDefinitionStores, ProjectEdit, ProjectId, ProjectRoot,
};
use donder_language::ownership::ValueSource;
use donder_language::patch::{Patch, PatchId};
use donder_language::sequence::{SequenceId, SequenceLayerId};
use donder_language::setup::{Setup, SetupId};
use donder_language::values::{
    Color, Distance, DonderDuration, DonderTime, Point3, sample_time_from_seconds_f32,
};
use donder_runtime::SequencePlayback;
use donder_sequence_api::SequenceGuiEdit;
use indexmap::IndexMap;
use serde::Serialize;
use std::time::Duration;
use wasm_bindgen::prelude::*;

#[derive(Serialize)]
struct DiagnosticView {
    start: usize,
    end: usize,
    message: String,
}

#[derive(Serialize)]
struct CompileView {
    definitions: Vec<String>,
    diagnostics: Vec<DiagnosticView>,
}

#[derive(Serialize)]
struct PixelView {
    red: u8,
    green: u8,
    blue: u8,
}

#[derive(Serialize)]
struct FrameView {
    revision: u32,
    seconds: f32,
    pixels: Vec<PixelView>,
}

#[derive(Serialize)]
struct EffectCreatedView {
    id: u32,
    name: String,
    revision: u32,
}

fn js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value).map_err(|error| {
        JsValue::from_str(&format!(
            "Could not serialize browser API response: {error}"
        ))
    })
}

fn diagnostics_view(diagnostics: Vec<donder_language::dsl::Diagnostic>) -> Vec<DiagnosticView> {
    diagnostics
        .into_iter()
        .map(|diagnostic| DiagnosticView {
            start: diagnostic.span.start,
            end: diagnostic.span.end,
            message: diagnostic.message,
        })
        .collect()
}

/// Compile an effect source document and return declaration names or source diagnostics.
#[wasm_bindgen(js_name = compileEffectSource)]
pub fn compile_effect_source(source: &str) -> Result<JsValue, JsValue> {
    let view = match compile_effects(source) {
        Ok(definitions) => CompileView {
            definitions: definitions
                .into_iter()
                .map(|definition| definition.name().as_str().to_owned())
                .collect(),
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => CompileView {
            definitions: Vec::new(),
            diagnostics: diagnostics_view(diagnostics),
        },
    };
    js_value(&view)
}

/// Compile an operator source document and return declaration names or source diagnostics.
#[wasm_bindgen(js_name = compileOperatorSource)]
pub fn compile_operator_source(source: &str) -> Result<JsValue, JsValue> {
    let view = match compile_operators(source) {
        Ok(definitions) => CompileView {
            definitions: definitions
                .into_iter()
                .map(|definition| definition.name().as_str().to_owned())
                .collect(),
            diagnostics: Vec::new(),
        },
        Err(diagnostics) => CompileView {
            definitions: Vec::new(),
            diagnostics: diagnostics_view(diagnostics),
        },
    };
    js_value(&view)
}

/// An in-memory example project and its prepared playback state.
///
/// The browser supplies the stage's pixel count and an editable effect source;
/// no project files or server-side runtime are involved.
#[wasm_bindgen]
pub struct BrowserSession {
    project: DonderProject,
    sequence_id: SequenceId,
    fixture_definition_id: FixtureDefinitionId,
    effect_definition_id: EffectDefinitionId,
    target: FixtureTarget,
    effect_id: EffectInstId,
    next_effect_id: u32,
    duration_seconds: f32,
    pixel_count: u32,
    character_positions: Vec<[f32; 2]>,
    revision: u32,
    playback: SequencePlayback,
}

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(constructor)]
    pub fn new(
        pixel_count: u32,
        frame_rate: u32,
        duration_seconds: f32,
        effect_source: &str,
    ) -> Result<BrowserSession, JsValue> {
        validate_session_values(pixel_count, frame_rate, duration_seconds)?;
        let effect = compile_one_effect(effect_source)?;
        let character_positions = default_character_positions(pixel_count);
        let document = DocumentId::new(uuid::Uuid::nil(), "browser-demo.donder".into());
        let project_source = SourceIdentity::from_document(document, "demo".into());
        let project_object = ObjectIdentity::from(project_source.clone());
        let setup_object = project_object.owned(OwnedObjectSlot::Setup);
        let setup_id = SetupId(setup_object.clone());
        let layout_id = LayoutId(setup_object.owned(OwnedObjectSlot::Layout));
        let patch_id = PatchId(setup_object.owned(OwnedObjectSlot::Patch));
        let fixture_definition_id = FixtureDefinitionId(SourceIdentity::from_document(
            project_source.document_id().clone(),
            "page-text".into(),
        ));
        let effect_definition_id = EffectDefinitionId(SourceIdentity::from_document(
            project_source.document_id().clone(),
            effect.name().as_str().to_owned(),
        ));
        let effect_id = EffectInstId(1);
        let fixture_definition = fixture_definition_at_positions(&character_positions);
        let layout = Layout {
            id: layout_id.clone(),
            fixtures: vec![LayoutFixture {
                id: FixtureInstanceId(1),
                name: "Page text".into(),
                kind: LayoutFixtureKind::Fixture {
                    definition: ValueSource::Reference(fixture_definition_id.clone()),
                    transform: FixtureTransform::default(),
                },
            }],
        };
        let setup = Setup {
            id: setup_id,
            layout: ValueSource::Inline(Box::new(layout)),
            patch: ValueSource::Inline(Box::new(Patch {
                id: patch_id,
                routes: Vec::new(),
            })),
            controllers: Vec::<
                ValueSource<Box<donder_language::controller::Controller>, ControllerId>,
            >::new(),
        };
        let duration =
            DonderDuration(Duration::try_from_secs_f32(duration_seconds).map_err(|_| {
                JsValue::from_str("Sequence duration is outside the supported range.")
            })?);
        let mut definitions = ProjectDefinitionStores::default();
        definitions
            .fixtures
            .definitions
            .insert(fixture_definition_id.clone(), fixture_definition);
        let data = ProjectData {
            root: ProjectRoot {
                id: ProjectId(project_source.clone()),
                setup: ValueSource::Inline(Box::new(setup)),
                sequences: Vec::new(),
            },
            setups: IndexMap::new(),
            layouts: IndexMap::new(),
            patches: IndexMap::new(),
            controllers: IndexMap::new(),
            sequences: IndexMap::new(),
            definitions,
        };
        let mut project = DonderProject::try_new(data).map_err(|error| {
            JsValue::from_str(&format!("Invalid browser demo project: {error}"))
        })?;
        let sequence_id = donder_language::ownership::edit::add_sequence(
            &mut project,
            duration.clone(),
            frame_rate,
            Color::BLACK,
        )
        .map_err(|error| JsValue::from_str(&error))?;
        let definition = EffectDefinition::custom(effect_definition_id.clone(), effect);
        project
            .apply_edits([ProjectEdit::SetEffectDefinition {
                id: effect_definition_id.clone(),
                value: definition,
            }])
            .map_err(|error| JsValue::from_str(&error))?;
        let mut sequence = project
            .sequence(&sequence_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("The demo sequence was not created."))?;
        let target = FixtureTarget {
            layout: layout_id,
            fixture: FixtureInstanceId(1),
        };
        sequence.effects.push(EffectInst {
            id: effect_id.clone(),
            layer_id: SequenceLayerId(0),
            start: DonderTime::from_nanos(0),
            duration,
            target: target.clone(),
            scope: EffectScope::WholeTarget,
            definition: EffectRef::Custom(effect_definition_id.clone()),
            param_overrides: IndexMap::new(),
        });
        project
            .replace_sequence(&sequence_id, sequence)
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&project, &sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The demo sequence could not be prepared."))?
            .into_playback();
        Ok(Self {
            project,
            sequence_id,
            fixture_definition_id,
            effect_definition_id,
            target,
            effect_id,
            next_effect_id: 2,
            duration_seconds,
            pixel_count,
            character_positions,
            revision: 0,
            playback,
        })
    }

    /// Evaluate the current prepared sequence at an absolute playback time.
    #[wasm_bindgen(js_name = render)]
    pub fn render(&mut self, seconds: f32) -> Result<JsValue, JsValue> {
        let sample_time = sample_time_from_seconds_f32(seconds).map_err(|_| {
            JsValue::from_str("Playback time must be a finite non-negative supported value.")
        })?;
        let frame = self.playback.evaluate(sample_time);
        let view = FrameView {
            revision: self.revision,
            seconds,
            pixels: frame
                .colors()
                .iter()
                .map(|color| PixelView {
                    red: color.red,
                    green: color.green,
                    blue: color.blue,
                })
                .collect(),
        };
        js_value(&view)
    }

    /// Replace the source for the demo effect, compile it, and prepare atomically.
    #[wasm_bindgen(js_name = setEffectSource)]
    pub fn set_effect_source(&mut self, source: &str) -> Result<JsValue, JsValue> {
        let effect = match compile_effects(source) {
            Ok(mut definitions) if definitions.len() == 1 => definitions.remove(0),
            Ok(_) => {
                return Err(JsValue::from_str(
                    "Effect source must contain exactly one effect declaration.",
                ));
            }
            Err(diagnostics) => {
                return js_value(&CompileView {
                    definitions: Vec::new(),
                    diagnostics: diagnostics_view(diagnostics),
                });
            }
        };
        if effect.name().as_str() != self.effect_definition_id.0.object() {
            return Err(JsValue::from_str(
                "The replacement effect must keep the original declaration name.",
            ));
        }
        let mut candidate = self.project.clone();
        candidate
            .apply_edits([ProjectEdit::SetEffectDefinition {
                id: self.effect_definition_id.clone(),
                value: EffectDefinition::custom(self.effect_definition_id.clone(), effect),
            }])
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.revision = revision;
        js_value(&CompileView {
            definitions: vec![self.effect_definition_id.0.object().to_owned()],
            diagnostics: Vec::new(),
        })
    }

    /// Add a compiled DSL effect as a new timeline clip.
    #[wasm_bindgen(js_name = addEffectSource)]
    pub fn add_effect_source(
        &mut self,
        source: &str,
        start_seconds: f32,
        duration_seconds: f32,
    ) -> Result<JsValue, JsValue> {
        let start = DonderTime::try_from_seconds_f32(start_seconds).map_err(|_| {
            JsValue::from_str("Effect start must be finite, non-negative, and supported.")
        })?;
        let duration = DonderDuration::try_from_seconds_f32(duration_seconds).map_err(|_| {
            JsValue::from_str("Effect duration must be finite, non-negative, and supported.")
        })?;
        if duration.is_zero() {
            return Err(JsValue::from_str(
                "Effect duration must be greater than zero.",
            ));
        }
        let effect = match compile_effects(source) {
            Ok(mut definitions) if definitions.len() == 1 => definitions.remove(0),
            Ok(_) => {
                return Err(JsValue::from_str(
                    "Effect source must contain exactly one effect declaration.",
                ));
            }
            Err(diagnostics) => {
                return js_value(&CompileView {
                    definitions: Vec::new(),
                    diagnostics: diagnostics_view(diagnostics),
                });
            }
        };
        let name = effect.name().as_str().to_owned();
        let definition_id = EffectDefinitionId(SourceIdentity::from_document(
            self.effect_definition_id.0.document_id().clone(),
            name.clone(),
        ));
        if self
            .project
            .definitions()
            .effects
            .definitions
            .contains_key(&definition_id)
        {
            return Err(JsValue::from_str(
                "An effect with that source name already exists in this demo.",
            ));
        }
        let effect_id = EffectInstId(self.next_effect_id);
        let mut candidate = self.project.clone();
        candidate
            .apply_edits([ProjectEdit::SetEffectDefinition {
                id: definition_id.clone(),
                value: EffectDefinition::custom(definition_id.clone(), effect),
            }])
            .map_err(|error| JsValue::from_str(&error))?;
        let mut sequence = candidate
            .sequence(&self.sequence_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
        sequence.effects.push(EffectInst {
            id: effect_id.clone(),
            layer_id: SequenceLayerId(0),
            start,
            duration,
            target: self.target.clone(),
            scope: EffectScope::WholeTarget,
            definition: EffectRef::Custom(definition_id),
            param_overrides: IndexMap::new(),
        });
        candidate
            .replace_sequence(&self.sequence_id, sequence)
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        let next_effect_id = self
            .next_effect_id
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The effect identity space is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.revision = revision;
        self.next_effect_id = next_effect_id;
        js_value(&EffectCreatedView {
            id: effect_id.0,
            name,
            revision,
        })
    }

    /// Move or resize one timeline effect clip and rebuild prepared playback.
    #[wasm_bindgen(js_name = setEffectWindow)]
    pub fn set_effect_window(
        &mut self,
        effect_id: u32,
        start_seconds: f32,
        duration_seconds: f32,
    ) -> Result<(), JsValue> {
        let start = DonderTime::try_from_seconds_f32(start_seconds).map_err(|_| {
            JsValue::from_str("Effect start must be finite, non-negative, and supported.")
        })?;
        let duration = DonderDuration::try_from_seconds_f32(duration_seconds).map_err(|_| {
            JsValue::from_str("Effect duration must be finite, non-negative, and supported.")
        })?;
        if duration.is_zero() {
            return Err(JsValue::from_str(
                "Effect duration must be greater than zero.",
            ));
        }
        let mut candidate = self.project.clone();
        let mut sequence = candidate
            .sequence(&self.sequence_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
        let effect = sequence
            .effects
            .iter_mut()
            .find(|effect| effect.id.0 == effect_id)
            .ok_or_else(|| JsValue::from_str("The requested effect clip was not found."))?;
        effect.start = start;
        effect.duration = duration;
        candidate
            .replace_sequence(&self.sequence_id, sequence)
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.revision = revision;
        Ok(())
    }

    /// Remove one effect clip from the timeline.
    #[wasm_bindgen(js_name = deleteEffect)]
    pub fn delete_effect(&mut self, effect_id: u32) -> Result<(), JsValue> {
        let mut candidate = self.project.clone();
        let mut sequence = candidate
            .sequence(&self.sequence_id)
            .cloned()
            .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
        let original_len = sequence.effects.len();
        sequence.effects.retain(|effect| effect.id.0 != effect_id);
        if sequence.effects.len() == original_len {
            return Err(JsValue::from_str(
                "The requested effect clip was not found.",
            ));
        }
        candidate
            .replace_sequence(&self.sequence_id, sequence)
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.revision = revision;
        Ok(())
    }

    /// Apply a typed desktop sequence edit through the browser bridge.
    /// The browser supports timeline edits that fit its one-fixture demo model;
    /// the exhaustive match keeps newly added command variants visible here.
    #[wasm_bindgen(js_name = applyEdit)]
    pub fn apply_edit(&mut self, edit: JsValue) -> Result<(), JsValue> {
        let edit: SequenceGuiEdit = serde_wasm_bindgen::from_value(edit).map_err(|error| {
            JsValue::from_str(&format!("Invalid sequence edit command: {error}"))
        })?;
        match edit {
            SequenceGuiEdit::MoveEffect {
                id,
                start_seconds,
                target,
            } => {
                if target.is_some_and(|target| target.fixture != 1) {
                    return Err(JsValue::from_str(
                        "The browser demo has one fixture target (fixture 1).",
                    ));
                }
                let duration_seconds = self
                    .playback
                    .sequence()
                    .clip(id)
                    .map(|clip| clip.duration().as_ticks() as f32 / 1_000_000.0)
                    .ok_or_else(|| JsValue::from_str("The requested effect clip was not found."))?;
                self.set_effect_window(id, start_seconds, duration_seconds)
            }
            SequenceGuiEdit::ResizeEffect {
                id,
                start_seconds,
                duration_seconds,
            } => self.set_effect_window(id, start_seconds, duration_seconds),
            SequenceGuiEdit::DeleteEffect { id } => self.delete_effect(id),
            SequenceGuiEdit::SetDuration { .. }
            | SequenceGuiEdit::SetAudio { .. }
            | SequenceGuiEdit::AddEffect { .. }
            | SequenceGuiEdit::CreateLayer { .. }
            | SequenceGuiEdit::CreateLayerAt { .. }
            | SequenceGuiEdit::RenameLayer { .. }
            | SequenceGuiEdit::SetLayerColor { .. }
            | SequenceGuiEdit::SetLayerEnabled { .. }
            | SequenceGuiEdit::SetEffectLayer { .. }
            | SequenceGuiEdit::ChangeEffectDefinition { .. }
            | SequenceGuiEdit::RetargetEffect { .. }
            | SequenceGuiEdit::SetEffectScope { .. }
            | SequenceGuiEdit::UpdateEffectParam { .. }
            | SequenceGuiEdit::AddGraphOperatorNode { .. }
            | SequenceGuiEdit::MoveGraphNodes { .. }
            | SequenceGuiEdit::DeleteGraphItems { .. }
            | SequenceGuiEdit::ConnectGraphNodes { .. }
            | SequenceGuiEdit::ReconnectGraphEdge { .. }
            | SequenceGuiEdit::UpdateGraphOperatorParam { .. }
            | SequenceGuiEdit::AddAutomationClip { .. }
            | SequenceGuiEdit::CreateAndBindAutomationClip { .. }
            | SequenceGuiEdit::MoveAutomationClip { .. }
            | SequenceGuiEdit::ResizeAutomationClip { .. }
            | SequenceGuiEdit::UpdateAutomationCurve { .. }
            | SequenceGuiEdit::UpdateAutomationParamMapping { .. }
            | SequenceGuiEdit::DeleteAutomationClip { .. }
            | SequenceGuiEdit::BindAutomationParam { .. }
            | SequenceGuiEdit::UnbindAutomationParam { .. }
            | SequenceGuiEdit::RebindDetachedAutomation { .. }
            | SequenceGuiEdit::DiscardDetachedAutomation { .. }
            | SequenceGuiEdit::CreateMarkCollection { .. }
            | SequenceGuiEdit::RenameMarkCollection { .. }
            | SequenceGuiEdit::DeleteMarkCollection { .. }
            | SequenceGuiEdit::SetMarkCollectionColor { .. }
            | SequenceGuiEdit::AddMark { .. }
            | SequenceGuiEdit::MoveMark { .. }
            | SequenceGuiEdit::ReassignMarkCollection { .. }
            | SequenceGuiEdit::DeleteMark { .. } => Err(JsValue::from_str(
                "This sequence edit is not supported by the browser demo session.",
            )),
        }
    }

    /// Change the number of text-character pixels and rebuild prepared geometry.
    #[wasm_bindgen(js_name = setPixelCount)]
    pub fn set_pixel_count(&mut self, pixel_count: u32) -> Result<(), JsValue> {
        if pixel_count == 0 || pixel_count > 100_000 {
            return Err(JsValue::from_str(
                "Pixel count must be between 1 and 100000.",
            ));
        }
        let mut candidate = self.project.clone();
        candidate
            .apply_edits([ProjectEdit::SetFixtureDefinition {
                id: self.fixture_definition_id.clone(),
                value: fixture_definition_at_positions(&default_character_positions(pixel_count)),
            }])
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.pixel_count = pixel_count;
        self.character_positions = default_character_positions(pixel_count);
        self.revision = revision;
        Ok(())
    }

    /// Set the stage-space position of every character pixel, in output order.
    /// Coordinates are normalized page-space values, typically derived from each
    /// character's measured rectangle relative to the animated page bounds.
    #[wasm_bindgen(js_name = setCharacterPositions)]
    pub fn set_character_positions(&mut self, positions: JsValue) -> Result<(), JsValue> {
        let positions: Vec<[f32; 2]> =
            serde_wasm_bindgen::from_value(positions).map_err(|error| {
                JsValue::from_str(&format!(
                    "Character positions must be an array of [x, y] pairs: {error}"
                ))
            })?;
        if positions.is_empty() || positions.len() > 100_000 {
            return Err(JsValue::from_str(
                "Character position count must be between 1 and 100000.",
            ));
        }
        if positions
            .iter()
            .flatten()
            .any(|coordinate| !coordinate.is_finite() || coordinate.abs() > 2_000.0)
        {
            return Err(JsValue::from_str(
                "Character positions must be finite normalized coordinates within +/-2000.",
            ));
        }
        let pixel_count = u32::try_from(positions.len()).map_err(|_| {
            JsValue::from_str("Character position count exceeds the supported range.")
        })?;
        let mut candidate = self.project.clone();
        candidate
            .apply_edits([ProjectEdit::SetFixtureDefinition {
                id: self.fixture_definition_id.clone(),
                value: fixture_definition_at_positions(&positions),
            }])
            .map_err(|error| JsValue::from_str(&error))?;
        let playback = prepare(&candidate, &self.sequence_id, PrepareOutputs::All)
            .ok_or_else(|| JsValue::from_str("The edited sequence could not be prepared."))?
            .into_playback();
        let revision = self
            .revision
            .checked_add(1)
            .ok_or_else(|| JsValue::from_str("The demo revision counter is exhausted."))?;
        self.project = candidate;
        self.playback = playback;
        self.pixel_count = pixel_count;
        self.character_positions = positions;
        self.revision = revision;
        Ok(())
    }

    #[wasm_bindgen(js_name = pixelCount)]
    pub fn pixel_count(&self) -> u32 {
        self.pixel_count
    }

    #[wasm_bindgen(js_name = characterPositions)]
    pub fn character_positions(&self) -> Result<JsValue, JsValue> {
        js_value(&self.character_positions)
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

    #[wasm_bindgen(js_name = effectId)]
    pub fn effect_id(&self) -> u32 {
        self.effect_id.0
    }
}

fn validate_session_values(
    pixel_count: u32,
    frame_rate: u32,
    duration_seconds: f32,
) -> Result<(), JsValue> {
    if pixel_count == 0 || pixel_count > 100_000 {
        return Err(JsValue::from_str(
            "Pixel count must be between 1 and 100000.",
        ));
    }
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

fn compile_one_effect(source: &str) -> Result<donder_language::dsl::CompiledEffect, JsValue> {
    match compile_effects(source) {
        Ok(mut definitions) if definitions.len() == 1 => Ok(definitions.remove(0)),
        Ok(_) => Err(JsValue::from_str(
            "Effect source must contain exactly one effect declaration.",
        )),
        Err(diagnostics) => {
            let messages = diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n");
            Err(JsValue::from_str(&messages))
        }
    }
}

fn default_character_positions(pixel_count: u32) -> Vec<[f32; 2]> {
    (0..pixel_count)
        .map(|index| [index as f32 / 100.0, 0.0])
        .collect()
}

fn fixture_definition_at_positions(positions: &[[f32; 2]]) -> FixtureDefinition {
    FixtureDefinition {
        elements: positions
            .iter()
            .enumerate()
            .map(|(index, [x, y])| FixtureElement {
                id: FixtureElementId(index as u32 + 1),
                name: format!("Character {}", index + 1),
                transform: FixtureTransform {
                    position: Point3 {
                        x: Distance::from_meters(*x),
                        y: Distance::from_meters(*y),
                        z: Distance::ZERO,
                    },
                    ..FixtureTransform::default()
                },
                diameter: donder_language::values::DistanceSpan::from_meters(0.01),
                reverse: false,
                shape: FixtureShape::Pixel,
            })
            .collect(),
    }
}
