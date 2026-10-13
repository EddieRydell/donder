use super::*;
use crate::controller::ControllerId;

/// Replace a membership with a reusable source, rejecting incompatible routing
/// or dangling targets without changing the accepted project.
pub fn use_existing(
    project: &mut DonderProject,
    site: &OwnershipSite,
    source: SourceIdentity,
) -> Result<(), String> {
    project.checked_edit(|project| use_existing_candidate(project, site, source))
}

fn use_existing_candidate(
    project: &mut DonderProject,
    site: &OwnershipSite,
    source: SourceIdentity,
) -> Result<(), String> {
    let target = ObjectIdentity::from(source.clone());
    match site {
        OwnershipSite::ProjectSetup => {
            let id = SetupId(target);
            let replacement = project.setups.get(&id).ok_or("Choose a reusable setup.")?;
            let to = replacement.layout.id().clone();
            let current = project
                .setup(project.root.setup.id())
                .ok_or("Setup was not found.")?;
            let from = current.layout.id().clone();
            let retires_layout = matches!(project.root.setup, ValueSource::Inline(_))
                && matches!(current.layout, ValueSource::Inline(_));
            if from != to {
                if retires_layout {
                    retarget(project, &from.0, &to.0);
                } else {
                    retarget_active_members(project, &from, &to)?;
                }
            }
            project.root.setup = ValueSource::Reference(id);
        }
        OwnershipSite::ProjectSequence(index) => {
            let id = SequenceId(target);
            if !project.sequences.contains_key(&id) {
                return Err("Choose a reusable sequence.".into());
            }
            *project
                .root
                .sequences
                .get_mut(*index)
                .ok_or("Sequence was not found.")? = ValueSource::Reference(id);
        }
        OwnershipSite::ProjectRig(index) => {
            let id = RigId(target);
            if !project.rigs.contains_key(&id) {
                return Err("Choose a reusable rig.".into());
            }
            *project
                .root
                .rigs
                .get_mut(*index)
                .ok_or("Rig was not found.")? = ValueSource::Reference(id);
        }
        OwnershipSite::SetupInputDevice { setup, index } => {
            let id = InputDeviceId(target);
            if !project.input_devices.contains_key(&id) {
                return Err("Choose a reusable input device.".into());
            }
            let current = project.setup(setup).ok_or("Setup was not found.")?;
            let current = current
                .input_devices
                .get(*index)
                .ok_or("Input device was not found.")?;
            let from = current.id().0.clone();
            if from == id.0 {
                return Ok(());
            }
            let retires_device = matches!(current, ValueSource::Inline(_));
            project
                .setup_mut(setup)
                .ok_or("Setup was not found.")?
                .input_devices[*index] = ValueSource::Reference(id.clone());
            if retires_device {
                retarget(project, &from, &id.0);
            }
        }
        OwnershipSite::SetupPatch(setup) => {
            let id = PatchId(target);
            if !project.patches.contains_key(&id) {
                return Err("Choose a reusable patch.".into());
            }
            project
                .setup_mut(setup)
                .ok_or("Setup was not found.")?
                .patch = ValueSource::Reference(id);
        }
        OwnershipSite::SetupLayout(setup) => {
            let id = LayoutId(target);
            if !project.layouts.contains_key(&id) {
                return Err("Choose a reusable layout.".into());
            }
            let current = project.setup(setup).ok_or("Setup was not found.")?;
            let from = current.layout.id().clone();
            if from == id {
                return Ok(());
            }
            let retires_layout = matches!(current.layout, ValueSource::Inline(_));
            copy_patch_if_linked(project, setup)?;
            let current = project.setup_mut(setup).ok_or("Setup was not found.")?;
            current.layout = ValueSource::Reference(id.clone());
            for route in &mut current
                .patch
                .inline_mut()
                .ok_or("Independent patch was not found.")?
                .routes
            {
                route.target.layout.0.rebase(&from.0, &id.0);
            }
            if retires_layout {
                retarget(project, &from.0, &id.0);
            } else if project.root.setup.id() == setup {
                retarget_active_members(project, &from, &id)?;
            }
        }
        OwnershipSite::SetupController { setup, index } => {
            let id = ControllerId(target);
            if !project.controllers.contains_key(&id) {
                return Err("Choose a reusable controller.".into());
            }
            let current = project.setup(setup).ok_or("Setup was not found.")?;
            let current = current
                .controllers
                .get(*index)
                .ok_or("Controller was not found.")?;
            let from = current.id().0.clone();
            if from == id.0 {
                return Ok(());
            }
            let retires_controller = matches!(current, ValueSource::Inline(_));
            copy_patch_if_linked(project, setup)?;
            let current = project.setup_mut(setup).ok_or("Setup was not found.")?;
            current.controllers[*index] = ValueSource::Reference(id.clone());
            for route in &mut current
                .patch
                .inline_mut()
                .ok_or("Independent patch was not found.")?
                .routes
            {
                route.controller.0.rebase(&from, &id.0);
            }
            if retires_controller {
                retarget(project, &from, &id.0);
            }
        }
        OwnershipSite::LayoutFixture { layout, fixture } => {
            let id = FixtureDefinitionId(source);
            if !project.definitions.fixtures.definitions.contains_key(&id) {
                return Err("Choose a reusable fixture.".into());
            }
            *fixture_mut(
                &mut project
                    .layout_mut(layout)
                    .ok_or("Layout was not found.")?
                    .fixtures,
                *fixture,
            )
            .ok_or("Fixture was not found.")? = FixtureSource::Reference(id);
        }
    }
    Ok(())
}
