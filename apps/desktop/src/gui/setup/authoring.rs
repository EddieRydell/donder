use super::{GuiMutationError, ensure_owned_target};
use dawn_language::setup::Setup;
use dawn_project_io::{ProjectSession, SourceObjectKind};

pub(super) fn copy_controller(
    session: &mut ProjectSession,
    setup: &Setup,
    controller: crate::dto::GuiObjectRef,
) -> Result<(), GuiMutationError> {
    ensure_owned_target(session, &setup.id.0)?;
    let original = dawn_language::controller::ControllerId(super::source_identity_from_gui(
        &controller.module_id,
        &controller.path,
        &controller.object_key,
    )?);
    let copy = super::create_object_document(
        session,
        SourceObjectKind::Controller,
        original.0.object(),
        "controllers",
        "controller",
    )?;
    let patch = super::create_object_document(
        session,
        SourceObjectKind::Patch,
        "patch_copy",
        "patches",
        "patch",
    )
    .map(dawn_language::patch::PatchId)?;
    dawn_language::setup::authoring::copy_controller(
        &mut session.project,
        &setup.id,
        &original,
        dawn_language::controller::ControllerId(copy.clone()),
        patch.clone(),
    )
    .map_err(GuiMutationError::Invalid)?;
    for (kind, identity) in [
        (SourceObjectKind::Controller, &copy),
        (SourceObjectKind::Patch, &patch.0),
    ] {
        dawn_project_io::ensure_document_can_reference_source(
            session,
            setup.id.0.document_id(),
            kind,
            identity,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    session
        .source
        .inherit_dependency_imports(setup.id.0.document_id(), patch.0.document_id())
        .map_err(GuiMutationError::Invalid)?;
    super::copies::ensure_patch_references(session, &patch)
}
