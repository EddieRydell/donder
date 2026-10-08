use donder_model::ControllerId;
use donder_model::SetupId;
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::model::object_identity_from_gui;
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, SetupGuiDocument, SetupGuiEdit};

use super::patch;

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(setup) = session.project.setup(&SetupId(resolved.object_identity())) else {
        return blocked("The requested setup is missing.", Vec::new());
    };
    let available_controllers = donder_project_io::available_reusable_sources(
        session,
        resolved.identity.document_id(),
        &[SourceObjectKind::Controller],
    )
    .into_iter()
    .map(|(_, id)| ControllerId(id.into()))
    .collect::<std::collections::HashSet<_>>();
    let controllers = setup
        .controllers
        .iter()
        .map(|source| {
            let id = source.id();
            session
                .project
                .controller(id)
                .map(|controller| super::controller::project_controller(session, id, controller))
                .ok_or_else(|| "Controller was not found.".to_string())
        })
        .collect::<Result<_, _>>();
    let controllers = match controllers {
        Ok(controllers) => controllers,
        Err(error) => return blocked(error, Vec::new()),
    };
    GuiDocument::Setup {
        document: SetupGuiDocument {
            description: setup.description.clone(),
            available_sources: super::ownership::available_sources(
                session,
                resolved.identity.document_id(),
                &[
                    SourceObjectKind::Layout,
                    SourceObjectKind::Patch,
                    SourceObjectKind::Controller,
                ],
            ),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            layout_ref: patch::object_ref(&setup.layout.id().0, SourceObjectKind::Layout),
            patch_ref: patch::object_ref(&setup.patch.id().0, SourceObjectKind::Patch),
            layout_read_only: !session
                .source
                .is_project_owned(setup.layout.id().0.document_id()),
            patch_read_only: !session
                .source
                .is_project_owned(setup.patch.id().0.document_id()),
            controllers,
            available_controllers: session
                .project
                .reusable_controllers()
                .iter()
                .filter(|(id, _)| {
                    available_controllers.contains(*id)
                        && !setup.controllers.iter().any(|source| source.id() == *id)
                })
                .map(|(id, controller)| {
                    super::controller::project_controller(session, id, controller)
                })
                .collect(),
        },
    }
}

pub(super) fn edit_setup(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SetupGuiEdit,
) -> Result<(), GuiMutationError> {
    let mut setup = session
        .project
        .setup(&SetupId(resolved.object_identity()))
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("The requested setup is missing.".to_string()))?;
    match edit {
        SetupGuiEdit::AttachController { controller } => {
            let identity = object_identity_from_gui(&controller)?;
            donder_model::attach_controller(
                &mut session.project,
                &setup.id,
                ControllerId(identity.clone()),
            )
            .map_err(GuiMutationError::Invalid)?;
            let source = identity
                .source()
                .ok_or_else(|| GuiMutationError::Invalid("Choose a reusable controller.".into()))?;
            donder_project_io::link_reusable_source(
                session,
                setup.id.0.document_id(),
                SourceObjectKind::Controller,
                source,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::DetachController {
            controller,
            remove_outputs,
        } => {
            let identity = object_identity_from_gui(&controller)?;
            if remove_outputs {
                ensure_owned_target(session, &setup.patch.id().0)?;
            }
            donder_model::detach_controller(
                &mut session.project,
                &setup.id,
                &ControllerId(identity),
                remove_outputs,
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        SetupGuiEdit::AddController { config, ports } => {
            use donder_model::OwnedObjectSlot;
            let taken = setup
                .controllers
                .iter()
                .filter_map(|source| match source.id().0.owned_path().last() {
                    Some(OwnedObjectSlot::Controller(name)) => Some(name.as_str().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            let next = super::model::fresh_name("controller", |name| {
                taken.iter().any(|taken| taken == name)
            });
            let id = ControllerId(setup.id.0.owned(OwnedObjectSlot::Controller(next)));
            let controller = super::controller::domain_controller(id, None, config, ports)?;
            setup
                .controllers
                .push(donder_model::ValueSource::Inline(Box::new(controller)));
            session
                .project
                .replace_setup(&setup.id.clone(), setup)
                .map_err(GuiMutationError::Invalid)?;
        }
    }
    Ok(())
}

pub(super) fn ensure_owned_target(
    session: &ProjectSession,
    identity: &donder_model::ObjectIdentity,
) -> Result<(), GuiMutationError> {
    if session.source.is_project_owned(identity.document_id()) {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(format!(
            "{} does not belong to this project.",
            identity.root_source().object()
        )))
    }
}
