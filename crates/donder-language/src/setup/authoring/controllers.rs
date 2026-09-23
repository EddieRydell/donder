use crate::controller::ControllerId;
use crate::model::DonderProject;
use crate::setup::SetupId;

/// Copy a controller and its setup's patch, keeping all other setups unchanged.
/// The caller supplies registered source identities and owns transactionality.
pub fn copy_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    original: &ControllerId,
    copy: ControllerId,
    patch_id: crate::patch::PatchId,
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    let index = setup
        .controllers
        .iter()
        .position(|id| id == original)
        .ok_or("Choose a controller in this setup.")?;
    if project.controllers.contains_key(&copy) || project.patches.contains_key(&patch_id) {
        return Err("Editable copies require new source identities.".into());
    }
    let controller = project
        .controllers
        .get(original)
        .ok_or("Controller was not found.")?
        .clone();
    let mut patch = project
        .patches
        .get(&setup.patch)
        .ok_or("Patch was not found.")?
        .clone();
    patch.id = patch_id.clone();
    for route in &mut patch.routes {
        if &route.controller == original {
            route.controller = copy.clone();
        }
    }
    project.controllers.insert(copy.clone(), controller);
    project.patches.insert(patch_id.clone(), patch);
    let setup = project
        .setups
        .get_mut(setup_id)
        .ok_or("Setup was not found.")?;
    setup.controllers[index] = copy;
    setup.patch = patch_id;
    Ok(())
}

pub fn attach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: ControllerId,
) -> Result<(), String> {
    if !project.controllers.contains_key(&controller) {
        return Err("Controller was not found.".into());
    }
    let setup = project
        .setups
        .get_mut(setup_id)
        .ok_or("Setup was not found.")?;
    if setup.controllers.contains(&controller) {
        return Err("This controller is already in the setup.".into());
    }
    setup.controllers.push(controller);
    Ok(())
}

/// Remove setup membership, retaining the reusable authored controller definition.
/// The caller owns transactionality and checks ownership of any changed patch.
pub fn detach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: &ControllerId,
    remove_outputs: bool,
) -> Result<(), String> {
    let setup = project.setups.get(setup_id).ok_or("Setup was not found.")?;
    if !setup.controllers.contains(controller) {
        return Err("Choose a controller in this setup.".into());
    }
    let patch_id = setup.patch.clone();
    let patch = project
        .patches
        .get(&patch_id)
        .ok_or("Patch was not found.")?;
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
            .setups
            .values()
            .any(|other| &other.id != setup_id && other.patch == patch_id)
        {
            return Err("Another setup uses this patch. Make an independent patch copy before removing its controller outputs.".into());
        }
        let patch = project
            .patches
            .get_mut(&patch_id)
            .ok_or("Patch was not found.")?;
        for sink in sinks {
            patch.remove_output(sink)?;
        }
    }
    project
        .setups
        .get_mut(setup_id)
        .ok_or("Setup was not found.")?
        .controllers
        .retain(|id| id != controller);
    Ok(())
}
