use crate::controller::ControllerId;
use crate::identity::ObjectKind;
use crate::ownership::OwnedMember;
use crate::project::DonderProject;
use crate::project::EditRejection;
use crate::setup::SetupId;

pub fn attach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: ControllerId,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| attach_controller_candidate(project, setup_id, controller))
}

fn attach_controller_candidate(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: ControllerId,
) -> Result<(), EditRejection> {
    if !project.controllers.contains_key(&controller) {
        return Err(EditRejection::Missing(ObjectKind::Controller));
    }
    let setup = project
        .find_mut::<crate::Setup>(*setup_id)
        .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
    if setup
        .controllers
        .iter()
        .any(|source| source.id() == &controller)
    {
        return Err(EditRejection::AlreadyInSetup(ObjectKind::Controller));
    }
    setup.controllers.push(OwnedMember::Reference(controller));
    Ok(())
}

/// Remove setup membership, retaining the reusable authored controller definition.
/// Rejected requests leave both setup membership and patch outputs unchanged.
pub fn detach_controller(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: &ControllerId,
    remove_outputs: bool,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| {
        detach_controller_candidate(project, setup_id, controller, remove_outputs)
    })
}

fn detach_controller_candidate(
    project: &mut DonderProject,
    setup_id: &SetupId,
    controller: &ControllerId,
    remove_outputs: bool,
) -> Result<(), EditRejection> {
    let setup = project
        .get::<crate::Setup>(*setup_id)
        .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
    if !setup
        .controllers
        .iter()
        .any(|source| source.id() == controller)
    {
        return Err(EditRejection::NotInSetup(ObjectKind::Controller));
    }
    let patch_id = *setup.patch.id();
    let patch = project
        .get::<crate::Patch>(patch_id)
        .ok_or(EditRejection::Missing(ObjectKind::Patch))?;
    let sinks: Vec<_> = patch
        .routes
        .iter()
        .filter(|route| &route.controller == controller)
        .map(|route| route.id)
        .collect();
    if !sinks.is_empty() {
        if !remove_outputs {
            return Err(EditRejection::ControllerHasOutputs(sinks.len()));
        }
        if project
            .objects::<crate::Setup>()
            .any(|other| &other.id != setup_id && other.patch.id() == &patch_id)
        {
            return Err(EditRejection::SharedPatch);
        }
        let patch = project
            .find_mut::<crate::Patch>(patch_id)
            .ok_or(EditRejection::Missing(ObjectKind::Patch))?;
        for sink in sinks {
            patch.remove_output(sink)?;
        }
    }
    project
        .find_mut::<crate::Setup>(*setup_id)
        .ok_or(EditRejection::Missing(ObjectKind::Setup))?
        .controllers
        .retain(|source| source.id() != controller);
    Ok(())
}
