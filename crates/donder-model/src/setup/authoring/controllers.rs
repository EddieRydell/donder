use crate::controller::ControllerId;
use crate::ownership::ValueSource;
use crate::project::DonderProject;
use crate::setup::SetupId;

pub fn attach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: ControllerId,
) -> Result<(), String> {
    project.checked_edit(|project| attach_controller_candidate(project, setup_id, controller))
}

fn attach_controller_candidate(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: ControllerId,
) -> Result<(), String> {
    if !project.controllers.contains_key(&controller) {
        return Err("Controller was not found.".into());
    }
    let setup = project.setup_mut(setup_id).ok_or("Setup was not found.")?;
    if setup
        .controllers
        .iter()
        .any(|source| source.id() == &controller)
    {
        return Err("This controller is already in the setup.".into());
    }
    setup.controllers.push(ValueSource::Reference(controller));
    Ok(())
}

/// Remove setup membership, retaining the reusable authored controller definition.
/// Rejected requests leave both setup membership and patch outputs unchanged.
pub fn detach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: &ControllerId,
    remove_outputs: bool,
) -> Result<(), String> {
    project.checked_edit(|project| {
        detach_controller_candidate(project, setup_id, controller, remove_outputs)
    })
}

fn detach_controller_candidate(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: &ControllerId,
    remove_outputs: bool,
) -> Result<(), String> {
    let setup = project.setup(setup_id).ok_or("Setup was not found.")?;
    if !setup
        .controllers
        .iter()
        .any(|source| source.id() == controller)
    {
        return Err("Choose a controller in this setup.".into());
    }
    let patch_id = setup.patch.id().clone();
    let patch = project.patch(&patch_id).ok_or("Patch was not found.")?;
    let sinks: Vec<_> = patch
        .routes
        .iter()
        .filter(|route| &route.controller == controller)
        .map(|route| route.id)
        .collect();
    if !sinks.is_empty() {
        if !remove_outputs {
            return Err(format!(
                "This controller has {} output assignments. Remove them first or choose Remove controller and outputs.",
                sinks.len()
            ));
        }
        if project
            .setups()
            .any(|other| &other.id != setup_id && other.patch.id() == &patch_id)
        {
            return Err("Another setup uses this patch. Make the patch independent first so output changes stay in this setup.".into());
        }
        let patch = project.patch_mut(&patch_id).ok_or("Patch was not found.")?;
        for sink in sinks {
            patch.remove_output(sink)?;
        }
    }
    project
        .setup_mut(setup_id)
        .ok_or("Setup was not found.")?
        .controllers
        .retain(|source| source.id() != controller);
    Ok(())
}
