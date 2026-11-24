mod params;
pub(super) use params::clip_params;
use params::sequence_node_automation;
pub(super) use params::{
    composition_graph, curve_library, gradient_library, invocation_params, layers_to_gui,
};

/// The inspector's view of `clip_ids` in the requested sequence, in the
/// requested order; ids the sequence does not have are skipped.
pub(super) fn sequence_clip_details(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    clip_ids: &[u32],
) -> Result<Vec<SequenceClipDetails>, String> {
    let sequence = resolved.view::<donder_model::Sequence>(session).object();
    Ok(clip_ids
        .iter()
        .filter_map(|id| sequence.clips.iter().find(|clip| clip.id.0 == *id))
        .map(|clip| SequenceClipDetails {
            id: clip.id.0,
            effect_reference: effect_ref_to_gui(session, &clip.definition),
            params: clip_params(session, sequence, clip),
        })
        .collect())
}

pub(super) fn project_sequence(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    project_sequence_clips(session, resolved, &|_| true)
}

/// The sequence document with only the clips `include` accepts.
pub(super) fn project_sequence_clips(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    include: &dyn Fn(&std::sync::Arc<donder_model::Clip>) -> bool,
) -> GuiDocument {
    let sequence = resolved.view::<donder_model::Sequence>(session).object();
    let lanes = layout_lanes(&session.project().active_layout());
    let clips = sequence
        .clips
        .iter()
        .enumerate()
        .filter(|(_, clip)| include(clip))
        .map(|(index, clip)| SequenceClip {
            index: index as u32,
            id: clip.id.0,
            name: clip.name.as_str().to_string(),
            description: clip.description.clone(),
            layer_id: clip.layer_id.0,
            start_seconds: clip.start.as_seconds_f64(),
            duration_seconds: clip.duration.as_seconds_f64(),
            target: target_to_gui(&clip.target),
            target_label: target_label(session, &clip.target),
            scope: match clip.scope {
                ClipScope::PerFixture => SequenceClipScope::PerFixture,
                ClipScope::WholeTarget => SequenceClipScope::WholeTarget,
            },
            effect: session
                .project()
                .reusable::<donder_model::EffectDefinition>()[&clip.definition]
                .name()
                .to_string(),
        })
        .collect();
    let composition_graph = composition_graph(
        session,
        &sequence.layers,
        &sequence.composition_graph,
        |node, param, value| sequence_node_automation(sequence, node, param, value),
    );
    GuiDocument::Sequence {
        document: SequenceGuiDocument {
            description: sequence.description.clone(),
            source_ref: resolved.source_ref(),
            duration_seconds: sequence.duration.as_seconds_f64(),
            frame_rate: sequence.frame_rate as f32,
            audio: sequence_audio(session, &sequence.audio),
            mark_collections: sequence
                .mark_collections
                .iter()
                .map(|collection| SequenceMarkCollection {
                    key: collection.key.name.as_str().to_string(),
                    description: collection.description.clone(),
                    color: collection.display_color.to_hex(),
                    marks_seconds: collection
                        .marks
                        .iter()
                        .map(|mark| mark.time.as_seconds_f64())
                        .collect(),
                    mark_durations_seconds: collection
                        .marks
                        .iter()
                        .map(|mark| mark.duration.as_seconds_f64())
                        .collect(),
                    mark_labels: collection
                        .marks
                        .iter()
                        .map(|mark| mark.label.clone())
                        .collect(),
                })
                .collect(),
            derived_data: super::audio::derived_status(session, sequence),
            lanes,
            effect_definitions: effect_definitions(session),
            curve_library: curve_library(session),
            gradient_library: gradient_library(session),
            layers: layers_to_gui(&sequence.layers),
            clips,
            composition_graph,
            automation_clips: automation_clips(sequence),
        },
    }
}

fn automation_clips(sequence: &donder_model::Sequence) -> Vec<SequenceAutomationClip> {
    sequence
        .automation_clips
        .iter()
        .map(|clip| SequenceAutomationClip {
            id: clip.id.0,
            start_seconds: clip.start.as_seconds_f64(),
            duration_seconds: clip.duration.as_seconds_f64(),
            row_target: target_to_gui(&clip.row_target),
            curve: clip
                .curve
                .points
                .iter()
                .map(|point| SequenceCurvePoint {
                    time: point.position,
                    value: point.value,
                })
                .collect(),
            bindings: clip
                .bindings
                .iter()
                .map(|binding| SequenceAutomationBinding {
                    target: automation_target_to_gui(&binding.target),
                    window: binding.window.into(),
                })
                .collect(),
            detached_bindings: clip
                .detached_bindings
                .iter()
                .map(|binding| SequenceDetachedAutomationBinding {
                    target: automation_target_to_gui(&binding.target),
                })
                .collect(),
        })
        .collect()
}

/// The id of a sequence's or console's clip, as the editor sends it.
pub(super) trait GuiClipId {
    fn to_gui(&self) -> u32;
    fn from_gui(id: u32) -> Self;
}

impl GuiClipId for donder_model::ClipId {
    fn to_gui(&self) -> u32 {
        self.0
    }
    fn from_gui(id: u32) -> Self {
        Self(id)
    }
}

impl GuiClipId for donder_model::ConsoleClipId {
    fn to_gui(&self) -> u32 {
        self.0
    }
    fn from_gui(id: u32) -> Self {
        Self(id)
    }
}

pub(super) fn automation_target_to_gui<Clip: GuiClipId>(
    target: &AutomationTarget<Clip>,
) -> SequenceAutomationTarget {
    match target {
        AutomationTarget::ClipParam { clip_id, param } => SequenceAutomationTarget::ClipParam {
            clip_id: clip_id.to_gui(),
            param: param.as_str().to_string(),
        },
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            SequenceAutomationTarget::CompositionNodeParam {
                node_id: node_id.0,
                param: param.as_str().to_string(),
            }
        }
    }
}

fn sequence_audio(
    session: &ProjectSession,
    audio: &donder_model::SequenceAudio,
) -> Option<SequenceAudio> {
    session.audio_asset(audio).map(|asset| SequenceAudio {
        import_path: asset.relative_path.to_string(),
        resolved_path: asset.absolute_path.to_string(),
        file_name: asset
            .relative_path
            .file_name()
            .map(ToString::to_string)
            .unwrap_or_else(|| asset.relative_path.to_string()),
        exists: asset.absolute_path.is_file(),
    })
}

/// The timeline lanes: the layout root walked depth-first, with each item's
/// depth. A member of several groups has a lane under each.
pub(crate) fn lane_walk(layout: &donder_model::Layout) -> Vec<(&donder_model::LayoutFixture, u32)> {
    fn push<'a>(
        layout: &'a donder_model::Layout,
        members: &[donder_model::FixtureInstanceId],
        depth: u32,
        lanes: &mut Vec<(&'a donder_model::LayoutFixture, u32)>,
    ) {
        for &member in members {
            if let Some(fixture) = layout.fixture(member) {
                lanes.push((fixture, depth));
                let listed = fixture.listed().collect::<Vec<_>>();
                push(layout, &listed, depth + 1, lanes);
            }
        }
    }
    let mut lanes = Vec::new();
    push(layout, &layout.root, 0, &mut lanes);
    lanes
}

/// Lanes with the hierarchy depth and how many lanes share each target.
pub(super) fn layout_lanes(layout: &donder_model::Layout) -> Vec<SequenceLane> {
    let mut lanes = lane_walk(layout)
        .into_iter()
        .map(|(fixture, depth)| SequenceLane {
            target: FixtureTarget {
                fixture: fixture.id.0,
            },
            label: fixture.name.as_str().to_string(),
            kind: match fixture.kind {
                donder_model::LayoutFixtureKind::Group { .. } => SequenceLaneKind::Group,
                donder_model::LayoutFixtureKind::Fixture { .. } => SequenceLaneKind::Fixture,
            },
            depth,
            occurrences: 0,
        })
        .collect::<Vec<_>>();
    let mut counts = std::collections::HashMap::<u32, u32>::new();
    for lane in &lanes {
        *counts.entry(lane.target.fixture).or_default() += 1;
    }
    for lane in &mut lanes {
        lane.occurrences = counts[&lane.target.fixture];
    }
    lanes
}

pub(super) fn target_to_gui(target: &DomainFixtureTarget) -> FixtureTarget {
    FixtureTarget {
        fixture: target.fixture.0,
    }
}

/// The name of the fixture or group a validated clip targets.
pub(super) fn target_label(session: &ProjectSession, target: &DomainFixtureTarget) -> String {
    session
        .project()
        .get::<donder_model::Layout>(target.layout)
        .and_then(|layout| layout.fixture(target.fixture))
        .map(|fixture| fixture.name.as_str().to_string())
        .unwrap_or_else(|| unreachable!("validation proved every clip target"))
}

pub(super) fn effect_ref_to_gui(
    session: &ProjectSession,
    id: &EffectDefinitionId,
) -> SequenceEffectReference {
    let identity = crate::gui::model::declared_identity(session, *id);
    SequenceEffectReference {
        path: identity.document().to_string(),
        effect_name: identity.object().to_string(),
    }
}

pub(super) fn effect_definitions(session: &ProjectSession) -> Vec<SequenceEffectDefinition> {
    session
        .project()
        .reusable::<donder_model::EffectDefinition>()
        .iter()
        .map(|(id, definition)| {
            let source = effect_ref_to_gui(session, id);
            SequenceEffectDefinition {
                name: definition.name().to_string(),
                description: definition.description().map(str::to_string),
                effect: source,
                params: params::definition_params_to_gui(definition.params()),
            }
        })
        .collect()
}
use donder_model::AutomationTarget;
use donder_model::FixtureTarget as DomainFixtureTarget;
use donder_model::{ClipScope, EffectDefinitionId};
use donder_project_io::ProjectSession;

mod spatial;
use super::ResolvedGuiObject;
use crate::dto::{
    FixtureTarget, GuiDocument, SequenceAudio, SequenceAutomationBinding, SequenceAutomationClip,
    SequenceAutomationTarget, SequenceClip, SequenceClipDetails, SequenceClipScope,
    SequenceCurvePoint, SequenceDetachedAutomationBinding, SequenceEffectDefinition,
    SequenceEffectReference, SequenceGuiDocument, SequenceLane, SequenceLaneKind,
    SequenceMarkCollection,
};
pub(super) use spatial::{project_fixture, project_layout};
