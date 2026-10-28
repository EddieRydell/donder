use donder_model::SetupId;
use donder_model::{ControllerId, InputDeviceId};
use donder_project_io::ProjectSession;

use super::model::{object_id_from_gui, object_ref};
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, SetupGuiDocument, SetupGuiEdit};

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(setup) = session
        .project
        .get::<donder_model::Setup>(resolved.id::<SetupId>())
    else {
        return blocked("The requested setup is missing.", Vec::new());
    };
    let available_controllers =
        donder_project_io::available_declarations(session, &[donder_model::ObjectKind::Controller])
            .into_iter()
            .filter_map(|id| ControllerId::try_from(id).ok())
            .collect::<std::collections::HashSet<_>>();
    let controllers = setup
        .controllers
        .iter()
        .map(|source| {
            let id = source.id();
            session
                .project
                .get::<donder_model::Controller>(*id)
                .map(|controller| super::controller::project_controller(session, id, controller))
                .ok_or_else(|| "Controller was not found.".to_string())
        })
        .collect::<Result<_, _>>();
    let controllers = match controllers {
        Ok(controllers) => controllers,
        Err(error) => return blocked(error, Vec::new()),
    };
    let available_devices = donder_project_io::available_declarations(
        session,
        &[donder_model::ObjectKind::InputDevice],
    )
    .into_iter()
    .filter_map(|id| InputDeviceId::try_from(id).ok())
    .collect::<std::collections::HashSet<_>>();
    let input_devices = setup
        .input_devices
        .iter()
        .map(|source| {
            let id = source.id();
            session
                .project
                .get::<donder_model::InputDevice>(*id)
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
            available_declarations: super::ownership::available_declarations(
                session,
                &[
                    donder_model::ObjectKind::Layout,
                    donder_model::ObjectKind::Patch,
                    donder_model::ObjectKind::Controller,
                    donder_model::ObjectKind::InputDevice,
                ],
            ),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            layout_ref: object_ref(session, *setup.layout.id()),
            patch_ref: object_ref(session, *setup.patch.id()),
            controllers,
            available_controllers: session
                .project
                .reusable::<donder_model::Controller>()
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
                .reusable::<donder_model::InputDevice>()
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
        .get::<donder_model::Setup>(resolved.id::<SetupId>())
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("The requested setup is missing.".to_string()))?;
    match edit {
        SetupGuiEdit::AttachController { controller } => {
            let id: ControllerId = object_id_from_gui(session, &controller)?;
            donder_model::attach_controller(&mut session.project, &setup.id, id)
                .map_err(GuiMutationError::from)?;
            donder_project_io::link_declaration(session, resolved.document_id(), id.into())
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::DetachController {
            controller,
            remove_outputs,
        } => {
            let id: ControllerId = object_id_from_gui(session, &controller)?;
            donder_model::detach_controller(&mut session.project, &setup.id, &id, remove_outputs)
                .map_err(GuiMutationError::from)?;
        }
        SetupGuiEdit::AttachInputDevice { device } => {
            let id: InputDeviceId = object_id_from_gui(session, &device)?;
            donder_model::attach_input_device(&mut session.project, &setup.id, id)
                .map_err(GuiMutationError::from)?;
            donder_project_io::link_declaration(session, resolved.document_id(), id.into())
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::DetachInputDevice { device } => {
            let id: InputDeviceId = object_id_from_gui(session, &device)?;
            donder_model::detach_input_device(&mut session.project, &setup.id, &id)
                .map_err(GuiMutationError::from)?;
        }
        SetupGuiEdit::AddInputDevice { driver, controls } => {
            let name = super::model::fresh_name("input_device", |name| {
                setup
                    .input_devices
                    .iter()
                    .any(|source| source.name().is_some_and(|taken| taken.as_str() == name))
            });
            let id = InputDeviceId(session.project.new_id());
            let device = super::input_device::domain_input_device(id, None, driver, controls)?;
            setup.input_devices.push(donder_model::OwnedMember::Inline {
                name,
                value: Box::new(device),
            });
            session.project.put(setup).map_err(GuiMutationError::from)?;
        }
        SetupGuiEdit::AddController { config, ports } => {
            let name = super::model::fresh_name("controller", |name| {
                setup
                    .controllers
                    .iter()
                    .any(|source| source.name().is_some_and(|taken| taken.as_str() == name))
            });
            let id = ControllerId(session.project.new_id());
            let controller = super::controller::domain_controller(id, None, config, ports)?;
            setup.controllers.push(donder_model::OwnedMember::Inline {
                name,
                value: Box::new(controller),
            });
            session.project.put(setup).map_err(GuiMutationError::from)?;
        }
    }
    Ok(())
}
