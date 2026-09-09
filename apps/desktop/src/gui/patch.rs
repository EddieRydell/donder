use crate::dto::*;
use crate::gui::GuiMutationError;
use crate::gui::model::{curve_from_points, parse_color, source_identity_from_gui};
use dawn_language::controller::{ControllerId, ControllerPortId};
use dawn_language::element::{
    ColorCapability, DiscreteColorMapping, DiscreteEmitter, ElementCellRange, ElementNodeId,
    ElementSelection, EmitterId,
};
use dawn_language::fixture_profile::{DimmingCurve, FixtureProfileId};
use dawn_language::identity::SourceIdentity;
use dawn_language::patch::*;

use dawn_project_io::{ProjectSession, SourceObjectKind};
use indexmap::IndexMap;

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &super::ResolvedGuiObject,
) -> GuiDocument {
    let Some(patch) = session
        .project
        .patches
        .get(&PatchId(resolved.identity.clone()))
    else {
        return super::blocked("Patch was not found.", Vec::new());
    };
    GuiDocument::Patch {
        document: PatchGuiDocument {
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            nodes: project_nodes(patch),
            edges: patch
                .edges
                .iter()
                .map(|edge| SetupPatchEdge {
                    from_node: edge.from.0,
                    from_port: edge.from_port.0,
                    to_node: edge.to.0,
                    to_port: edge.to_port.0,
                })
                .collect(),
            element_trees: session
                .project
                .element_trees
                .iter()
                .map(|(id, tree)| PatchElementTree {
                    source_ref: object_ref(&id.0, SourceObjectKind::ElementTree),
                    elements: tree
                        .nodes
                        .iter()
                        .filter_map(|(id, node)| {
                            node.kind.cell_count().map(|cell_count| ElementCellOption {
                                id: id.0,
                                name: node.name.clone(),
                                cell_count,
                            })
                        })
                        .collect(),
                })
                .collect(),
            controllers: session
                .project
                .controllers
                .iter()
                .map(|(id, controller)| {
                    super::controller::project_controller(session, id, controller, Vec::new())
                })
                .collect(),
            profiles: profiles(session),
        },
    }
}

pub(super) fn object_ref(identity: &SourceIdentity, kind: SourceObjectKind) -> GuiObjectRef {
    GuiObjectRef {
        module_id: identity.module_id().to_string(),
        path: identity.document().to_string(),
        object_key: identity.object().to_string(),
        id: identity.object().to_string(),
        kind: ObjectKind::from(&kind),
    }
}

pub(super) fn profiles(session: &ProjectSession) -> Vec<GuiObjectRef> {
    session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .keys()
        .map(|id| object_ref(&id.0, SourceObjectKind::FixtureProfile))
        .collect()
}

pub(super) fn project_nodes(patch: &PatchGraph) -> Vec<PatchGuiNode> {
    patch
        .nodes
        .iter()
        .map(|(id, node)| PatchGuiNode {
            id: id.0,
            definition: match node {
                PatchNode::Source(source) => PatchGuiNodeDefinition::Source {
                    tree: object_ref(&source.selection.tree.0, SourceObjectKind::ElementTree),
                    node: source.selection.node.0,
                    cells: source.selection.cells.map(|range| PatchGuiCellRange {
                        start: range.start,
                        count: range.count,
                    }),
                    output: match &source.output {
                        PatchValueType::Color { width } => PatchGuiValueType::Color {
                            width: *width as u32,
                        },
                        PatchValueType::Scalar { width } => PatchGuiValueType::Scalar {
                            width: *width as u32,
                        },
                        PatchValueType::Indexed { width } => PatchGuiValueType::Indexed {
                            width: *width as u32,
                        },
                        PatchValueType::Components { width } => PatchGuiValueType::Components {
                            width: *width as u32,
                        },
                        PatchValueType::Slots { width } => PatchGuiValueType::Slots {
                            width: *width as u32,
                        },
                        PatchValueType::FixtureState { width, profile } => {
                            PatchGuiValueType::FixtureState {
                                width: *width as u32,
                                profile: object_ref(&profile.0, SourceObjectKind::FixtureProfile),
                            }
                        }
                    },
                },
                PatchNode::Sink(sink) => PatchGuiNodeDefinition::Sink {
                    controller: object_ref(&sink.controller.0, SourceObjectKind::Controller),
                    port: sink.port.0,
                    start_slot: sink.start_slot,
                    slot_count: sink.slot_count,
                },
                PatchNode::Filter(filter) => PatchGuiNodeDefinition::Filter {
                    filter: match filter {
                        FilterDefinition::ColorBreakdown {
                            capability,
                            cell_count,
                        } => PatchGuiFilter::ColorBreakdown {
                            capability: project_capability(capability),
                            cell_count: *cell_count as u32,
                        },
                        FilterDefinition::DimmingCurve { curve, width } => {
                            PatchGuiFilter::DimmingCurve {
                                curve: project_curve(curve),
                                width: *width as u32,
                            }
                        }
                        FilterDefinition::ScaleInvert {
                            scale,
                            invert,
                            width,
                        } => PatchGuiFilter::ScaleInvert {
                            scale: *scale,
                            invert: *invert,
                            width: *width as u32,
                        },
                        FilterDefinition::FanOut { width, outputs } => PatchGuiFilter::FanOut {
                            width: *width as u32,
                            outputs: *outputs,
                        },
                        FilterDefinition::ComponentReorder {
                            components_per_cell,
                            order,
                            cell_count,
                        } => PatchGuiFilter::ComponentReorder {
                            components_per_cell: *components_per_cell,
                            order: order.clone(),
                            cell_count: *cell_count as u32,
                        },
                        FilterDefinition::IndexedValueMapping { entries, width } => {
                            PatchGuiFilter::IndexedValueMapping {
                                entries: entries
                                    .iter()
                                    .map(|(id, value)| PatchGuiIndexedEntry {
                                        id: *id,
                                        value: *value,
                                    })
                                    .collect(),
                                width: *width as u32,
                            }
                        }
                        FilterDefinition::ScalarToComponents { width } => {
                            PatchGuiFilter::ScalarToComponents {
                                width: *width as u32,
                            }
                        }
                        FilterDefinition::Quantize8 { width } => PatchGuiFilter::Quantize8 {
                            width: *width as u32,
                        },
                        FilterDefinition::Quantize16 { width, byte_order } => {
                            PatchGuiFilter::Quantize16 {
                                width: *width as u32,
                                byte_order: match byte_order {
                                    ByteOrder::CoarseFine => GuiByteOrder::CoarseFine,
                                    ByteOrder::FineCoarse => GuiByteOrder::FineCoarse,
                                },
                            }
                        }
                        FilterDefinition::FixtureProfileEncoding {
                            profile,
                            fixture_count,
                            slot_count,
                        } => PatchGuiFilter::FixtureProfileEncoding {
                            profile: object_ref(&profile.0, SourceObjectKind::FixtureProfile),
                            fixture_count: *fixture_count as u32,
                            slot_count: *slot_count as u32,
                        },
                    },
                },
            },
        })
        .collect()
}

pub(super) fn project_curve(curve: &DimmingCurve) -> GuiDimmingCurve {
    match curve {
        DimmingCurve::Linear => GuiDimmingCurve::Linear,
        DimmingCurve::Gamma(exponent) => GuiDimmingCurve::Gamma {
            exponent: *exponent,
        },
        DimmingCurve::Custom(curve) => GuiDimmingCurve::Custom {
            points: curve
                .points
                .iter()
                .map(|point| SequenceCurvePoint {
                    time: point.position,
                    value: point.value,
                })
                .collect(),
        },
    }
}

pub(super) fn project_capability(capability: &ColorCapability) -> GuiColorCapability {
    match capability {
        ColorCapability::Rgb => GuiColorCapability::Rgb,
        ColorCapability::Rgbw => GuiColorCapability::Rgbw,
        ColorCapability::Discrete { emitters, mappings } => GuiColorCapability::Discrete {
            emitters: emitters
                .iter()
                .map(|emitter| GuiDiscreteEmitter {
                    id: emitter.id.0,
                    name: emitter.name.clone(),
                })
                .collect(),
            mappings: mappings
                .iter()
                .map(|mapping| GuiDiscreteColorMapping {
                    color: mapping.color.to_hex(),
                    levels: mapping
                        .levels
                        .iter()
                        .map(|(id, value)| PatchGuiIndexedEntry {
                            id: id.0,
                            value: *value,
                        })
                        .collect(),
                })
                .collect(),
        },
    }
}

type References = Vec<(SourceObjectKind, SourceIdentity)>;

fn reference(
    value: GuiObjectRef,
    kind: SourceObjectKind,
    references: &mut References,
) -> Result<SourceIdentity, GuiMutationError> {
    let identity = source_identity_from_gui(&value.module_id, &value.path, &value.object_key)?;
    references.push((kind, identity.clone()));
    Ok(identity)
}

pub(super) fn domain_curve(curve: GuiDimmingCurve) -> DimmingCurve {
    match curve {
        GuiDimmingCurve::Linear => DimmingCurve::Linear,
        GuiDimmingCurve::Gamma { exponent } => DimmingCurve::Gamma(exponent),
        GuiDimmingCurve::Custom { points } => DimmingCurve::Custom(curve_from_points(points)),
    }
}

fn indexed_entries(
    entries: Vec<PatchGuiIndexedEntry>,
) -> Result<IndexMap<u32, f32>, GuiMutationError> {
    let mut result = IndexMap::new();
    for entry in entries {
        if result.insert(entry.id, entry.value).is_some() {
            return Err(GuiMutationError::Invalid(format!(
                "Duplicate mapping identifier {}.",
                entry.id
            )));
        }
    }
    Ok(result)
}

pub(super) fn domain_capability(
    capability: GuiColorCapability,
) -> Result<ColorCapability, GuiMutationError> {
    Ok(match capability {
        GuiColorCapability::Rgb => ColorCapability::Rgb,
        GuiColorCapability::Rgbw => ColorCapability::Rgbw,
        GuiColorCapability::Discrete { emitters, mappings } => ColorCapability::Discrete {
            emitters: emitters
                .into_iter()
                .map(|emitter| DiscreteEmitter {
                    id: EmitterId(emitter.id),
                    name: emitter.name,
                })
                .collect(),
            mappings: mappings
                .into_iter()
                .map(|mapping| {
                    Ok(DiscreteColorMapping {
                        color: parse_color(&mapping.color)?,
                        levels: indexed_entries(mapping.levels)?
                            .into_iter()
                            .map(|(id, value)| (EmitterId(id), value))
                            .collect(),
                    })
                })
                .collect::<Result<_, GuiMutationError>>()?,
        },
    })
}

fn domain_filter(
    filter: PatchGuiFilter,
    references: &mut References,
) -> Result<FilterDefinition, GuiMutationError> {
    Ok(match filter {
        PatchGuiFilter::ColorBreakdown {
            capability,
            cell_count,
        } => FilterDefinition::ColorBreakdown {
            capability: domain_capability(capability)?,
            cell_count: cell_count as usize,
        },
        PatchGuiFilter::DimmingCurve { curve, width } => FilterDefinition::DimmingCurve {
            curve: domain_curve(curve),
            width: width as usize,
        },
        PatchGuiFilter::ScaleInvert {
            scale,
            invert,
            width,
        } => FilterDefinition::ScaleInvert {
            scale,
            invert,
            width: width as usize,
        },
        PatchGuiFilter::FanOut { width, outputs } => FilterDefinition::FanOut {
            width: width as usize,
            outputs,
        },
        PatchGuiFilter::ComponentReorder {
            components_per_cell,
            order,
            cell_count,
        } => FilterDefinition::ComponentReorder {
            components_per_cell,
            order,
            cell_count: cell_count as usize,
        },
        PatchGuiFilter::IndexedValueMapping { entries, width } => {
            FilterDefinition::IndexedValueMapping {
                entries: indexed_entries(entries)?,
                width: width as usize,
            }
        }
        PatchGuiFilter::ScalarToComponents { width } => FilterDefinition::ScalarToComponents {
            width: width as usize,
        },
        PatchGuiFilter::Quantize8 { width } => FilterDefinition::Quantize8 {
            width: width as usize,
        },
        PatchGuiFilter::Quantize16 { width, byte_order } => FilterDefinition::Quantize16 {
            width: width as usize,
            byte_order: match byte_order {
                GuiByteOrder::CoarseFine => ByteOrder::CoarseFine,
                GuiByteOrder::FineCoarse => ByteOrder::FineCoarse,
            },
        },
        PatchGuiFilter::FixtureProfileEncoding {
            profile,
            fixture_count,
            slot_count,
        } => FilterDefinition::FixtureProfileEncoding {
            profile: FixtureProfileId(reference(
                profile,
                SourceObjectKind::FixtureProfile,
                references,
            )?),
            fixture_count: fixture_count as usize,
            slot_count: slot_count as usize,
        },
    })
}

pub(super) fn replace(
    session: &mut ProjectSession,
    id: &PatchId,
    nodes: Vec<PatchGuiNode>,
    edges: Vec<SetupPatchEdge>,
) -> Result<(), GuiMutationError> {
    super::setup::ensure_owned_target(session, &id.0)?;
    let mut references = Vec::new();
    let mut patch_nodes = IndexMap::new();
    for node in nodes {
        let definition = match node.definition {
            PatchGuiNodeDefinition::Source {
                tree,
                node,
                cells,
                output,
            } => {
                let tree = dawn_language::element::ElementTreeId(reference(
                    tree,
                    SourceObjectKind::ElementTree,
                    &mut references,
                )?);
                let output = match output {
                    PatchGuiValueType::Color { width } => PatchValueType::Color {
                        width: width as usize,
                    },
                    PatchGuiValueType::Scalar { width } => PatchValueType::Scalar {
                        width: width as usize,
                    },
                    PatchGuiValueType::Indexed { width } => PatchValueType::Indexed {
                        width: width as usize,
                    },
                    PatchGuiValueType::Components { width } => PatchValueType::Components {
                        width: width as usize,
                    },
                    PatchGuiValueType::Slots { width } => PatchValueType::Slots {
                        width: width as usize,
                    },
                    PatchGuiValueType::FixtureState { width, profile } => {
                        PatchValueType::FixtureState {
                            width: width as usize,
                            profile: FixtureProfileId(reference(
                                profile,
                                SourceObjectKind::FixtureProfile,
                                &mut references,
                            )?),
                        }
                    }
                };
                PatchNode::Source(PatchSource {
                    selection: ElementSelection {
                        tree,
                        node: ElementNodeId(node),
                        cells: cells.map(|cells| ElementCellRange {
                            start: cells.start,
                            count: cells.count,
                        }),
                    },
                    output,
                })
            }
            PatchGuiNodeDefinition::Filter { filter } => {
                PatchNode::Filter(domain_filter(filter, &mut references)?)
            }
            PatchGuiNodeDefinition::Sink {
                controller,
                port,
                start_slot,
                slot_count,
            } => PatchNode::Sink(PatchSink {
                controller: ControllerId(reference(
                    controller,
                    SourceObjectKind::Controller,
                    &mut references,
                )?),
                port: ControllerPortId(port),
                start_slot,
                slot_count,
            }),
        };
        if patch_nodes
            .insert(PatchNodeId(node.id), definition)
            .is_some()
        {
            return Err(GuiMutationError::Invalid(format!(
                "Duplicate patch node {}.",
                node.id
            )));
        }
    }
    let patch = PatchGraph {
        id: id.clone(),
        nodes: patch_nodes,
        edges: edges
            .into_iter()
            .map(|edge| PatchEdge {
                from: PatchNodeId(edge.from_node),
                from_port: PatchPortId(edge.from_port),
                to: PatchNodeId(edge.to_node),
                to_port: PatchPortId(edge.to_port),
            })
            .collect(),
    };
    session.project.patches.insert(id.clone(), patch);
    for (kind, identity) in references {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            id.0.document_id(),
            kind,
            &identity,
        )
        .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
    }
    Ok(())
}
