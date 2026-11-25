//! Where in its declaration an object, or a member of one, is written. Loading
//! numbers every member by its position in the document, so a member id the
//! model names finds its text again.
use donder_language::compiler::TextSpan;
use donder_language::data::{Name, Params};
use donder_model::{Field, Member, Site};
use donder_runtime_types::Identifier;

use crate::document::{Declaration, types};

/// The span of the member `site` names in `declaration`.
pub(super) fn site_span(declaration: &Declaration, site: &Site) -> Option<TextSpan> {
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
    let field = site.field.as_ref();
    let with_param = |name: &Name, params: &Params| match field {
        Some(Field::Param(param_name)) => param(params, param_name).or(Some(name_span(name))),
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
    let clip = |clips: &[types::Clip], id: u32| {
        let clip = nth(clips, id)?;
        match (field, &clip.placement, &clip.content) {
            (Some(Field::Played(name)), types::Placement::Played { values }, _) => values
                .iter()
                .find(|value| &value.param.0.value == name)
                .map(|value| name_span(&value.param))
                .or(Some(name_span(&clip.name))),
            (_, _, types::ClipContent::Effect { params, .. }) => with_param(&clip.name, params),
            (_, _, types::ClipContent::Envelope { .. }) => Some(name_span(&clip.name)),
        }
    };
    let member = site.member.as_ref()?;
    match (declaration, member) {
        (Declaration::Sequence(sequence), member) => match member {
            Member::Layer(id) => layer(&sequence.layers, id.0),
            Member::Clip(id) => clip(&sequence.clips, id.0),
            Member::Marks(id) => sequence
                .marks
                .get(usize::try_from(id.0).ok()?)
                .map(|marks| name_span(&marks.name)),
            Member::Node(id) => node(&sequence.graph, id.0),
            _ => None,
        },
        (Declaration::Console(console), member) => match member {
            Member::Layer(id) => layer(&console.layers, id.0),
            Member::Node(id) => node(&console.graph, id.0),
            Member::Marks(id) => console
                .marks
                .get(usize::try_from(id.0).ok()?)
                .map(|marks| name_span(&marks.name)),
            Member::Clip(id) => clip(&console.clips, id.0),
            Member::Event(id) => Some(nth(&console.events, id.0)?.control.span),
            _ => None,
        },
        (Declaration::InputDevice(device), Member::Control(id)) => {
            nth(&device.controls, id.0).map(|control| name_span(control.name()))
        }
        (Declaration::Controller(controller), Member::Port(id)) => match &controller.protocol {
            types::Protocol::Donder { ports, .. } => nth(ports, id.0).map(|port| port.name.0.span),
            types::Protocol::E131 { ports, .. } | types::Protocol::ArtNet { ports, .. } => {
                nth(ports, id.0).map(|port| port.name.0.span)
            }
        },
        (Declaration::Layout(layout), Member::LayoutItem(id)) => {
            let item = nth(&layout.items, id.0)?;
            let (types::LayoutItem::Group { name, .. } | types::LayoutItem::Fixture { name, .. }) =
                item;
            Some(name_span(name))
        }
        (Declaration::Project(project), Member::Route(id)) => {
            nth(&project.routes, id.0).map(|route| route.target.span)
        }
        (Declaration::FixtureDefinition(definition), Member::Shape(id)) => {
            nth(&definition.shapes, id.0).map(|shape| name_span(&shape.name))
        }
        _ => None,
    }
}
