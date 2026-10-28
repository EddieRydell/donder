//! Ownership edits preserve accepted-project invariants. Objects keep their ids
//! when they change owners; copies take fresh ids, and references follow them
//! by exact id replacement. The caller names promoted values in its documents.
mod duplicate;
mod link;
use super::{MemberSource, ValueSource};
use crate::project::references::References;
use crate::{
    controller::ControllerId,
    fixture::{FixtureDefinitionId, FixtureSource},
    identity::{ObjectId, ObjectKind},
    input_device::InputDeviceId,
    layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId},
    patch::PatchId,
    project::{DonderProject, EditRejection},
    rig::{Rig, RigId},
    sequence::SequenceId,
    setup::{Setup, SetupId},
};
use donder_runtime_types::Identifier;
pub use duplicate::duplicate_layout_fixture;
pub use link::use_existing;
use std::collections::HashMap;

/// A place one object owns or links another: the object `member` that a
/// slot of `owner` holds, or a layout fixture's definition, which several
/// fixtures may link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OwnershipSite {
    Member {
        owner: ObjectId,
        member: ObjectId,
    },
    Fixture {
        layout: LayoutId,
        fixture: FixtureInstanceId,
    },
}

/// The slot a site names, by its position in the owner.
#[derive(Clone, Debug)]
enum Slot {
    ProjectSetup,
    ProjectSequence(usize),
    ProjectRig(usize),
    SetupLayout(SetupId),
    SetupPatch(SetupId),
    SetupController {
        setup: SetupId,
        index: usize,
    },
    SetupInputDevice {
        setup: SetupId,
        index: usize,
    },
    LayoutFixture {
        layout: LayoutId,
        fixture: FixtureInstanceId,
    },
}

fn position(
    mut ids: impl Iterator<Item = ObjectId>,
    member: ObjectId,
) -> Result<usize, EditRejection> {
    ids.position(|id| id == member)
        .ok_or(EditRejection::MemberNotInOwner)
}

impl OwnershipSite {
    fn slot(&self, project: &DonderProject) -> Result<Slot, EditRejection> {
        use crate::identity::ObjectKind as Kind;
        Ok(match *self {
            Self::Fixture { layout, fixture } => Slot::LayoutFixture { layout, fixture },
            Self::Member {
                owner: ObjectId::Project(id),
                member,
            } if id == project.root.id => {
                let root = &project.root;
                match member.kind() {
                    Kind::Setup if ObjectId::from(*root.setup.id()) == member => Slot::ProjectSetup,
                    Kind::Sequence => Slot::ProjectSequence(position(
                        &mut root.sequences.iter().map(|source| (*source.id()).into()),
                        member,
                    )?),
                    Kind::Rig => Slot::ProjectRig(position(
                        &mut root.rigs.iter().map(|source| (*source.id()).into()),
                        member,
                    )?),
                    _ => return Err(EditRejection::MemberNotInOwner),
                }
            }
            Self::Member {
                owner: ObjectId::Setup(id),
                member,
            } => {
                let setup = project
                    .get::<crate::Setup>(id)
                    .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
                match member.kind() {
                    Kind::Layout if ObjectId::from(*setup.layout.id()) == member => {
                        Slot::SetupLayout(id)
                    }
                    Kind::Patch if ObjectId::from(*setup.patch.id()) == member => {
                        Slot::SetupPatch(id)
                    }
                    Kind::Controller => Slot::SetupController {
                        setup: id,
                        index: position(
                            &mut setup.controllers.iter().map(|source| (*source.id()).into()),
                            member,
                        )?,
                    },
                    Kind::InputDevice => Slot::SetupInputDevice {
                        setup: id,
                        index: position(
                            &mut setup
                                .input_devices
                                .iter()
                                .map(|source| (*source.id()).into()),
                            member,
                        )?,
                    },
                    _ => return Err(EditRejection::MemberNotInOwner),
                }
            }
            Self::Member { .. } => return Err(EditRejection::NoMembers),
        })
    }
}

/// Exact replacements for objects that other objects refer to.
#[derive(Default)]
struct Renames(HashMap<ObjectId, ObjectId>);

impl Renames {
    fn of(from: impl Into<ObjectId>, to: impl Into<ObjectId>) -> Self {
        Self(HashMap::from([(from.into(), to.into())]))
    }

    fn insert(&mut self, from: impl Into<ObjectId>, to: impl Into<ObjectId>) {
        self.0.insert(from.into(), to.into());
    }

    fn target(&self, _: ObjectId, target: &mut ObjectId) {
        if let Some(&to) = self.0.get(target) {
            *target = to;
        }
    }

    /// Whether `value` refers to a renamed object.
    fn moves(&self, value: &impl References) -> bool {
        let mut moves = false;
        value.references(&mut |target| moves |= self.0.contains_key(&target));
        moves
    }

    fn apply(&self, value: &mut impl References) {
        value.retarget(&mut |owner, target| self.target(owner, target));
    }
}

/// A name for a newly owned collection member, unique among its owner's
/// owned members of that kind.
fn next_local_name<'a, T: 'a, I: 'a>(
    members: impl Iterator<Item = &'a MemberSource<T, I>>,
    prefix: &str,
) -> Identifier {
    let taken = members
        .filter_map(MemberSource::name)
        .map(Identifier::as_str)
        .collect::<std::collections::HashSet<_>>();
    donder_language::unique_name(prefix, |name| taken.contains(name))
}

/// The preferred name of a reusable value copied into a collection.
fn preferred_name(
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
    id: impl Into<ObjectId>,
) -> Result<Identifier, EditRejection> {
    name_of(id.into()).ok_or(EditRejection::Unnamed)
}

fn fixture_mut(
    fixtures: &mut [LayoutFixture],
    id: FixtureInstanceId,
) -> Option<&mut FixtureSource> {
    fixtures
        .iter_mut()
        .find(|fixture| fixture.id == id)
        .and_then(|fixture| match &mut fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => Some(definition),
            LayoutFixtureKind::Group { .. } => None,
        })
}

/// Point every reference in the project at the renamed objects.
fn retarget(project: &mut DonderProject, renames: &Renames) {
    project.retarget_references(&mut |owner, target| renames.target(owner, target));
}

fn promote<T, I: Copy>(source: &mut ValueSource<Box<T>, I>) -> Result<Box<T>, EditRejection>
where
    T: super::Identified<Id = I>,
{
    let ValueSource::Inline(value) = source else {
        return Err(EditRejection::AlreadyReusable);
    };
    let id = *value.id();
    let ValueSource::Inline(value) = std::mem::replace(source, ValueSource::Reference(id)) else {
        return Err(EditRejection::MissingItem);
    };
    Ok(value)
}

fn promote_member<T, I: Copy>(source: &mut MemberSource<T, I>) -> Result<Box<T>, EditRejection>
where
    T: super::Identified<Id = I>,
{
    let MemberSource::Inline { value, .. } = source else {
        return Err(EditRejection::AlreadyReusable);
    };
    let id = *value.id();
    let MemberSource::Inline { value, .. } = std::mem::replace(source, MemberSource::Reference(id))
    else {
        return Err(EditRejection::MissingItem);
    };
    Ok(value)
}

/// Move an owned value into its reusable store under its id, which every
/// reference keeps. Returns that id; an owned fixture definition, which has
/// none, takes a fresh one.
pub fn make_reusable(
    project: &mut DonderProject,
    site: &OwnershipSite,
) -> Result<ObjectId, crate::EditError> {
    project.checked_edit(|project| {
        let slot = site.slot(project)?;
        make_reusable_candidate(project, &slot)
    })
}

fn make_reusable_candidate(
    project: &mut DonderProject,
    site: &Slot,
) -> Result<ObjectId, EditRejection> {
    Ok(match site {
        Slot::ProjectSetup => {
            let value = promote(&mut project.root.setup)?;
            let id = value.id;
            project.setups.insert(id, *value);
            id.into()
        }
        Slot::ProjectSequence(index) => {
            let source = project
                .root
                .sequences
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Sequence))?;
            let value = promote_member(source)?;
            let id = value.id;
            project.sequences.insert(id, *value);
            id.into()
        }
        Slot::ProjectRig(index) => {
            let source = project
                .root
                .rigs
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Rig))?;
            let value = promote_member(source)?;
            let id = value.id;
            project.rigs.insert(id, *value);
            id.into()
        }
        Slot::SetupLayout(setup) => {
            let source = &mut project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .layout;
            let value = promote(source)?;
            let id = value.id;
            project.layouts.insert(id, *value);
            id.into()
        }
        Slot::SetupPatch(setup) => {
            let source = &mut project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .patch;
            let value = promote(source)?;
            let id = value.id;
            project.patches.insert(id, *value);
            id.into()
        }
        Slot::SetupController { setup, index } => {
            let source = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .controllers
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Controller))?;
            let value = promote_member(source)?;
            let id = value.id;
            project.controllers.insert(id, *value);
            id.into()
        }
        Slot::SetupInputDevice { setup, index } => {
            let source = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .input_devices
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::InputDevice))?;
            let value = promote_member(source)?;
            let id = value.id;
            project.input_devices.insert(id, *value);
            id.into()
        }
        Slot::LayoutFixture { layout, fixture } => {
            let id = FixtureDefinitionId(project.new_id());
            let source = fixture_mut(
                &mut project
                    .find_mut::<crate::Layout>(*layout)
                    .ok_or(EditRejection::Missing(ObjectKind::Layout))?
                    .fixtures,
                *fixture,
            )
            .ok_or(EditRejection::MissingItem)?;
            if matches!(source, FixtureSource::Reference(_)) {
                return Err(EditRejection::AlreadyReusable);
            }
            let FixtureSource::Inline(value) =
                std::mem::replace(source, FixtureSource::Reference(id))
            else {
                return Err(EditRejection::MissingItem);
            };
            project.fixtures.insert(id, value);
            id.into()
        }
    })
}

/// Copy linked values into their ownership slot under fresh ids. Related
/// routes and active sequence targets follow a copied layout/controller
/// without editing shared sources. `name_of` names reusable values that are
/// copied into a named collection; the copy's name is made unique there.
pub fn make_independent(
    project: &mut DonderProject,
    site: &OwnershipSite,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), crate::EditError> {
    project.checked_edit(|project| {
        let slot = site.slot(project)?;
        make_independent_candidate(project, &slot, name_of)
    })
}

/// Give a copied setup and everything it owns fresh ids, and point its own
/// patch at its copied layout and controllers.
fn fresh_setup(project: &mut DonderProject, setup: &mut Setup) -> Renames {
    let mut renames = Renames::default();
    setup.id = SetupId(project.new_id());
    if let ValueSource::Inline(layout) = &mut setup.layout {
        let id = LayoutId(project.new_id());
        renames.insert(layout.id, id);
        layout.id = id;
    }
    for source in &mut setup.controllers {
        if let Some(controller) = source.inline_mut() {
            let id = ControllerId(project.new_id());
            renames.insert(controller.id, id);
            controller.id = id;
        }
    }
    for source in &mut setup.input_devices {
        if let Some(device) = source.inline_mut() {
            let id = InputDeviceId(project.new_id());
            renames.insert(device.id, id);
            device.id = id;
        }
    }
    if let ValueSource::Inline(patch) = &mut setup.patch {
        patch.id = PatchId(project.new_id());
        renames.apply(patch);
    }
    renames
}

fn make_independent_candidate(
    project: &mut DonderProject,
    site: &Slot,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), EditRejection> {
    match site {
        Slot::ProjectSetup => {
            let ValueSource::Reference(id) = &project.root.setup else {
                return Err(EditRejection::AlreadyIndependent(ObjectKind::Setup));
            };
            let mut value = project
                .get::<crate::Setup>(*id)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .clone();
            let renames = fresh_setup(project, &mut value);
            let setup = value.id;
            let patch_id = *value.patch.id();
            project.root.setup = ValueSource::Inline(Box::new(value));
            let patch = project
                .get::<crate::Patch>(patch_id)
                .ok_or(EditRejection::Missing(ObjectKind::Patch))?;
            if renames.moves(patch) {
                copy_patch_if_linked(project, &setup)?;
                let patch_id = *project
                    .get::<crate::Setup>(setup)
                    .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                    .patch
                    .id();
                renames.apply(
                    project
                        .find_mut::<crate::Patch>(patch_id)
                        .ok_or(EditRejection::Missing(ObjectKind::Patch))?,
                );
            }
            follow_active(project, &renames, name_of)?;
        }
        Slot::ProjectSequence(index) => {
            copy_sequence(project, *index, name_of)?;
        }
        Slot::ProjectRig(index) => {
            copy_rig(project, *index, name_of)?;
        }
        Slot::SetupPatch(setup) => {
            copy_patch(project, setup)?;
        }
        Slot::SetupLayout(setup) => {
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let ValueSource::Reference(from) = current.layout else {
                return Err(EditRejection::AlreadyIndependent(ObjectKind::Layout));
            };
            let mut value = project
                .get::<crate::Layout>(from)
                .ok_or(EditRejection::Missing(ObjectKind::Layout))?
                .clone();
            value.id = LayoutId(project.new_id());
            let renames = Renames::of(from, value.id);
            copy_patch_if_linked(project, setup)?;
            let current = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            current.layout = ValueSource::Inline(Box::new(value));
            renames.apply(
                current
                    .patch
                    .inline_mut()
                    .ok_or(EditRejection::Missing(ObjectKind::Patch))?,
            );
            if project.root.setup.id() == setup {
                follow_active(project, &renames, name_of)?;
            }
        }
        Slot::SetupInputDevice { setup, index } => {
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let source = current
                .input_devices
                .get(*index)
                .ok_or(EditRejection::Missing(ObjectKind::InputDevice))?;
            let MemberSource::Reference(from) = *source else {
                return Err(EditRejection::AlreadyIndependent(ObjectKind::InputDevice));
            };
            let name = next_local_name(
                current.input_devices.iter(),
                preferred_name(name_of, from)?.as_str(),
            );
            let mut value = project
                .get::<crate::InputDevice>(from)
                .ok_or(EditRejection::Missing(ObjectKind::InputDevice))?
                .clone();
            value.id = InputDeviceId(project.new_id());
            let renames = Renames::of(from, value.id);
            project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?
                .input_devices[*index] = MemberSource::Inline {
                name,
                value: Box::new(value),
            };
            if project.root.setup.id() == setup {
                follow_active(project, &renames, name_of)?;
            }
        }
        Slot::SetupController { setup, index } => {
            let current = project
                .get::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            let source = current
                .controllers
                .get(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Controller))?;
            let MemberSource::Reference(from) = *source else {
                return Err(EditRejection::AlreadyIndependent(ObjectKind::Controller));
            };
            let name = next_local_name(
                current.controllers.iter(),
                preferred_name(name_of, from)?.as_str(),
            );
            let mut value = project
                .get::<crate::Controller>(from)
                .ok_or(EditRejection::Missing(ObjectKind::Controller))?
                .clone();
            value.id = ControllerId(project.new_id());
            let renames = Renames::of(from, value.id);
            copy_patch_if_linked(project, setup)?;
            let current = project
                .find_mut::<crate::Setup>(*setup)
                .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
            current.controllers[*index] = MemberSource::Inline {
                name,
                value: Box::new(value),
            };
            renames.apply(
                current
                    .patch
                    .inline_mut()
                    .ok_or(EditRejection::Missing(ObjectKind::Patch))?,
            );
        }
        Slot::LayoutFixture { layout, fixture } => {
            let placement = project
                .get::<crate::Layout>(*layout)
                .and_then(|layout| layout.fixture(*fixture))
                .ok_or(EditRejection::MissingItem)?;
            let LayoutFixtureKind::Fixture {
                definition: FixtureSource::Reference(id),
                ..
            } = &placement.kind
            else {
                return Err(EditRejection::AlreadyIndependent(
                    ObjectKind::FixtureDefinition,
                ));
            };
            let value = project
                .reusable::<crate::FixtureDefinition>()
                .get(id)
                .ok_or(EditRejection::MissingItem)?
                .clone();
            *fixture_mut(
                &mut project
                    .find_mut::<crate::Layout>(*layout)
                    .ok_or(EditRejection::Missing(ObjectKind::Layout))?
                    .fixtures,
                *fixture,
            )
            .ok_or(EditRejection::MissingItem)? = FixtureSource::Inline(value);
        }
    }
    Ok(())
}

fn copy_patch_if_linked(project: &mut DonderProject, setup: &SetupId) -> Result<(), EditRejection> {
    if matches!(
        project
            .get::<crate::Setup>(*setup)
            .ok_or(EditRejection::Missing(ObjectKind::Setup))?
            .patch,
        ValueSource::Reference(_)
    ) {
        copy_patch(project, setup)?;
    }
    Ok(())
}
fn copy_patch(project: &mut DonderProject, setup: &SetupId) -> Result<(), EditRejection> {
    let current = project
        .get::<crate::Setup>(*setup)
        .ok_or(EditRejection::Missing(ObjectKind::Setup))?;
    let ValueSource::Reference(id) = &current.patch else {
        return Err(EditRejection::AlreadyIndependent(ObjectKind::Patch));
    };
    let mut value = project
        .get::<crate::Patch>(*id)
        .ok_or(EditRejection::Missing(ObjectKind::Patch))?
        .clone();
    value.id = PatchId(project.new_id());
    project
        .find_mut::<crate::Setup>(*setup)
        .ok_or(EditRejection::Missing(ObjectKind::Setup))?
        .patch = ValueSource::Inline(Box::new(value));
    Ok(())
}
fn copy_sequence(
    project: &mut DonderProject,
    index: usize,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), EditRejection> {
    let source = project
        .root
        .sequences
        .get(index)
        .ok_or(EditRejection::Missing(ObjectKind::Sequence))?;
    let MemberSource::Reference(id) = *source else {
        return Err(EditRejection::AlreadyIndependent(ObjectKind::Sequence));
    };
    let name = next_local_name(
        project.root.sequences.iter(),
        preferred_name(name_of, id)?.as_str(),
    );
    let mut value = project
        .get::<crate::Sequence>(id)
        .ok_or(EditRejection::Missing(ObjectKind::Sequence))?
        .clone();
    value.id = SequenceId(project.new_id());
    project.root.sequences[index] = MemberSource::Inline {
        name,
        value: Box::new(value),
    };
    Ok(())
}
fn copy_rig(
    project: &mut DonderProject,
    index: usize,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), EditRejection> {
    let source = project
        .root
        .rigs
        .get(index)
        .ok_or(EditRejection::Missing(ObjectKind::Rig))?;
    let MemberSource::Reference(id) = *source else {
        return Err(EditRejection::AlreadyIndependent(ObjectKind::Rig));
    };
    let name = next_local_name(
        project.root.rigs.iter(),
        preferred_name(name_of, id)?.as_str(),
    );
    let mut value = project
        .get::<crate::Rig>(id)
        .ok_or(EditRejection::Missing(ObjectKind::Rig))?
        .clone();
    value.id = RigId(project.new_id());
    project.root.rigs[index] = MemberSource::Inline {
        name,
        value: Box::new(value),
    };
    Ok(())
}

/// Follow objects of the active setup that were copied or replaced: active
/// sequences and rigs that refer to one become independent before their
/// targets and controls change.
fn follow_active(
    project: &mut DonderProject,
    renames: &Renames,
    name_of: &dyn Fn(ObjectId) -> Option<Identifier>,
) -> Result<(), EditRejection> {
    for index in 0..project.root.rigs.len() {
        let id = project.root.rigs[index].id();
        let rig = project
            .get::<crate::Rig>(*id)
            .ok_or(EditRejection::Missing(ObjectKind::Rig))?;
        if !renames.moves(rig) {
            continue;
        }
        if matches!(project.root.rigs[index], MemberSource::Reference(_)) {
            copy_rig(project, index, name_of)?;
        }
        renames.apply(
            project.root.rigs[index]
                .inline_mut()
                .ok_or(EditRejection::Missing(ObjectKind::Rig))?,
        );
    }
    for index in 0..project.root.sequences.len() {
        let id = project.root.sequences[index].id();
        let sequence = project
            .get::<crate::Sequence>(*id)
            .ok_or(EditRejection::Missing(ObjectKind::Sequence))?;
        if !renames.moves(sequence) {
            continue;
        }
        if matches!(project.root.sequences[index], MemberSource::Reference(_)) {
            copy_sequence(project, index, name_of)?;
        }
        renames.apply(
            project.root.sequences[index]
                .inline_mut()
                .ok_or(EditRejection::Missing(ObjectKind::Sequence))?,
        );
    }
    Ok(())
}

/// Add an independently owned sequence to the active project.
pub fn add_sequence(
    project: &mut DonderProject,
    duration: donder_language::DonderDuration,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<SequenceId, crate::EditError> {
    project.checked_edit(|project| add_sequence_candidate(project, duration, frame_rate, color))
}

fn add_sequence_candidate(
    project: &mut DonderProject,
    duration: donder_language::DonderDuration,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<SequenceId, EditRejection> {
    use crate::sequence::*;
    if duration.is_zero() || frame_rate == 0 {
        return Err(EditRejection::NonPositiveTiming);
    }
    let name = next_local_name(project.root.sequences.iter(), "sequence");
    let id = SequenceId(project.new_id());
    let literal = |text: &str| {
        Identifier::new(text.to_string())
            .unwrap_or_else(|_| unreachable!("literal names are valid"))
    };
    let (layers, composition_graph) = default_composition(color);
    let sequence = Sequence {
        id,
        description: None,
        duration,
        frame_rate,
        audio: SequenceAudio::None,
        mark_collections: vec![MarkCollection {
            key: MarkCollectionKey {
                name: literal("marks"),
            },
            description: None,
            display_color: color,
            marks: Vec::new(),
        }],
        layers,
        effects: Vec::new(),
        composition_graph,
        automation_clips: Vec::new(),
    };
    project.root.sequences.push(MemberSource::Inline {
        name,
        value: Box::new(sequence),
    });
    Ok(id)
}

/// Add an independently owned rig to the active project.
pub fn add_rig(
    project: &mut DonderProject,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<RigId, crate::EditError> {
    project.checked_edit(|project| {
        let name = next_local_name(project.root.rigs.iter(), "rig");
        let id = RigId(project.new_id());
        let (layers, composition_graph) = default_composition(color);
        project.root.rigs.push(MemberSource::Inline {
            name,
            value: Box::new(Rig {
                id,
                description: None,
                frame_rate,
                live_marks: Vec::new(),
                layers,
                voices: Vec::new(),
                switches: Vec::new(),
                mappings: Vec::new(),
                composition_graph,
            }),
        });
        Ok(id)
    })
}

/// A new sequence's or rig's composition: the default layer drawn straight
/// to the output.
fn default_composition(
    color: donder_runtime_types::Color,
) -> (
    Vec<crate::sequence::SequenceLayer>,
    crate::sequence::SequenceCompositionGraph,
) {
    use crate::sequence::*;
    let layer_id = SequenceLayerId(0);
    let layers = vec![SequenceLayer {
        id: layer_id.clone(),
        name: Identifier::new("default".to_string())
            .unwrap_or_else(|_| unreachable!("literal names are valid")),
        description: None,
        color,
        enabled: true,
    }];
    let graph = SequenceCompositionGraph {
        nodes: vec![
            CompositionGraphNode {
                id: CompositionGraphNodeId(1),
                position: GraphNodePosition { x: 80.0, y: 80.0 },
                kind: CompositionGraphNodeKind::Layer { layer_id },
            },
            CompositionGraphNode {
                id: CompositionGraphNodeId(2),
                position: GraphNodePosition { x: 420.0, y: 80.0 },
                kind: CompositionGraphNodeKind::Output,
            },
        ],
        edges: vec![EffectGraphEdge {
            from: CompositionGraphNodeId(1),
            from_port: GraphPortId("output".to_string()),
            to: CompositionGraphNodeId(2),
            to_port: GraphPortId("input".to_string()),
        }],
    };
    (layers, graph)
}
