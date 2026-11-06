//! Decoded declarations to typed state. Session identities are assigned here
//! in document order, and names resolve to them. Identities inside another
//! object (fixtures, ports) follow from that object's declaration alone, so a
//! reference resolves without the referenced object being built first.
use donder_model::ObjectKind;
use std::sync::Arc;

use crate::address::{DocumentId, ObjectAddress, OwnedObjectSlot, SourceIdentity};
use donder_language::compiler::{ParamDecl, TextSpan};
use donder_language::data::{
    Data, Decoder, Meters, Name, OwnedNamedRecord, OwnedRecord, Params, Reference,
};
use donder_language::data::{DataValue, Spanned};
use donder_language::{
    Distance, DistanceSpan, DonderDuration, DonderTime, Point3, Rotation3, Scale3,
};
use donder_model::GraphOperatorNode;
use donder_model::ObjectId;
use donder_model::ProjectData;
use donder_model::{
    Action, Console, ConsoleClip, ConsoleClipId, ConsoleId, InputEvent, LengthSource, LiveMarks,
    On, OnId, Play, PlayLength,
};
use donder_model::{
    ArtNetConfig, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
use donder_model::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationTarget,
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition, Mark, MarkCollection,
    MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph, SequenceId,
    SequenceLayer, SequenceLayerId, Window,
};
use donder_model::{
    AxisInput, ButtonInput, ControlRef, InputControl, InputControlId, InputControlKind,
    InputDevice, InputDeviceId, InputDriver, MidiChannel, NoteRange, OscAddress, PlayedValue,
};
use donder_model::{Clip, ClipId, EffectDefinitionId, OwnedCurve, OwnedGradient, ParamValue};
use donder_model::{
    FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
    FixtureTransform, OwnedFixture,
};
use donder_model::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutFixture, LayoutFixtureKind, LayoutId,
};
use donder_model::{Owned, OwnedMember};
use donder_model::{Patch, PatchId, PixelRoute, PixelRouteId, PixelSpan};
use donder_model::{Setup, SetupId};
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, CurvePoint, Gradient, GradientStop};
use donder_runtime_types::{Identifier, Type};
use indexmap::IndexMap;

use super::locate::{Located, owned};
use super::{DataDocument, Loader};
use crate::diagnostics::LoadError;
use crate::document::{Declaration, types};
use crate::index::{LinkTarget, ScriptMember};

pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut ProjectData,
}

/// Each layer of a sequence or console by name, with its declaring name's span.
type LayerIds = IndexMap<Identifier, (SequenceLayerId, TextSpan)>;

/// Each graph node by name: its identity, whether it is the root, the name
/// it is declared by, and its operator.
type NodeIds = IndexMap<
    Identifier,
    (
        CompositionGraphNodeId,
        bool,
        Option<TextSpan>,
        Option<donder_model::OperatorDefinitionId>,
    ),
>;

/// The identity a layout assigns its item `name`, and the item's name span:
/// items are numbered from 1 in document order.
fn fixture_index(items: &[types::LayoutItem], name: &Identifier) -> Option<(u32, TextSpan)> {
    items.iter().zip(1..).find_map(|(item, id)| {
        let (types::LayoutItem::Group {
            name: item_name, ..
        }
        | types::LayoutItem::Fixture {
            name: item_name, ..
        }) = item;
        (&item_name.0.value == name).then_some((id, item_name.0.span))
    })
}

pub(super) fn curve(points: &[(f32, f32)]) -> Curve {
    Curve {
        points: points
            .iter()
            .map(|&(position, value)| CurvePoint { position, value })
            .collect(),
    }
}

pub(super) fn gradient(
    space: &types::GradientSpace,
    stops: &[(f32, donder_runtime_types::Color)],
) -> Gradient {
    Gradient {
        space: space.0,
        stops: stops
            .iter()
            .map(|&(position, color)| GradientStop { position, color })
            .collect(),
    }
}

/// Coordinates within 2 km of the origin.
fn distance(meters: Meters) -> Option<Distance> {
    (meters.0.abs() <= 2_000_000_000)
        .then(|| i32::try_from(meters.0).ok())
        .flatten()
        .map(|micrometers| Distance { micrometers })
}

fn name(name: &Name) -> Identifier {
    name.0.value.clone()
}

fn window((low, high): (f32, f32)) -> Window {
    Window { low, high }
}

/// A kind of object a declaration resolves to: its document type, where
/// loading stores it, and how it is built from its document.
pub(super) trait Resolve: donder_model::Reusable {
    type Document;
    const KIND: ObjectKind;
    fn id(raw: u32) -> Self::Id;
    fn document(declaration: &Declaration) -> Option<&Self::Document>;
    fn loaded(project: &mut ProjectData) -> &mut IndexMap<Self::Id, Self>;
    /// The object `id`, written as `value` in `document`; `span` is its
    /// declaration's, or its member name's when owned under a name.
    fn build(
        resolver: &mut DomainResolver<'_>,
        id: &Self::Id,
        document: &DocumentId,
        span: TextSpan,
        value: &Self::Document,
    ) -> Result<Self, LoadError>;
}

/// Each kind's model type, document type, declaration and object kind share
/// its name.
macro_rules! resolve_kinds {
    ($($kind:ident($id:ident) in $store:ident =
        |$resolver:pat_param, $object:pat_param, $document:pat_param, $span:pat_param, $value:pat_param|
        $build:expr;)+) => {$(
        impl Resolve for $kind {
            type Document = types::$kind;
            const KIND: ObjectKind = ObjectKind::$kind;
            fn id(raw: u32) -> $id {
                $id(raw)
            }
            fn document(declaration: &Declaration) -> Option<&types::$kind> {
                match declaration {
                    Declaration::$kind(value) => Some(value),
                    _ => None,
                }
            }
            fn loaded(project: &mut ProjectData) -> &mut IndexMap<$id, Self> {
                &mut project.$store
            }
            fn build(
                $resolver: &mut DomainResolver<'_>,
                $object: &$id,
                $document: &DocumentId,
                $span: TextSpan,
                $value: &types::$kind,
            ) -> Result<Self, LoadError> {
                $build
            }
        }
    )+};
}

resolve_kinds! {
    Setup(SetupId) in setups = |resolver, id, document, _, value| resolver.setup(id, document, value);
    Layout(LayoutId) in layouts = |resolver, id, document, _, value| resolver.layout(id, document, value);
    Patch(PatchId) in patches = |resolver, id, document, _, value| resolver.patch(id, document, value);
    Controller(ControllerId) in controllers =
        |resolver, id, document, span, value| resolver.controller(id, document, span, value);
    InputDevice(InputDeviceId) in input_devices =
        |_, id, _, _, value| Ok(DomainResolver::input_device(id, value));
    Sequence(SequenceId) in sequences =
        |resolver, id, document, span, value| resolver.sequence(id, document, span, value);
    Console(ConsoleId) in consoles = |resolver, id, document, _, value| resolver.console(id, document, value);
    FixtureDefinition(FixtureDefinitionId) in fixtures =
        |resolver, _, document, _, value| resolver.fixture_definition(document, value);
}

impl DomainResolver<'_> {
    /// A script declaration or one of its members.
    fn script(&self, id: impl Into<ObjectId>, member: ScriptMember) -> LinkTarget {
        let address = self.loader.address(id);
        let identity = address.root_source();
        LinkTarget::Script {
            document: identity.document_id().clone(),
            declaration: identity.object().to_string(),
            member,
        }
    }

    fn invalid(
        &self,
        document: &DocumentId,
        span: TextSpan,
        message: impl Into<String>,
    ) -> LoadError {
        self.loader.invalid(document, span, message)
    }

    /// A declared object's document and declaration span.
    fn declared(
        &self,
        identity: &SourceIdentity,
    ) -> Result<(Arc<DataDocument>, TextSpan), LoadError> {
        let data = self.loader.declaration(identity)?;
        let (span, _) = data.declared(identity.object());
        Ok((data, span))
    }

    fn wrong_kind(&self, identity: &SourceIdentity) -> LoadError {
        crate::diagnostics::invalid_reference(
            identity.document().to_path_buf(),
            None,
            identity.object().to_string(),
        )
    }

    // Owned records: a declared object, resolved once, or one written in place.

    /// A declared object of kind `T`, resolved once.
    pub(super) fn resolve<T: Resolve>(&mut self, id: T::Id) -> Result<(), LoadError> {
        if T::loaded(self.project).contains_key(&id) {
            return Ok(());
        }
        let address = self.loader.address(id);
        let source = address.root_source();
        let (data, span) = self.declared(source)?;
        let value =
            T::document(data.declared(source.object()).1).ok_or_else(|| self.wrong_kind(source))?;
        let value = T::build(self, &id, source.document_id(), span, value)?;
        T::loaded(self.project).insert(id, value);
        Ok(())
    }

    /// The declared object of kind `T` a reference names, resolved.
    fn declared_reference<T: Resolve>(
        &mut self,
        document: &DocumentId,
        reference: &Reference,
    ) -> Result<T::Id, LoadError> {
        let object = self
            .loader
            .resolve_reference(document, reference, T::KIND)?;
        let id =
            T::Id::try_from(object).map_err(|_| self.loader.unresolved(document, reference))?;
        self.resolve::<T>(id)?;
        Ok(id)
    }

    /// An owned record: a declared object, or one written in place at `slot`
    /// of `owner`.
    pub(super) fn owned<T: Resolve>(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        slot: OwnedObjectSlot,
        record: &OwnedRecord<T::Document>,
    ) -> Result<Owned<Box<T>, T::Id>, LoadError> {
        match record {
            OwnedRecord::Reference(reference) => Ok(Owned::Reference(
                self.declared_reference::<T>(document, reference)?,
            )),
            OwnedRecord::Inline(value) => {
                let id = T::id(self.loader.intern(owner.owned(slot)));
                Ok(Owned::Inline(Box::new(T::build(
                    self,
                    &id,
                    document,
                    donder_language::data::NO_SPAN,
                    value,
                )?)))
            }
        }
    }

    /// An owned collection member: a declared object, or one written in place
    /// under its name, at the slot `slot` makes of it.
    pub(super) fn owned_member<T: Resolve>(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        slot: fn(Identifier) -> OwnedObjectSlot,
        record: &OwnedNamedRecord<T::Document>,
    ) -> Result<OwnedMember<T, T::Id>, LoadError> {
        match record {
            OwnedNamedRecord::Reference(reference) => Ok(OwnedMember::Reference(
                self.declared_reference::<T>(document, reference)?,
            )),
            OwnedNamedRecord::Inline(member, value) => {
                let id = T::id(self.loader.intern(owner.owned(slot(name(member)))));
                Ok(OwnedMember::Inline {
                    name: name(member),
                    value: Box::new(T::build(self, &id, document, member.0.span, value)?),
                })
            }
        }
    }

    // Identities inside other objects, from their declarations.

    fn with_owned<R>(
        &self,
        identity: &ObjectAddress,
        read: impl FnOnce(Located<'_>) -> Option<R>,
    ) -> Option<R> {
        let root = identity.root_source();
        let data = self.loader.declaration(root).ok()?;
        let (_, declaration) = data.declaration(root.object())?;
        read(owned(declaration, identity.owned_path())?)
    }

    /// `layout.fixture`: a fixture or group of a layout, at any depth.
    fn fixture_target(
        &self,
        document: &DocumentId,
        reference: &Reference,
    ) -> Result<FixtureTarget, LoadError> {
        let Some((fixture, layout)) = reference.segments.split_last() else {
            return Err(self.loader.unresolved(document, reference));
        };
        let layout_reference = Reference {
            segments: layout.to_vec(),
            span: reference.span,
        };
        let address = self.loader.resolve_object_reference(
            document,
            &layout_reference,
            ObjectKind::Layout,
        )?;
        let layout = LayoutId(self.loader.intern(address.clone()));
        let (index, item) = self
            .with_owned(&address, |owned| match owned {
                Located::Layout(layout) => fixture_index(&layout.items, &fixture.value),
                _ => None,
            })
            .ok_or_else(|| {
                self.invalid(
                    document,
                    fixture.span,
                    format!(
                        "`{}` has no fixture or group `{}`",
                        layout_reference.text(),
                        fixture.value.as_str()
                    ),
                )
            })?;
        self.loader.link(
            document,
            fixture.span,
            LinkTarget::Data {
                document: address.root_source().document_id().clone(),
                span: item,
            },
        );
        Ok(FixtureTarget {
            layout,
            fixture: FixtureInstanceId(index),
        })
    }

    fn port(
        &self,
        document: &DocumentId,
        controller: &ControllerId,
        port: &Name,
    ) -> Result<ControllerPortId, LoadError> {
        let address = self.loader.address(*controller);
        self.with_owned(&address, |owned| match owned {
            Located::Controller(controller) => controller
                .ports
                .iter()
                .position(|candidate| candidate.name == *port)
                .map(|index| (index, controller.ports[index].name.0.span)),
            _ => None,
        })
        .map(|(index, span)| {
            self.loader.link(
                document,
                port.0.span,
                LinkTarget::Data {
                    document: address.root_source().document_id().clone(),
                    span,
                },
            );
            ControllerPortId(index as u32 + 1)
        })
        .ok_or_else(|| {
            self.invalid(
                document,
                port.0.span,
                format!("the controller has no port `{}`", port.as_str()),
            )
        })
    }

    // Objects.

    fn setup(
        &mut self,
        id: &SetupId,
        document: &DocumentId,
        setup: &types::Setup,
    ) -> Result<Setup, LoadError> {
        let address = self.loader.address(*id);
        let layout = self.owned(document, &address, OwnedObjectSlot::Layout, &setup.layout)?;
        let controllers = setup
            .controllers
            .iter()
            .map(|record| {
                self.owned_member(document, &address, OwnedObjectSlot::Controller, record)
            })
            .collect::<Result<_, _>>()?;
        let input_devices = setup
            .input_devices
            .iter()
            .map(|record| {
                self.owned_member(document, &address, OwnedObjectSlot::InputDevice, record)
            })
            .collect::<Result<_, _>>()?;
        let patch = self.owned(document, &address, OwnedObjectSlot::Patch, &setup.patch)?;
        Ok(Setup {
            id: *id,
            description: setup.description.clone(),
            layout,
            patch,
            controllers,
            input_devices,
        })
    }

    fn input_device(id: &InputDeviceId, device: &types::InputDevice) -> InputDevice {
        let controls = device
            .controls
            .iter()
            .zip(1..)
            .map(|(control, index)| {
                let (control_name, description, kind) = match control {
                    types::InputControl::Button {
                        name,
                        description,
                        input,
                    } => (
                        name,
                        description,
                        InputControlKind::Button(match input {
                            types::ButtonInput::Key { code } => ButtonInput::Key(code.0),
                            types::ButtonInput::Notes { low, high, channel } => {
                                ButtonInput::Notes {
                                    notes: NoteRange {
                                        low: *low,
                                        high: *high,
                                    },
                                    channel: channel.map(MidiChannel),
                                }
                            }
                            types::ButtonInput::ControlChange { number, channel } => {
                                ButtonInput::ControlChange {
                                    number: *number,
                                    channel: channel.map(MidiChannel),
                                }
                            }
                            types::ButtonInput::Address { path } => {
                                ButtonInput::Address(OscAddress(path.clone()))
                            }
                        }),
                    ),
                    types::InputControl::Axis {
                        name,
                        description,
                        input,
                    } => (
                        name,
                        description,
                        InputControlKind::Axis(match input {
                            types::AxisInput::ControlChange { number, channel } => {
                                AxisInput::ControlChange {
                                    number: *number,
                                    channel: channel.map(MidiChannel),
                                }
                            }
                            types::AxisInput::PitchBend { channel } => AxisInput::PitchBend {
                                channel: channel.map(MidiChannel),
                            },
                            types::AxisInput::Address { path, low, high } => AxisInput::Address {
                                path: OscAddress(path.clone()),
                                low: *low,
                                high: *high,
                            },
                        }),
                    ),
                };
                InputControl {
                    id: InputControlId(index),
                    name: name(control_name),
                    description: description.clone(),
                    kind,
                }
            })
            .collect();
        InputDevice {
            id: *id,
            description: device.description.clone(),
            driver: match &device.driver {
                types::InputDriver::Keyboard => InputDriver::Keyboard,
                types::InputDriver::Midi { port } => InputDriver::Midi { port: port.clone() },
                types::InputDriver::Osc { port } => InputDriver::Osc { port: *port },
            },
            controls,
        }
    }

    /// `device.control`: a control of an input device.
    fn control_ref(
        &self,
        document: &DocumentId,
        reference: &Reference,
    ) -> Result<ControlRef, LoadError> {
        if self
            .loader
            .resolve_object_reference(document, reference, ObjectKind::InputDevice)
            .is_ok()
        {
            return Err(self.invalid(
                document,
                reference.span,
                format!(
                    "`{0}` is an input device; an input is one of its controls, `{0}.control`",
                    reference.text()
                ),
            ));
        }
        let Some((control, device)) = reference.segments.split_last() else {
            return Err(self.loader.unresolved(document, reference));
        };
        let device_reference = Reference {
            segments: device.to_vec(),
            span: reference.span,
        };
        let address = self.loader.resolve_object_reference(
            document,
            &device_reference,
            ObjectKind::InputDevice,
        )?;
        let device = InputDeviceId(self.loader.intern(address.clone()));
        let (index, span) = self
            .with_owned(&address, |owned| match owned {
                Located::InputDevice(device) => {
                    device
                        .controls
                        .iter()
                        .zip(1..)
                        .find_map(|(candidate, index)| {
                            let name = candidate.name();
                            (name.0.value == control.value).then_some((index, name.0.span))
                        })
                }
                _ => None,
            })
            .ok_or_else(|| {
                self.invalid(
                    document,
                    control.span,
                    format!(
                        "`{}` has no control `{}`",
                        device_reference.text(),
                        control.value.as_str()
                    ),
                )
            })?;
        self.loader.link(
            document,
            control.span,
            LinkTarget::Data {
                document: address.root_source().document_id().clone(),
                span,
            },
        );
        Ok(ControlRef {
            device,
            control: InputControlId(index),
        })
    }

    fn controller(
        &self,
        id: &ControllerId,
        document: &DocumentId,
        span: TextSpan,
        controller: &types::Controller,
    ) -> Result<Controller, LoadError> {
        let protocol = match &controller.protocol {
            types::Protocol::E131 {
                sender_name,
                bind_address,
                priority,
                mode,
            } => ControllerProtocol::E131(E131Config {
                sender_name: sender_name.clone(),
                bind_address: bind_address.0,
                priority: *priority,
                mode: match mode {
                    types::E131Mode::Multicast => E131Mode::Multicast,
                    types::E131Mode::Unicast { destination } => E131Mode::Unicast {
                        destination: destination.0,
                    },
                },
            }),
            types::Protocol::ArtNet {
                bind_address,
                destination,
                mode,
            } => ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: bind_address.0,
                destination: destination.0,
                mode: mode.0,
            }),
            types::Protocol::Donder { device } => ControllerProtocol::Donder(DonderConfig {
                device: DonderDeviceId::parse(device).ok_or_else(|| {
                    self.invalid(document, span, "a Donder device is 12 lowercase hex digits")
                })?,
            }),
        };
        let ports = controller
            .ports
            .iter()
            .zip(1..)
            .map(|(port, index)| ControllerPort {
                id: ControllerPortId(index),
                name: name(&port.name),
                address: match port.address {
                    types::PortAddress::Universe { universe } => {
                        ControllerPortAddress::E131Universe(universe)
                    }
                    types::PortAddress::ArtNetPort { port_address } => {
                        ControllerPortAddress::ArtNetPort(port_address)
                    }
                    types::PortAddress::Output { output } => {
                        ControllerPortAddress::DonderOutput(output)
                    }
                },
                slot_count: port.slots,
            })
            .collect();
        Ok(Controller {
            id: *id,
            description: controller.description.clone(),
            protocol,
            ports,
        })
    }

    fn layout(
        &mut self,
        id: &LayoutId,
        document: &DocumentId,
        layout: &types::Layout,
    ) -> Result<Layout, LoadError> {
        // Items are numbered in authored order; members may name later items.
        let mut ids = std::collections::HashMap::new();
        for (index, item) in layout.items.iter().enumerate() {
            let (types::LayoutItem::Group {
                name: item_name, ..
            }
            | types::LayoutItem::Fixture {
                name: item_name, ..
            }) = item;
            let id = u32::try_from(index + 1)
                .map(FixtureInstanceId)
                .map_err(|_| self.invalid(document, item_name.0.span, "too many layout items"))?;
            ids.entry(name(item_name)).or_insert(id);
        }
        let members = |names: &[Name]| {
            names
                .iter()
                .map(|member| {
                    ids.get(&member.0.value).copied().ok_or_else(|| {
                        self.invalid(
                            document,
                            member.0.span,
                            format!("unknown layout member `{}`", member.0.value.as_str()),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>()
        };
        let root = members(&layout.root)?;
        let groups = layout
            .items
            .iter()
            .map(|item| match item {
                types::LayoutItem::Group { members: names, .. } => members(names),
                types::LayoutItem::Fixture { .. } => Ok(Vec::new()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut fixtures = Vec::with_capacity(layout.items.len());
        for ((item, group), id) in layout.items.iter().zip(groups).zip(1..) {
            fixtures.push(self.layout_item(document, FixtureInstanceId(id), item, group)?);
        }
        Ok(Layout {
            id: *id,
            description: layout.description.clone(),
            fixtures,
            root,
        })
    }

    fn layout_item(
        &mut self,
        document: &DocumentId,
        id: FixtureInstanceId,
        item: &types::LayoutItem,
        members: Vec<FixtureInstanceId>,
    ) -> Result<LayoutFixture, LoadError> {
        Ok(match item {
            types::LayoutItem::Group {
                name: group,
                description,
                ..
            } => LayoutFixture {
                id,
                name: name(group),
                description: description.clone(),
                kind: LayoutFixtureKind::Group { members },
            },
            types::LayoutItem::Fixture {
                name: fixture,
                description,
                definition,
                transform,
            } => LayoutFixture {
                id,
                name: name(fixture),
                description: description.clone(),
                kind: LayoutFixtureKind::Fixture {
                    definition: match definition {
                        OwnedRecord::Reference(reference) => OwnedFixture::Reference(
                            self.declared_reference::<FixtureDefinition>(document, reference)?,
                        ),
                        OwnedRecord::Inline(definition) => {
                            OwnedFixture::Inline(self.fixture_definition(document, definition)?)
                        }
                    },
                    transform: self.transform(document, fixture.0.span, transform)?,
                },
            },
        })
    }

    fn transform(
        &self,
        document: &DocumentId,
        span: TextSpan,
        transform: &types::Transform,
    ) -> Result<FixtureTransform, LoadError> {
        let (x, y, z) = transform.position;
        let position = match (distance(x), distance(y), distance(z)) {
            (Some(x), Some(y), Some(z)) => Point3 { x, y, z },
            _ => {
                return Err(self.invalid(
                    document,
                    span,
                    "positions are within 2,000 meters of the origin",
                ));
            }
        };
        let (rx, ry, rz) = transform.rotation;
        let (sx, sy, sz) = transform.scale;
        Ok(FixtureTransform {
            position,
            rotation: Rotation3 {
                x: rx,
                y: ry,
                z: rz,
            },
            scale: Scale3 {
                x: sx,
                y: sy,
                z: sz,
            },
        })
    }

    fn fixture_definition(
        &self,
        document: &DocumentId,
        definition: &types::FixtureDefinition,
    ) -> Result<FixtureDefinition, LoadError> {
        let elements = definition
            .shapes
            .iter()
            .enumerate()
            .map(|(index, shape)| {
                let span = shape.name.0.span;
                let diameter = u32::try_from(shape.diameter.0).map_err(|_| {
                    self.invalid(document, span, "a pixel's diameter is 1µm to 100m")
                })?;
                Ok(FixtureElement {
                    id: FixtureElementId(index as u32 + 1),
                    name: name(&shape.name),
                    transform: self.transform(document, span, &shape.transform)?,
                    diameter: DistanceSpan {
                        micrometers: diameter,
                    },
                    reverse: shape.reverse,
                    shape: match &shape.geometry {
                        types::Geometry::Pixel => FixtureShape::Pixel,
                        types::Geometry::Line { length, count } => FixtureShape::Line {
                            length: *length,
                            count: *count,
                        },
                        types::Geometry::Polyline { points, count } => FixtureShape::Polyline {
                            points: points
                                .iter()
                                .map(|&(x, y, z)| match (distance(x), distance(y), distance(z)) {
                                    (Some(x), Some(y), Some(z)) => Ok(Point3 { x, y, z }),
                                    _ => Err(self.invalid(
                                        document,
                                        span,
                                        "points are within 2,000 meters of the origin",
                                    )),
                                })
                                .collect::<Result<_, _>>()?,
                            count: *count,
                        },
                        types::Geometry::Arc {
                            radius,
                            start_degrees,
                            sweep_degrees,
                            count,
                            closed,
                        } => FixtureShape::Arc {
                            radius: *radius,
                            start_degrees: *start_degrees,
                            sweep_degrees: *sweep_degrees,
                            count: *count,
                            closed: *closed,
                        },
                        types::Geometry::Grid {
                            columns,
                            rows,
                            width,
                            height,
                            axis,
                            corner,
                            serpentine,
                        } => FixtureShape::Grid {
                            columns: *columns,
                            rows: *rows,
                            width: *width,
                            height: *height,
                            axis: axis.0,
                            corner: corner.0,
                            serpentine: *serpentine,
                        },
                    },
                })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        Ok(FixtureDefinition {
            description: definition.description.clone(),
            elements,
        })
    }

    fn patch(
        &mut self,
        id: &PatchId,
        document: &DocumentId,
        patch: &types::Patch,
    ) -> Result<Patch, LoadError> {
        let routes = patch
            .routes
            .iter()
            .enumerate()
            .map(|(index, route)| {
                let address = self.loader.resolve_object_reference(
                    document,
                    &route.controller,
                    ObjectKind::Controller,
                )?;
                // An owned address names a controller only if its owner declares one there.
                if self
                    .with_owned(&address, |owned| {
                        matches!(owned, Located::Controller(_)).then_some(())
                    })
                    .is_none()
                {
                    return Err(self.loader.unresolved(document, &route.controller));
                }
                let controller = ControllerId(self.loader.intern(address));
                Ok(PixelRoute {
                    id: PixelRouteId(index as u32 + 1),
                    target: self.fixture_target(document, &route.target)?,
                    pixels: route.pixels.as_ref().map(|span| PixelSpan {
                        start: span.start,
                        count: span.count,
                    }),
                    port: self.port(document, &controller, &route.port)?,
                    controller,
                    start_slot: route.start_slot,
                    encoding: match route.encoding {
                        types::Encoding::Rgb { order: (r, g, b) } => {
                            PixelEncoding::Rgb { order: [r, g, b] }
                        }
                        types::Encoding::Rgbw {
                            order: (r, g, b, w),
                        } => PixelEncoding::Rgbw {
                            order: [r, g, b, w],
                        },
                    },
                    gamma: route.gamma,
                    brightness: route.brightness,
                    dither: route.dither,
                })
            })
            .collect::<Result<_, LoadError>>()?;
        Ok(Patch {
            id: *id,
            description: patch.description.clone(),
            routes,
        })
    }

    /// Link a name at `span` to a name of the same document.
    fn link_local(&self, document: &DocumentId, span: TextSpan, target: TextSpan) {
        self.loader.link(
            document,
            span,
            LinkTarget::Data {
                document: document.clone(),
                span: target,
            },
        );
    }

    /// The layers of a sequence or console, numbered from 0, the default layer.
    fn layers(layers: &[types::Layer]) -> (Vec<SequenceLayer>, LayerIds) {
        let mut layer_ids = IndexMap::new();
        let layers = layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                // Layer 0, the first, is the default layer.
                let id = SequenceLayerId(index as u32);
                layer_ids
                    .entry(name(&layer.name))
                    .or_insert((id.clone(), layer.name.0.span));
                SequenceLayer {
                    id,
                    name: name(&layer.name),
                    description: layer.description.clone(),
                    color: layer.color,
                    enabled: layer.enabled,
                }
            })
            .collect();
        (layers, layer_ids)
    }

    fn layer(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        layer: &Name,
    ) -> Result<SequenceLayerId, LoadError> {
        let (id, span) = layer_ids.get(&layer.0.value).cloned().ok_or_else(|| {
            self.invalid(
                document,
                layer.0.span,
                format!("no layer `{}`", layer.as_str()),
            )
        })?;
        self.link_local(document, layer.0.span, span);
        Ok(id)
    }

    /// A graph node by name: its identity, whether it is the root, and its
    /// operator.
    fn node(
        &self,
        document: &DocumentId,
        node_ids: &NodeIds,
        node: &Spanned<Identifier>,
    ) -> Result<
        (
            CompositionGraphNodeId,
            bool,
            Option<donder_model::OperatorDefinitionId>,
        ),
        LoadError,
    > {
        let (id, root, declared, operator) =
            node_ids.get(&node.value).cloned().ok_or_else(|| {
                self.invalid(
                    document,
                    node.span,
                    format!("no graph node `{}`", node.value.as_str()),
                )
            })?;
        if let Some(declared) = declared {
            self.link_local(document, node.span, declared);
        }
        Ok((id, root, operator))
    }

    /// The composition graph of a sequence or console, and its nodes by name.
    fn graph(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        graph: &types::Graph,
    ) -> Result<(SequenceCompositionGraph, NodeIds), LoadError> {
        let mut node_ids = NodeIds::new();
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for (index, node) in graph.nodes.iter().enumerate() {
            let id = CompositionGraphNodeId(index as u32 + 1);
            let (node_name, root, position, kind, declared, operator) = match node {
                types::Node::LayerNode {
                    layer: node_layer,
                    position,
                } => (
                    name(node_layer),
                    false,
                    position,
                    CompositionGraphNodeKind::Layer {
                        layer_id: self.layer(document, layer_ids, node_layer)?,
                    },
                    layer_ids
                        .get(&node_layer.0.value)
                        .map(|(_, layer_span)| *layer_span),
                    None,
                ),
                types::Node::OperatorNode {
                    name: node_name,
                    operator,
                    params,
                    position,
                } => {
                    let ObjectId::OperatorDefinition(definition) = self.loader.resolve_reference(
                        document,
                        operator,
                        ObjectKind::OperatorDefinition,
                    )?
                    else {
                        return Err(self.loader.unresolved(document, operator));
                    };
                    let declarations = self
                        .project
                        .operators
                        .get(&definition)
                        .map(|definition| definition.params().to_vec())
                        .unwrap_or_else(|| {
                            unreachable!("a resolved operator definition is stored")
                        });
                    let params =
                        self.params(document, definition.into(), &declarations, marks, params)?;
                    (
                        name(node_name),
                        false,
                        position,
                        CompositionGraphNodeKind::Operator(GraphOperatorNode {
                            name: name(node_name),
                            operator: definition,
                            params,
                        }),
                        Some(node_name.0.span),
                        Some(definition),
                    )
                }
                types::Node::RootNode { position } => (
                    donder_language::object_name(donder_language::data::ROOT_NODE),
                    true,
                    position,
                    CompositionGraphNodeKind::Root,
                    None,
                    None,
                ),
            };
            node_ids
                .entry(node_name)
                .or_insert((id.clone(), root, declared, operator));
            nodes.push(CompositionGraphNode {
                id,
                position: GraphNodePosition {
                    x: position.0,
                    y: position.1,
                },
                kind,
            });
        }
        let edges = graph
            .edges
            .iter()
            .map(|edge| {
                let (from, ..) = self.node(document, &node_ids, &edge.from.0)?;
                let (to, input) = match edge.to.segments.as_slice() {
                    [to] => match self.node(document, &node_ids, to)? {
                        (to, true, _) => (to, None),
                        _ => {
                            return Err(self.invalid(
                                document,
                                edge.to.span,
                                "an edge to an operator names its input: `node.input`",
                            ));
                        }
                    },
                    [to, input] => match self.node(document, &node_ids, to)? {
                        (to, false, operator) => {
                            if let Some(operator) = operator {
                                self.loader.link(
                                    document,
                                    input.span,
                                    self.script(
                                        operator,
                                        ScriptMember::Input(input.value.as_str().to_string()),
                                    ),
                                );
                            }
                            (to, Some(input.value.clone()))
                        }
                        _ => {
                            return Err(self.invalid(
                                document,
                                edge.to.span,
                                "an edge to the root is `to: root`",
                            ));
                        }
                    },
                    _ => return Err(self.loader.unresolved(document, &edge.to)),
                };
                Ok(EffectGraphEdge { from, to, input })
            })
            .collect::<Result<Vec<_>, LoadError>>()?;
        Ok((SequenceCompositionGraph { nodes, edges }, node_ids))
    }

    fn effect_definition(
        &self,
        document: &DocumentId,
        effect: &Reference,
    ) -> Result<EffectDefinitionId, LoadError> {
        let ObjectId::EffectDefinition(definition) =
            self.loader
                .resolve_reference(document, effect, ObjectKind::EffectDefinition)?
        else {
            return Err(self.loader.unresolved(document, effect));
        };
        Ok(definition)
    }

    fn sequence(
        &mut self,
        id: &SequenceId,
        document: &DocumentId,
        span: TextSpan,
        sequence: &types::Sequence,
    ) -> Result<Sequence, LoadError> {
        let audio = match &sequence.audio {
            None => SequenceAudio::None,
            Some(path) => SequenceAudio::Asset(self.audio(document, span, &path.0)?),
        };
        let mark_collections = sequence
            .marks
            .iter()
            .map(|collection| MarkCollection {
                key: MarkCollectionKey {
                    name: name(&collection.name),
                },
                description: collection.description.clone(),
                display_color: collection.color,
                marks: collection
                    .times
                    .iter()
                    .map(|mark| Mark {
                        time: DonderTime(mark.time),
                        label: mark.label.clone(),
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let marks = sequence
            .marks
            .iter()
            .map(|collection| (name(&collection.name), collection.name.0.span))
            .collect::<Vec<_>>();
        let (layers, layer_ids) = Self::layers(&sequence.layers);
        let mut clip_ids = IndexMap::new();
        let mut clips = Vec::with_capacity(sequence.clips.len());
        for (index, clip) in sequence.clips.iter().enumerate() {
            let id = ClipId(index as u32 + 1);
            let definition = self.effect_definition(document, &clip.effect)?;
            let params = self.definition_params(&definition);
            clip_ids
                .entry(name(&clip.name))
                .or_insert((id.clone(), clip.name.0.span, definition));
            clips.push(std::sync::Arc::new(Clip {
                id,
                name: name(&clip.name),
                description: clip.description.clone(),
                layer_id: self.layer(document, &layer_ids, &clip.layer)?,
                start: DonderTime(clip.start),
                duration: DonderDuration(clip.duration),
                target: self.fixture_target(document, &clip.target)?,
                scope: clip.scope.0.clone(),
                definition,
                param_overrides: self.params(
                    document,
                    definition.into(),
                    &params,
                    &marks,
                    &clip.params,
                )?,
            }));
        }
        let (composition_graph, node_ids) =
            self.graph(document, &layer_ids, &marks, &sequence.graph)?;
        let binding = |binding: &types::Binding| {
            self.automation_target(document, &clip_ids, &node_ids, binding)
        };
        let mut automation_clips = Vec::with_capacity(sequence.automation.len());
        for (index, clip) in sequence.automation.iter().enumerate() {
            let bindings = clip
                .bindings
                .iter()
                .map(|bound| {
                    binding(&bound.binding).map(|target| AutomationBinding {
                        target,
                        window: window(bound.window),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            let detached_bindings = clip
                .detached
                .iter()
                .map(|detached| {
                    Ok(DetachedAutomationBinding {
                        target: binding(&detached.binding)?,
                    })
                })
                .collect::<Result<Vec<_>, LoadError>>()?;
            automation_clips.push(AutomationClip {
                id: AutomationClipId(index as u32 + 1),
                start: DonderTime(clip.start),
                duration: DonderDuration(clip.duration),
                row_target: self.fixture_target(document, &clip.row)?,
                curve: curve(&clip.curve.points),
                bindings,
                detached_bindings,
            });
        }
        Ok(Sequence {
            id: *id,
            description: sequence.description.clone(),
            duration: DonderDuration(sequence.duration),
            frame_rate: sequence.frame_rate,
            audio,
            mark_collections,
            layers,
            clips,
            composition_graph,
            automation_clips,
        })
    }

    /// A sequence's or console's automation binding, its clip named in
    /// `clip_ids` or its node in `node_ids`.
    fn automation_target<Clip: Clone>(
        &self,
        document: &DocumentId,
        clip_ids: &IndexMap<Identifier, (Clip, TextSpan, EffectDefinitionId)>,
        node_ids: &NodeIds,
        binding: &types::Binding,
    ) -> Result<AutomationTarget<Clip>, LoadError> {
        Ok(match binding {
            types::Binding::ClipParam { clip, param } => {
                let (clip_id, span, effect) =
                    clip_ids.get(&clip.0.value).cloned().ok_or_else(|| {
                        self.invalid(
                            document,
                            clip.0.span,
                            format!("no clip `{}`", clip.as_str()),
                        )
                    })?;
                self.link_local(document, clip.0.span, span);
                self.link_param(document, effect, param);
                AutomationTarget::ClipParam {
                    clip_id,
                    param: name(param),
                }
            }
            types::Binding::NodeParam {
                node: target,
                param,
            } => {
                let (node_id, _, operator) = self.node(document, node_ids, &target.0)?;
                if let Some(operator) = operator {
                    self.loader.link(
                        document,
                        param.0.span,
                        self.script(operator, ScriptMember::Param(param.as_str().to_string())),
                    );
                }
                AutomationTarget::CompositionNodeParam {
                    node_id,
                    param: name(param),
                }
            }
        })
    }

    fn console(
        &self,
        id: &ConsoleId,
        document: &DocumentId,
        console: &types::Console,
    ) -> Result<Console, LoadError> {
        let mut mark_names = IndexMap::new();
        for collection in &console.marks {
            mark_names
                .entry(name(&collection.name))
                .or_insert(collection.name.0.span);
        }
        let marks = mark_names.clone().into_iter().collect::<Vec<_>>();
        let (layers, layer_ids) = Self::layers(&console.layers);
        let mut clip_ids = IndexMap::new();
        let mut clips = Vec::with_capacity(console.clips.len());
        for (clip, index) in console.clips.iter().zip(1..) {
            let definition = self.effect_definition(document, &clip.effect)?;
            let declarations = self.definition_params(&definition);
            clip_ids.entry(name(&clip.name)).or_insert((
                ConsoleClipId(index),
                clip.name.0.span,
                definition,
            ));
            clips.push(ConsoleClip {
                id: ConsoleClipId(index),
                name: name(&clip.name),
                description: clip.description.clone(),
                layer_id: self.layer(document, &layer_ids, &clip.layer)?,
                target: self.fixture_target(document, &clip.target)?,
                scope: clip.scope.0.clone(),
                param_overrides: self.params(
                    document,
                    definition.into(),
                    &declarations,
                    &marks,
                    &clip.params,
                )?,
                definition,
            });
        }
        let clip = |clip: &Name| {
            let (id, span, definition) = clip_ids.get(&clip.0.value).copied().ok_or_else(|| {
                self.invalid(
                    document,
                    clip.0.span,
                    format!("no clip `{}`", clip.as_str()),
                )
            })?;
            self.link_local(document, clip.0.span, span);
            Ok::<_, LoadError>((id, definition))
        };
        let (composition_graph, node_ids) =
            self.graph(document, &layer_ids, &marks, &console.graph)?;
        let always = console
            .always
            .iter()
            .map(|name| clip(name).map(|(id, _)| id))
            .collect::<Result<_, _>>()?;
        let values = |definition: EffectDefinitionId, values: &[types::PlayedValue]| {
            values
                .iter()
                .map(|value| {
                    self.link_param(document, definition, &value.param);
                    PlayedValue {
                        param: name(&value.param),
                        from: value.from.0,
                        window: window(value.window),
                    }
                })
                .collect()
        };
        let mut events = Vec::with_capacity(console.events.len());
        for (on, index) in console.events.iter().zip(1..) {
            let control = self.control_ref(document, on.event.control())?;
            let actions = on
                .actions
                .iter()
                .map(|action| {
                    Ok(match action {
                        types::Action::Play {
                            clip: played,
                            copies,
                            length,
                            values: played_values,
                        } => {
                            let (clip, definition) = clip(played)?;
                            Action::Play(Play {
                                clip,
                                copies: *copies,
                                length: match length {
                                    types::PlayLength::Fixed { duration } => {
                                        PlayLength::Fixed(DonderDuration(*duration))
                                    }
                                    types::PlayLength::Scaled {
                                        by,
                                        shortest,
                                        longest,
                                    } => PlayLength::Scaled {
                                        by: match by {
                                            types::LengthSource::Velocity => LengthSource::Velocity,
                                            types::LengthSource::Axis { control } => {
                                                LengthSource::Axis(
                                                    self.control_ref(document, control)?,
                                                )
                                            }
                                        },
                                        shortest: DonderDuration(*shortest),
                                        longest: DonderDuration(*longest),
                                    },
                                },
                                values: values(definition, played_values),
                            })
                        }
                        types::Action::Start {
                            clip: started,
                            values: played_values,
                        } => {
                            let (clip, definition) = clip(started)?;
                            Action::Start {
                                clip,
                                values: values(definition, played_values),
                            }
                        }
                        types::Action::Stop { clip: stopped } => Action::Stop {
                            clip: clip(stopped)?.0,
                        },
                        types::Action::Toggle {
                            clip: toggled,
                            values: played_values,
                        } => {
                            let (clip, definition) = clip(toggled)?;
                            Action::Toggle {
                                clip,
                                values: values(definition, played_values),
                            }
                        }
                        types::Action::Mark { into } => {
                            if let Some(span) = mark_names.get(&into.0.value) {
                                self.link_local(document, into.0.span, *span);
                            }
                            Action::Mark(MarkCollectionKey { name: name(into) })
                        }
                        types::Action::Set {
                            param,
                            window: set_window,
                        } => Action::Set {
                            target: self
                                .automation_target(document, &clip_ids, &node_ids, param)?,
                            window: window(*set_window),
                        },
                    })
                })
                .collect::<Result<_, LoadError>>()?;
            events.push(On {
                id: OnId(index),
                event: match on.event {
                    types::InputEvent::Press { .. } => InputEvent::Press(control),
                    types::InputEvent::Release { .. } => InputEvent::Release(control),
                    types::InputEvent::Move { .. } => InputEvent::Move(control),
                },
                actions,
            });
        }
        Ok(Console {
            id: *id,
            description: console.description.clone(),
            frame_rate: console.frame_rate,
            live_marks: console
                .marks
                .iter()
                .map(|collection| LiveMarks {
                    key: MarkCollectionKey {
                        name: name(&collection.name),
                    },
                    description: collection.description.clone(),
                    display_color: collection.color,
                })
                .collect(),
            layers,
            clips,
            always,
            events,
            composition_graph,
        })
    }

    /// Link `param` to its declaration in `definition`, when it declares it.
    fn link_param(&self, document: &DocumentId, definition: EffectDefinitionId, param: &Name) {
        if self
            .definition_params(&definition)
            .iter()
            .any(|declaration| declaration.name == param.0.value)
        {
            self.loader.link(
                document,
                param.0.span,
                self.script(definition, ScriptMember::Param(param.as_str().to_string())),
            );
        }
    }

    fn definition_params(&self, definition: &EffectDefinitionId) -> Vec<ParamDecl> {
        self.project
            .effects
            .get(definition)
            .map(|definition| definition.params().to_vec())
            .unwrap_or_else(|| unreachable!("a resolved effect definition is stored"))
    }

    fn audio(
        &mut self,
        document: &DocumentId,
        span: TextSpan,
        path: &str,
    ) -> Result<AssetId, LoadError> {
        let root = self.loader.workspace.root.clone();
        crate::source::reference_asset(&mut self.loader.referenced_assets, &root, document, path)
            .map_err(|error| match error {
                crate::AssetError::Read(source) => {
                    crate::diagnostics::read_error(root.join(path), &source)
                }
                error => self.invalid(document, span, format!("audio asset {path}: {error}")),
            })
    }

    /// Parameter values, typed by their declarations and written in
    /// declaration order.
    fn params(
        &self,
        document: &DocumentId,
        definition: ObjectId,
        declarations: &[ParamDecl],
        marks: &[(Identifier, TextSpan)],
        params: &Params,
    ) -> Result<IndexMap<Identifier, ParamValue>, LoadError> {
        let mut values = IndexMap::new();
        let mut previous = None;
        for (param, value) in &params.0 {
            let Some(index) = declarations
                .iter()
                .position(|declaration| declaration.name == param.value)
            else {
                return Err(self.invalid(
                    document,
                    param.span,
                    format!("no parameter `{}`", param.value.as_str()),
                ));
            };
            if previous.is_some_and(|previous| index <= previous) {
                return Err(self.invalid(
                    document,
                    param.span,
                    "parameters are written once each, in the definition's order",
                ));
            }
            previous = Some(index);
            self.loader.link(
                document,
                param.span,
                self.script(
                    definition,
                    ScriptMember::Param(param.value.as_str().to_string()),
                ),
            );
            let value = self.param_value(
                document,
                (definition, &param.value),
                &declarations[index].ty,
                marks,
                value,
            )?;
            values.insert(param.value.clone(), value);
        }
        Ok(values)
    }

    fn param_value(
        &self,
        document: &DocumentId,
        param: (ObjectId, &Identifier),
        ty: &Type,
        marks: &[(Identifier, TextSpan)],
        value: &Spanned<DataValue>,
    ) -> Result<ParamValue, LoadError> {
        let reference = |segments: &[Spanned<Identifier>]| Reference {
            segments: segments.to_vec(),
            span: value.span,
        };
        Ok(match (ty, &value.value) {
            (Type::Int, DataValue::Integer(integer)) => ParamValue::Int(
                i32::try_from(*integer)
                    .map_err(|_| self.invalid(document, value.span, "this int is out of range"))?,
            ),
            (Type::Float, DataValue::Float(float)) => ParamValue::Float(*float),
            (Type::Bool, DataValue::Bool(bool)) => ParamValue::Bool(*bool),
            (Type::Color, DataValue::Color(color)) => ParamValue::Color(*color),
            (Type::Enum(options), DataValue::Variant(option))
                if options.contains(&option.value) =>
            {
                self.loader.link(
                    document,
                    option.span,
                    self.script(
                        param.0,
                        ScriptMember::Option {
                            param: param.1.as_str().to_string(),
                            option: option.value.as_str().to_string(),
                        },
                    ),
                );
                ParamValue::Enum(option.value.clone())
            }
            (Type::Marks, DataValue::Reference(segments))
                if let [segment] = segments.as_slice()
                    && let Some((_, span)) =
                        marks.iter().find(|(mark, _)| mark == &segment.value) =>
            {
                self.loader.link(
                    document,
                    segment.span,
                    LinkTarget::Data {
                        document: document.clone(),
                        span: *span,
                    },
                );
                ParamValue::Marks(Some(MarkCollectionKey {
                    name: segment.value.clone(),
                }))
            }
            (Type::Marks, DataValue::None) => ParamValue::Marks(None),
            (Type::Curve, DataValue::Reference(segments)) => {
                let ObjectId::Curve(curve) = self.loader.resolve_reference(
                    document,
                    &reference(segments),
                    ObjectKind::Curve,
                )?
                else {
                    return Err(self.loader.unresolved(document, &reference(segments)));
                };
                ParamValue::Curve(OwnedCurve::Reference(curve))
            }
            (Type::Curve, DataValue::Record(..)) => ParamValue::Curve(OwnedCurve::Inline(curve(
                &self.decode::<types::CurveValue>(document, value)?.points,
            ))),
            (Type::Gradient, DataValue::Reference(segments)) => {
                let ObjectId::Gradient(gradient) = self.loader.resolve_reference(
                    document,
                    &reference(segments),
                    ObjectKind::Gradient,
                )?
                else {
                    return Err(self.loader.unresolved(document, &reference(segments)));
                };
                ParamValue::Gradient(OwnedGradient::Reference(gradient))
            }
            (Type::Gradient, DataValue::Record(..)) => {
                let literal = self.decode::<types::GradientValue>(document, value)?;
                ParamValue::Gradient(OwnedGradient::Inline(gradient(
                    &literal.space,
                    &literal.stops,
                )))
            }
            (Type::Array(item), DataValue::List(items)) => ParamValue::Array(
                items
                    .iter()
                    .map(|value| self.param_value(document, param, item, marks, value))
                    .collect::<Result<_, _>>()?,
            ),
            _ => {
                return Err(self.invalid(
                    document,
                    value.span,
                    format!("expected {}", describe(ty, marks)),
                ));
            }
        })
    }

    fn decode<T: Data>(
        &self,
        document: &DocumentId,
        value: &Spanned<DataValue>,
    ) -> Result<T, LoadError> {
        let mut decoder = Decoder::default();
        let decoded = T::decode(value, &mut decoder);
        match (decoded, decoder.diagnostics.into_iter().next()) {
            (Some(decoded), None) => Ok(decoded),
            (_, Some(diagnostic)) => {
                Err(self.invalid(document, diagnostic.span, diagnostic.message))
            }
            (None, None) => Err(self.invalid(document, value.span, "invalid value")),
        }
    }
}

/// The values a parameter type takes, as a diagnostic names them.
fn describe(ty: &Type, marks: &[(Identifier, TextSpan)]) -> String {
    match ty {
        Type::Int => "an int like `3`".into(),
        Type::Float => "a float like `1.0`".into(),
        Type::Bool => "`true` or `false`".into(),
        Type::Color => "a color like `#ff8800`".into(),
        Type::Enum(options) => format!(
            "one of {}",
            options
                .iter()
                .map(|option| format!("`{}`", option.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Marks if marks.is_empty() => "a mark collection, but this sequence has none".into(),
        Type::Marks => format!(
            "a mark collection: {}",
            marks
                .iter()
                .map(|(mark, _)| format!("`{}`", mark.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Curve => {
            "a curve: a reference or `Curve { points: [(position, value), ...] }`".into()
        }
        Type::Gradient => "a gradient: a reference or \
            `Gradient { space: LinearRgb, stops: [(position, #color), ...] }`"
            .into(),
        Type::Array(item) => format!("a list of {}", describe(item, marks)),
        Type::Void => "no value".into(),
    }
}
