use crate::input_device::InputDeviceId;
use crate::ownership::ValueSource;
use crate::project::DonderProject;
use crate::setup::SetupId;

pub fn attach_input_device(
    project: &mut DonderProject,
    setup_id: &SetupId,
    device: InputDeviceId,
) -> Result<(), String> {
    project.checked_edit(|project| {
        if !project.input_devices.contains_key(&device) {
            return Err("Input device was not found.".into());
        }
        let setup = project.setup_mut(setup_id).ok_or("Setup was not found.")?;
        if setup
            .input_devices
            .iter()
            .any(|source| source.id() == &device)
        {
            return Err("This input device is already in the setup.".into());
        }
        setup.input_devices.push(ValueSource::Reference(device));
        Ok(())
    })
}

/// Remove setup membership, retaining a reusable device's authored source.
/// The project's rigs must no longer play from it.
pub fn detach_input_device(
    project: &mut DonderProject,
    setup_id: &SetupId,
    device: &InputDeviceId,
) -> Result<(), String> {
    project.checked_edit(|project| {
        let setup = project.setup_mut(setup_id).ok_or("Setup was not found.")?;
        let count = setup.input_devices.len();
        setup.input_devices.retain(|source| source.id() != device);
        if setup.input_devices.len() == count {
            return Err("Choose an input device in this setup.".into());
        }
        Ok(())
    })
}
