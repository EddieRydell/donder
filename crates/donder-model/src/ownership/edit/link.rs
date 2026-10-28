use super::*;

/// Replace a membership with a reusable declaration, rejecting incompatible routing
/// or dangling targets without changing the accepted project.
pub fn use_existing(
    project: &mut DonderProject,
    site: &OwnershipSite,
    source: ObjectId,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| {
        let slot = site.slot(project)?;
        use_existing_candidate(project, &slot, source, name_of)
    })
}

fn use_existing_candidate(
    project: &mut DonderProject,
    site: &Slot,
    source: ObjectId,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), EditRejection> {
    match (site, source) {
        (Slot::ProjectSetup, ObjectId::Setup(id)) => {
            let replacement = project
                .setups
                .get(&id)
                .ok_or(EditRejection::ChooseReusable(ObjectKind::Setup))?;
            let to = *replacement.layout.id();
            let current = project
                .get::<crate::Setup>(*project.root.setup.id())
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let from = *current.layout.id();
            let retires_layout = matches!(project.root.setup, Owned::Inline(_))
                && matches!(current.layout, Owned::Inline(_));
            if from != to {
                let renames = Renames::of(from, to);
                if retires_layout {
                    retarget(project, &renames);
                } else {
                    follow_active(project, &renames, name_of)?;
                }
            }
            project.root.setup = Owned::Reference(id);
        }
        (Slot::ProjectSequence(index), ObjectId::Sequence(id)) => {
            if !project.sequences.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::Sequence));
            }
            *project
                .root
                .sequences
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Sequence))? = OwnedMember::Reference(id);
        }
        (Slot::ProjectRig(index), ObjectId::Rig(id)) => {
            if !project.rigs.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::Rig));
            }
            *project
                .root
                .rigs
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Rig))? = OwnedMember::Reference(id);
        }
        (Slot::SetupInputDevice { setup, index }, ObjectId::InputDevice(id)) => {
            if !project.input_devices.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::InputDevice));
            }
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let current = current
                .input_devices
                .get(*index)
                .ok_or(EditRejection::Missing(ObjectKind::InputDevice))?;
            let from = *current.id();
            if from == id {
                return Ok(());
            }
            let retires_device = matches!(current, OwnedMember::Inline { .. });
            project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .input_devices[*index] = OwnedMember::Reference(id);
            if retires_device {
                retarget(project, &Renames::of(from, id));
            }
        }
        (Slot::SetupPatch(setup), ObjectId::Patch(id)) => {
            if !project.patches.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::Patch));
            }
            project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .patch = Owned::Reference(id);
        }
        (Slot::SetupLayout(setup), ObjectId::Layout(id)) => {
            if !project.layouts.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::Layout));
            }
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let from = *current.layout.id();
            if from == id {
                return Ok(());
            }
            let retires_layout = matches!(current.layout, Owned::Inline(_));
            let renames = Renames::of(from, id);
            copy_patch_if_linked(project, setup)?;
            let current = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            current.layout = Owned::Reference(id);
            renames.apply(
                current
                    .patch
                    .inline_mut()
                    .ok_or(EditRejection::Missing(ObjectKind::Patch))?,
            );
            if retires_layout {
                retarget(project, &renames);
            } else if project.root.setup.id() == setup {
                follow_active(project, &renames, name_of)?;
            }
        }
        (Slot::SetupController { setup, index }, ObjectId::Controller(id)) => {
            if !project.controllers.contains_key(&id) {
                return Err(EditRejection::ChooseReusable(ObjectKind::Controller));
            }
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let current = current
                .controllers
                .get(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Controller))?;
            let from = *current.id();
            if from == id {
                return Ok(());
            }
            let retires_controller = matches!(current, OwnedMember::Inline { .. });
            let renames = Renames::of(from, id);
            copy_patch_if_linked(project, setup)?;
            let current = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            current.controllers[*index] = OwnedMember::Reference(id);
            renames.apply(
                current
                    .patch
                    .inline_mut()
                    .ok_or(EditRejection::Missing(ObjectKind::Patch))?,
            );
            if retires_controller {
                retarget(project, &renames);
            }
        }
        (Slot::LayoutFixture { layout, fixture }, ObjectId::FixtureDefinition(id)) => {
            if !project
                .reusable::<crate::FixtureDefinition>()
                .contains_key(&id)
            {
                return Err(EditRejection::ChooseReusable(ObjectKind::FixtureDefinition));
            }
            *fixture_mut(
                &mut project
                    .find_mut::<crate::Layout>(*layout)
                    .ok_or(EditRejection::Missing(ObjectKind::Layout))?
                    .fixtures,
                *fixture,
            )
            .ok_or(EditRejection::MissingItem)? = OwnedFixture::Reference(id);
        }
        _ => return Err(EditRejection::ChooseMatchingKind),
    }
    Ok(())
}
