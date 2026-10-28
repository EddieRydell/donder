use crate::identity::ObjectKind;
use crate::input_device::InputDeviceId;
use crate::ownership::OwnedMember;
use crate::project::DonderProject;
use crate::project::EditRejection;
use crate::setup::SetupId;

pub fn attach_input_device(
    project: &mut DonderProject,
    setup_id: &SetupId,
    device: InputDeviceId,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| {
        if !project.input_devices.contains_key(&device) {
            return Err(EditRejection::Missing(ObjectKind::InputDevice));
        }
        let setup = project
            .find_mut::<crate::Setup>(*setup_id)
            .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
        if setup
            .input_devices
            .iter()
            .any(|source| source.id() == &device)
        {
            return Err(EditRejection::AlreadyInSetup(ObjectKind::InputDevice));
        }
        setup.input_devices.push(OwnedMember::Reference(device));
        Ok(())
    })
}

/// Remove setup membership, retaining a reusable device's authored source.
/// The project's rigs must no longer play from it.
pub fn detach_input_device(
    project: &mut DonderProject,
    setup_id: &SetupId,
    device: &InputDeviceId,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| {
        let setup = project
            .find_mut::<crate::Setup>(*setup_id)
            .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
        let count = setup.input_devices.len();
        setup.input_devices.retain(|source| source.id() != device);
        if setup.input_devices.len() == count {
            return Err(EditRejection::NotInSetup(ObjectKind::InputDevice));
        }
        Ok(())
    })
}
