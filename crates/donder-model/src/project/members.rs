//! The member a [`Site`] names, found in the object that holds it.
use std::sync::Arc;

use super::DonderProject;
use crate::composition::{CompositionGraphNode, Layer, MarkCollection};
use crate::console::{Console, On};
use crate::controller::ControllerPort;
use crate::effect::Clip;
use crate::fixture::{FixtureDefinition, FixtureElement};
use crate::identity::ObjectId;
use crate::input_device::{InputControl, InputDevice};
use crate::layout::{Layout, LayoutFixture};
use crate::patch::PixelRoute;
use crate::sequence::Sequence;
use crate::site::{Member, Site};

/// A member of a project object, borrowed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MemberRef<'a> {
    Layer(&'a Layer),
    Clip(&'a Clip),
    Marks(&'a MarkCollection),
    Node(&'a CompositionGraphNode),
    Event(&'a On),
    Control(&'a InputControl),
    Port(&'a ControllerPort),
    Route(&'a PixelRoute),
    LayoutItem(&'a LayoutFixture),
    Shape(&'a FixtureElement),
}

/// A member of a project object, to change in place. A sequence clip stays
/// shared until it is written.
#[derive(Debug)]
pub(crate) enum MemberMut<'a> {
    Layer(&'a mut Layer),
    Clip(&'a mut Arc<Clip>),
    Marks(&'a mut MarkCollection),
    Node(&'a mut CompositionGraphNode),
    Event(&'a mut On),
    Control(&'a mut InputControl),
    Route(&'a mut PixelRoute),
    LayoutItem(&'a mut LayoutFixture),
    Shape(&'a mut FixtureElement),
}

impl DonderProject {
    /// The member `site` names, when its object holds it.
    pub fn member(&self, site: &Site) -> Option<MemberRef<'_>> {
        let member = site.member.as_ref()?;
        match site.object {
            ObjectId::Sequence(id) => self.get::<Sequence>(id)?.composition.member(member),
            ObjectId::Console(id) => {
                let console = self.get::<Console>(id)?;
                match member {
                    Member::Event(id) => console
                        .events
                        .iter()
                        .find(|on| &on.id == id)
                        .map(MemberRef::Event),
                    _ => console.composition.member(member),
                }
            }
            ObjectId::InputDevice(id) => match member {
                Member::Control(control) => self
                    .get::<InputDevice>(id)?
                    .controls
                    .iter()
                    .find(|candidate| &candidate.id == control)
                    .map(MemberRef::Control),
                _ => None,
            },
            ObjectId::Controller(id) => match member {
                Member::Port(port) => self
                    .get::<crate::Controller>(id)?
                    .port(*port)
                    .map(MemberRef::Port),
                _ => None,
            },
            ObjectId::Project(_) => match member {
                Member::Route(route) => self
                    .root
                    .routes
                    .iter()
                    .find(|candidate| &candidate.id == route)
                    .map(MemberRef::Route),
                _ => None,
            },
            ObjectId::Layout(id) => match member {
                Member::LayoutItem(item) => self
                    .get::<Layout>(id)?
                    .fixture(*item)
                    .map(MemberRef::LayoutItem),
                _ => None,
            },
            ObjectId::FixtureDefinition(id) => match member {
                Member::Shape(shape) => self
                    .get::<FixtureDefinition>(id)?
                    .elements
                    .iter()
                    .find(|element| &element.id == shape)
                    .map(MemberRef::Shape),
                _ => None,
            },
            ObjectId::Curve(_) | ObjectId::Gradient(_) | ObjectId::Script(_) => None,
        }
    }

    /// The member `site` names, to change in place. A controller's ports
    /// change with the controller.
    pub(crate) fn member_mut(&mut self, site: &Site) -> Option<MemberMut<'_>> {
        let member = site.member.as_ref()?;
        match site.object {
            ObjectId::Sequence(id) => self
                .find_mut::<Sequence>(id)?
                .composition
                .member_mut(member),
            ObjectId::Console(id) => {
                let console = self.find_mut::<Console>(id)?;
                match member {
                    Member::Event(id) => console
                        .events
                        .iter_mut()
                        .find(|on| &on.id == id)
                        .map(MemberMut::Event),
                    _ => console.composition.member_mut(member),
                }
            }
            ObjectId::InputDevice(id) => match member {
                Member::Control(control) => self
                    .find_mut::<InputDevice>(id)?
                    .controls
                    .iter_mut()
                    .find(|candidate| &candidate.id == control)
                    .map(MemberMut::Control),
                _ => None,
            },
            ObjectId::Project(_) => match member {
                Member::Route(route) => self
                    .root
                    .routes
                    .iter_mut()
                    .find(|candidate| &candidate.id == route)
                    .map(MemberMut::Route),
                _ => None,
            },
            ObjectId::Layout(id) => match member {
                Member::LayoutItem(item) => self
                    .find_mut::<Layout>(id)?
                    .fixture_mut(*item)
                    .map(MemberMut::LayoutItem),
                _ => None,
            },
            ObjectId::FixtureDefinition(id) => match member {
                Member::Shape(shape) => self
                    .find_mut::<FixtureDefinition>(id)?
                    .elements
                    .iter_mut()
                    .find(|element| &element.id == shape)
                    .map(MemberMut::Shape),
                _ => None,
            },
            ObjectId::Controller(_)
            | ObjectId::Curve(_)
            | ObjectId::Gradient(_)
            | ObjectId::Script(_) => None,
        }
    }

    /// Remove the member `site` names; whether it was there. Whatever named
    /// it is left to validation and its repairs.
    pub(crate) fn remove_member(&mut self, site: &Site) -> bool {
        fn retain<T>(items: &mut Vec<T>, keep: impl FnMut(&T) -> bool) -> bool {
            let count = items.len();
            items.retain(keep);
            items.len() != count
        }
        let Some(member) = site.member.as_ref() else {
            return false;
        };
        match (site.object, member) {
            (ObjectId::Sequence(id), member) => self
                .find_mut::<Sequence>(id)
                .is_some_and(|sequence| sequence.composition.remove(member)),
            (ObjectId::Console(id), member) => {
                let Some(console) = self.find_mut::<Console>(id) else {
                    return false;
                };
                match member {
                    Member::Event(id) => retain(&mut console.events, |on| &on.id != id),
                    _ => console.composition.remove(member),
                }
            }
            (ObjectId::InputDevice(id), Member::Control(control)) => self
                .find_mut::<InputDevice>(id)
                .is_some_and(|device| retain(&mut device.controls, |c| &c.id != control)),
            (ObjectId::Project(_), Member::Route(route)) => {
                retain(&mut self.root.routes, |candidate| &candidate.id != route)
            }
            (ObjectId::Layout(id), Member::LayoutItem(item)) => self
                .find_mut::<Layout>(id)
                .is_some_and(|layout| layout.remove_items(&[*item]).is_ok()),
            (ObjectId::FixtureDefinition(id), Member::Shape(shape)) => self
                .find_mut::<FixtureDefinition>(id)
                .is_some_and(|definition| {
                    retain(&mut definition.elements, |element| &element.id != shape)
                }),
            _ => false,
        }
    }
}

/// A member of a project object, as an edit writes it.
#[derive(Clone, Debug, PartialEq)]
pub enum MemberValue {
    Layer(Layer),
    Clip(Clip),
    Marks(MarkCollection),
    Node(CompositionGraphNode),
    Event(On),
    Control(InputControl),
    Route(PixelRoute),
    LayoutItem(LayoutFixture),
    Shape(FixtureElement),
}

impl MemberValue {
    /// The member the value is, by its id.
    fn member(&self) -> Member {
        match self {
            Self::Layer(layer) => Member::Layer(layer.id.clone()),
            Self::Clip(clip) => Member::Clip(clip.id),
            Self::Marks(marks) => Member::Marks(marks.id),
            Self::Node(node) => Member::Node(node.id.clone()),
            Self::Event(on) => Member::Event(on.id),
            Self::Control(control) => Member::Control(control.id),
            Self::Route(route) => Member::Route(route.id),
            Self::LayoutItem(item) => Member::LayoutItem(item.id),
            Self::Shape(shape) => Member::Shape(shape.id),
        }
    }
}

impl DonderProject {
    /// Write `value` in place of the member `site` names, which has its id.
    pub(crate) fn put_member(
        &mut self,
        site: &Site,
        value: MemberValue,
    ) -> Result<(), crate::EditRejection> {
        let missing = crate::EditRejection::MissingMember;
        if site.member.as_ref() != Some(&value.member()) {
            return Err(missing);
        }
        match (self.member_mut(site).ok_or(missing)?, value) {
            (MemberMut::Layer(slot), MemberValue::Layer(value)) => *slot = value,
            (MemberMut::Clip(slot), MemberValue::Clip(value)) => *slot = Arc::new(value),
            (MemberMut::Marks(slot), MemberValue::Marks(value)) => *slot = value,
            (MemberMut::Node(slot), MemberValue::Node(value)) => *slot = value,
            (MemberMut::Event(slot), MemberValue::Event(value)) => *slot = value,
            (MemberMut::Control(slot), MemberValue::Control(value)) => *slot = value,
            (MemberMut::Route(slot), MemberValue::Route(value)) => *slot = value,
            (MemberMut::LayoutItem(slot), MemberValue::LayoutItem(value)) => *slot = value,
            (MemberMut::Shape(slot), MemberValue::Shape(value)) => *slot = value,
            _ => return Err(missing),
        }
        Ok(())
    }

    /// Remove the part of a member `site`'s field names: a played value, an
    /// event's `Set`s of a parameter, or an envelope's binding; with no such
    /// field, the member. Whether anything went.
    pub(crate) fn remove_at(&mut self, site: &Site) -> bool {
        fn retain<T>(items: &mut Vec<T>, keep: impl FnMut(&T) -> bool) -> bool {
            let count = items.len();
            items.retain(keep);
            items.len() != count
        }
        match (&site.field, self.member_mut(site)) {
            (Some(crate::Field::Played(param)), Some(MemberMut::Clip(clip))) => {
                clip.played().iter().any(|value| &value.param == param)
                    && match &mut Arc::make_mut(clip).placement {
                        crate::effect::Placement::Played(values) => {
                            retain(values, |value| &value.param != param)
                        }
                        crate::effect::Placement::Timed { .. } => false,
                    }
            }
            (Some(crate::Field::Set(param)), Some(MemberMut::Event(on))) => retain(
                &mut on.actions,
                |action| !matches!(action, crate::Action::Set { target, .. } if target.param() == param),
            ),
            (Some(crate::Field::Binding(target)), Some(MemberMut::Clip(clip))) => {
                clip.envelope()
                    .is_some_and(|envelope| envelope.binding(target).is_some())
                    && Arc::make_mut(clip).envelope_mut().is_some_and(|envelope| {
                        retain(&mut envelope.bindings, |binding| &binding.target != target)
                    })
            }
            _ => self.remove_member(site),
        }
    }
}

impl MemberRef<'_> {
    /// The member as an edit writes it; a controller's ports are written
    /// with their controller.
    pub fn to_value(self) -> Option<MemberValue> {
        Some(match self {
            Self::Layer(layer) => MemberValue::Layer(layer.clone()),
            Self::Clip(clip) => MemberValue::Clip(clip.clone()),
            Self::Marks(marks) => MemberValue::Marks(marks.clone()),
            Self::Node(node) => MemberValue::Node(node.clone()),
            Self::Event(on) => MemberValue::Event(on.clone()),
            Self::Control(control) => MemberValue::Control(control.clone()),
            Self::Route(route) => MemberValue::Route(route.clone()),
            Self::LayoutItem(item) => MemberValue::LayoutItem(item.clone()),
            Self::Shape(shape) => MemberValue::Shape(shape.clone()),
            Self::Port(_) => return None,
        })
    }
}
