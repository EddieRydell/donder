use super::*;
use crate::controller::ControllerId;

/// Replace a membership with a reusable source. Typed validation at the caller's
/// transaction boundary rejects incompatible routing or dangling external targets.
pub fn use_existing(
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
                    retarget_active_sequences(project, &from, &to)?;
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
                retarget_active_sequences(project, &from, &id)?;
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
