mod params;
use params::{
    curve_library, effect_params, gradient_library, graph_node_id,
    graph_operator_definition_to_gui, sequence_composition_graph_node,
};

pub(super) fn project_sequence(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = SequenceId(resolved.object_identity());
    let Some(sequence) = session.project.sequence(&id) else {
        return blocked(
            "Sequence is not available in the checked project model.",
            vec![gui_diagnostic(
                resolved.identity.document().as_ref(),
                "gui.sequence",
                "Sequence is not available in the checked project model.",
            )],
        );
    };
    let lanes = active_layout(session)
        .map(|layout| {
            let mut lanes = Vec::new();
            push_lanes(&layout.fixtures, 0, &mut lanes);
            lanes
        })
        .unwrap_or_default();
    let effects = sequence
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| SequenceEffect {
            index: index as u32,
            id: effect.id.0,
            name: effect.name.as_str().to_string(),
            description: effect.description.clone(),
            layer_id: effect.layer_id.0,
            start_seconds: effect.start.as_seconds_f32(),
            duration_seconds: effect.duration.as_seconds_f32(),
            target: effect_target(&effect.target),
            target_label: effect_target_label(session, &effect.target),
            scope: match effect.scope {
                EffectScope::PerFixture => SequenceEffectScope::PerFixture,
                EffectScope::WholeTarget => SequenceEffectScope::WholeTarget,
            },
            effect: session
                .project
                .definitions()
                .effects
                .resolve(&effect.definition)
                .map(|definition| definition.display_name.clone())
                .unwrap_or_else(|| "Missing effect".to_string()),
            effect_reference: effect_ref_to_gui(&effect.definition),
            params: effect_params(session, sequence, effect),
            kind: SequenceTimelineClipKind::Effect,
        })
        .collect();
    let composition_graph = SequenceCompositionGraph {
        id: 0,
        operator_catalog: session
            .project
            .definitions()
            .operators
            .definitions
            .iter()
            .filter(|(id, _)| session.source.is_project_owned(id.0.document_id()))
            .map(|(id, definition)| {
                graph_operator_definition_to_gui(OperatorRef::Custom(id.clone()), definition)
            })
            .collect(),
        nodes: sequence
            .composition_graph
            .nodes
            .iter()
            .map(|node| sequence_composition_graph_node(session, sequence, node))
            .collect(),
        edges: sequence
            .composition_graph
            .edges
            .iter()
            .map(|edge| SequenceGraphEdge {
                from_node: graph_node_id(&edge.from),
                from_port: edge.from_port.0.clone(),
                to_node: graph_node_id(&edge.to),
                to_port: edge.to_port.0.clone(),
            })
            .collect(),
    };
    GuiDocument::Sequence {
        document: SequenceGuiDocument {
            description: sequence.description.clone(),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            duration_seconds: sequence.duration.as_seconds_f32(),
            frame_rate: sequence.frame_rate as f32,
            audio: sequence_audio(session, resolved.identity.document_id(), &sequence.audio),
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
                        .map(|mark| mark.as_seconds_f32())
                        .collect(),
                })
                .collect(),
            lanes,
            effect_definitions: effect_definitions(session),
            curve_library: curve_library(session, resolved.identity.document_id()),
            gradient_library: gradient_library(session, resolved.identity.document_id()),
            layers: sequence
                .layers
                .iter()
                .map(|layer| SequenceLayer {
                    id: layer.id.0,
                    name: layer.name.as_str().to_string(),
                    description: layer.description.clone(),
                    color: layer.color.to_hex(),
                    enabled: layer.enabled,
                    is_default: layer.id.0 == 0,
                })
                .collect(),
            effects,
            composition_graph,
            automation_clips: automation_clips(sequence),
        },
    }
}

fn automation_clips(sequence: &donder_language::sequence::Sequence) -> Vec<SequenceAutomationClip> {
    sequence
        .automation_clips
        .iter()
        .map(|clip| SequenceAutomationClip {
            id: clip.id.0,
            start_seconds: clip.start.as_seconds_f32(),
            duration_seconds: clip.duration.as_seconds_f32(),
            row_target: effect_target(&clip.row_target),
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
                })
                .collect(),
            detached_bindings: clip
                .detached_bindings
                .iter()
                .map(|binding| SequenceDetachedAutomationBinding {
                    target: automation_target_to_gui(&binding.target),
                    reason: match binding.reason {
                        AutomationDetachmentReason::TargetDeleted => {
                            SequenceAutomationDetachmentReason::TargetDeleted
                        }
                        AutomationDetachmentReason::DefinitionChanged => {
                            SequenceAutomationDetachmentReason::DefinitionChanged
                        }
                    },
                })
                .collect(),
        })
        .collect()
}

fn automation_target_to_gui(target: &AutomationTarget) -> SequenceAutomationTarget {
    match target {
        AutomationTarget::EffectParam { effect_id, param } => {
            SequenceAutomationTarget::EffectParam {
                effect_id: effect_id.0,
                param: param.as_str().to_string(),
            }
        }
        AutomationTarget::CompositionNodeParam { node_id, param } => {
            SequenceAutomationTarget::CompositionNodeParam {
                node_id: graph_node_id(node_id),
                param: param.as_str().to_string(),
            }
        }
    }
}

pub(super) fn active_layout(session: &ProjectSession) -> Option<&Layout> {
    session
        .project
        .setup(session.project.root().setup.id())
        .and_then(|setup| session.project.layout(setup.layout.id()))
}

fn sequence_audio(
    session: &ProjectSession,
    document: &donder_language::identity::DocumentId,
    audio: &donder_language::sequence::SequenceAudio,
) -> Option<SequenceAudio> {
    let donder_language::sequence::SequenceAudio::Asset(id) = audio else {
        return None;
    };
    session
        .source
        .referenced_assets
        .iter()
        .find(|asset| asset.id == *id && asset.module_id == document.module_id())
        .map(|asset| SequenceAudio {
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

/// Lanes in layout order with each lane's depth, so editors can show the tree.
fn push_lanes(
    fixtures: &[donder_language::layout::LayoutFixture],
    depth: u32,
    lanes: &mut Vec<SequenceLane>,
) {
    for fixture in fixtures {
        let children = match &fixture.kind {
            donder_language::layout::LayoutFixtureKind::Group { children } => Some(children),
            donder_language::layout::LayoutFixtureKind::Fixture { .. } => None,
        };
        lanes.push(SequenceLane {
            target: FixtureTarget {
                fixture: fixture.id.0,
            },
            label: fixture.name.as_str().to_string(),
            kind: if children.is_some() {
                SequenceLaneKind::Group
            } else {
                SequenceLaneKind::Fixture
            },
            depth,
        });
        if let Some(children) = children {
            push_lanes(children, depth + 1, lanes);
        }
    }
}

fn effect_target(target: &DomainFixtureTarget) -> FixtureTarget {
    FixtureTarget {
        fixture: target.fixture.0,
    }
}

fn effect_target_label(session: &ProjectSession, target: &DomainFixtureTarget) -> String {
    session
        .project
        .layout(&target.layout)
        .and_then(|layout| layout.fixture(target.fixture))
        .map(|fixture| fixture.name.as_str().to_string())
        .unwrap_or_else(|| format!("Missing fixture {}", target.fixture.0))
}

fn effect_ref_to_gui(reference: &EffectRef) -> SequenceEffectReference {
    match reference {
        EffectRef::Custom(id) => SequenceEffectReference::Custom {
            module_id: id.0.module_id().to_string(),
            path: id.0.document().to_string(),
            effect_name: id.0.object().to_string(),
        },
    }
}

fn effect_definitions(session: &ProjectSession) -> Vec<SequenceEffectDefinition> {
    session
        .project
        .definitions()
        .effects
        .definitions
        .iter()
        .map(|(id, definition)| {
            let source = effect_ref_to_gui(&EffectRef::Custom(id.clone()));
            SequenceEffectDefinition {
                name: definition.display_name.clone(),
                description: definition.description().map(str::to_string),
                effect: source,
                import_path: Some(id.0.document().to_string()),
                params: params::definition_params_to_gui(definition.params()),
            }
        })
        .collect()
}
use donder_language::effect::{EffectRef, EffectScope};
use donder_language::layout::{FixtureTarget as DomainFixtureTarget, Layout};
use donder_language::operator::OperatorRef;
use donder_language::sequence::{AutomationDetachmentReason, AutomationTarget, SequenceId};
use donder_project_io::ProjectSession;

mod spatial;
use super::{ResolvedGuiObject, blocked, gui_diagnostic};
use crate::dto::{
    FixtureTarget, GuiDocument, SequenceAudio, SequenceAutomationBinding, SequenceAutomationClip,
    SequenceAutomationDetachmentReason, SequenceAutomationTarget, SequenceCompositionGraph,
    SequenceCurvePoint, SequenceDetachedAutomationBinding, SequenceEffect,
    SequenceEffectDefinition, SequenceEffectReference, SequenceEffectScope, SequenceGraphEdge,
    SequenceGuiDocument, SequenceLane, SequenceLaneKind, SequenceLayer, SequenceMarkCollection,
    SequenceTimelineClipKind,
};
pub(super) use spatial::{project_fixture, project_layout};
