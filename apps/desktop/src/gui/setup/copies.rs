use super::{GuiMutationError, ensure_owned_target};
use donder_language::layout::{LayoutFixtureKind, LayoutId};
use donder_language::patch::PatchId;
use donder_language::setup::Setup;
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};

pub(super) fn copy_layout(
    session: &mut ProjectSession,
    setup: &Setup,
) -> Result<(), GuiMutationError> {
    let sequences = if setup.id == session.project.root.setup {
        session
            .project
            .root
            .sequences
            .iter()
            .filter(|id| {
                session.project.sequences[*id]
                    .effects
                    .iter()
                    .any(|effect| effect.target.layout == setup.layout)
            })
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    for id in &sequences {
        ensure_owned_target(session, &id.0)?;
    }
    let layout = LayoutId(super::create_object_document(
        session,
        SourceObjectKind::Layout,
        "layout_copy",
        "layouts",
        "layout",
    )?);
    let patch = PatchId(super::create_object_document(
        session,
        SourceObjectKind::Patch,
        "patch_copy",
        "patches",
        "patch",
    )?);
    for (original, copy) in [(&setup.layout.0, &layout.0), (&setup.patch.0, &patch.0)] {
        session
            .source
            .inherit_dependency_imports(setup.id.0.document_id(), copy.document_id())
            .map_err(GuiMutationError::Invalid)?;
        session
            .source
            .inherit_dependency_imports(original.document_id(), copy.document_id())
            .map_err(GuiMutationError::Invalid)?;
    }
    donder_language::setup::authoring::copy_setup_layout(
        &mut session.project,
        &setup.id,
        layout.clone(),
        patch.clone(),
        &sequences,
    )
    .map_err(GuiMutationError::Invalid)?;
    for (kind, identity) in [
        (SourceObjectKind::Layout, &layout.0),
        (SourceObjectKind::Patch, &patch.0),
    ] {
        ensure_document_can_reference_source(session, setup.id.0.document_id(), kind, identity)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    let definitions = session.project.layouts[&layout]
        .iter_fixtures()
        .filter_map(|fixture| match &fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => Some(definition.clone()),
            LayoutFixtureKind::Group { .. } => None,
        })
        .collect::<indexmap::IndexSet<_>>();
    for definition in definitions {
        ensure_document_can_reference_source(
            session,
            layout.0.document_id(),
            SourceObjectKind::FixtureDefinition,
            &definition.0,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    ensure_patch_references(session, &patch)?;
    for id in sequences {
        ensure_document_can_reference_source(
            session,
            id.0.document_id(),
            SourceObjectKind::Layout,
            &layout.0,
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    Ok(())
}

pub(super) fn ensure_patch_references(
    session: &mut ProjectSession,
    patch: &PatchId,
) -> Result<(), GuiMutationError> {
    let references = session.project.patches[patch]
        .routes
        .iter()
        .flat_map(|route| {
            [
                (SourceObjectKind::Layout, route.target.layout.0.clone()),
                (SourceObjectKind::Controller, route.controller.0.clone()),
            ]
        })
        .collect::<Vec<_>>();
    for (kind, identity) in references {
        ensure_document_can_reference_source(session, patch.0.document_id(), kind, &identity)
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    Ok(())
}
