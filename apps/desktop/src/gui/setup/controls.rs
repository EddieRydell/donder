use crate::gui::{GuiMutationError, model::source_identity_from_gui};
use dawn_language::element::ElementNodeId;
use dawn_language::setup::Setup;
use dawn_project_io::{ProjectSession, SourceObjectKind};
pub(super) fn assign_output(
    session: &mut ProjectSession,
    setup: &Setup,
    assignment: crate::dto::SetupControlOutputAssignment,
    mode: crate::dto::SetupOutputAssignmentMode,
) -> Result<(), GuiMutationError> {
    use crate::dto::{SetupControlOutputMapping, SetupOutputAssignmentMode};
    use dawn_language::setup::authoring::{ControlOutputAssignment, ControlOutputMapping};
    super::ensure_owned_target(session, &setup.patch.0)?;
    let controller = source_identity_from_gui(
        &assignment.controller.module_id,
        &assignment.controller.path,
        &assignment.controller.object_key,
    )?;
    let mapping = match assignment.mapping {
        SetupControlOutputMapping::Scalar => ControlOutputMapping::Scalar,
        SetupControlOutputMapping::Indexed { entries } => {
            let mut values = indexmap::IndexMap::new();
            for entry in entries {
                if values.insert(entry.id, entry.value).is_some() {
                    return Err(GuiMutationError::Invalid(
                        "Each indexed option needs exactly one channel value.".into(),
                    ));
                }
            }
            ControlOutputMapping::Indexed { entries: values }
        }
    };
    let assign = match mode {
        SetupOutputAssignmentMode::Add => dawn_language::setup::authoring::assign_control_output,
        SetupOutputAssignmentMode::Replace => {
            dawn_language::setup::authoring::replace_control_outputs
        }
    };
    assign(
        &mut session.project,
        &setup.id,
        ControlOutputAssignment {
            node: ElementNodeId(assignment.node),
            controller: dawn_language::controller::ControllerId(controller.clone()),
            port: dawn_language::controller::ControllerPortId(assignment.port),
            start_slot: assignment.start_slot,
            mapping,
        },
    )
    .map_err(GuiMutationError::Invalid)?;
    for (kind, identity) in [
        (SourceObjectKind::Controller, &controller),
        (SourceObjectKind::ElementTree, &setup.elements.0),
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
