//! Every reference from one object to another, listed once. A reference is a
//! link to an object written elsewhere; objects owned in place are not
//! references of their owner.
use super::{DonderProject, ProjectRoot};
use crate::effect::{EffectInst, EffectParamValue};
use crate::identity::ObjectId;
use crate::layout::{Layout, LayoutFixtureKind};
use crate::ownership::{MemberSource, ValueSource};
use crate::patch::Patch;
use crate::rig::{MappingAction, Rig, RigEffect};
use crate::sequence::{CompositionGraphNodeKind, Sequence, SequenceCompositionGraph};
use crate::setup::Setup;

/// Sees each reference as `(referrer, target)`; it may change the target to
/// another object of the same kind.
type Visit<'a> = dyn FnMut(ObjectId, &mut ObjectId) + 'a;

/// Visit one reference held at `$place`, recording whether it would change.
macro_rules! visit_read {
    ($f:ident, $changed:ident, $owner:expr, $place:expr) => {{
        let before: ObjectId = ($place).into();
        let mut target = before;
        $f($owner, &mut target);
        *$changed |= target != before;
    }};
}

/// Visit one reference held at `$place` and store its new target.
macro_rules! visit_write {
    ($f:ident, $changed:ident, $owner:expr, $place:expr) => {{
        let before: ObjectId = ($place).into();
        let mut target = before;
        $f($owner, &mut target);
        if target != before {
            $place = target
                .try_into()
                .unwrap_or_else(|_| unreachable!("a reference keeps its kind"));
            *$changed = true;
        }
    }};
}

/// A clip is shared between project snapshots; it is visited in place only
/// when one of its references changes.
macro_rules! clip_read {
    ($f:ident, $changed:ident, $owner:expr, $clip:expr) => {
        super::read::effect($owner, &**$clip, $f, $changed)
    };
}

macro_rules! clip_write {
    ($f:ident, $changed:ident, $owner:expr, $clip:expr) => {{
        let mut moves = false;
        super::read::effect($owner, &**$clip, $f, &mut moves);
        if moves {
            super::write::effect($owner, std::sync::Arc::make_mut($clip), $f, $changed);
        }
    }};
}

/// The reference listing, read-only or in place; `$($m)*` is empty or `mut`.
macro_rules! walks {
    ($visit:ident, $clip:ident, $($m:tt)*) => {
        use super::*;

        pub(super) fn root(owner: ObjectId, root: & $($m)* ProjectRoot, f: &mut Visit<'_>, changed: &mut bool) {
            if let ValueSource::Reference(id) = & $($m)* root.setup {
                $visit!(f, changed, owner, *id);
            }
            for source in & $($m)* root.sequences {
                if let MemberSource::Reference(id) = source {
                    $visit!(f, changed, owner, *id);
                }
            }
            for source in & $($m)* root.rigs {
                if let MemberSource::Reference(id) = source {
                    $visit!(f, changed, owner, *id);
                }
            }
        }

        pub(super) fn setup(owner: ObjectId, setup: & $($m)* Setup, f: &mut Visit<'_>, changed: &mut bool) {
            if let ValueSource::Reference(id) = & $($m)* setup.layout {
                $visit!(f, changed, owner, *id);
            }
            if let ValueSource::Reference(id) = & $($m)* setup.patch {
                $visit!(f, changed, owner, *id);
            }
            for source in & $($m)* setup.controllers {
                if let MemberSource::Reference(id) = source {
                    $visit!(f, changed, owner, *id);
                }
            }
            for source in & $($m)* setup.input_devices {
                if let MemberSource::Reference(id) = source {
                    $visit!(f, changed, owner, *id);
                }
            }
        }

        pub(super) fn layout(owner: ObjectId, layout: & $($m)* Layout, f: &mut Visit<'_>, changed: &mut bool) {
            for fixture in & $($m)* layout.fixtures {
                if let LayoutFixtureKind::Fixture {
                    definition: ValueSource::Reference(id),
                    ..
                } = & $($m)* fixture.kind
                {
                    $visit!(f, changed, owner, *id);
                }
            }
        }

        pub(super) fn patch(owner: ObjectId, patch: & $($m)* Patch, f: &mut Visit<'_>, changed: &mut bool) {
            for route in & $($m)* patch.routes {
                $visit!(f, changed, owner, route.target.layout);
                $visit!(f, changed, owner, route.controller);
            }
        }

        pub(super) fn sequence(owner: ObjectId, sequence: & $($m)* Sequence, f: &mut Visit<'_>, changed: &mut bool) {
            for clip in & $($m)* sequence.effects {
                $clip!(f, changed, owner, clip);
            }
            for clip in & $($m)* sequence.automation_clips {
                $visit!(f, changed, owner, clip.row_target.layout);
            }
            graph(owner, & $($m)* sequence.composition_graph, f, changed);
        }

        pub(super) fn effect(owner: ObjectId, effect: & $($m)* EffectInst, f: &mut Visit<'_>, changed: &mut bool) {
            $visit!(f, changed, owner, effect.target.layout);
            $visit!(f, changed, owner, effect.definition);
            for value in (& $($m)* effect.param_overrides).values_mut_or_ref() {
                param(owner, value, f, changed);
            }
        }

        pub(super) fn rig(owner: ObjectId, rig: & $($m)* Rig, f: &mut Visit<'_>, changed: &mut bool) {
            for voice in & $($m)* rig.voices {
                rig_effect(owner, & $($m)* voice.effect, f, changed);
            }
            for switch in & $($m)* rig.switches {
                $visit!(f, changed, owner, switch.input.device);
            }
            for mapping in & $($m)* rig.mappings {
                $visit!(f, changed, owner, mapping.input.device);
                if let MappingAction::Trigger(trigger) = & $($m)* mapping.action {
                    rig_effect(owner, & $($m)* trigger.effect, f, changed);
                }
            }
            graph(owner, & $($m)* rig.composition_graph, f, changed);
        }

        fn rig_effect(owner: ObjectId, effect: & $($m)* RigEffect, f: &mut Visit<'_>, changed: &mut bool) {
            $visit!(f, changed, owner, effect.target.layout);
            $visit!(f, changed, owner, effect.definition);
            for value in (& $($m)* effect.param_overrides).values_mut_or_ref() {
                param(owner, value, f, changed);
            }
        }

        fn graph(owner: ObjectId, graph: & $($m)* SequenceCompositionGraph, f: &mut Visit<'_>, changed: &mut bool) {
            for node in & $($m)* graph.nodes {
                if let CompositionGraphNodeKind::Operator(operator) = & $($m)* node.kind {
                    $visit!(f, changed, owner, operator.operator);
                    for value in (& $($m)* operator.params).values_mut_or_ref() {
                        param(owner, value, f, changed);
                    }
                }
            }
        }

        /// A parameter value's curve and gradient references.
        fn param(owner: ObjectId, value: & $($m)* EffectParamValue, f: &mut Visit<'_>, changed: &mut bool) {
            match value {
                EffectParamValue::Curve(ValueSource::Reference(id)) => {
                    $visit!(f, changed, owner, *id)
                }
                EffectParamValue::Gradient(ValueSource::Reference(id)) => {
                    $visit!(f, changed, owner, *id)
                }
                EffectParamValue::Array(values) => {
                    for value in values {
                        param(owner, value, f, changed);
                    }
                }
                EffectParamValue::Int(_)
                | EffectParamValue::Float(_)
                | EffectParamValue::Bool(_)
                | EffectParamValue::Color(_)
                | EffectParamValue::Enum(_)
                | EffectParamValue::Marks(_)
                | EffectParamValue::Curve(ValueSource::Inline(_))
                | EffectParamValue::Gradient(ValueSource::Inline(_)) => {}
            }
        }
    };
}

/// `values()` for a read walk and `values_mut()` for an in-place one, so the
/// listing above reads the same in both.
trait ValuesOf<'a, V: 'a> {
    type Values: Iterator<Item = V>;
    fn values_mut_or_ref(self) -> Self::Values;
}

impl<'a, K, V> ValuesOf<'a, &'a V> for &'a indexmap::IndexMap<K, V> {
    type Values = indexmap::map::Values<'a, K, V>;
    fn values_mut_or_ref(self) -> Self::Values {
        self.values()
    }
}

impl<'a, K, V> ValuesOf<'a, &'a mut V> for &'a mut indexmap::IndexMap<K, V> {
    type Values = indexmap::map::ValuesMut<'a, K, V>;
    fn values_mut_or_ref(self) -> Self::Values {
        self.values_mut()
    }
}

mod read {
    walks!(visit_read, clip_read,);
}

mod write {
    walks!(visit_write, clip_write, mut);
}

/// Retarget the references every object of a kind holds. The read walk runs
/// first, so an object whose references stay the same is never detached from
/// the snapshots that share it.
macro_rules! retarget {
    ($project:expr, $f:ident, $walk:ident, $kind:ty) => {
        let ids: Vec<_> = $project.objects::<$kind>().map(|value| value.id).collect();
        for id in ids {
            let owner = id.into();
            let mut moves = false;
            if let Some(object) = $project.get::<$kind>(id) {
                read::$walk(owner, object, $f, &mut moves);
            }
            if moves && let Some(object) = $project.find_mut::<$kind>(id) {
                write::$walk(owner, object, $f, &mut false);
            }
        }
    };
}

impl DonderProject {
    /// Every object-to-object reference, as `(referrer, target)`. The referrer
    /// is the object that holds the reference, whether it is declared or owned
    /// in place.
    pub fn for_each_reference(&self, visit: &mut dyn FnMut(ObjectId, ObjectId)) {
        let f: &mut Visit<'_> = &mut |owner, target: &mut ObjectId| visit(owner, *target);
        let mut changed = false;
        let changed = &mut changed;
        read::root(self.root.id.into(), &self.root, f, changed);
        for setup in self.objects::<Setup>() {
            read::setup(setup.id.into(), setup, f, changed);
        }
        for layout in self.objects::<Layout>() {
            read::layout(layout.id.into(), layout, f, changed);
        }
        for patch in self.objects::<Patch>() {
            read::patch(patch.id.into(), patch, f, changed);
        }
        for sequence in self.objects::<Sequence>() {
            read::sequence(sequence.id.into(), sequence, f, changed);
        }
        for rig in self.objects::<Rig>() {
            read::rig(rig.id.into(), rig, f, changed);
        }
    }

    /// Point references at other objects: `retarget(referrer, target)` may
    /// replace the target with another object of its kind. It is called more
    /// than once for a reference, so it must answer the same each time. Only
    /// the objects and clips whose references change are detached.
    pub(crate) fn retarget_references(
        &mut self,
        retarget: &mut dyn FnMut(ObjectId, &mut ObjectId),
    ) {
        let f: &mut Visit<'_> = retarget;
        let mut moves = false;
        read::root(self.root.id.into(), &self.root, f, &mut moves);
        if moves {
            let mut changed = false;
            let owner = self.root.id.into();
            write::root(owner, &mut self.root, f, &mut changed);
        }
        retarget!(self, f, setup, Setup);
        retarget!(self, f, layout, Layout);
        retarget!(self, f, patch, Patch);
        retarget!(self, f, sequence, Sequence);
        retarget!(self, f, rig, Rig);
    }
}

/// An object whose own references can be read and retargeted alone.
pub(crate) trait References {
    fn references(&self, visit: &mut dyn FnMut(ObjectId));
    fn retarget(&mut self, retarget: &mut dyn FnMut(ObjectId, &mut ObjectId));
}

macro_rules! references_of {
    ($($kind:ty => $walk:ident),*) => {$(
        impl References for $kind {
            fn references(&self, visit: &mut dyn FnMut(ObjectId)) {
                let f: &mut Visit<'_> = &mut |_, target: &mut ObjectId| visit(*target);
                read::$walk(self.id.into(), self, f, &mut false);
            }
            fn retarget(&mut self, retarget: &mut dyn FnMut(ObjectId, &mut ObjectId)) {
                write::$walk(self.id.into(), self, retarget, &mut false);
            }
        }
    )*};
}
references_of!(Patch => patch, Rig => rig, Sequence => sequence);

impl<T: References> References for Box<T> {
    fn references(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.as_ref().references(visit);
    }
    fn retarget(&mut self, retarget: &mut dyn FnMut(ObjectId, &mut ObjectId)) {
        self.as_mut().retarget(retarget);
    }
}
