//! Where in its declaration an object, or a member of one, is written. Loading
//! numbers every member by its position in the document, so a member id the
//! model names finds its text again.
use donder_language::compiler::TextSpan;
use donder_language::data::{Name, OwnedNamedRecord, OwnedRecord, Params};
use donder_model::{Member, MemberPart, ProblemSite};
use donder_runtime_types::Identifier;

use crate::address::OwnedObjectSlot;
use crate::document::{Declaration, types};

/// A declaration, or an object written in place inside one.
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
    fn member<'a, T>(sources: &'a [OwnedNamedRecord<T>], name: &Identifier) -> Option<&'a T> {
        sources.iter().find_map(|source| match source {
            OwnedNamedRecord::Inline(member, value) if &member.0.value == name => Some(value),
            _ => None,
        })
    }
    for slot in path {
        current = match (current, slot) {
            (Located::Project(project), OwnedObjectSlot::Setup) => match &project.setup {
                OwnedRecord::Inline(setup) => Located::Setup(setup),
                OwnedRecord::Reference(_) => return None,
            },
            (Located::Project(project), OwnedObjectSlot::Sequence(name)) => {
                Located::Sequence(member(&project.sequences, name)?)
            }
            (Located::Project(project), OwnedObjectSlot::Console(name)) => {
                Located::Console(member(&project.consoles, name)?)
            }
            (Located::Setup(setup), OwnedObjectSlot::Layout) => match &setup.layout {
                OwnedRecord::Inline(layout) => Located::Layout(layout),
                OwnedRecord::Reference(_) => return None,
            },
            (Located::Setup(setup), OwnedObjectSlot::Patch) => match &setup.patch {
                OwnedRecord::Inline(patch) => Located::Patch(patch),
                OwnedRecord::Reference(_) => return None,
            },
            (Located::Setup(setup), OwnedObjectSlot::Controller(name)) => {
                Located::Controller(member(&setup.controllers, name)?)
            }
            (Located::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
                Located::InputDevice(member(&setup.input_devices, name)?)
            }
            _ => return None,
        };
    }
    Some(current)
}

/// The name span of the owned collection member at the end of `path`.
pub(super) fn member_name(declaration: &Declaration, path: &[OwnedObjectSlot]) -> Option<TextSpan> {
    let (last, parent) = path.split_last()?;
    fn named<T>(sources: &[OwnedNamedRecord<T>], name: &Identifier) -> Option<TextSpan> {
        sources.iter().find_map(|source| match source {
            OwnedNamedRecord::Inline(member, _) if &member.0.value == name => Some(member.0.span),
            _ => None,
        })
    }
    match (owned(declaration, parent)?, last) {
        (Located::Project(project), OwnedObjectSlot::Sequence(name)) => {
            named(&project.sequences, name)
        }
        (Located::Project(project), OwnedObjectSlot::Console(name)) => {
            named(&project.consoles, name)
        }
        (Located::Setup(setup), OwnedObjectSlot::Controller(name)) => {
            named(&setup.controllers, name)
        }
        (Located::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
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
                with_param(&clip.name, &clip.params)
            }
            Member::LiveAutomation(id) => {
                let automation = nth(&console.automations, id.0)?;
                match part {
                    Some(MemberPart::Param(name)) => automation
                        .bindings
                        .iter()
                        .map(|binding| match binding {
                            types::Binding::ClipParam { param, .. }
                            | types::Binding::NodeParam { param, .. } => param,
                        })
                        .find(|param| &param.0.value == name)
                        .map(name_span),
                    _ => Some(name_span(&automation.name)),
                }
            }
            Member::Event(id) => {
                let on = nth(&console.events, id.0)?;
                let control = on.event.control().span;
                match part {
                    Some(MemberPart::PlayedValue(name)) => on
                        .actions
                        .iter()
                        .flat_map(|action| match action {
                            types::Action::Play { values, .. }
                            | types::Action::Start { values, .. }
                            | types::Action::Toggle { values, .. } => values.as_slice(),
                            types::Action::Stop { .. }
                            | types::Action::Mark { .. }
                            | types::Action::Set { .. } => &[],
                        })
                        .find(|value| &value.param.0.value == name)
                        .map(|value| name_span(&value.param))
                        .or(Some(control)),
                    _ => Some(control),
                }
            }
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
