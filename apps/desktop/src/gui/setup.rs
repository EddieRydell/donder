use dawn_language::controller::ControllerId;
use dawn_language::element::ElementNodeId;
use dawn_language::patch::{PatchNode, PatchNodeId};
use dawn_language::setup::SetupId;
use dawn_project_io::{ProjectSession, SourceObjectKind};

use super::model::source_identity_from_gui;
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    GuiDocument, SetupElementCell, SetupFixtureProfile, SetupGuiDocument, SetupGuiEdit,
    SetupOutputAssignment, SetupPatchEdge, SetupPatchNode, SetupPatchNodeKind, SetupPreviewLink,
};

pub(crate) mod authoring;
mod controls;
mod copies;
mod fixtures;
use super::patch;

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(setup) = session
        .project
        .setups
        .get(&SetupId(resolved.identity.clone()))
    else {
        return blocked("The requested setup is missing.", Vec::new());
    };
    let Some(tree) = session.project.element_trees.get(&setup.elements) else {
        return blocked("Active element tree is missing.", Vec::new());
    };
    let elements = super::elements::project_nodes(tree);
    let fixture_profiles = session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .iter()
        .map(|(id, profile)| SetupFixtureProfile {
            id: source_key(&id.0),
            name: id.0.object().to_string(),
            function_count: profile.functions.len() as u32,
            channel_count: profile.channels.len() as u32,
            behavior_rule_count: profile.behavior_rules.len() as u32,
            source_ref: patch::object_ref(&id.0, dawn_project_io::SourceObjectKind::FixtureProfile),
            read_only: !session.source.is_project_owned(id.0.document_id()),
            definition: super::fixture_profile::project(profile),
        })
        .collect();
    let Some(preview) = session.project.preview_layouts.get(&setup.preview) else {
        return blocked("Preview layout was not found.", Vec::new());
    };
    let mut preview_links = Vec::new();
    for prop in &preview.props {
        let Some(definition) = session
            .project
            .definitions
            .props
            .definitions
            .get(&prop.definition)
        else {
            return blocked("A preview prop definition is missing.", Vec::new());
        };
        preview_links.push(SetupPreviewLink {
            prop_id: prop.id.0,
            name: prop.name.clone(),
            definition_ref: patch::object_ref(&prop.definition.0, SourceObjectKind::PropDefinition),
            point_count: definition.geometry.point_count() as u32,
            geometry: super::projection::geometry::geometry(&definition.geometry),
            bulb_diameter_meters: definition.bulb_radius.as_meters_f32() * 2.0,
            position: crate::preview::point3_meters(prop.position),
            bindings: prop
                .bindings
                .iter()
                .map(|binding| SetupElementCell {
                    node: binding.node.0,
                    cell: binding.cell,
                })
                .collect(),
        });
    }
    let (patch_nodes, patch_edges) = session
        .project
        .patches
        .get(&setup.patch)
        .map(|patch| {
            let nodes = patch
                .nodes
                .iter()
                .map(|(id, node)| {
                    let (kind, label, width) = match node {
                        PatchNode::Source(source) => (
                            SetupPatchNodeKind::Source,
                            format!("Element {}", source.selection.node.0),
                            source.output.width(),
                        ),
                        PatchNode::Filter(filter) => (
                            SetupPatchNodeKind::Filter,
                            format!("{filter:?}"),
                            filter_width(filter),
                        ),
                        PatchNode::Sink(sink) => (
                            SetupPatchNodeKind::Sink,
                            format!(
                                "{} / port {} / slots {}-{}",
                                sink.controller.0.object(),
                                sink.port.0,
                                sink.start_slot,
                                u32::from(sink.start_slot) + u32::from(sink.slot_count)
                            ),
                            usize::from(sink.slot_count),
                        ),
                    };
                    SetupPatchNode {
                        id: id.0,
                        kind,
                        label,
                        width: width as u32,
                    }
                })
                .collect();
            let edges = patch
                .edges
                .iter()
                .map(|edge| SetupPatchEdge {
                    from_node: edge.from.0,
                    from_port: edge.from_port.0,
                    to_node: edge.to.0,
                    to_port: edge.to_port.0,
                })
                .collect();
            (nodes, edges)
        })
        .unwrap_or_default();
    let Some(patch) = session.project.patches.get(&setup.patch) else {
        return blocked("Setup patch is missing.", Vec::new());
    };
    let project_controller =
        |id: &ControllerId, controller: &dawn_language::controller::Controller| {
            let assignments = patch
                .nodes
                .iter()
                .filter_map(|(node_id, node)| match node {
                    PatchNode::Sink(sink) if sink.controller == *id => {
                        Some(SetupOutputAssignment {
                            sink: node_id.0,
                            controller: source_key(&id.0),
                            port: sink.port.0,
                            start_channel: sink.start_slot + 1,
                            channel_count: sink.slot_count,
                        })
                    }
                    _ => None,
                })
                .collect();
            super::controller::project_controller(session, id, controller, assignments)
        };
    let mut controllers = Vec::new();
    for id in &setup.controllers {
        let Some(controller) = session.project.controllers.get(id) else {
            return blocked("Setup controller is missing.", Vec::new());
        };
        controllers.push(project_controller(id, controller));
    }
    let available_controllers = session
        .project
        .controllers
        .iter()
        .filter(|(id, _)| !setup.controllers.contains(id))
        .map(|(id, controller)| project_controller(id, controller))
        .collect();
    GuiDocument::Setup {
        document: SetupGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            elements_ref: patch::object_ref(
                &setup.elements.0,
                dawn_project_io::SourceObjectKind::ElementTree,
            ),
            preview_ref: patch::object_ref(
                &setup.preview.0,
                dawn_project_io::SourceObjectKind::PreviewLayout,
            ),
            patch_ref: patch::object_ref(&setup.patch.0, dawn_project_io::SourceObjectKind::Patch),
            elements_read_only: !session
                .source
                .is_project_owned(setup.elements.0.document_id()),
            preview_read_only: !session
                .source
                .is_project_owned(setup.preview.0.document_id()),
            patch_read_only: !session.source.is_project_owned(setup.patch.0.document_id()),
            root_ids: tree.roots.iter().map(|id| id.0).collect(),
            elements,
            fixture_profiles,
            preview_links,
            patch_nodes,
            patch_edges,
            patch_definitions: patch::project_nodes(patch),
            patch_profiles: patch::profiles(session),
            output_assignments: controllers
                .iter()
                .flat_map(|controller| controller.assignments.iter().cloned())
                .collect(),
            controllers,
            available_controllers,
        },
    }
}

pub(super) fn edit_setup(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SetupGuiEdit,
) -> Result<(), GuiMutationError> {
    let setup = session
        .project
        .setups
        .get(&SetupId(resolved.identity.clone()))
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("The requested setup is missing.".to_string()))?;
    match edit {
        SetupGuiEdit::CopyLayout => copies::copy_layout(session, &setup)?,
        SetupGuiEdit::AssignControlOutput { assignment, mode } => {
            controls::assign_output(session, &setup, assignment, mode)?;
        }
        SetupGuiEdit::CopyController { controller } => {
            authoring::copy_controller(session, &setup, controller)?;
        }
        SetupGuiEdit::AssignFixtureOutput {
            node,
            controller,
            port,
            start_slot,
            mode,
        } => {
            fixtures::assign_output(session, &setup, node, controller, port, start_slot, mode)?;
        }
        SetupGuiEdit::CreateFixtureProfile { name, definition } => {
            super::fixture_profile::create(session, name, definition)?;
        }
        SetupGuiEdit::RemoveOutput { sink } => {
            ensure_owned_target(session, &setup.patch.0)?;
            session
                .project
                .patches
                .get_mut(&setup.patch)
                .ok_or_else(|| GuiMutationError::Invalid("Patch was not found.".into()))?
                .remove_output(PatchNodeId(sink))
                .map_err(GuiMutationError::Invalid)?;
        }
        SetupGuiEdit::AssignPixelOutput {
            node,
            controller,
            first_port,
            start_slot,
            component_order,
            mode,
        } => {
            ensure_owned_target(session, &setup.patch.0)?;
            let controller = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            let assignment = dawn_language::setup::authoring::PixelOutputAssignment {
                node: ElementNodeId(node),
                controller: ControllerId(controller.clone()),
                first_port: dawn_language::controller::ControllerPortId(first_port),
                start_slot,
                component_order,
            };
            match mode {
                crate::dto::SetupOutputAssignmentMode::Add => {
                    dawn_language::setup::authoring::assign_pixel_output(
                        &mut session.project,
                        &setup.id,
                        assignment,
                    )
                }
                crate::dto::SetupOutputAssignmentMode::Replace => {
                    dawn_language::setup::authoring::replace_pixel_outputs(
                        &mut session.project,
                        &setup.id,
                        assignment,
                    )
                }
            }
            .map_err(GuiMutationError::Invalid)?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.patch.0.document_id(),
                dawn_project_io::SourceObjectKind::Controller,
                &controller,
            )
            .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.patch.0.document_id(),
                dawn_project_io::SourceObjectKind::ElementTree,
                &setup.elements.0,
            )
            .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
        }
        SetupGuiEdit::AttachController { controller } => {
            let identity = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            dawn_language::setup::authoring::attach_controller(
                &mut session.project,
                &setup.id,
                ControllerId(identity.clone()),
            )
            .map_err(GuiMutationError::Invalid)?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.id.0.document_id(),
                dawn_project_io::SourceObjectKind::Controller,
                &identity,
            )
            .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
        }
        SetupGuiEdit::DetachController {
            controller,
            remove_outputs,
        } => {
            let identity = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            if remove_outputs {
                ensure_owned_target(session, &setup.patch.0)?;
            }
            dawn_language::setup::authoring::detach_controller(
                &mut session.project,
                &setup.id,
                &ControllerId(identity),
                remove_outputs,
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        SetupGuiEdit::AddController { config, ports } => {
            let controller = super::controller::domain_controller(config, ports)?;
            let identity = create_object_document(
                session,
                dawn_project_io::SourceObjectKind::Controller,
                "controller",
                "controllers",
                "controller",
            )?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.id.0.document_id(),
                dawn_project_io::SourceObjectKind::Controller,
                &identity,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            let id = ControllerId(identity);
            session.project.controllers.insert(id.clone(), controller);
            session
                .project
                .setups
                .get_mut(&setup.id)
                .ok_or_else(|| GuiMutationError::Invalid("Setup was not found.".into()))?
                .controllers
                .push(id);
        }
    }
    Ok(())
}

pub(super) fn ensure_owned_target(
    session: &ProjectSession,
    identity: &dawn_language::identity::SourceIdentity,
) -> Result<(), GuiMutationError> {
    if session.source.is_project_owned(identity.document_id()) {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(format!(
            "{} belongs to a dependency. Make a project-owned copy before editing it.",
            identity.object()
        )))
    }
}

pub(super) fn source_key(id: &dawn_language::identity::SourceIdentity) -> String {
    format!("{}#{}", id.document(), id.object())
}

pub(super) fn create_object_document(
    session: &mut ProjectSession,
    kind: dawn_project_io::SourceObjectKind,
    name: &str,
    directory: &str,
    suffix: &str,
) -> Result<dawn_language::identity::SourceIdentity, GuiMutationError> {
    let mut key = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while key.contains("__") {
        key = key.replace("__", "_");
    }
    key = key.trim_matches('_').to_string();
    if key.is_empty() || key.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        key = format!("item_{key}");
    }
    for index in 1_u32.. {
        let stem = if index == 1 {
            key.clone()
        } else {
            format!("{key}_{index}")
        };
        let path = camino::Utf8PathBuf::from(format!("{directory}/{stem}.{suffix}.dawn"));
        let document = session.source.project_document(path.clone());
        if session.source.documents.contains_key(&document)
            || session.source.project_root().join(&path).exists()
        {
            continue;
        }
        return session
            .source
            .add_yaml_document(path, vec![(kind, stem)])
            .map_err(GuiMutationError::Invalid)?
            .into_iter()
            .next()
            .ok_or_else(|| GuiMutationError::Invalid("New document has no object.".into()));
    }
    Err(GuiMutationError::Invalid(
        "No source document names remain.".into(),
    ))
}

fn create_layout_document(
    session: &mut ProjectSession,
    name: &str,
) -> Result<
    (
        dawn_language::identity::SourceIdentity,
        dawn_language::identity::SourceIdentity,
    ),
    GuiMutationError,
> {
    for index in 1_u32.. {
        let stem = if index == 1 {
            name.to_string()
        } else {
            format!("{name}_{index}")
        };
        let path = camino::Utf8PathBuf::from(format!("layouts/{stem}.layout.dawn"));
        let document = session.source.project_document(path.clone());
        if session.source.documents.contains_key(&document)
            || session.source.project_root().join(&path).exists()
        {
            continue;
        }
        let mut identities = session
            .source
            .add_yaml_document(
                path,
                vec![
                    (
                        dawn_project_io::SourceObjectKind::ElementTree,
                        "elements".into(),
                    ),
                    (
                        dawn_project_io::SourceObjectKind::PreviewLayout,
                        "preview".into(),
                    ),
                ],
            )
            .map_err(GuiMutationError::Invalid)?
            .into_iter();
        let elements = identities
            .next()
            .ok_or_else(|| GuiMutationError::Invalid("Layout has no element tree.".into()))?;
        let preview = identities
            .next()
            .ok_or_else(|| GuiMutationError::Invalid("Layout has no preview.".into()))?;
        return Ok((elements, preview));
    }
    Err(GuiMutationError::Invalid(
        "No layout document names remain.".into(),
    ))
}

fn filter_width(filter: &dawn_language::patch::FilterDefinition) -> usize {
    use dawn_language::patch::FilterDefinition;
    match filter {
        FilterDefinition::ColorBreakdown { cell_count, .. } => *cell_count,
        FilterDefinition::DimmingCurve { width, .. }
        | FilterDefinition::ScaleInvert { width, .. }
        | FilterDefinition::FanOut { width, .. }
        | FilterDefinition::IndexedValueMapping { width, .. }
        | FilterDefinition::ScalarToComponents { width }
        | FilterDefinition::Quantize8 { width }
        | FilterDefinition::Quantize16 { width, .. } => *width,
        FilterDefinition::ComponentReorder {
            components_per_cell,
            cell_count,
            ..
        } => usize::from(*components_per_cell) * *cell_count,
        FilterDefinition::FixtureProfileEncoding { slot_count, .. } => *slot_count,
    }
}
