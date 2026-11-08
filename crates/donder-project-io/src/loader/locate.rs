//! Where in its declaration an object, or a member of one, is written. Loading
//! numbers every member by its position in the document, so a member id the
//! model names finds its text again.
use donder_language::compiler::TextSpan;
use donder_language::data::{Name, OwnedNamedRecord, OwnedRecord, Params};
use donder_model::{Member, MemberPart, ProblemSite};
use donder_runtime_types::Identifier;

use crate::address::{OwnedObjectSlot, OwningField};
use crate::document::{Declaration, types};

/// A declaration, or an object written in place inside one.
#[derive(Clone, Copy)]
pub(super) enum Located<'a> {
    Project(&'a types::Project),
    Setup(&'a types::Setup),
    Controller(&'a types::Controller),
    InputDevice(&'a types::InputDevice),
    Layout(&'a types::Layout),
    Patch(&'a types::Patch),
    Sequence(&'a types::Sequence),
    Console(&'a types::Console),
    Fixture(&'a types::FixtureDefinition),
    Other,
}

/// The object written in place at `slot` of `owner`, and the span of its
/// name when it is a collection member.
fn child<'a>(
    owner: Located<'a>,
    slot: &OwnedObjectSlot,
) -> Option<(Located<'a>, Option<TextSpan>)> {
    fn single<'a, T>(
        record: &'a OwnedRecord<T>,
        located: fn(&'a T) -> Located<'a>,
    ) -> Option<(Located<'a>, Option<TextSpan>)> {
        match record {
            OwnedRecord::Inline(value) => Some((located(value), None)),
            OwnedRecord::Reference(_) => None,
        }
    }
    fn member<'a, T>(
        records: &'a [OwnedNamedRecord<T>],
        name: Option<&Identifier>,
        located: fn(&'a T) -> Located<'a>,
    ) -> Option<(Located<'a>, Option<TextSpan>)> {
        let name = name?;
        records.iter().find_map(|record| match record {
            OwnedNamedRecord::Inline(member, value) if &member.0.value == name => {
                Some((located(value), Some(member.0.span)))
            }
            _ => None,
        })
    }
    let (field, name) = slot.field()?;
    match (owner, field) {
        (Located::Project(project), OwningField::Setup) => single(&project.setup, Located::Setup),
        (Located::Project(project), OwningField::Sequences) => {
            member(&project.sequences, name, Located::Sequence)
        }
        (Located::Project(project), OwningField::Consoles) => {
            member(&project.consoles, name, Located::Console)
        }
        (Located::Setup(setup), OwningField::Layout) => single(&setup.layout, Located::Layout),
        (Located::Setup(setup), OwningField::Patch) => single(&setup.patch, Located::Patch),
        (Located::Setup(setup), OwningField::Controllers) => {
            member(&setup.controllers, name, Located::Controller)
        }
        (Located::Setup(setup), OwningField::InputDevices) => {
            member(&setup.input_devices, name, Located::InputDevice)
        }
        _ => None,
    }
}

/// The object at `path` inside `declaration`.
pub(super) fn owned<'a>(
    declaration: &'a Declaration,
    path: &[OwnedObjectSlot],
) -> Option<Located<'a>> {
    let mut current = match declaration {
        Declaration::Project(project) => Located::Project(project),
        Declaration::Setup(setup) => Located::Setup(setup),
        Declaration::Controller(controller) => Located::Controller(controller),
        Declaration::InputDevice(device) => Located::InputDevice(device),
        Declaration::Layout(layout) => Located::Layout(layout),
        Declaration::Patch(patch) => Located::Patch(patch),
        Declaration::Sequence(sequence) => Located::Sequence(sequence),
        Declaration::Console(console) => Located::Console(console),
        Declaration::FixtureDefinition(definition) => Located::Fixture(definition),
        Declaration::Curve(_) | Declaration::Gradient(_) => Located::Other,
    };
    for slot in path {
        current = child(current, slot)?.0;
    }
    Some(current)
}

/// The name span of the owned collection member at the end of `path`.
pub(super) fn member_name(declaration: &Declaration, path: &[OwnedObjectSlot]) -> Option<TextSpan> {
    let (last, parent) = path.split_last()?;
    child(owned(declaration, parent)?, last)?.1
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
        types::Node::RootNode { .. } => None,
    };
    let layer = |layers: &[types::Layer], id: u32| {
        layers
            .get(usize::try_from(id).ok()?)
            .map(|layer| name_span(&layer.name))
    };
    match (owned(declaration, path)?, &site.member) {
        (Located::Sequence(sequence), member) => match member {
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
        (Located::Console(console), member) => match member {
            Member::Layer(id) => layer(&console.layers, id.0),
            Member::Node(id) => node(&console.graph, id.0),
            Member::LiveMarks(name) => console
                .marks
                .iter()
                .find(|marks| &marks.name.0.value == name)
                .map(|marks| name_span(&marks.name)),
            Member::ConsoleClip(id) => {
                let clip = nth(&console.clips, id.0)?;
                match part {
                    Some(MemberPart::PlayedValue(name)) => clip
                        .played
                        .iter()
                        .find(|value| &value.param.0.value == name)
                        .map(|value| name_span(&value.param))
                        .or(Some(name_span(&clip.name))),
                    _ => with_param(&clip.name, &clip.params),
                }
            }
            Member::Event(id) => Some(nth(&console.events, id.0)?.event.control().span),
            _ => None,
        },
        (Located::InputDevice(device), Member::Control(id)) => {
            nth(&device.controls, id.0).map(|control| name_span(control.name()))
        }
        (Located::Controller(controller), Member::Port(id)) => {
            nth(&controller.ports, id.0).map(|port| name_span(&port.name))
        }
        (Located::Layout(layout), Member::LayoutItem(id)) => {
            let item = nth(&layout.items, id.0)?;
            match (item, part) {
                (
                    types::LayoutItem::Fixture {
                        definition: OwnedRecord::Inline(definition),
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
        (Located::Patch(patch), Member::Route(id)) => {
            nth(&patch.routes, id.0).map(|route| route.target.span)
        }
        (Located::Fixture(definition), Member::Shape(id)) => {
            nth(&definition.shapes, id.0).map(|shape| name_span(&shape.name))
        }
        _ => None,
    }
}
