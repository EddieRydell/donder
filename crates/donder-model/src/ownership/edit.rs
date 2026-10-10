//! Ownership edits preserve accepted-project invariants. The caller registers
//! reusable source identities before promoting values.
mod duplicate;
mod link;
use super::ValueSource;
use crate::{
    controller::Controller,
    fixture::{FixtureDefinitionId, FixtureSource},
    identity::{ObjectIdentity, OwnedObjectSlot, SourceIdentity},
    layout::{FixtureInstanceId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId},
    patch::{Patch, PatchId},
    project::DonderProject,
    sequence::{Sequence, SequenceId},
    setup::{Setup, SetupId},
};
pub use duplicate::duplicate_layout_fixture;
pub use link::use_existing;

#[derive(Clone, Debug)]
pub enum OwnershipSite {
    ProjectSetup,
    ProjectSequence(usize),
    SetupLayout(SetupId),
    SetupPatch(SetupId),
    SetupController {
        setup: SetupId,
        index: usize,
    },
    LayoutFixture {
        layout: LayoutId,
        fixture: FixtureInstanceId,
    },
}

trait Rebase {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity);
}
impl Rebase for Layout {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity) {
        self.id.0.rebase(from, to);
    }
}
impl Rebase for Controller {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity) {
        self.id.0.rebase(from, to);
    }
}
impl Rebase for Patch {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity) {
        self.id.0.rebase(from, to);
        for route in &mut self.routes {
            route.target.layout.0.rebase(from, to);
            route.controller.0.rebase(from, to);
        }
    }
}
impl Rebase for Sequence {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity) {
        self.id.0.rebase(from, to);
        for clip in &mut self.automation_clips {
            clip.row_target.layout.0.rebase(from, to);
        }
        rebase_effect_layouts(&mut self.effects, from, to);
    }
}
impl Rebase for Setup {
    fn rebase(&mut self, from: &ObjectIdentity, to: &ObjectIdentity) {
        self.id.0.rebase(from, to);
        if let ValueSource::Inline(layout) = &mut self.layout {
            layout.rebase(from, to);
        }
        if let ValueSource::Inline(patch) = &mut self.patch {
            patch.rebase(from, to);
        }
        for controller in &mut self.controllers {
            if let ValueSource::Inline(controller) = controller {
                controller.rebase(from, to);
            }
        }
    }
}

/// A name for a newly owned sequence or controller, unique among its owner's
/// owned members of that kind.
fn next_local_name<'a>(
    identities: impl Iterator<Item = &'a ObjectIdentity>,
    sequence: bool,
    prefix: &str,
) -> donder_runtime_types::Identifier {
    let taken = identities
        .filter_map(|id| match id.owned_path().last() {
            Some(OwnedObjectSlot::Sequence(name)) if sequence => Some(name.as_str()),
            Some(OwnedObjectSlot::Controller(name)) if !sequence => Some(name.as_str()),
            _ => None,
        })
        .collect::<std::collections::HashSet<_>>();
    donder_language::unique_name(prefix, |name| taken.contains(name))
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

fn retarget(project: &mut DonderProject, from: &ObjectIdentity, to: &ObjectIdentity) {
    for patch in project.patches_mut() {
        for route in &mut patch.routes {
            route.target.layout.0.rebase(from, to);
            route.controller.0.rebase(from, to);
        }
    }
    for sequence in project.sequences_mut() {
        for clip in &mut sequence.automation_clips {
            clip.row_target.layout.0.rebase(from, to);
        }
        rebase_effect_layouts(&mut sequence.effects, from, to);
    }
}

fn promote<T, I>(source: &mut ValueSource<Box<T>, I>, id: I) -> Result<Box<T>, String>
where
    T: Rebase + super::Identified<Id = I>,
    I: AsRef<ObjectIdentity> + Clone,
{
    let ValueSource::Inline(value) = source else {
        return Err("This value is already reusable.".into());
    };
    let from = value.id().as_ref().clone();
    value.rebase(&from, id.as_ref());
    let ValueSource::Inline(value) = std::mem::replace(source, ValueSource::Reference(id)) else {
        return Err("Owned value was not found.".into());
    };
    Ok(value)
}

pub fn make_reusable(
    project: &mut DonderProject,
    site: &OwnershipSite,
    destination: SourceIdentity,
) -> Result<(), String> {
    project.checked_edit(|project| make_reusable_candidate(project, site, destination))
}

fn make_reusable_candidate(
    project: &mut DonderProject,
    site: &OwnershipSite,
    destination: SourceIdentity,
) -> Result<(), String> {
    let to = ObjectIdentity::from(destination.clone());
    let occupied = match site {
        OwnershipSite::ProjectSetup => project.setups.contains_key(&SetupId(to.clone())),
        OwnershipSite::ProjectSequence(_) => {
            project.sequences.contains_key(&SequenceId(to.clone()))
        }
        OwnershipSite::SetupLayout(_) => project.layouts.contains_key(&LayoutId(to.clone())),
        OwnershipSite::SetupPatch(_) => project.patches.contains_key(&PatchId(to.clone())),
        OwnershipSite::SetupController { .. } => project
            .controllers
            .contains_key(&crate::controller::ControllerId(to.clone())),
        OwnershipSite::LayoutFixture { .. } => project
            .definitions
            .fixtures
            .definitions
            .contains_key(&FixtureDefinitionId(destination.clone())),
    };
    if occupied {
        return Err("A reusable value already exists at this source identity.".into());
    }
    let from = match site {
        OwnershipSite::ProjectSetup => {
            let from = project.root.setup.id().0.clone();
            let value = promote(&mut project.root.setup, SetupId(to.clone()))?;
            project.setups.insert(value.id.clone(), *value);
            from
        }
        OwnershipSite::ProjectSequence(index) => {
            let source = project
                .root
                .sequences
                .get_mut(*index)
                .ok_or("Sequence was not found.")?;
            let from = source.id().0.clone();
            let value = promote(source, SequenceId(to.clone()))?;
            project.sequences.insert(value.id.clone(), *value);
            from
        }
        OwnershipSite::SetupLayout(setup) => {
            let source = &mut project
                .setup_mut(setup)
                .ok_or("Setup was not found.")?
                .layout;
            let from = source.id().0.clone();
            let value = promote(source, LayoutId(to.clone()))?;
            project.layouts.insert(value.id.clone(), *value);
            from
        }
        OwnershipSite::SetupPatch(setup) => {
            let source = &mut project
                .setup_mut(setup)
                .ok_or("Setup was not found.")?
                .patch;
            let from = source.id().0.clone();
            let value = promote(source, PatchId(to.clone()))?;
            project.patches.insert(value.id.clone(), *value);
            from
        }
        OwnershipSite::SetupController { setup, index } => {
            let source = project
                .setup_mut(setup)
                .ok_or("Setup was not found.")?
                .controllers
                .get_mut(*index)
                .ok_or("Controller was not found.")?;
            let from = source.id().0.clone();
            let value = promote(source, crate::controller::ControllerId(to.clone()))?;
            project.controllers.insert(value.id.clone(), *value);
            from
        }
        OwnershipSite::LayoutFixture { layout, fixture } => {
            let source = fixture_mut(
                &mut project
                    .layout_mut(layout)
                    .ok_or("Layout was not found.")?
                    .fixtures,
                *fixture,
            )
            .ok_or("Fixture was not found.")?;
            if matches!(source, FixtureSource::Reference(_)) {
                return Err("This fixture is already reusable.".into());
            }
            let id = FixtureDefinitionId(destination);
            let FixtureSource::Inline(value) =
                std::mem::replace(source, FixtureSource::Reference(id.clone()))
            else {
                return Err("Owned fixture was not found.".into());
            };
            project.definitions.fixtures.definitions.insert(id, value);
            layout.0.owned(OwnedObjectSlot::Fixture(fixture.0))
        }
    };
    retarget(project, &from, &to);
    Ok(())
}

/// Copy linked values into their ownership slot. Related routes and active
/// sequence targets follow a copied layout/controller without editing shared sources.
pub fn make_independent(project: &mut DonderProject, site: &OwnershipSite) -> Result<(), String> {
    project.checked_edit(|project| make_independent_candidate(project, site))
}

fn make_independent_candidate(
    project: &mut DonderProject,
    site: &OwnershipSite,
) -> Result<(), String> {
    match site {
        OwnershipSite::ProjectSetup => {
            let ValueSource::Reference(id) = &project.root.setup else {
                return Err("This setup is already independent.".into());
            };
            let from = id.0.clone();
            let to = ObjectIdentity::from(project.root.id.0.clone()).owned(OwnedObjectSlot::Setup);
            let mut value = project.setup(id).ok_or("Setup was not found.")?.clone();
            let old_layout = value.layout.id().clone();
            value.rebase(&from, &to);
            let new_layout = value.layout.id().clone();
            project.root.setup = ValueSource::Inline(Box::new(value));
            let setup = SetupId(to.clone());
            let patch_id = project
                .setup(&setup)
                .ok_or("Setup was not found.")?
                .patch
                .id()
                .clone();
            let patch = project.patch(&patch_id).ok_or("Patch was not found.")?;
            let needs_retarget = patch.routes.iter().any(|route| {
                let mut layout = route.target.layout.0.clone();
                let mut controller = route.controller.0.clone();
                layout.rebase(&from, &to);
                controller.rebase(&from, &to);
                layout != route.target.layout.0 || controller != route.controller.0
            });
            if needs_retarget {
                copy_patch_if_linked(project, &setup)?;
                let patch_id = project
                    .setup(&setup)
                    .ok_or("Setup was not found.")?
                    .patch
                    .id()
                    .clone();
                let patch = project.patch_mut(&patch_id).ok_or("Patch was not found.")?;
                for route in &mut patch.routes {
                    route.target.layout.0.rebase(&from, &to);
                    route.controller.0.rebase(&from, &to);
                }
            }
            if old_layout != new_layout {
                retarget_active_sequences(project, &old_layout, &new_layout)?;
            }
        }
        OwnershipSite::ProjectSequence(index) => {
            copy_sequence(project, *index)?;
        }
        OwnershipSite::SetupPatch(setup) => {
            copy_patch(project, setup)?;
        }
        OwnershipSite::SetupLayout(setup) => {
            let current = project.setup(setup).ok_or("Setup was not found.")?;
            let ValueSource::Reference(id) = &current.layout else {
                return Err("This layout is already independent.".into());
            };
            let from = id.clone();
            let to = LayoutId(setup.0.owned(OwnedObjectSlot::Layout));
            let mut value = project.layout(id).ok_or("Layout was not found.")?.clone();
            value.rebase(&from.0, &to.0);
            copy_patch_if_linked(project, setup)?;
            let current = project.setup_mut(setup).ok_or("Setup was not found.")?;
            current.layout = ValueSource::Inline(Box::new(value));
            let patch = current
                .patch
                .inline_mut()
                .ok_or("Independent patch was not found.")?;
            for route in &mut patch.routes {
                route.target.layout.0.rebase(&from.0, &to.0);
            }
            if project.root.setup.id() == setup {
                retarget_active_sequences(project, &from, &to)?;
            }
        }
        OwnershipSite::SetupController { setup, index } => {
            let current = project.setup(setup).ok_or("Setup was not found.")?;
            let source = current
                .controllers
                .get(*index)
                .ok_or("Controller was not found.")?;
            let ValueSource::Reference(id) = source else {
                return Err("This controller is already independent.".into());
            };
            let from = id.0.clone();
            let next = next_local_name(
                current.controllers.iter().map(|source| &source.id().0),
                false,
                from.root_source().object(),
            );
            let to = setup.0.owned(OwnedObjectSlot::Controller(next));
            let mut value = project
                .controller(id)
                .ok_or("Controller was not found.")?
                .clone();
            value.rebase(&from, &to);
            copy_patch_if_linked(project, setup)?;
            let current = project.setup_mut(setup).ok_or("Setup was not found.")?;
            current.controllers[*index] = ValueSource::Inline(Box::new(value));
            let patch = current
                .patch
                .inline_mut()
                .ok_or("Independent patch was not found.")?;
            for route in &mut patch.routes {
                route.controller.0.rebase(&from, &to);
            }
        }
        OwnershipSite::LayoutFixture { layout, fixture } => {
            let placement = project
                .layout(layout)
                .and_then(|layout| layout.fixture(*fixture))
                .ok_or("Fixture was not found.")?;
            let LayoutFixtureKind::Fixture {
                definition: FixtureSource::Reference(id),
                ..
            } = &placement.kind
            else {
                return Err("This fixture is already independent.".into());
            };
            let value = project
                .definitions
                .fixtures
                .definitions
                .get(id)
                .ok_or("Fixture source was not found.")?
                .clone();
            *fixture_mut(
                &mut project
                    .layout_mut(layout)
                    .ok_or("Layout was not found.")?
                    .fixtures,
                *fixture,
            )
            .ok_or("Fixture was not found.")? = FixtureSource::Inline(value);
        }
    }
    Ok(())
}

fn copy_patch_if_linked(project: &mut DonderProject, setup: &SetupId) -> Result<(), String> {
    if matches!(
        project.setup(setup).ok_or("Setup was not found.")?.patch,
        ValueSource::Reference(_)
    ) {
        copy_patch(project, setup)?;
    }
    Ok(())
}
fn copy_patch(project: &mut DonderProject, setup: &SetupId) -> Result<(), String> {
    let current = project.setup(setup).ok_or("Setup was not found.")?;
    let ValueSource::Reference(id) = &current.patch else {
        return Err("This patch is already independent.".into());
    };
    let from = id.0.clone();
    let to = setup.0.owned(OwnedObjectSlot::Patch);
    let mut value = project.patch(id).ok_or("Patch was not found.")?.clone();
    value.rebase(&from, &to);
    project
        .setup_mut(setup)
        .ok_or("Setup was not found.")?
        .patch = ValueSource::Inline(Box::new(value));
    Ok(())
}
fn copy_sequence(project: &mut DonderProject, index: usize) -> Result<(), String> {
    let source = project
        .root
        .sequences
        .get(index)
        .ok_or("Sequence was not found.")?;
    let ValueSource::Reference(id) = source else {
        return Err("This sequence is already independent.".into());
    };
    let from = id.0.clone();
    let next = next_local_name(
        project.root.sequences.iter().map(|source| &source.id().0),
        true,
        from.root_source().object(),
    );
    let to = ObjectIdentity::from(project.root.id.0.clone()).owned(OwnedObjectSlot::Sequence(next));
    let mut value = project
        .sequence(id)
        .ok_or("Sequence was not found.")?
        .clone();
    value.rebase(&from, &to);
    project.root.sequences[index] = ValueSource::Inline(Box::new(value));
    Ok(())
}
fn retarget_active_sequences(
    project: &mut DonderProject,
    from: &LayoutId,
    to: &LayoutId,
) -> Result<(), String> {
    for index in 0..project.root.sequences.len() {
        let id = project.root.sequences[index].id();
        let sequence = project.sequence(id).ok_or("Sequence was not found.")?;
        if !sequence
            .effects
            .iter()
            .any(|effect| &effect.target.layout == from)
            && !sequence
                .automation_clips
                .iter()
                .any(|clip| &clip.row_target.layout == from)
        {
            continue;
        }
        if matches!(project.root.sequences[index], ValueSource::Reference(_)) {
            copy_sequence(project, index)?;
        }
        let value = project.root.sequences[index]
            .inline_mut()
            .ok_or("Independent sequence was not found.")?;
        for clip in &mut value.automation_clips {
            if &clip.row_target.layout == from {
                clip.row_target.layout = to.clone();
            }
        }
        for effect in &mut value.effects {
            if &effect.target.layout == from {
                std::sync::Arc::make_mut(effect).target.layout = to.clone();
            }
        }
    }
    Ok(())
}

/// Add an independently owned sequence to the active project.
pub fn add_sequence(
    project: &mut DonderProject,
    duration: donder_language::DonderDuration,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<SequenceId, String> {
    project.checked_edit(|project| add_sequence_candidate(project, duration, frame_rate, color))
}

fn add_sequence_candidate(
    project: &mut DonderProject,
    duration: donder_language::DonderDuration,
    frame_rate: u32,
    color: donder_runtime_types::Color,
) -> Result<SequenceId, String> {
    use crate::sequence::*;
    if duration.as_seconds_f32() <= 0.0 || !duration.as_seconds_f32().is_finite() || frame_rate == 0
    {
        return Err("Sequence duration and frame rate must be positive.".into());
    }
    let next = next_local_name(
        project.root.sequences.iter().map(|source| &source.id().0),
        true,
        "sequence",
    );
    let id = SequenceId(
        ObjectIdentity::from(project.root.id.0.clone()).owned(OwnedObjectSlot::Sequence(next)),
    );
    let layer_id = SequenceLayerId(0);
    let name = |text: &str| {
        donder_runtime_types::Identifier::new(text.to_string())
            .unwrap_or_else(|_| unreachable!("literal names are valid"))
    };
    let sequence = Sequence {
        id: id.clone(),
        description: None,
        duration,
        frame_rate,
        audio: SequenceAudio::None,
        mark_collections: vec![MarkCollection {
            key: MarkCollectionKey {
                name: name("marks"),
            },
            description: None,
            display_color: color,
            marks: Vec::new(),
        }],
        layers: vec![SequenceLayer {
            id: layer_id.clone(),
            name: name("default"),
            description: None,
            color,
            enabled: true,
        }],
        effects: Vec::new(),
        composition_graph: SequenceCompositionGraph {
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
        },
        automation_clips: Vec::new(),
    };
    project
        .root
        .sequences
        .push(ValueSource::Inline(Box::new(sequence)));
    Ok(id)
}

/// Rebase clip targets, detaching only the clips whose layout moves.
fn rebase_effect_layouts(
    effects: &mut [std::sync::Arc<crate::effect::EffectInst>],
    from: &ObjectIdentity,
    to: &ObjectIdentity,
) {
    for effect in effects {
        let mut layout = effect.target.layout.0.clone();
        layout.rebase(from, to);
        if layout != effect.target.layout.0 {
            std::sync::Arc::make_mut(effect).target.layout.0 = layout;
        }
    }
}
