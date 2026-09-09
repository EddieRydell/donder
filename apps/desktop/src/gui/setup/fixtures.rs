use crate::dto::*;
use crate::gui::{GuiMutationError, model::source_identity_from_gui};
use dawn_language::setup::Setup;
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn assign_output(
    session: &mut ProjectSession,
    setup: &Setup,
    node: u32,
    controller: GuiObjectRef,
    port: u32,
    start_slot: u16,
    mode: SetupOutputAssignmentMode,
) -> Result<(), GuiMutationError> {
    super::ensure_owned_target(session, &setup.patch.0)?;
    let controller = source_identity_from_gui(
        &controller.module_id,
        &controller.path,
        &controller.object_key,
    )?;
    let profile = match session
        .project
        .element_trees
        .get(&setup.elements)
        .and_then(|tree| tree.nodes.get(&dawn_language::element::ElementNodeId(node)))
        .map(|node| &node.kind)
    {
        Some(dawn_language::element::ElementNodeKind::Fixture { profile }) => profile.clone(),
        _ => {
            return Err(GuiMutationError::Invalid(
                "Choose a fixture element.".into(),
            ));
        }
    };
    let assign = match mode {
        SetupOutputAssignmentMode::Add => dawn_language::setup::authoring::assign_fixture_output,
        SetupOutputAssignmentMode::Replace => {
            dawn_language::setup::authoring::replace_fixture_outputs
        }
    };
    assign(
        &mut session.project,
        &setup.id,
        dawn_language::element::ElementNodeId(node),
        dawn_language::controller::ControllerId(controller.clone()),
        dawn_language::controller::ControllerPortId(port),
        start_slot,
    )
    .map_err(GuiMutationError::Invalid)?;
    for (kind, identity) in [
        (SourceObjectKind::Controller, &controller),
        (SourceObjectKind::ElementTree, &setup.elements.0),
        (SourceObjectKind::FixtureProfile, &profile.0),
    ] {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            setup.patch.0.document_id(),
            kind,
            identity,
        )
        .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
    }
    Ok(())
}
