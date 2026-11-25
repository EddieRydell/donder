//! Decoded declarations to typed state. Session identities are assigned here
//! in document order, and names resolve to them. Identities inside another
//! object (fixtures, ports) follow from that object's declaration alone, so a
//! reference resolves without the referenced object being built first.
use std::num::NonZeroU32;
use std::sync::Arc;

use crate::address::{DocumentId, SourceIdentity};
use donder_language::compiler::{ParamDecl, TextSpan};
use donder_language::data::{Data, Decoder, Meters, Name, Params, Reference};
use donder_language::data::{DataValue, Spanned};
use donder_language::{
    Distance, DistanceSpan, DonderDuration, DonderTime, Point3, Rotation3, Scale3,
};
use donder_model::GroupMember;
use donder_model::ObjectId;
use donder_model::ProjectData;
use donder_model::{Action, Always, Console, ConsoleId, LengthSource, On, OnId, Play, PlayLength};
use donder_model::{
    ArtNetConfig, Controller, ControllerId, ControllerPort, ControllerPortId, ControllerProtocol,
    DonderConfig, DonderDeviceId, DonderPort, E131Config, E131Mode, RouteStart, Universes,
};
use donder_model::{
    AssetId, AutomationTarget, Binding, CompositionGraph, CompositionGraphNode,
    CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge, Envelope, GraphNodePosition,
    Layer, LayerId, Mark, MarkCollection, MarkCollectionId, Sequence, SequenceAudio, SequenceId,
    Window,
};
use donder_model::{
    AxisInput, ButtonInput, ControlRef, InputControl, InputControlId, InputControlKind,
    InputDevice, InputDeviceId, InputDriver, MidiChannel, NoteRange, OscAddress, PlayedValue,
};
use donder_model::{
    Clip, ClipContent, ClipId, CurveValue, Effect, GradientValue, ParamValue, Placement, ScriptId,
};
use donder_model::{Composition, GraphOperatorNode};
use donder_model::{
    FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
    FixtureTransform,
};
use donder_model::{FixtureId, Layout, LayoutFixture, LayoutFixtureKind, LayoutId};
use donder_model::{PixelRoute, PixelRouteId, PixelSpan};
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, CurvePoint, Gradient, GradientStop};
use donder_runtime_types::{Identifier, Type};
use indexmap::IndexMap;

use super::{DataDocument, Loader};
use crate::diagnostics::LoadError;
use crate::document::{Declaration, types};
use crate::index::{LinkTarget, ScriptMember};

pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut ProjectData,
}

/// Each layer of a sequence or console by name, with its declaring name's span.
type LayerIds = IndexMap<Identifier, (LayerId, TextSpan)>;

/// Each clip of a sequence or console by name, with its declaring name's span
/// and its effect.
/// Each clip by name: its id, the name it is declared by, and an effect
/// clip's effect.
type ClipIds = IndexMap<Identifier, (ClipId, TextSpan, Option<ScriptId>)>;

/// Each operator node and the root by name: its identity, whether it is the
/// root, the name it is declared by, and its operator. Layer nodes are named
/// by their layer, in edges only.
type NodeIds = IndexMap<
    Identifier,
    (
        CompositionGraphNodeId,
        bool,
        Option<TextSpan>,
        Option<donder_model::ScriptId>,
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

fn distance(meters: Meters) -> Option<Distance> {
    i32::try_from(meters.0)
        .ok()
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
pub(super) trait Resolve: donder_model::ProjectObject {
    type Document;
    fn document(declaration: &Declaration) -> Option<&Self::Document>;
    fn loaded(project: &mut ProjectData) -> &mut IndexMap<Self::Id, Self>;
    /// The object written as `value` in `document`; `span` is its
    /// declaration's.
    fn build(
        resolver: &mut DomainResolver<'_>,
        document: &DocumentId,
        span: TextSpan,
        value: &Self::Document,
    ) -> Result<Self, LoadError>;
}

/// Each kind's model type, document type, declaration and object kind share
/// its name.
macro_rules! resolve_kinds {
    ($($kind:ident($id:ident) in $store:ident =
        |$resolver:pat_param, $document:pat_param, $span:pat_param, $value:pat_param|
        $build:expr;)+) => {$(
        impl Resolve for $kind {
            type Document = types::$kind;
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
    Layout(LayoutId) in layouts = |resolver, document, _, value| resolver.layout(document, value);
    Controller(ControllerId) in controllers =
        |resolver, document, span, value| resolver.controller(document, span, value);
    InputDevice(InputDeviceId) in input_devices =
        |_, _, _, value| Ok(DomainResolver::input_device(value));
    Sequence(SequenceId) in sequences =
        |resolver, document, span, value| resolver.sequence(document, span, value);
    Console(ConsoleId) in consoles = |resolver, document, _, value| resolver.console(document, value);
    FixtureDefinition(FixtureDefinitionId) in fixtures =
        |resolver, document, _, value| resolver.fixture_definition(document, value);
}

impl DomainResolver<'_> {
    /// A script declaration or one of its members.
    fn script(&self, id: impl Into<ObjectId>, member: ScriptMember) -> LinkTarget {
        let identity = self.loader.address(id);
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

    /// A declared object of kind `T`, resolved once.
    pub(super) fn resolve<T: Resolve>(&mut self, id: T::Id) -> Result<(), LoadError> {
        if T::loaded(self.project).contains_key(&id) {
            return Ok(());
        }
        let source = self.loader.address(id);
        let (data, span) = self.declared(&source)?;
        let value = T::document(data.declared(source.object()).1)
            .ok_or_else(|| self.wrong_kind(&source))?;
        let value = T::build(self, source.document_id(), span, value)?;
        T::loaded(self.project).insert(id, value);
        Ok(())
    }

    /// The declared object of kind `T` a reference names, resolved.
    fn declared_reference<T: Resolve>(
        &mut self,
        document: &DocumentId,
        reference: &Reference,
    ) -> Result<T::Id, LoadError> {
        let id = self
            .loader
            .resolve_reference::<T::Id>(document, reference)?;
        self.resolve::<T>(id)?;
        Ok(id)
    }

    // Identities inside other objects, from their declarations.

    /// What `read` finds in the declaration of `identity`.
    fn with_declaration<R>(
        &self,
        identity: &SourceIdentity,
        read: impl FnOnce(&Declaration) -> Option<R>,
    ) -> Option<R> {
        let data = self.loader.declaration(identity).ok()?;
        let (_, declaration) = data.declaration(identity.object())?;
        read(declaration)
    }

    /// `layout.fixture`: a fixture or group of a layout, at any depth.
    /// A fixture or group of the project's layout, by its name alone.
    fn fixture_target(
        &self,
        document: &DocumentId,
        reference: &Reference,
    ) -> Result<FixtureId, LoadError> {
        let [fixture] = reference.segments.as_slice() else {
            return Err(self.loader.unresolved(document, reference));
        };
        let Some(address) = self.loader.layout.clone() else {
            unreachable!("the project's layout is found before its fixtures are named");
        };
        let (index, item) = self
            .with_declaration(&address, |declaration| match declaration {
                Declaration::Layout(layout) => fixture_index(&layout.items, &fixture.value),
                _ => None,
            })
            .ok_or_else(|| {
                self.invalid(
                    document,
                    fixture.span,
                    format!(
                        "the layout has no fixture or group `{}`",
                        fixture.value.as_str()
                    ),
                )
            })?;
        self.loader.link(
            document,
            fixture.span,
            LinkTarget::Data {
                document: address.document_id().clone(),
                span: item,
            },
        );
        Ok(FixtureId(index))
    }

    fn port(
        &self,
        document: &DocumentId,
        controller: &ControllerId,
        port: &types::PortName,
    ) -> Result<ControllerPortId, LoadError> {
        let address = self.loader.address(*controller);
        self.with_declaration(&address, |declaration| match declaration {
            Declaration::Controller(types::Controller { protocol, .. }) => {
                let names = match protocol {
                    types::Protocol::E131 { ports, .. } | types::Protocol::ArtNet { ports, .. } => {
                        ports.iter().map(|port| &port.name).collect::<Vec<_>>()
                    }
                    types::Protocol::Donder { ports, .. } => {
                        ports.iter().map(|port| &port.name).collect()
                    }
                };
                names
                    .iter()
                    .position(|candidate| **candidate == *port)
                    .map(|index| (index, names[index].0.span))
            }
            _ => None,
        })
        .map(|(index, span)| {
            self.loader.link(
                document,
                port.0.span,
                LinkTarget::Data {
                    document: address.document_id().clone(),
                    span,
                },
            );
            ControllerPortId(index as u32 + 1)
        })
        .ok_or_else(|| {
            self.invalid(
                document,
                port.0.span,
                format!("the controller has no port \"{}\"", port.0.value),
            )
        })
    }

    // Objects.

    fn input_device(device: &types::InputDevice) -> InputDevice {
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
            .resolve_reference::<InputDeviceId>(document, reference)
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
        let device = self
            .loader
            .resolve_reference::<InputDeviceId>(document, &device_reference)?;
        let address = self.loader.address(device);
        let (index, span) = self
            .with_declaration(&address, |declaration| match declaration {
                Declaration::InputDevice(device) => {
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
                document: address.document_id().clone(),
                span,
            },
        );
        Ok(ControlRef {
            device,
            control: InputControlId(index),
        })
    }

    fn fseq_channel(
        &self,
        document: &DocumentId,
        span: TextSpan,
        channel: Option<u32>,
    ) -> Result<Option<NonZeroU32>, LoadError> {
        channel
            .map(|channel| {
                NonZeroU32::new(channel)
                    .ok_or_else(|| self.invalid(document, span, "FSEQ channels count from 1"))
            })
            .transpose()
    }

    fn controller(
        &self,
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
                start_universe,
                universes,
                channels,
                fseq_channel,
                ports,
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
                universes: Universes {
                    start: *start_universe,
                    count: *universes,
                    channels: *channels,
                },
                fseq_channel: self.fseq_channel(document, span, *fseq_channel)?,
                ports: controller_ports(ports.iter().map(|port| (&port.name, &port.description))),
            }),
            types::Protocol::ArtNet {
                bind_address,
                destination,
                mode,
                start_universe,
                universes,
                channels,
                fseq_channel,
                ports,
            } => ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: bind_address.0,
                destination: destination.0,
                mode: mode.0,
                universes: Universes {
                    start: *start_universe,
                    count: *universes,
                    channels: *channels,
                },
                fseq_channel: self.fseq_channel(document, span, *fseq_channel)?,
                ports: controller_ports(ports.iter().map(|port| (&port.name, &port.description))),
            }),
            types::Protocol::Donder { device, ports } => ControllerProtocol::Donder(DonderConfig {
                device: DonderDeviceId::parse(device).ok_or_else(|| {
                    self.invalid(document, span, "a Donder device is 12 lowercase hex digits")
                })?,
                ports: controller_ports(ports.iter().map(|port| (&port.name, &port.description)))
                    .into_iter()
                    .zip(ports)
                    .map(|(port, document)| DonderPort {
                        port,
                        output: document.output,
                        slot_count: document.slots,
                    })
                    .collect(),
            }),
        };
        Ok(Controller {
            description: controller.description.clone(),
            protocol,
        })
    }

    fn layout(
        &mut self,
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
                .map(FixtureId)
                .map_err(|_| self.invalid(document, item_name.0.span, "too many layout items"))?;
            ids.entry(name(item_name)).or_insert(id);
        }
        let item = |member: &Name| {
            ids.get(&member.0.value).copied().ok_or_else(|| {
                self.invalid(
                    document,
                    member.0.span,
                    format!("unknown layout member `{}`", member.0.value.as_str()),
                )
            })
        };
        let root = layout
            .root
            .iter()
            .map(item)
            .collect::<Result<Vec<_>, _>>()?;
        let groups = layout
            .items
            .iter()
            .map(|layout_item| match layout_item {
                types::LayoutItem::Group { members, .. } => members
                    .iter()
                    .map(|member| {
                        Ok::<_, LoadError>(match member {
                            types::GroupMember::Item(name) => GroupMember::Item(item(name)?),
                            types::GroupMember::Slice(slice) => GroupMember::Slice {
                                fixture: item(&slice.fixture)?,
                                pixels: donder_model::PixelSpan {
                                    start: slice.start,
                                    count: slice.count,
                                },
                            },
                        })
                    })
                    .collect::<Result<Vec<_>, _>>(),
                types::LayoutItem::Fixture { .. } => Ok(Vec::new()),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut fixtures = Vec::with_capacity(layout.items.len());
        for ((item, group), id) in layout.items.iter().zip(groups).zip(1..) {
            fixtures.push(self.layout_item(document, FixtureId(id), item, group)?);
        }
        Ok(Layout {
            description: layout.description.clone(),
            fixtures,
            root,
        })
    }

    fn layout_item(
        &mut self,
        document: &DocumentId,
        id: FixtureId,
        item: &types::LayoutItem,
        members: Vec<GroupMember>,
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
                    definition: self
                        .declared_reference::<FixtureDefinition>(document, definition)?,
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
                    "a position's coordinates are -2,147m to 2,147m",
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
                                        "a point's coordinates are -2,147m to 2,147m",
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

    pub(super) fn routes(
        &mut self,
        document: &DocumentId,
        routes: &[types::Route],
    ) -> Result<Vec<PixelRoute>, LoadError> {
        routes
            .iter()
            .enumerate()
            .map(|(index, route)| {
                let controller = self
                    .loader
                    .resolve_reference::<ControllerId>(document, &route.controller)?;
                Ok(PixelRoute {
                    id: PixelRouteId(index as u32 + 1),
                    target: self.fixture_target(document, &route.target)?,
                    pixels: route.pixels.as_ref().map(|span| PixelSpan {
                        start: span.start,
                        count: span.count,
                    }),
                    reverse: route.reverse,
                    port: self.port(document, &controller, &route.port)?,
                    controller,
                    start: match route.start {
                        types::RouteStart::Next => RouteStart::Next,
                        types::RouteStart::Channel { channel } => RouteStart::Channel(channel),
                    },
                    encoding: match &route.encoding {
                        types::Encoding::Rgb { order: (r, g, b) } => {
                            PixelEncoding::Rgb { order: [*r, *g, *b] }
                        }
                        types::Encoding::Rgbw {
                            order: (r, g, b, w),
                        } => PixelEncoding::Rgbw {
                            order: [*r, *g, *b, *w],
                        },
                        types::Encoding::Channels { colors } => {
                            let bytes: Vec<[u8; 3]> = colors
                                .iter()
                                .map(|color| {
                                    [color.red, color.green, color.blue]
                                        .map(|part| (part.clamp(0.0, 1.0) * 255.0).round() as u8)
                                })
                                .collect();
                            PixelEncoding::Channels {
                                colors: donder_runtime_types::ChannelColors::new(&bytes)
                                    .ok_or_else(|| {
                                        self.invalid(
                                            document,
                                            route.port.0.span,
                                            "a route's channel colors are 1 to 4 colors, none black",
                                        )
                                    })?,
                            }
                        }
                    },
                    gamma: route.gamma,
                    brightness: route.brightness,
                    dither: route.dither,
                })
            })
            .collect()
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

    /// The mark collections of a sequence or console, numbered from 0, and
    /// their names with the spans that declare them.
    fn mark_collections(
        collections: &[types::MarkCollection],
    ) -> (Vec<MarkCollection>, Vec<(Identifier, TextSpan)>) {
        let names = collections
            .iter()
            .map(|collection| (name(&collection.name), collection.name.0.span))
            .collect();
        let collections = collections
            .iter()
            .zip(0..)
            .map(|(collection, id)| MarkCollection {
                id: MarkCollectionId(id),
                name: name(&collection.name),
                description: collection.description.clone(),
                display_color: collection.color,
                marks: collection
                    .times
                    .iter()
                    .map(|mark| Mark {
                        time: DonderTime(mark.time),
                        duration: DonderDuration(mark.duration),
                        label: mark.label.clone(),
                    })
                    .collect(),
            })
            .collect();
        (collections, names)
    }

    /// The layers of a sequence or console, numbered from 0, the default layer.
    fn layers(layers: &[types::Layer]) -> (Vec<Layer>, LayerIds) {
        let mut layer_ids = IndexMap::new();
        let layers = layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                // Layer 0, the first, is the default layer.
                let id = LayerId(index as u32);
                layer_ids
                    .entry(name(&layer.name))
                    .or_insert((id.clone(), layer.name.0.span));
                Layer {
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
    ) -> Result<LayerId, LoadError> {
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
    ) -> Result<(CompositionGraphNodeId, bool, Option<donder_model::ScriptId>), LoadError> {
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

    /// The composition graph of a sequence or console, and its operator nodes
    /// and root by name.
    fn graph(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        graph: &types::Graph,
    ) -> Result<(CompositionGraph, NodeIds), LoadError> {
        let mut node_ids = NodeIds::new();
        let mut layer_nodes = IndexMap::new();
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for (index, node) in graph.nodes.iter().enumerate() {
            let id = CompositionGraphNodeId(index as u32 + 1);
            let (position, kind, named) = match node {
                types::Node::LayerNode {
                    layer: node_layer,
                    position,
                } => {
                    let layer_id = self.layer(document, layer_ids, node_layer)?;
                    layer_nodes.entry(layer_id.clone()).or_insert(id.clone());
                    (position, CompositionGraphNodeKind::Layer { layer_id }, None)
                }
                types::Node::OperatorNode {
                    name: node_name,
                    operator,
                    params,
                    position,
                } => {
                    let definition = self
                        .loader
                        .resolve_reference::<donder_model::ScriptId>(document, operator)?;
                    let declarations = self
                        .project
                        .scripts
                        .get(&definition)
                        .map(|definition| definition.params().to_vec())
                        .unwrap_or_else(|| {
                            unreachable!("a resolved operator definition is stored")
                        });
                    let params =
                        self.params(document, definition.into(), &declarations, marks, params)?;
                    (
                        position,
                        CompositionGraphNodeKind::Operator(GraphOperatorNode {
                            name: name(node_name),
                            operator: definition,
                            params,
                        }),
                        Some((
                            name(node_name),
                            false,
                            Some(node_name.0.span),
                            Some(definition),
                        )),
                    )
                }
                types::Node::RootNode { position } => (
                    position,
                    CompositionGraphNodeKind::Root,
                    Some((
                        donder_language::object_name(donder_language::data::ROOT_NODE),
                        true,
                        None,
                        None,
                    )),
                ),
            };
            if let Some((node_name, root, declared, operator)) = named {
                node_ids
                    .entry(node_name)
                    .or_insert((id.clone(), root, declared, operator));
            }
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
                let from = match edge.from.segments.as_slice() {
                    [scope, layer]
                        if scope.value.as_str() == donder_language::data::LAYERS_SCOPE =>
                    {
                        let layer_id = self.layer(document, layer_ids, &Name(layer.clone()))?;
                        layer_nodes.get(&layer_id).cloned().ok_or_else(|| {
                            self.invalid(
                                document,
                                layer.span,
                                format!("layer `{}` has no graph node", layer.value.as_str()),
                            )
                        })?
                    }
                    [node] => self.node(document, &node_ids, node)?.0,
                    _ => return Err(self.loader.unresolved(document, &edge.from)),
                };
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
        Ok((CompositionGraph { nodes, edges }, node_ids))
    }

    fn effect_definition(
        &self,
        document: &DocumentId,
        effect: &Reference,
    ) -> Result<ScriptId, LoadError> {
        self.loader.resolve_reference(document, effect)
    }

    fn sequence(
        &mut self,
        document: &DocumentId,
        span: TextSpan,
        sequence: &types::Sequence,
    ) -> Result<Sequence, LoadError> {
        let audio = match &sequence.audio {
            None => SequenceAudio::None,
            Some(path) => SequenceAudio::Asset(self.asset(document, span, "audio", &path.0)?),
        };
        let (mark_collections, marks) = Self::mark_collections(&sequence.marks);
        let (layers, layer_ids) = Self::layers(&sequence.layers);
        let (composition_graph, node_ids) =
            self.graph(document, &layer_ids, &marks, &sequence.graph)?;
        let (clips, _) = self.clips(document, &layer_ids, &marks, &node_ids, &sequence.clips)?;
        Ok(Sequence {
            description: sequence.description.clone(),
            duration: DonderDuration(sequence.duration),
            frame_rate: sequence.frame_rate,
            audio,
            composition: Composition {
                marks: mark_collections,
                layers,
                graph: composition_graph,
                clips,
            },
        })
    }

    /// The clips of a sequence or console, numbered from 1, and each by name
    /// with its declaring name's span and effect. An envelope binds clips and
    /// operator nodes by name, declared before or after it.
    fn clips(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        node_ids: &NodeIds,
        clips: &[types::Clip],
    ) -> Result<(Vec<Arc<Clip>>, ClipIds), LoadError> {
        let mut clip_ids = ClipIds::new();
        for (clip, index) in clips.iter().zip(1..) {
            let definition = match &clip.content {
                types::ClipContent::Effect { effect, .. } => {
                    Some(self.effect_definition(document, effect)?)
                }
                types::ClipContent::Envelope { .. } => None,
            };
            clip_ids.entry(name(&clip.name)).or_insert((
                ClipId(index),
                clip.name.0.span,
                definition,
            ));
        }
        let mut resolved = Vec::with_capacity(clips.len());
        for (clip, index) in clips.iter().zip(1..) {
            let id = ClipId(index);
            let (content, definition) = match &clip.content {
                types::ClipContent::Effect {
                    layer,
                    scope,
                    effect,
                    params,
                } => {
                    let definition = self.effect_definition(document, effect)?;
                    let declarations = self.definition_params(&definition);
                    (
                        ClipContent::Effect(Effect {
                            layer: self.layer(document, layer_ids, layer)?,
                            scope: scope.0,
                            definition,
                            params: self.params(
                                document,
                                definition.into(),
                                &declarations,
                                marks,
                                params,
                            )?,
                        }),
                        Some(definition),
                    )
                }
                types::ClipContent::Envelope { curve, bindings } => (
                    ClipContent::Envelope(Envelope {
                        curve: self::curve(&curve.points),
                        bindings: bindings
                            .iter()
                            .map(|bound| {
                                Ok(Binding {
                                    target: self.automation_target(
                                        document,
                                        &clip_ids,
                                        node_ids,
                                        &bound.binding,
                                    )?,
                                    window: window(bound.window),
                                })
                            })
                            .collect::<Result<_, LoadError>>()?,
                    }),
                    None,
                ),
            };
            resolved.push(Arc::new(Clip {
                id,
                name: name(&clip.name),
                description: clip.description.clone(),
                target: self.fixture_target(document, &clip.target)?,
                content,
                placement: match &clip.placement {
                    types::Placement::Timed { start, duration } => Placement::Timed {
                        start: DonderTime(*start),
                        duration: DonderDuration(*duration),
                    },
                    types::Placement::Played { values } => Placement::Played(
                        values
                            .iter()
                            .map(|value| {
                                if let Some(definition) = definition {
                                    self.link_param(document, definition, &value.param);
                                }
                                PlayedValue {
                                    param: name(&value.param),
                                    from: value.from.0,
                                    window: window(value.window),
                                }
                            })
                            .collect(),
                    ),
                },
            }));
        }
        Ok((resolved, clip_ids))
    }

    /// A sequence's or console's automation binding, its clip named in
    /// `clip_ids` or its node in `node_ids`.
    fn automation_target(
        &self,
        document: &DocumentId,
        clip_ids: &ClipIds,
        node_ids: &NodeIds,
        binding: &types::Binding,
    ) -> Result<AutomationTarget, LoadError> {
        Ok(match binding {
            types::Binding::ClipParam { clip, param } => {
                let (clip_id, span, effect) =
                    clip_ids.get(&clip.0.value).copied().ok_or_else(|| {
                        self.invalid(
                            document,
                            clip.0.span,
                            format!("no clip `{}`", clip.as_str()),
                        )
                    })?;
                self.link_local(document, clip.0.span, span);
                if let Some(effect) = effect {
                    self.link_param(document, effect, param);
                }
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
        document: &DocumentId,
        console: &types::Console,
    ) -> Result<Console, LoadError> {
        let (mark_collections, marks) = Self::mark_collections(&console.marks);
        let (layers, layer_ids) = Self::layers(&console.layers);
        let (composition_graph, node_ids) =
            self.graph(document, &layer_ids, &marks, &console.graph)?;
        let (clips, clip_ids) =
            self.clips(document, &layer_ids, &marks, &node_ids, &console.clips)?;
        let clip = |clip: &Name| {
            let (id, span, _) = clip_ids.get(&clip.0.value).copied().ok_or_else(|| {
                self.invalid(
                    document,
                    clip.0.span,
                    format!("no clip `{}`", clip.as_str()),
                )
            })?;
            self.link_local(document, clip.0.span, span);
            Ok::<_, LoadError>(id)
        };
        let always = console
            .always
            .iter()
            .map(|always| {
                Ok::<_, LoadError>(Always {
                    clip: clip(&always.clip)?,
                    length: DonderDuration(always.length),
                })
            })
            .collect::<Result<_, _>>()?;
        let mut events = Vec::with_capacity(console.events.len());
        for (on, index) in console.events.iter().zip(1..) {
            let control = self.control_ref(document, &on.control)?;
            let actions = on
                .actions
                .iter()
                .map(|action| {
                    Ok(match action {
                        types::Action::Play {
                            clip: played,
                            copies,
                            length,
                        } => Action::Play(Play {
                            clip: clip(played)?,
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
                                            LengthSource::Axis(self.control_ref(document, control)?)
                                        }
                                    },
                                    shortest: DonderDuration(*shortest),
                                    longest: DonderDuration(*longest),
                                },
                            },
                        }),
                        types::Action::Start {
                            clip: started,
                            length,
                        } => Action::Start {
                            clip: clip(started)?,
                            length: DonderDuration(*length),
                        },
                        types::Action::Stop { clip: stopped } => Action::Stop {
                            clip: clip(stopped)?,
                        },
                        types::Action::Toggle {
                            clip: toggled,
                            length,
                        } => Action::Toggle {
                            clip: clip(toggled)?,
                            length: DonderDuration(*length),
                        },
                        types::Action::Mark { into } => {
                            let Some(index) =
                                marks.iter().position(|(mark, _)| *mark == into.0.value)
                            else {
                                return Err(self.invalid(
                                    document,
                                    into.0.span,
                                    format!(
                                        "the console has no mark collection `{}`",
                                        into.0.value.as_str()
                                    ),
                                ));
                            };
                            self.link_local(document, into.0.span, marks[index].1);
                            Action::Mark(MarkCollectionId(index as u32))
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
                happening: on.happening.0,
                control,
                actions,
            });
        }
        Ok(Console {
            description: console.description.clone(),
            frame_rate: console.frame_rate,
            composition: Composition {
                marks: mark_collections,
                layers,
                graph: composition_graph,
                clips,
            },
            always,
            events,
        })
    }

    /// Link `param` to its declaration in `definition`, when it declares it.
    fn link_param(&self, document: &DocumentId, definition: ScriptId, param: &Name) {
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

    fn definition_params(&self, definition: &ScriptId) -> Vec<ParamDecl> {
        self.project
            .scripts
            .get(definition)
            .map(|definition| definition.params().to_vec())
            .unwrap_or_else(|| unreachable!("a resolved effect definition is stored"))
    }

    /// The asset at `path`, which `document` references as `kind`.
    fn asset(
        &self,
        document: &DocumentId,
        span: TextSpan,
        kind: &str,
        path: &str,
    ) -> Result<AssetId, LoadError> {
        let root = &self.loader.workspace.root;
        let mut assets = self.loader.referenced_assets.borrow_mut();
        crate::source::reference_asset(&mut assets, root, document, path)
            .map_err(|error| self.invalid(document, span, format!("{kind} asset {path}: {error}")))
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
                Some((definition, &param.value)),
                &declarations[index].ty,
                &declarations[index].option_fields,
                marks,
                value,
            )?;
            values.insert(param.value.clone(), value);
        }
        Ok(values)
    }

    /// A value of type `ty`, whose enum options carry `option_fields`. The
    /// options of a declared parameter, `param`, link to their declarations;
    /// a field's do not.
    fn param_value(
        &self,
        document: &DocumentId,
        param: Option<(ObjectId, &Identifier)>,
        ty: &Type,
        option_fields: &[Vec<ParamDecl>],
        marks: &[(Identifier, TextSpan)],
        value: &Spanned<DataValue>,
    ) -> Result<ParamValue, LoadError> {
        let fields_of = |option: &Identifier| {
            let Type::Enum(options) = ty else {
                return None;
            };
            let index = options.iter().position(|candidate| candidate == option)?;
            Some(option_fields.get(index).map_or(&[][..], Vec::as_slice))
        };
        let link_option = |option: &Spanned<Identifier>| {
            if let Some((definition, param)) = param {
                self.loader.link(
                    document,
                    option.span,
                    self.script(
                        definition,
                        ScriptMember::Option {
                            param: param.as_str().to_string(),
                            option: option.value.as_str().to_string(),
                        },
                    ),
                );
            }
        };
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
            (Type::Enum(_), DataValue::Variant(option))
                if fields_of(&option.value).is_some_and(<[_]>::is_empty) =>
            {
                link_option(option);
                ParamValue::Enum {
                    option: option.value.clone(),
                    fields: IndexMap::new(),
                }
            }
            (Type::Enum(_), DataValue::Record(option, fields))
                if let Some(declared) = fields_of(&option.value)
                    && !declared.is_empty() =>
            {
                link_option(option);
                let names = declared
                    .iter()
                    .map(|field| format!("`{}`", field.name.as_str()))
                    .collect::<Vec<_>>()
                    .join(", ");
                if fields.value.len() != declared.len()
                    || fields
                        .value
                        .iter()
                        .zip(declared)
                        .any(|(field, declared)| field.name.value != declared.name)
                {
                    return Err(self.invalid(
                        document,
                        fields.span,
                        format!(
                            "`{}` has the fields {names}, each once, in that order",
                            option.value.as_str()
                        ),
                    ));
                }
                ParamValue::Enum {
                    option: option.value.clone(),
                    fields: fields
                        .value
                        .iter()
                        .zip(declared)
                        .map(|(field, declared)| {
                            self.param_value(
                                document,
                                None,
                                &declared.ty,
                                &declared.option_fields,
                                marks,
                                &field.value,
                            )
                            .map(|value| (declared.name.clone(), value))
                        })
                        .collect::<Result<_, _>>()?,
                }
            }
            (Type::Marks, DataValue::Reference(segments))
                if let [segment] = segments.as_slice()
                    && let Some(index) =
                        marks.iter().position(|(mark, _)| mark == &segment.value) =>
            {
                self.loader.link(
                    document,
                    segment.span,
                    LinkTarget::Data {
                        document: document.clone(),
                        span: marks[index].1,
                    },
                );
                ParamValue::Marks(Some(MarkCollectionId(index as u32)))
            }
            (Type::Marks, DataValue::None) => ParamValue::Marks(None),
            (Type::Curve, DataValue::Reference(segments)) => {
                let curve = self
                    .loader
                    .resolve_reference(document, &reference(segments))?;
                ParamValue::Curve(CurveValue::Linked(curve))
            }
            (Type::Curve, DataValue::Record(..)) => ParamValue::Curve(CurveValue::Literal(curve(
                &self.decode::<types::CurveValue>(document, value)?.points,
            ))),
            (Type::Gradient, DataValue::Reference(segments)) => {
                let gradient = self
                    .loader
                    .resolve_reference(document, &reference(segments))?;
                ParamValue::Gradient(GradientValue::Linked(gradient))
            }
            (Type::Gradient, DataValue::Record(..)) => {
                let literal = self.decode::<types::GradientValue>(document, value)?;
                ParamValue::Gradient(GradientValue::Literal(gradient(
                    &literal.space,
                    &literal.stops,
                )))
            }
            (Type::Array(item), DataValue::List(items)) => ParamValue::Array(
                items
                    .iter()
                    .map(|value| self.param_value(document, param, item, &[], marks, value))
                    .collect::<Result<_, _>>()?,
            ),
            (Type::Text, DataValue::String(text)) => ParamValue::Text(text.clone()),
            (Type::Font, DataValue::Record(..)) => {
                let font = self.decode::<types::FontValue>(document, value)?;
                ParamValue::Font(Some(donder_model::FontFace {
                    file: self.asset(document, value.span, "font", &font.file.0)?,
                    height: font.height,
                }))
            }
            (Type::Font, DataValue::None) => ParamValue::Font(None),
            (Type::Image, DataValue::Record(..)) => {
                let image = self.decode::<types::ImageValue>(document, value)?;
                ParamValue::Image(Some(donder_model::ImageSource {
                    file: self.asset(document, value.span, "image", &image.file.0)?,
                    size: image.size,
                }))
            }
            (Type::Image, DataValue::None) => ParamValue::Image(None),
            _ => {
                return Err(self.invalid(
                    document,
                    value.span,
                    format!("expected {}", describe(ty, option_fields, marks)),
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
fn describe(
    ty: &Type,
    option_fields: &[Vec<ParamDecl>],
    marks: &[(Identifier, TextSpan)],
) -> String {
    match ty {
        Type::Int => "an int like `3`".into(),
        Type::Float => "a float like `1.0`".into(),
        Type::Bool => "`true` or `false`".into(),
        Type::Color => "a color like `#ff8800`".into(),
        Type::Enum(options) => format!(
            "one of {}",
            options
                .iter()
                .enumerate()
                .map(|(index, option)| match option_fields.get(index) {
                    Some(fields) if !fields.is_empty() => format!(
                        "`{} {{ {} }}`",
                        option.as_str(),
                        fields
                            .iter()
                            .map(|field| format!("{}: ...", field.name.as_str()))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => format!("`{}`", option.as_str()),
                })
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
        Type::Track => "no value: audio samples are bound by preparation".into(),
        Type::Text => "a text like `\"Hello\"`".into(),
        Type::Font => "`none` or a font like `Font { file: <fonts/sans.ttf>, height: 8 }`".into(),
        Type::Image => {
            "`none` or an image like `Image { file: <images/tree.png>, size: 32 }`".into()
        }
        Type::Curve => {
            "a curve: a reference or `Curve { points: [(position, value), ...] }`".into()
        }
        Type::Gradient => "a gradient: a reference or \
            `Gradient { space: LinearRgb, stops: [(position, #color), ...] }`"
            .into(),
        Type::Array(item) => format!("a list of {}", describe(item, &[], marks)),
        Type::Void => "no value".into(),
    }
}

/// A controller's ports, numbered from 1 in authored order.
fn controller_ports<'a>(
    ports: impl Iterator<Item = (&'a types::PortName, &'a Option<String>)>,
) -> Vec<ControllerPort> {
    ports
        .zip(1..)
        .map(|((name, description), index)| ControllerPort {
            id: ControllerPortId(index),
            name: name.0.value.clone(),
            description: description.clone(),
        })
        .collect()
}
