//! Where in its declaration an object, or a member of one, is written. Loading
//! numbers every member by its position in the document, so a member id the
//! model names finds its text again.
use donder_language::compiler::TextSpan;
use donder_language::data::{Name, NamedSource, Params, Source};
use donder_model::{Member, MemberPart, ProblemSite};
use donder_runtime_types::Identifier;

use crate::address::OwnedObjectSlot;
use crate::document::{Declaration, types};

/// A declaration, or an object written in place inside one.
pub(super) enum Owned<'a> {
    Project(&'a types::Project),
    Setup(&'a types::Setup),
    Controller(&'a types::Controller),
    InputDevice(&'a types::InputDevice),
    Layout(&'a types::Layout),
    Patch(&'a types::Patch),
    Sequence(&'a types::Sequence),
    Rig(&'a types::Rig),
    Fixture(&'a types::FixtureDefinition),
    Other,
}

/// The object at `path` inside `declaration`.
pub(super) fn owned<'a>(
    declaration: &'a Declaration,
    path: &[OwnedObjectSlot],
) -> Option<Owned<'a>> {
    let mut current = match declaration {
        Declaration::Project(project) => Owned::Project(project),
        Declaration::Setup(setup) => Owned::Setup(setup),
        Declaration::Controller(controller) => Owned::Controller(controller),
        Declaration::InputDevice(device) => Owned::InputDevice(device),
        Declaration::Layout(layout) => Owned::Layout(layout),
        Declaration::Patch(patch) => Owned::Patch(patch),
        Declaration::Sequence(sequence) => Owned::Sequence(sequence),
        Declaration::Rig(rig) => Owned::Rig(rig),
        Declaration::FixtureDefinition(definition) => Owned::Fixture(definition),
        Declaration::Curve(_) | Declaration::Gradient(_) => Owned::Other,
    };
    fn member<'a, T>(sources: &'a [NamedSource<T>], name: &Identifier) -> Option<&'a T> {
        sources.iter().find_map(|source| match source {
            NamedSource::Inline(member, value) if &member.0.value == name => Some(value),
            _ => None,
        })
    }
    for slot in path {
        current = match (current, slot) {
            (Owned::Project(project), OwnedObjectSlot::Setup) => match &project.setup {
                Source::Inline(setup) => Owned::Setup(setup),
                Source::Reference(_) => return None,
            },
            (Owned::Project(project), OwnedObjectSlot::Sequence(name)) => {
                Owned::Sequence(member(&project.sequences, name)?)
            }
            (Owned::Project(project), OwnedObjectSlot::Rig(name)) => {
                Owned::Rig(member(&project.rigs, name)?)
            }
            (Owned::Setup(setup), OwnedObjectSlot::Layout) => match &setup.layout {
                Source::Inline(layout) => Owned::Layout(layout),
                Source::Reference(_) => return None,
            },
            (Owned::Setup(setup), OwnedObjectSlot::Patch) => match &setup.patch {
                Source::Inline(patch) => Owned::Patch(patch),
                Source::Reference(_) => return None,
            },
            (Owned::Setup(setup), OwnedObjectSlot::Controller(name)) => {
                Owned::Controller(member(&setup.controllers, name)?)
            }
            (Owned::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
                Owned::InputDevice(member(&setup.input_devices, name)?)
            }
            _ => return None,
        };
    }
    Some(current)
}

/// The name span of the owned collection member at the end of `path`.
pub(super) fn member_name(declaration: &Declaration, path: &[OwnedObjectSlot]) -> Option<TextSpan> {
    let (last, parent) = path.split_last()?;
    fn named<T>(sources: &[NamedSource<T>], name: &Identifier) -> Option<TextSpan> {
        sources.iter().find_map(|source| match source {
            NamedSource::Inline(member, _) if &member.0.value == name => Some(member.0.span),
            _ => None,
        })
    }
    match (owned(declaration, parent)?, last) {
        (Owned::Project(project), OwnedObjectSlot::Sequence(name)) => {
            named(&project.sequences, name)
        }
        (Owned::Project(project), OwnedObjectSlot::Rig(name)) => named(&project.rigs, name),
        (Owned::Setup(setup), OwnedObjectSlot::Controller(name)) => named(&setup.controllers, name),
        (Owned::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
            named(&setup.input_devices, name)
        }
        _ => None,
    }
}

/// The span of the member `site` names in the object at `path`.
pub(super) fn site_span(
    declaration: &Declaration,
    path: &[OwnedObjectSlot],
    site: &ProblemSite,
) -> Option<TextSpan> {
    /// The member numbered `id`, counting from 1.
    fn nth<T>(items: &[T], id: u32) -> Option<&T> {
        items.get(usize::try_from(id).ok()?.checked_sub(1)?)
    }
    fn param(params: &Params, name: &Identifier) -> Option<TextSpan> {
        params
            .0
            .iter()
            .find(|(param, _)| &param.value == name)
            .map(|(param, _)| param.span)
    }
    fn name_span(name: &Name) -> TextSpan {
        name.0.span
    }
    let part = site.part.as_ref();
    let with_param = |name: &Name, params: &Params| match part {
        Some(MemberPart::Param(param_name)) => param(params, param_name).or(Some(name_span(name))),
        _ => Some(name_span(name)),
    };
    let node = |graph: &types::Graph, id: u32| match nth(&graph.nodes, id)? {
        types::Node::LayerNode { layer, .. } => Some(name_span(layer)),
        types::Node::OperatorNode { name, params, .. } => with_param(name, params),
        types::Node::OutputNode { .. } => None,
    };
    let layer = |layers: &[types::Layer], id: u32| {
        layers
            .get(usize::try_from(id).ok()?)
            .map(|layer| name_span(&layer.name))
    };
    match (owned(declaration, path)?, &site.member) {
        (Owned::Sequence(sequence), member) => match member {
            Member::Layer(id) => layer(&sequence.layers, id.0),
            Member::Clip(id) => {
                let clip = nth(&sequence.clips, id.0)?;
                with_param(&clip.name, &clip.params)
            }
            Member::Marks(name) => sequence
                .marks
                .iter()
                .find(|marks| &marks.name.0.value == name)
                .map(|marks| name_span(&marks.name)),
            Member::AutomationClip(id) => nth(&sequence.automation, id.0).map(|clip| clip.row.span),
            Member::Node(id) => node(&sequence.graph, id.0),
            _ => None,
        },
        (Owned::Rig(rig), member) => match member {
            Member::Layer(id) => layer(&rig.layers, id.0),
            Member::Node(id) => node(&rig.graph, id.0),
            Member::LiveMarks(name) => rig
                .marks
                .iter()
                .find(|marks| &marks.name.0.value == name)
                .map(|marks| name_span(&marks.name)),
            Member::Voice(id) => {
                let voice = nth(&rig.scene, id.0)?;
                with_param(&voice.name, &voice.params)
            }
            Member::Switch(id) => {
                let switch = nth(&rig.switches, id.0)?;
                Some(match part {
                    Some(MemberPart::Input) => switch.input.span,
                    _ => name_span(&switch.name),
                })
            }
            Member::Mapping(id) => {
                let mapping = nth(&rig.mappings, id.0)?;
                match (part, &mapping.action) {
                    (Some(MemberPart::Input), _) => Some(mapping.input.span),
                    (
                        Some(MemberPart::PlayedValue(name)),
                        types::MappingAction::Trigger { values, .. },
                    ) => values
                        .iter()
                        .find(|value| &value.param.0.value == name)
                        .map(|value| name_span(&value.param)),
                    (Some(MemberPart::Param(_)), types::MappingAction::Trigger { params, .. }) => {
                        with_param(&mapping.name, params)
                    }
                    (Some(MemberPart::Param(_)), types::MappingAction::Control { param, .. }) => {
                        Some(name_span(param))
                    }
                    _ => Some(name_span(&mapping.name)),
                }
            }
            _ => None,
        },
        (Owned::InputDevice(device), Member::Control(id)) => {
            nth(&device.controls, id.0).map(|control| name_span(control.name()))
        }
        (Owned::Controller(controller), Member::Port(id)) => {
            nth(&controller.ports, id.0).map(|port| name_span(&port.name))
        }
        (Owned::Layout(layout), Member::LayoutItem(id)) => {
            let item = nth(&layout.items, id.0)?;
            match (item, part) {
                (
                    types::LayoutItem::Fixture {
                        definition: Source::Inline(definition),
                        ..
                    },
                    Some(MemberPart::Shape(shape)),
                ) => nth(&definition.shapes, shape.0).map(|shape| name_span(&shape.name)),
                (
                    types::LayoutItem::Group { name, .. } | types::LayoutItem::Fixture { name, .. },
                    _,
                ) => Some(name_span(name)),
            }
        }
        (Owned::Patch(patch), Member::Route(id)) => {
            nth(&patch.routes, id.0).map(|route| route.target.span)
        }
        (Owned::Fixture(definition), Member::Shape(id)) => {
            nth(&definition.shapes, id.0).map(|shape| name_span(&shape.name))
        }
        _ => None,
    }
}
