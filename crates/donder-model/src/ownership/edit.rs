//! Ownership edits preserve accepted-project invariants. Objects keep their ids
//! when they change owners. The caller names promoted values in its documents.
mod duplicate;
use super::{Owned, OwnedMember};
use crate::{
    console::{Console, ConsoleId},
    fixture::{FixtureDefinitionId, OwnedFixture},
    identity::{ObjectId, ObjectKind},
    layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId},
    project::{DonderProject, EditRejection},
    sequence::SequenceId,
    setup::SetupId,
};
use donder_runtime_types::Identifier;
pub use duplicate::duplicate_layout_fixture;

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
    ProjectConsole(usize),
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
                    Kind::Console => Slot::ProjectConsole(position(
                        &mut root.consoles.iter().map(|source| (*source.id()).into()),
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

/// A name for a newly owned collection member, unique among its owner's
/// owned members of that kind.
fn next_local_name<'a, T: 'a, I: 'a>(
    members: impl Iterator<Item = &'a OwnedMember<T, I>>,
    prefix: &str,
) -> Identifier {
    let taken = members
        .filter_map(OwnedMember::name)
        .map(Identifier::as_str)
        .collect::<std::collections::HashSet<_>>();
    donder_language::unique_name(prefix, |name| taken.contains(name))
}

fn fixture_mut(fixtures: &mut [LayoutFixture], id: FixtureInstanceId) -> Option<&mut OwnedFixture> {
    fixtures
        .iter_mut()
        .find(|fixture| fixture.id == id)
        .and_then(|fixture| match &mut fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => Some(definition),
            LayoutFixtureKind::Group { .. } => None,
        })
}

fn promote<T, I: Copy>(source: &mut Owned<Box<T>, I>) -> Result<Box<T>, EditRejection>
where
    T: super::Identified<Id = I>,
{
    let Owned::Inline(value) = source else {
        return Err(EditRejection::AlreadyReusable);
    };
    let id = *value.id();
    let Owned::Inline(value) = std::mem::replace(source, Owned::Reference(id)) else {
        return Err(EditRejection::MissingItem);
    };
    Ok(value)
}

fn promote_member<T, I: Copy>(source: &mut OwnedMember<T, I>) -> Result<Box<T>, EditRejection>
where
    T: super::Identified<Id = I>,
{
    let OwnedMember::Inline { value, .. } = source else {
        return Err(EditRejection::AlreadyReusable);
    };
    let id = *value.id();
    let OwnedMember::Inline { value, .. } = std::mem::replace(source, OwnedMember::Reference(id))
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
        Slot::ProjectConsole(index) => {
            let source = project
                .root
                .consoles
                .get_mut(*index)
                .ok_or(EditRejection::Missing(ObjectKind::Console))?;
            let value = promote_member(source)?;
            let id = value.id;
            project.consoles.insert(id, *value);
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
            if matches!(source, OwnedFixture::Reference(_)) {
                return Err(EditRejection::AlreadyReusable);
            }
            let OwnedFixture::Inline(value) =
                std::mem::replace(source, OwnedFixture::Reference(id))
            else {
                return Err(EditRejection::MissingItem);
            };
            project.fixtures.insert(id, value);
            id.into()
        }
    })
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
        clips: Vec::new(),
        composition_graph,
        automation_clips: Vec::new(),
    };
    project.root.sequences.push(OwnedMember::Inline {
        name,
        value: Box::new(sequence),
    });
    Ok(id)
}

/// Add an independently owned console to the active project.
pub fn add_console(
    project: &mut DonderProject,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<ConsoleId, crate::EditError> {
    project.checked_edit(|project| {
        let name = next_local_name(project.root.consoles.iter(), "console");
        let id = ConsoleId(project.new_id());
        let (layers, composition_graph) = default_composition(color);
        project.root.consoles.push(OwnedMember::Inline {
            name,
            value: Box::new(Console {
                id,
                description: None,
                frame_rate,
                live_marks: Vec::new(),
                layers,
                clips: Vec::new(),
                always: Vec::new(),
                events: Vec::new(),
                composition_graph,
            }),
        });
        Ok(id)
    })
}

/// A new sequence's or console's composition: the default layer drawn straight
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
                kind: CompositionGraphNodeKind::Root,
            },
        ],
        edges: vec![EffectGraphEdge {
            from: CompositionGraphNodeId(1),

            to: CompositionGraphNodeId(2),
            input: None,
        }],
    };
    (layers, graph)
}
