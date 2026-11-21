//! Every reference from one object to another, listed once. A reference is a
//! link to an object written elsewhere; objects owned in place are not
//! references of their owner.
use super::{DonderProject, ProjectRoot};
use crate::console::{Action, Console, LengthSource, Play, PlayLength};
use crate::effect::{Clip, ParamValue};
use crate::identity::ObjectId;
use crate::layout::{Layout, LayoutFixtureKind};
use crate::ownership::{Owned, OwnedMember};
use crate::patch::Patch;
use crate::sequence::{CompositionGraphNodeKind, Sequence, SequenceCompositionGraph};
use crate::setup::Setup;

/// Sees each reference as `(referrer, target)`.
type Visit<'a> = dyn FnMut(ObjectId, ObjectId) + 'a;

fn root(owner: ObjectId, root: &ProjectRoot, f: &mut Visit<'_>) {
    if let Owned::Reference(id) = &root.setup {
        f(owner, (*id).into());
    }
    for source in &root.sequences {
        if let OwnedMember::Reference(id) = source {
            f(owner, (*id).into());
        }
    }
    for source in &root.consoles {
        if let OwnedMember::Reference(id) = source {
            f(owner, (*id).into());
        }
    }
}

fn setup(owner: ObjectId, setup: &Setup, f: &mut Visit<'_>) {
    if let Owned::Reference(id) = &setup.layout {
        f(owner, (*id).into());
    }
    if let Owned::Reference(id) = &setup.patch {
        f(owner, (*id).into());
    }
    for source in &setup.controllers {
        if let OwnedMember::Reference(id) = source {
            f(owner, (*id).into());
        }
    }
    for source in &setup.input_devices {
        if let OwnedMember::Reference(id) = source {
            f(owner, (*id).into());
        }
    }
}

fn layout(owner: ObjectId, layout: &Layout, f: &mut Visit<'_>) {
    for fixture in &layout.fixtures {
        if let LayoutFixtureKind::Fixture {
            definition: Owned::Reference(id),
            ..
        } = &fixture.kind
        {
            f(owner, (*id).into());
        }
    }
}

fn patch(owner: ObjectId, patch: &Patch, f: &mut Visit<'_>) {
    for route in &patch.routes {
        f(owner, route.target.layout.into());
        f(owner, route.controller.into());
    }
}

fn sequence(owner: ObjectId, sequence: &Sequence, f: &mut Visit<'_>) {
    for clip in &sequence.clips {
        self::clip(owner, clip, f);
    }
    for clip in &sequence.automation_clips {
        f(owner, clip.row_target.layout.into());
    }
    graph(owner, &sequence.composition_graph, f);
}

fn clip(owner: ObjectId, clip: &Clip, f: &mut Visit<'_>) {
    f(owner, clip.target.layout.into());
    f(owner, clip.definition.into());
    for value in clip.param_overrides.values() {
        param(owner, value, f);
    }
}

fn console(owner: ObjectId, console: &Console, f: &mut Visit<'_>) {
    for clip in &console.clips {
        f(owner, clip.target.layout.into());
        f(owner, clip.definition.into());
        for value in clip.param_overrides.values() {
            param(owner, value, f);
        }
    }
    for on in &console.events {
        f(owner, on.event.control().device.into());
        for action in &on.actions {
            if let Action::Play(Play {
                length:
                    PlayLength::Scaled {
                        by: LengthSource::Axis(control),
                        ..
                    },
                ..
            }) = action
            {
                f(owner, control.device.into());
            }
        }
    }
    graph(owner, &console.composition_graph, f);
}

fn graph(owner: ObjectId, graph: &SequenceCompositionGraph, f: &mut Visit<'_>) {
    for node in &graph.nodes {
        if let CompositionGraphNodeKind::Operator(operator) = &node.kind {
            f(owner, operator.operator.into());
            for value in operator.params.values() {
                param(owner, value, f);
            }
        }
    }
}

/// A parameter value's curve and gradient references.
fn param(owner: ObjectId, value: &ParamValue, f: &mut Visit<'_>) {
    match value {
        ParamValue::Curve(Owned::Reference(id)) => f(owner, (*id).into()),
        ParamValue::Gradient(Owned::Reference(id)) => f(owner, (*id).into()),
        ParamValue::Array(values) => {
            for value in values {
                param(owner, value, f);
            }
        }
        ParamValue::Int(_)
        | ParamValue::Float(_)
        | ParamValue::Bool(_)
        | ParamValue::Color(_)
        | ParamValue::Enum { .. }
        | ParamValue::Marks(_)
        | ParamValue::Curve(Owned::Inline(_))
        | ParamValue::Gradient(Owned::Inline(_)) => {}
    }
}

impl DonderProject {
    /// Every object-to-object reference, as `(referrer, target)`. The referrer
    /// is the object that holds the reference, whether it is declared or owned
    /// in place.
    pub fn for_each_reference(&self, visit: &mut dyn FnMut(ObjectId, ObjectId)) {
        root(self.root.id.into(), &self.root, visit);
        for value in self.objects::<Setup>() {
            setup(value.id.into(), value, visit);
        }
        for value in self.objects::<Layout>() {
            layout(value.id.into(), value, visit);
        }
        for value in self.objects::<Patch>() {
            patch(value.id.into(), value, visit);
        }
        for value in self.objects::<Sequence>() {
            sequence(value.id.into(), value, visit);
        }
        for value in self.objects::<Console>() {
            console(value.id.into(), value, visit);
        }
    }
}
