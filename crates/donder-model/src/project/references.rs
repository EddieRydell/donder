//! Every reference from one object to another, listed once. A reference names
//! another object by its id.
use super::{DonderProject, ProjectRoot};
use crate::composition::{CompositionGraph, CompositionGraphNodeKind};
use crate::console::{Action, Console, LengthSource, Play, PlayLength};
use crate::effect::{Clip, ParamValue};
use crate::identity::ObjectId;
use crate::layout::{Layout, LayoutFixtureKind};
use crate::sequence::Sequence;

/// Sees each reference as `(referrer, target)`.
type Visit<'a> = dyn FnMut(ObjectId, ObjectId) + 'a;

fn root(owner: ObjectId, root: &ProjectRoot, f: &mut Visit<'_>) {
    f(owner, root.layout.into());
    for route in &root.routes {
        f(owner, route.controller.into());
    }
}

fn layout(owner: ObjectId, layout: &Layout, f: &mut Visit<'_>) {
    for fixture in &layout.fixtures {
        if let LayoutFixtureKind::Fixture { definition, .. } = &fixture.kind {
            f(owner, (*definition).into());
        }
    }
}

fn sequence(owner: ObjectId, sequence: &Sequence, f: &mut Visit<'_>) {
    for clip in &sequence.composition.clips {
        self::clip(owner, clip, f);
    }
    graph(owner, &sequence.composition.graph, f);
}

fn clip(owner: ObjectId, clip: &Clip, f: &mut Visit<'_>) {
    let Some(effect) = clip.effect() else {
        return;
    };
    f(owner, effect.definition.into());
    for value in effect.params.values() {
        param(owner, value, f);
    }
}

fn console(owner: ObjectId, console: &Console, f: &mut Visit<'_>) {
    for clip in &console.composition.clips {
        self::clip(owner, clip, f);
    }
    for on in &console.events {
        f(owner, on.control.device.into());
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
    graph(owner, &console.composition.graph, f);
}

fn graph(owner: ObjectId, graph: &CompositionGraph, f: &mut Visit<'_>) {
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
        ParamValue::Curve(crate::CurveValue::Linked(id)) => f(owner, (*id).into()),
        ParamValue::Gradient(crate::GradientValue::Linked(id)) => f(owner, (*id).into()),
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
        | ParamValue::Text(_)
        | ParamValue::Font(_)
        | ParamValue::Image(_)
        | ParamValue::Curve(crate::CurveValue::Literal(_))
        | ParamValue::Gradient(crate::GradientValue::Literal(_)) => {}
    }
}

impl DonderProject {
    /// Every object-to-object reference, as `(referrer, target)`. The referrer
    /// is the object that holds the reference.
    pub fn for_each_reference(&self, visit: &mut dyn FnMut(ObjectId, ObjectId)) {
        root(self.root.id.into(), &self.root, visit);
        for (value_id, value) in self.objects::<Layout>() {
            layout(value_id.into(), value, visit);
        }
        for (value_id, value) in self.objects::<Sequence>() {
            sequence(value_id.into(), value, visit);
        }
        for (value_id, value) in self.objects::<Console>() {
            console(value_id.into(), value, visit);
        }
    }
}

impl DonderProject {
    /// Every asset file an object uses, as `(user, asset)`: a sequence's
    /// audio and the fonts its own or a console's parameters name.
    pub fn for_each_asset(&self, visit: &mut dyn FnMut(ObjectId, &crate::AssetId)) {
        fn params<'a>(
            owner: ObjectId,
            values: impl Iterator<Item = &'a ParamValue>,
            visit: &mut dyn FnMut(ObjectId, &crate::AssetId),
        ) {
            for value in values {
                for asset in value.assets() {
                    visit(owner, asset);
                }
            }
        }
        fn operators(
            owner: ObjectId,
            graph: &CompositionGraph,
            visit: &mut dyn FnMut(ObjectId, &crate::AssetId),
        ) {
            for node in &graph.nodes {
                if let CompositionGraphNodeKind::Operator(operator) = &node.kind {
                    params(owner, operator.params.values(), visit);
                }
            }
        }
        for (sequence_id, sequence) in self.objects::<Sequence>() {
            let owner = sequence_id.into();
            if let crate::SequenceAudio::Asset(asset) = &sequence.audio {
                visit(owner, asset);
            }
            for effect in sequence
                .composition
                .clips
                .iter()
                .filter_map(|clip| clip.effect())
            {
                params(owner, effect.params.values(), visit);
            }
            operators(owner, &sequence.composition.graph, visit);
        }
        for (console_id, console) in self.objects::<Console>() {
            let owner = console_id.into();
            for effect in console
                .composition
                .clips
                .iter()
                .filter_map(|clip| clip.effect())
            {
                params(owner, effect.params.values(), visit);
            }
            operators(owner, &console.composition.graph, visit);
        }
    }
}
