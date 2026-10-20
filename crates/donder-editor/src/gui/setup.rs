use donder_model::SetupId;
use donder_model::{ControllerId, InputDeviceId};
use donder_project_io::{ProjectSession, SourceObjectKind};

use super::model::object_identity_from_gui;
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_sequence_api::{GuiDocument, SetupGuiDocument, SetupGuiEdit};

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
    let available_devices = donder_project_io::available_reusable_sources(
        session,
        resolved.identity.document_id(),
        &[SourceObjectKind::InputDevice],
    )
    .into_iter()
    .map(|(_, id)| InputDeviceId(id.into()))
    .collect::<std::collections::HashSet<_>>();
    let input_devices = setup
        .input_devices
        .iter()
        .map(|source| {
            let id = source.id();
            session
                .project
                .input_device(id)
                .map(|device| super::input_device::project_input_device(session, id, device))
                .ok_or_else(|| "Input device was not found.".to_string())
        })
        .collect::<Result<_, _>>();
    let input_devices = match input_devices {
        Ok(input_devices) => input_devices,
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
                    SourceObjectKind::InputDevice,
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
            input_devices,
            available_input_devices: session
                .project
                .reusable_input_devices()
                .iter()
                .filter(|(id, _)| {
                    available_devices.contains(*id)
                        && !setup.input_devices.iter().any(|source| source.id() == *id)
                })
                .map(|(id, device)| super::input_device::project_input_device(session, id, device))
                .collect(),
            key_codes: super::input_device::key_codes(),
            input_limits: super::input_device::input_limits(),
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
        SetupGuiEdit::AttachInputDevice { device } => {
            let identity = object_identity_from_gui(&device)?;
            donder_model::attach_input_device(
                &mut session.project,
                &setup.id,
                InputDeviceId(identity.clone()),
            )
            .map_err(GuiMutationError::Invalid)?;
            let source = identity.source().ok_or_else(|| {
                GuiMutationError::Invalid("Choose a reusable input device.".into())
            })?;
            donder_project_io::link_reusable_source(
                session,
                setup.id.0.document_id(),
                SourceObjectKind::InputDevice,
                source,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::DetachInputDevice { device } => {
            let identity = object_identity_from_gui(&device)?;
            donder_model::detach_input_device(
                &mut session.project,
                &setup.id,
                &InputDeviceId(identity),
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        SetupGuiEdit::AddInputDevice { driver, controls } => {
            use donder_model::OwnedObjectSlot;
            let next = super::model::fresh_name("input_device", |name| {
                setup.input_devices.iter().any(|source| {
                    matches!(source.id().0.owned_path().last(), Some(OwnedObjectSlot::InputDevice(taken)) if taken.as_str() == name)
                })
            });
            let id = InputDeviceId(setup.id.0.owned(OwnedObjectSlot::InputDevice(next)));
            let device = super::input_device::domain_input_device(id, None, driver, controls)?;
            setup
                .input_devices
                .push(donder_model::ValueSource::Inline(Box::new(device)));
            session
                .project
                .replace_setup(&setup.id.clone(), setup)
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
