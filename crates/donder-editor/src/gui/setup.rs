use donder_model::{ControllerId, InputDeviceId};
use donder_project_io::ProjectSession;

use super::model::{object_id_from_gui, object_ref};
use super::{GuiMutationError, ResolvedGuiObject};
use crate::dto::{GuiDocument, SetupGuiDocument, SetupGuiEdit};

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let setup = resolved.view::<donder_model::Setup>(session);
    let available_controllers =
        donder_project_io::available_declarations(session, &[donder_model::ObjectKind::Controller])
            .into_iter()
            .filter_map(|id| ControllerId::try_from(id).ok())
            .collect::<std::collections::HashSet<_>>();
    let controllers = setup
        .controllers()
        .map(|controller| {
            super::controller::project_controller(session, &controller.id(), &controller)
        })
        .collect();
    let available_devices = donder_project_io::available_declarations(
        session,
        &[donder_model::ObjectKind::InputDevice],
    )
    .into_iter()
    .filter_map(|id| InputDeviceId::try_from(id).ok())
    .collect::<std::collections::HashSet<_>>();
    let input_devices = setup
        .input_devices()
        .map(|device| super::input_device::project_input_device(session, &device.id(), &device))
        .collect();
    GuiDocument::Setup {
        document: SetupGuiDocument {
            description: setup.description.clone(),
            source_ref: resolved.source_ref(),
            layout_ref: object_ref(session, *setup.layout.id()),
            patch_ref: object_ref(session, *setup.patch.id()),
            controllers,
            available_controllers: session
                .project()
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
                .project()
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
            new_controller_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::Controller,
            ),
            new_input_device_name: super::model::new_object_name(
                session,
                donder_model::ObjectKind::InputDevice,
            ),
        },
    }
}

pub(super) fn edit_setup(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SetupGuiEdit,
) -> Result<(), GuiMutationError> {
    let mut setup = donder_model::Setup::clone(&resolved.view(session));
    match edit {
        SetupGuiEdit::AttachController { controller } => {
            let id: ControllerId = object_id_from_gui(session, &controller)?;
            session.edit_project(|project| {
                donder_model::attach_controller(project, &setup.id, id)
                    .map_err(GuiMutationError::from)
            })?;
            donder_project_io::link_declaration(session, resolved.document_id(), id.into())
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::AttachInputDevice { device } => {
            let id: InputDeviceId = object_id_from_gui(session, &device)?;
            session.edit_project(|project| {
                donder_model::attach_input_device(project, &setup.id, id)
                    .map_err(GuiMutationError::from)
            })?;
            donder_project_io::link_declaration(session, resolved.document_id(), id.into())
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
        }
        SetupGuiEdit::AddInputDevice {
            driver,
            controls,
            name,
        } => {
            let id = InputDeviceId(session.new_id());
            let device = super::input_device::domain_input_device(id, None, driver, controls)?;
            setup.input_devices.push(donder_model::OwnedMember::Inline {
                name: super::model::typed_name(&name)?,
                value: Box::new(device),
            });
            let owner = setup.id.into();
            session.put(setup).map_err(GuiMutationError::from)?;
            super::ownership::store_new_member(session, owner, id.into(), &name)?;
        }
        SetupGuiEdit::AddController {
            config,
            ports,
            name,
        } => {
            let id = ControllerId(session.new_id());
            let controller = super::controller::domain_controller(id, None, config, ports)?;
            setup.controllers.push(donder_model::OwnedMember::Inline {
                name: super::model::typed_name(&name)?,
                value: Box::new(controller),
            });
            let owner = setup.id.into();
            session.put(setup).map_err(GuiMutationError::from)?;
            super::ownership::store_new_member(session, owner, id.into(), &name)?;
        }
    }
    Ok(())
}
