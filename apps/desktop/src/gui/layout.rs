use super::fixture::{bulb_radius, checked_point, domain_geometry};
use super::setup::ensure_owned_target;
use dawn_language::element::{ElementCellAddress, ElementNodeId};
use dawn_language::preview::{PreviewLayoutId, PropDefinitionId};
use dawn_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};

use super::model::{domain_point3_meters, rotation3_degrees, scale3};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::PreviewGuiEdit;

pub(super) fn edit_layout(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: PreviewGuiEdit,
) -> Result<(), GuiMutationError> {
    let layout_id = PreviewLayoutId(resolved.identity.clone());
    match edit {
        PreviewGuiEdit::EditElements { edit } => {
            let tree = session
                .project
                .preview_layouts
                .get(&layout_id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .element_tree
                .clone();
            super::elements::edit(session, &tree, edit)?;
        }
        PreviewGuiEdit::AddPixelLight { light } => add_pixel_light(session, &layout_id, light)?,
        PreviewGuiEdit::PlaceFixture {
            name,
            parent,
            capability,
            definition,
            position,
        } => {
            let definition = PropDefinitionId(super::model::source_identity_from_gui(
                &definition.module_id,
                &definition.path,
                &definition.object_key,
            )?);
            let tree = session
                .project
                .preview_layouts
                .get(&layout_id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .element_tree
                .clone();
            ensure_owned_target(session, &tree.0)?;
            ensure_document_can_reference_source(
                session,
                layout_id.0.document_id(),
                SourceObjectKind::PropDefinition,
                &definition.0,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            dawn_language::preview::authoring::place_fixture(
                &mut session.project,
                &layout_id,
                dawn_language::preview::authoring::FixturePlacement {
                    name,
                    parent: parent.map(ElementNodeId),
                    capability: super::patch::domain_capability(capability)?,
                    definition,
                    position: checked_point(position)?,
                },
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        PreviewGuiEdit::DuplicatePlacement { id } => {
            let tree = session
                .project
                .preview_layouts
                .get(&layout_id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .element_tree
                .clone();
            ensure_owned_target(session, &tree.0)?;
            dawn_language::preview::authoring::duplicate_placement(
                &mut session.project,
                &layout_id,
                dawn_language::preview::PropInstanceId(id),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        PreviewGuiEdit::RemovePlacement { id } => {
            placement_mut(session, &layout_id, id)?;
            session
                .project
                .preview_layouts
                .get_mut(&layout_id)
                .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
                .props
                .retain(|prop| prop.id.0 != id);
        }
        PreviewGuiEdit::UpdatePlacementTransform { id, transform } => {
            let fixture = placement_mut(session, &layout_id, id)?;
            fixture.position = domain_point3_meters(transform.position);
            fixture.rotation = rotation3_degrees(transform.rotation);
            fixture.scale = scale3(transform.scale);
        }
        PreviewGuiEdit::SetPlacementBindings { id, bindings } => {
            let fixture = placement_mut(session, &layout_id, id)?;
            fixture.bindings = bindings
                .into_iter()
                .map(|binding| ElementCellAddress {
                    node: ElementNodeId(binding.node),
                    cell: binding.cell,
                })
                .collect();
        }
        PreviewGuiEdit::CopyPlacementDefinition { id } => copy_definition(session, resolved, id)?,
    }
    Ok(())
}

fn placement_mut<'a>(
    session: &'a mut ProjectSession,
    layout: &PreviewLayoutId,
    id: u32,
) -> Result<&'a mut dawn_language::preview::PropInstance, GuiMutationError> {
    session
        .project
        .preview_layouts
        .get_mut(layout)
        .and_then(|layout| layout.props.iter_mut().find(|prop| prop.id.0 == id))
        .ok_or_else(|| GuiMutationError::Invalid("Fixture placement was not found.".into()))
}

fn copy_definition(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    id: u32,
) -> Result<(), GuiMutationError> {
    let layout_id = PreviewLayoutId(resolved.identity.clone());
    let original = session
        .project
        .preview_layouts
        .get(&layout_id)
        .and_then(|layout| layout.props.iter().find(|prop| prop.id.0 == id))
        .ok_or_else(|| GuiMutationError::Invalid("Fixture placement was not found.".into()))?
        .definition
        .clone();
    let definition = session
        .project
        .definitions
        .props
        .definitions
        .get(&original)
        .ok_or_else(|| GuiMutationError::Invalid("Fixture definition was not found.".into()))?
        .clone();
    let identity = super::setup::create_object_document(
        session,
        SourceObjectKind::PropDefinition,
        original.0.object(),
        "fixtures",
        "fixture",
    )?;
    ensure_document_can_reference_source(
        session,
        resolved.identity.document_id(),
        SourceObjectKind::PropDefinition,
        &identity,
    )
    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    let copy = PropDefinitionId(identity);
    session
        .project
        .definitions
        .props
        .definitions
        .insert(copy.clone(), definition);
    let layout = session
        .project
        .preview_layouts
        .get_mut(&layout_id)
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?;
    layout
        .props
        .iter_mut()
        .find(|prop| prop.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Fixture placement was not found.".into()))?
        .definition = copy;

    Ok(())
}

pub(crate) fn add_pixel_light(
    session: &mut ProjectSession,
    layout_id: &dawn_language::preview::PreviewLayoutId,
    light: crate::dto::SetupPixelLight,
) -> Result<(), GuiMutationError> {
    let crate::dto::SetupPixelLight {
        name,
        parent,
        capability,
        geometry,
        bulb_diameter_meters,
        position,
    } = light;
    let capability = super::patch::domain_capability(capability)?;
    let tree_id = session
        .project
        .preview_layouts
        .get(layout_id)
        .ok_or_else(|| GuiMutationError::Invalid("Layout was not found.".into()))?
        .element_tree
        .clone();
    ensure_owned_target(session, &tree_id.0)?;
    ensure_owned_target(session, &layout_id.0)?;
    let bulb_radius = bulb_radius(bulb_diameter_meters)?;
    let geometry = domain_geometry(geometry)?;
    let position = checked_point(position)?;
    let identity = super::setup::create_object_document(
        session,
        SourceObjectKind::PropDefinition,
        &name,
        "fixtures",
        "fixture",
    )?;
    dawn_project_io::ensure_document_can_reference_source(
        session,
        layout_id.0.document_id(),
        SourceObjectKind::PropDefinition,
        &identity,
    )
    .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    let definition = PropDefinitionId(identity);
    session.project.definitions.props.definitions.insert(
        definition.clone(),
        dawn_language::preview::PropDefinition {
            geometry,
            bulb_radius,
        },
    );
    dawn_language::preview::authoring::place_fixture(
        &mut session.project,
        layout_id,
        dawn_language::preview::authoring::FixturePlacement {
            name,
            capability,
            parent: parent.map(ElementNodeId),
            definition,
            position,
        },
    )
    .map_err(GuiMutationError::Invalid)?;
    Ok(())
}
