//! Generic lookup: one store per kind of reusable object, keyed by id, and
//! the owners that hold objects in place.
use super::*;
use crate::ownership::OwnedMember;

/// A kind of project object, found by its id wherever it is written.
pub trait ProjectObject: Sized + 'static {
    type Id: Copy + Eq + std::hash::Hash + std::fmt::Debug + Into<ObjectId> + TryFrom<ObjectId>;
    fn find(project: &DonderProject, id: Self::Id) -> Option<&Self>;
    /// Every object of the kind once: the reusable ones, then those owned in
    /// place.
    fn all(project: &DonderProject) -> impl Iterator<Item = &Self>;
}

/// A kind with a store of reusable objects.
pub trait Reusable: ProjectObject {
    fn store(project: &DonderProject) -> &IndexMap<Self::Id, Self>;
}

/// In-place access for edits, which must detach only what they change.
pub(crate) trait Stored: Reusable + Clone {
    fn store_mut(project: &mut DonderProject) -> &mut IndexMap<Self::Id, Self>;
    fn find_mut(project: &mut DonderProject, id: Self::Id) -> Option<&mut Self>;
}

impl ProjectObject for ProjectRoot {
    type Id = ProjectId;
    fn find(project: &DonderProject, id: ProjectId) -> Option<&Self> {
        (project.root.id == id).then_some(&*project.root)
    }
    fn all(project: &DonderProject) -> impl Iterator<Item = &Self> {
        std::iter::once(&*project.root)
    }
}

/// Kinds stored only in their reusable store.
macro_rules! store_only {
    ($($kind:ty => $id:ty, $store:ident;)*) => {$(
        impl ProjectObject for $kind {
            type Id = $id;
            fn find(project: &DonderProject, id: $id) -> Option<&Self> {
                project.$store.get(&id)
            }
            fn all(project: &DonderProject) -> impl Iterator<Item = &Self> {
                project.$store.values()
            }
        }
        impl Reusable for $kind {
            fn store(project: &DonderProject) -> &IndexMap<$id, Self> {
                &project.$store
            }
        }
        impl Stored for $kind {
            fn store_mut(project: &mut DonderProject) -> &mut IndexMap<$id, Self> {
                &mut project.$store
            }
            fn find_mut(project: &mut DonderProject, id: $id) -> Option<&mut Self> {
                project.$store.get_mut(&id)
            }
        }
    )*};
}

store_only! {
    FixtureDefinition => FixtureDefinitionId, fixtures;
    CurveDefinition => CurveId, curves;
    GradientDefinition => GradientId, gradients;
    EffectDefinition => EffectDefinitionId, effects;
    OperatorDefinition => OperatorDefinitionId, operators;
}

/// Kinds that are reusable or owned in place: `$owned` lists the owned ones
/// and `$owned_mut` finds one to edit.
macro_rules! owned_kind {
    ($kind:ty => $id:ty, $store:ident, |$p:ident| $owned:expr, |$pm:ident, $want:ident| $owned_mut:expr) => {
        impl ProjectObject for $kind {
            type Id = $id;
            fn find(project: &DonderProject, id: $id) -> Option<&Self> {
                project.$store.get(&id).or_else(|| {
                    let $p = project;
                    $owned.find(|value: &&Self| value.id == id)
                })
            }
            fn all(project: &DonderProject) -> impl Iterator<Item = &Self> {
                let $p = project;
                project.$store.values().chain($owned)
            }
        }
        impl Reusable for $kind {
            fn store(project: &DonderProject) -> &IndexMap<$id, Self> {
                &project.$store
            }
        }
        impl Stored for $kind {
            fn store_mut(project: &mut DonderProject) -> &mut IndexMap<$id, Self> {
                &mut project.$store
            }
            fn find_mut(project: &mut DonderProject, $want: $id) -> Option<&mut Self> {
                if project.$store.contains_key(&$want) {
                    return project.$store.get_mut(&$want);
                }
                let $pm = project;
                $owned_mut
            }
        }
    };
}

/// The owner holding `id` among its members, and the member's position.
fn member_position<T: crate::ownership::Identified<Id = I>, I: PartialEq>(
    members: &[OwnedMember<T, I>],
    id: &I,
) -> Option<usize> {
    members.iter().position(|source| source.id() == id)
}

owned_kind!(Setup => SetupId, setups,
|project| project.root.setup.inline().map(Box::as_ref).into_iter(),
|project, id| {
    if project.root.setup.id() != &id {
        return None;
    }
    project.root.setup.inline_mut().map(Box::as_mut)
});

owned_kind!(Layout => LayoutId, layouts,
|project| project
    .objects::<Setup>()
    .filter_map(|setup| setup.layout.inline().map(Box::as_ref)),
|project, id| {
    let owner = project.objects::<Setup>().find(|setup| setup.layout.id() == &id)?.id;
    project.find_mut::<Setup>(owner)?.layout.inline_mut().map(Box::as_mut)
});

owned_kind!(Patch => PatchId, patches,
|project| project
    .objects::<Setup>()
    .filter_map(|setup| setup.patch.inline().map(Box::as_ref)),
|project, id| {
    let owner = project.objects::<Setup>().find(|setup| setup.patch.id() == &id)?.id;
    project.find_mut::<Setup>(owner)?.patch.inline_mut().map(Box::as_mut)
});

owned_kind!(Controller => ControllerId, controllers,
|project| project
    .objects::<Setup>()
    .flat_map(|setup| setup.controllers.iter())
    .filter_map(OwnedMember::inline),
|project, id| {
    let (owner, index) = project.objects::<Setup>().find_map(|setup| {
        member_position(&setup.controllers, &id).map(|index| (setup.id, index))
    })?;
    project.find_mut::<Setup>(owner)?.controllers[index].inline_mut()
});

owned_kind!(InputDevice => InputDeviceId, input_devices,
|project| project
    .objects::<Setup>()
    .flat_map(|setup| setup.input_devices.iter())
    .filter_map(OwnedMember::inline),
|project, id| {
    let (owner, index) = project.objects::<Setup>().find_map(|setup| {
        member_position(&setup.input_devices, &id).map(|index| (setup.id, index))
    })?;
    project.find_mut::<Setup>(owner)?.input_devices[index].inline_mut()
});

owned_kind!(Sequence => SequenceId, sequences,
|project| project.root.sequences.iter().filter_map(OwnedMember::inline),
|project, id| {
    let index = member_position(&project.root.sequences, &id)?;
    project.root.sequences[index].inline_mut()
});

owned_kind!(Rig => RigId, rigs,
|project| project.root.rigs.iter().filter_map(OwnedMember::inline),
|project, id| {
    let index = member_position(&project.root.rigs, &id)?;
    project.root.rigs[index].inline_mut()
});
