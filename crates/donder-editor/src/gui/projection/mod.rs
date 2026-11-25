mod params;
pub(super) use params::clip_params;
use params::sequence_node_automation;
pub(super) use params::{
    composition_graph, curve_library, gradient_library, invocation_params, layers_to_gui,
};

/// The inspector's view of the effect clips of `clip_ids` in the requested
/// sequence or console, in the requested order; ids it does not have are
/// skipped.
pub(super) fn sequence_clip_details(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    clip_ids: &[u32],
) -> Result<Vec<SequenceClipDetails>, String> {
    let sequence = (resolved.kind == donder_model::ObjectKind::Sequence)
        .then(|| resolved.view::<donder_model::Sequence>(session).object());
    let composition = match sequence {
        Some(sequence) => &sequence.composition,
        None => {
            &resolved
                .view::<donder_model::Console>(session)
                .object()
                .composition
        }
    };
    Ok(clip_ids
        .iter()
        .filter_map(|id| composition.clip(donder_model::ClipId(*id)))
        .filter_map(|clip| {
            let effect = clip.effect()?;
            Some(SequenceClipDetails {
                id: clip.id.0,
                effect_reference: effect_ref_to_gui(session, &effect.definition),
                params: match sequence {
                    Some(sequence) => clip_params(session, sequence, clip, effect),
                    None => {
                        invocation_params(session, &effect.definition, &effect.params, |_, _| None)
                    }
                },
            })
        })
        .collect())
}

/// A sequence's or console's clip; `always` are a console's.
pub(super) fn project_clip(
    session: &ProjectSession,
    always: &[donder_model::Always],
    clip: &donder_model::Clip,
) -> GuiClip {
    GuiClip {
        id: clip.id.0,
        name: clip.name.as_str().to_string(),
        description: clip.description.clone(),
        target: target_to_gui(clip.target),
        target_label: target_label(session, clip.target),
        content: match &clip.content {
            donder_model::ClipContent::Effect(effect) => GuiClipContent::Effect {
                layer_id: effect.layer.0,
                scope: super::model::scope_to_gui(effect.scope),
                effect: effect_ref_to_gui(session, &effect.definition),
                effect_name: session
                    .project()
                    .reusable::<donder_model::ScriptDefinition>()[&effect.definition]
                    .name()
                    .to_string(),
            },
            donder_model::ClipContent::Envelope(envelope) => GuiClipContent::Envelope {
                curve: super::model::curve_points(&envelope.curve),
                bindings: envelope
                    .bindings
                    .iter()
                    .map(|binding| SequenceAutomationBinding {
                        target: automation_target_to_gui(&binding.target),
                        window: binding.window.into(),
                    })
                    .collect(),
            },
        },
        placement: match &clip.placement {
            donder_model::Placement::Timed { start, duration } => GuiPlacement::Timed {
                start_seconds: start.as_seconds_f64(),
                duration_seconds: duration.as_seconds_f64(),
            },
            donder_model::Placement::Played(values) => GuiPlacement::Played {
                values: values
                    .iter()
                    .map(super::console::played_value_to_gui)
                    .collect(),
            },
        },
        always_length_seconds: always
            .iter()
            .find(|always| always.clip == clip.id)
            .map(|always| always.length.as_seconds_f64()),
    }
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
    let targets = layout_lanes(&session.project().layout());
    let clips = sequence
        .composition
        .clips
        .iter()
        .filter(|clip| include(clip))
        .map(|clip| project_clip(session, &[], clip))
        .collect();
    let composition_graph = composition_graph(
        session,
        &sequence.composition.layers,
        &sequence.composition.graph,
        |node, param, value| sequence_node_automation(sequence, node, param, value),
    );
    GuiDocument::Sequence {
        document: SequenceGuiDocument {
            description: sequence.description.clone(),
            source_ref: resolved.source_ref(),
            duration_seconds: sequence.duration.as_seconds_f64(),
            frame_rate: sequence.frame_rate as f32,
            audio: sequence_audio(session, &sequence.audio),
            mark_collections: mark_collections_to_gui(&sequence.composition.marks),
            derived_data: super::audio::derived_status(session, sequence),
            targets,
            effect_definitions: effect_definitions(session),
            curve_library: curve_library(session),
            gradient_library: gradient_library(session),
            layers: layers_to_gui(&sequence.composition.layers),
            clips,
            composition_graph,
        },
    }
}

/// The mark collections of a sequence or console.
pub(super) fn mark_collections_to_gui(
    collections: &[donder_model::MarkCollection],
) -> Vec<SequenceMarkCollection> {
    collections
        .iter()
        .map(|collection| SequenceMarkCollection {
            id: collection.id.0,
            name: collection.name.as_str().to_string(),
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
        .collect()
}

/// The id of a sequence's or console's clip, as the editor sends it.
pub(crate) fn automation_target_to_gui(target: &AutomationTarget) -> SequenceAutomationTarget {
    match target {
        AutomationTarget::ClipParam { clip_id, param } => SequenceAutomationTarget::ClipParam {
            clip_id: clip_id.0,
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
        exists: asset.absolute_path.is_file(),
    })
}

/// The timeline lanes: the layout root walked depth-first, with each item's
/// depth. A member of several groups has a lane under each.
pub(crate) fn lane_walk(layout: &donder_model::Layout) -> Vec<(&donder_model::LayoutFixture, u32)> {
    fn push<'a>(
        layout: &'a donder_model::Layout,
        members: &[donder_model::FixtureId],
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

pub(super) fn target_to_gui(target: donder_model::FixtureId) -> FixtureTarget {
    FixtureTarget { fixture: target.0 }
}

/// The name of the fixture or group a validated clip targets.
pub(super) fn target_label(session: &ProjectSession, target: donder_model::FixtureId) -> String {
    session
        .project()
        .layout()
        .object()
        .fixture(target)
        .map(|fixture| fixture.name.as_str().to_string())
        .unwrap_or_else(|| unreachable!("validation proved every clip target"))
}

pub(super) fn effect_ref_to_gui(
    session: &ProjectSession,
    id: &ScriptId,
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
        .reusable::<donder_model::ScriptDefinition>()
        .iter()
        .filter(|(_, definition)| definition.kind() == DeclarationKind::Effect)
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
use donder_language::compiler::DeclarationKind;
use donder_model::AutomationTarget;
use donder_model::ScriptId;
use donder_project_io::ProjectSession;

mod spatial;
use super::ResolvedGuiObject;
use crate::dto::{
    FixtureTarget, GuiClip, GuiClipContent, GuiDocument, GuiPlacement, SequenceAudio,
    SequenceAutomationBinding, SequenceAutomationTarget, SequenceClipDetails,
    SequenceEffectDefinition, SequenceEffectReference, SequenceGuiDocument, SequenceLane,
    SequenceLaneKind, SequenceMarkCollection,
};
pub(super) use spatial::{project_fixture, project_layout};
