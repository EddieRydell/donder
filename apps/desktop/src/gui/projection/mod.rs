mod params;
use params::{
    automation_mapping_to_gui, curve_library, effect_params, gradient_library, graph_node_id,
    graph_operator_definition_to_gui, param_kind, sequence_composition_graph_node,
};

pub(super) fn project_sequence(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = SequenceId(resolved.identity.clone());
    let Some(sequence) = session.project.sequences.get(&id) else {
        return blocked(
            "Sequence is not available in the checked project model.",
            vec![gui_diagnostic(
                resolved.identity.document().as_ref(),
                "gui.sequence",
                "Sequence is not available in the checked project model.",
            )],
        );
    };
    let lanes = active_element_tree(session)
        .map(|tree| {
            tree.nodes
                .iter()
                .map(|(node_id, node)| SequenceLane {
                    target: element_target(*node_id, &node.kind),
                    label: node.name.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    let effects = sequence
        .effects
        .iter()
        .enumerate()
        .map(|(index, effect)| SequenceEffect {
            index: index as u32,
            id: effect.id.0,
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
                .definitions
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
        operator_catalog: BuiltinOperator::ALL
            .iter()
            .map(|builtin| {
                graph_operator_definition_to_gui(
                    OperatorRef::Builtin(*builtin),
                    dawn_language::operator::builtin_operator_definition(*builtin),
                )
            })
            .chain(
                session
                    .project
                    .definitions
                    .operators
                    .definitions
                    .iter()
                    .map(|(id, definition)| {
                        graph_operator_definition_to_gui(
                            OperatorRef::Custom(id.clone()),
                            definition,
                        )
                    }),
            )
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
    let control_channels = match super::controls::channels(session) {
        Ok(channels) => channels,
        Err(message) => return blocked(&message, Vec::new()),
    };
    let control_clips = sequence
        .control_clips
        .iter()
        .map(|clip| SequenceControlClip {
            id: clip.id.0,
            start_seconds: clip.start.as_seconds_f32(),
            duration_seconds: clip.duration.as_seconds_f32(),
            target: super::controls::project_target(&clip.target),
            target_label: effect_target_label(session, clip.target.selection()),
            value: super::controls::project_value(&clip.value),
        })
        .collect();
    GuiDocument::Sequence {
        document: SequenceGuiDocument {
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
                    key: collection.key.name.clone(),
                    name: collection.name.clone(),
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
            curve_library: curve_library(session),
            gradient_library: gradient_library(session),
            layers: sequence
                .layers
                .iter()
                .map(|layer| SequenceLayer {
                    id: layer.id.0,
                    name: layer.name.clone(),
                    color: layer.color.to_hex(),
                    enabled: layer.enabled,
                    is_default: layer.id.0 == 0,
                })
                .collect(),
            effects,
            control_clips,
            control_channels,
            composition_graph,
            automation_clips: automation_clips(sequence),
        },
    }
}

fn automation_clips(sequence: &dawn_language::sequence::Sequence) -> Vec<SequenceAutomationClip> {
    sequence
        .automation_clips
        .iter()
        .map(|clip| SequenceAutomationClip {
            id: clip.id.0,
            start_seconds: clip.start.as_seconds_f32(),
            duration_seconds: clip.duration.as_seconds_f32(),
            anchor_lane_index: clip.anchor_lane_index,
            lane_index: clip.lane_index,
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
                    mapping: automation_mapping_to_gui(&binding.mapping),
                })
                .collect(),
            detached_bindings: clip
                .detached_bindings
                .iter()
                .map(|binding| SequenceDetachedAutomationBinding {
                    target: automation_target_to_gui(&binding.target),
                    mapping: automation_mapping_to_gui(&binding.mapping),
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

pub(super) fn project_layout(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = PreviewLayoutId(resolved.identity.clone());
    let Some(layout) = session.project.preview_layouts.get(&id) else {
        return blocked(
            "Layout is not available in the checked project model.",
            Vec::new(),
        );
    };
    let hierarchy = match super::elements::project_tree(session, &layout.element_tree) {
        Ok(hierarchy) => hierarchy,
        Err(error) => return blocked(error.message(), Vec::new()),
    };
    let mut fixtures = Vec::new();
    for fixture in &layout.props {
        let Some(definition) = session
            .project
            .definitions
            .props
            .definitions
            .get(&fixture.definition)
        else {
            return blocked("Layout fixture definition was not found.", Vec::new());
        };
        let resolved_fixture = ResolvedPreviewProp {
            name: fixture.definition.0.object().to_string(),
            color_model: "rgb".to_string(),
            bulb_diameter_meters: definition.bulb_radius.as_meters_f32() * 2.0,
            geometry_summary: geometry_summary(&definition.geometry),
            render_plan: render_plan(&definition.geometry, definition.bulb_radius),
        };
        fixtures.push(PreviewPropPlacement {
            definition_ref: ResolvedGuiObject {
                identity: fixture.definition.0.clone(),
                kind: SourceObjectKind::PropDefinition,
            }
            .source_ref(),
            bindings: fixture
                .bindings
                .iter()
                .map(|binding| crate::dto::SetupElementCell {
                    node: binding.node.0,
                    cell: binding.cell,
                })
                .collect(),
            id: fixture.id.0,
            name: fixture.name.clone(),
            transform: Transform {
                position: point3_meters(fixture.position),
                rotation: Rotation3Degrees {
                    x_degrees: fixture.rotation.x,
                    y_degrees: fixture.rotation.y,
                    z_degrees: fixture.rotation.z,
                },
                scale: Scale3 {
                    x: fixture.scale.x,
                    y: fixture.scale.y,
                    z: fixture.scale.z,
                },
            },
            resolved_fixture,
        });
    }
    let render_bounds = layout_bounds(&fixtures);
    GuiDocument::Preview {
        document: PreviewGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            name: resolved.identity.object().to_string(),
            render_bounds,
            fixtures,
            hierarchy,
            available_fixtures: session
                .project
                .definitions
                .props
                .definitions
                .keys()
                .map(|id| super::patch::object_ref(&id.0, SourceObjectKind::PropDefinition))
                .collect(),
        },
    }
}

pub(super) fn project_fixture(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let Some(definition) = session
        .project
        .definitions
        .props
        .definitions
        .get(&PropDefinitionId(resolved.identity.clone()))
    else {
        return blocked("Fixture definition was not found.", Vec::new());
    };
    GuiDocument::Prop {
        document: PropGuiDocument {
            path: resolved.identity.document().to_string(),
            fixture: PropDefinition {
                source_ref: resolved.source_ref(),
                object_key: resolved.identity.object().to_string(),
                name: resolved.identity.object().to_string(),
                color_model: "rgb".to_string(),
                bulb_diameter_meters: definition.bulb_radius.as_meters_f32() * 2.0,
                geometry: geometry(&definition.geometry),
                geometry_summary: geometry_summary(&definition.geometry),
                render_plan: render_plan(&definition.geometry, definition.bulb_radius),
            },
        },
    }
}

pub(super) fn active_element_tree(session: &ProjectSession) -> Option<&ElementTree> {
    session
        .project
        .setups
        .get(&session.project.root.setup)
        .and_then(|setup| session.project.element_trees.get(&setup.elements))
}

fn sequence_audio(
    session: &ProjectSession,
    document: &dawn_language::identity::DocumentId,
    audio: &dawn_language::sequence::SequenceAudio,
) -> Option<SequenceAudio> {
    let dawn_language::sequence::SequenceAudio::Asset(id) = audio else {
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

fn element_target(id: ElementNodeId, kind: &ElementNodeKind) -> ElementTarget {
    ElementTarget {
        kind: if matches!(kind, ElementNodeKind::Group { .. }) {
            ElementTargetKind::Group
        } else {
            ElementTargetKind::Element
        },
        name: id.0.to_string(),
    }
}

fn effect_target(target: &ElementSelection) -> ElementTarget {
    active_target(target.node, None)
}

fn active_target(id: ElementNodeId, kind: Option<&ElementNodeKind>) -> ElementTarget {
    ElementTarget {
        kind: if kind.is_some_and(|kind| matches!(kind, ElementNodeKind::Group { .. })) {
            ElementTargetKind::Group
        } else {
            ElementTargetKind::Element
        },
        name: id.0.to_string(),
    }
}

fn effect_target_label(session: &ProjectSession, target: &ElementSelection) -> String {
    session
        .project
        .element_trees
        .get(&target.tree)
        .and_then(|tree| tree.nodes.get(&target.node))
        .map(|node| node.name.clone())
        .unwrap_or_else(|| format!("Element {}", target.node.0))
}

fn effect_ref_to_gui(reference: &EffectRef) -> SequenceEffectReference {
    match reference {
        EffectRef::Builtin(effect) => SequenceEffectReference::Builtin {
            effect: builtin_effect_to_gui(*effect),
        },
        EffectRef::Custom(id) => SequenceEffectReference::Custom {
            module_id: id.0.module_id().to_string(),
            path: id.0.document().to_string(),
            effect_name: id.0.object().to_string(),
        },
    }
}

fn builtin_effect_to_gui(effect: BuiltinEffect) -> SequenceBuiltinEffect {
    match effect {
        BuiltinEffect::Pulse => SequenceBuiltinEffect::Pulse,
        BuiltinEffect::Chase => SequenceBuiltinEffect::Chase,
        BuiltinEffect::Spin => SequenceBuiltinEffect::Spin,
        BuiltinEffect::MarkPulse => SequenceBuiltinEffect::MarkPulse,
        BuiltinEffect::MarkChase => SequenceBuiltinEffect::MarkChase,
    }
}

fn effect_definitions(session: &ProjectSession) -> Vec<SequenceEffectDefinition> {
    BuiltinEffect::ALL
        .into_iter()
        .map(|builtin| {
            let definition = dawn_language::effect::builtin_effect_definition(builtin);
            SequenceEffectDefinition {
                name: definition.display_name.clone(),
                kind: match definition.kind {
                    EffectKind::Sample => SequenceEffectDefinitionKind::Sample,
                    EffectKind::Generator => SequenceEffectDefinitionKind::Generator,
                },
                effect: effect_ref_to_gui(&EffectRef::Builtin(builtin)),
                import_path: None,
                params: definition
                    .params
                    .iter()
                    .filter_map(|param| {
                        Some(SequenceEffectDefinitionParam {
                            fixed: param.fixed,
                            supports_automation: param.supports_automation(),
                            name: param.name.as_str().to_string(),
                            kind: param_kind(&param.ty)?,
                        })
                    })
                    .collect(),
            }
        })
        .chain(
            session
                .project
                .definitions
                .effects
                .definitions
                .iter()
                .map(|(id, definition)| {
                    let source = effect_ref_to_gui(&EffectRef::Custom(id.clone()));
                    SequenceEffectDefinition {
                        name: definition.display_name.clone(),
                        kind: match definition.kind {
                            EffectKind::Sample => SequenceEffectDefinitionKind::Sample,
                            EffectKind::Generator => SequenceEffectDefinitionKind::Generator,
                        },
                        effect: source,
                        import_path: Some(id.0.document().to_string()),
                        params: definition
                            .params
                            .iter()
                            .filter_map(|param| {
                                Some(SequenceEffectDefinitionParam {
                                    fixed: param.fixed,
                                    supports_automation: param.supports_automation(),
                                    name: param.name.as_str().to_string(),
                                    kind: param_kind(&param.ty)?,
                                })
                            })
                            .collect(),
                    }
                }),
        )
        .collect()
}
use dawn_language::dsl::EffectKind;
use dawn_language::effect::{BuiltinEffect, EffectRef, EffectScope};
use dawn_language::element::{ElementNodeId, ElementNodeKind, ElementSelection, ElementTree};
use dawn_language::operator::{BuiltinOperator, OperatorRef};
use dawn_language::preview::{PreviewLayoutId, PropDefinitionId};
use dawn_language::sequence::{AutomationDetachmentReason, AutomationTarget, SequenceId};
use dawn_project_io::{ProjectSession, SourceObjectKind};

use self::geometry::{geometry, geometry_summary, layout_bounds, render_plan};
use super::{ResolvedGuiObject, blocked, gui_diagnostic};
use crate::dto::{
    ElementTarget, ElementTargetKind, GuiDocument, PreviewGuiDocument, PreviewPropPlacement,
    PropDefinition, PropGuiDocument, ResolvedPreviewProp, Rotation3Degrees, Scale3, SequenceAudio,
    SequenceAutomationBinding, SequenceAutomationClip, SequenceAutomationDetachmentReason,
    SequenceAutomationTarget, SequenceBuiltinEffect, SequenceCompositionGraph, SequenceControlClip,
    SequenceCurvePoint, SequenceDetachedAutomationBinding, SequenceEffect,
    SequenceEffectDefinition, SequenceEffectDefinitionKind, SequenceEffectDefinitionParam,
    SequenceEffectReference, SequenceEffectScope, SequenceGraphEdge, SequenceGuiDocument,
    SequenceLane, SequenceLayer, SequenceMarkCollection, SequenceTimelineClipKind, Transform,
};
use crate::preview::point3_meters;

pub(super) mod geometry;
