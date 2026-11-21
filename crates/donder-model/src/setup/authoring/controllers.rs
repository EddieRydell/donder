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
