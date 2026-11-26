//! Typed project state to document types. Every typed object has exactly one
//! encoding, and references are written relative to the document holding them.
use super::types;
use crate::ExportProjectError;
use crate::address::{DocumentId, SourceIdentity};
use crate::source::ProjectSession;
use donder_language::Point3;
use donder_language::compiler::ParamDecl;
use donder_language::data::{DataValue, Spanned};
use donder_language::data::{Meters, Name, Params, Path, Reference};
use donder_model::ObjectId;
use donder_model::ObjectKind;
use donder_model::{
    Action, ClipContent, ClipId, Composition, Console, LengthSource, Placement, PlayLength,
};
use donder_model::{AutomationTarget, CompositionGraphNodeKind, Sequence, SequenceAudio, Window};
use donder_model::{
    AxisInput, ButtonInput, CompositionGraph, ControlRef, InputControlKind, InputDevice,
    InputDriver, Layer,
};
use donder_model::{Controller, ControllerProtocol, E131Mode, GroupMember, RouteStart};
use donder_model::{CurveValue, GradientValue, ParamValue};
use donder_model::{FixtureDefinition, FixtureShape, FixtureTransform};
use donder_model::{FixtureId, Layout, LayoutFixture, LayoutFixtureKind};
use donder_runtime_types::Identifier;
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;
use std::num::NonZeroU32;

/// The names of a sequence's or console's mark collections, by id.
type MarkNames<'a> = IndexMap<donder_model::MarkCollectionId, &'a Identifier>;

fn mark_names(collections: &[donder_model::MarkCollection]) -> MarkNames<'_> {
    collections
        .iter()
        .map(|collection| (collection.id, &collection.name))
        .collect()
}

fn sequence_marks(sequence: &Sequence) -> MarkNames<'_> {
    mark_names(&sequence.composition.marks)
}

/// The mark collections of a sequence or console.
fn mark_collections(collections: &[donder_model::MarkCollection]) -> Vec<types::MarkCollection> {
    collections
        .iter()
        .map(|collection| types::MarkCollection {
            name: name(&collection.name),
            description: collection.description.clone(),
            color: collection.display_color,
            times: collection
                .marks
                .iter()
                .map(|mark| types::MarkTime {
                    time: mark.time.0,
                    duration: mark.duration.0,
                    label: mark.label.clone(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) struct Encoder<'a> {
    pub(crate) session: &'a ProjectSession,
    /// Every object's address, from [`ProjectSession::addresses`].
    pub(crate) addresses: &'a std::collections::HashMap<ObjectId, SourceIdentity>,
    /// The document being written; references are relative to it.
    pub(crate) document: &'a DocumentId,
}

fn invalid(document: &DocumentId, reference: &str, message: &str) -> ExportProjectError {
    ExportProjectError::InvalidReference {
        path: document.path().to_path_buf(),
        reference: reference.to_string(),
        message: message.to_string(),
    }
}

fn name(identifier: &Identifier) -> Name {
    Name::new(identifier.clone())
}

fn spanned(value: DataValue) -> Spanned<DataValue> {
    donder_language::data::spanned(value)
}

fn meters(micrometers: i32) -> Meters {
    Meters(i64::from(micrometers))
}

fn window_document(window: Window) -> (f32, f32) {
    (window.low, window.high)
}

fn point(point: &Point3) -> (Meters, Meters, Meters) {
    (
        meters(point.x.micrometers),
        meters(point.y.micrometers),
        meters(point.z.micrometers),
    )
}

impl Encoder<'_> {
    /// An asset's path, as its document wrote it.
    fn asset_path(&self, id: &donder_model::AssetId) -> Result<Path, ExportProjectError> {
        Ok(Path(
            self.session
                .source
                .referenced_assets
                .iter()
                .find(|asset| &asset.id == id)
                .ok_or_else(|| self.error("", "an asset is missing"))?
                .relative_path
                .to_string(),
        ))
    }

    fn error(&self, reference: &str, message: &str) -> ExportProjectError {
        invalid(self.document, reference, message)
    }

    /// `name` or `alias.name` for a declared object.
    fn source_reference(
        &self,
        kind: ObjectKind,
        identity: &SourceIdentity,
    ) -> Result<Reference, ExportProjectError> {
        let text =
            crate::imports::write_source_reference(self.session, self.document, kind, identity)?;
        Ok(Reference::new(
            text.split('.')
                .map(|segment| {
                    Identifier::new(segment.to_string())
                        .map_err(|_| self.error(&text, "invalid reference segment"))
                })
                .collect::<Result<_, _>>()?,
        ))
    }

    /// `name` or `alias.name` for an object of the project.
    fn object_reference(
        &self,
        kind: ObjectKind,
        id: impl Into<ObjectId>,
    ) -> Result<Reference, ExportProjectError> {
        let id = id.into();
        let identity = self
            .addresses
            .get(&id)
            .unwrap_or_else(|| unreachable!("every object of a session has an address"));
        self.source_reference(kind, identity)
    }

    /// A fixture or group of the project's layout, by its name alone.
    fn fixture_target(&self, target: FixtureId) -> Reference {
        let Some(fixture) = self.session.project.layout().object().fixture(target) else {
            unreachable!("validation proved every fixture target");
        };
        Reference {
            segments: vec![Spanned::new(
                fixture.name.clone(),
                donder_language::data::NO_SPAN,
            )],
            span: donder_language::data::NO_SPAN,
        }
    }

    pub(crate) fn project(&self) -> Result<types::Project, ExportProjectError> {
        let root = self.session.project.root();
        Ok(types::Project {
            format: crate::PROJECT_FORMAT_VERSION,
            description: root.description.clone(),
            layout: self.object_reference(ObjectKind::Layout, root.layout)?,
            routes: self.routes(&root.routes)?,
        })
    }

    pub(crate) fn layout(&self, layout: &Layout) -> Result<types::Layout, ExportProjectError> {
        let item_name = |member: FixtureId| {
            layout
                .fixture(member)
                .map(|fixture| name(&fixture.name))
                .ok_or_else(|| self.error(&member.0.to_string(), "the layout member is missing"))
        };
        let member_names = |members: &[FixtureId]| {
            members
                .iter()
                .map(|&member| item_name(member))
                .collect::<Result<Vec<_>, _>>()
        };
        let group_members = |members: &[GroupMember]| {
            members
                .iter()
                .map(|member| {
                    Ok(match *member {
                        GroupMember::Item(item) => types::GroupMember::Item(item_name(item)?),
                        GroupMember::Slice { fixture, pixels } => {
                            types::GroupMember::Slice(types::Slice {
                                fixture: item_name(fixture)?,
                                start: pixels.start,
                                count: pixels.count,
                            })
                        }
                    })
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()
        };
        Ok(types::Layout {
            description: layout.description.clone(),
            root: member_names(&layout.root)?,
            items: layout
                .fixtures
                .iter()
                .map(|fixture| self.layout_item(fixture, &group_members))
                .collect::<Result<_, _>>()?,
        })
    }

    fn layout_item(
        &self,
        fixture: &LayoutFixture,
        group_members: &impl Fn(&[GroupMember]) -> Result<Vec<types::GroupMember>, ExportProjectError>,
    ) -> Result<types::LayoutItem, ExportProjectError> {
        Ok(match &fixture.kind {
            LayoutFixtureKind::Group { members } => types::LayoutItem::Group {
                name: name(&fixture.name),
                description: fixture.description.clone(),
                members: group_members(members)?,
            },
            LayoutFixtureKind::Fixture {
                definition,
                transform,
            } => types::LayoutItem::Fixture {
                name: name(&fixture.name),
                description: fixture.description.clone(),
                definition: self.object_reference(ObjectKind::FixtureDefinition, *definition)?,
                transform: transform_document(transform),
            },
        })
    }

    pub(crate) fn routes(
        &self,
        routes: &[donder_model::PixelRoute],
    ) -> Result<Vec<types::Route>, ExportProjectError> {
        routes
            .iter()
            .map(|route| {
                let Some(port) = self
                    .session
                    .project
                    .get::<donder_model::Controller>(route.controller)
                    .and_then(|controller| controller.port(route.port))
                else {
                    unreachable!("validation proved every route's controller and port");
                };
                Ok(types::Route {
                    target: self.fixture_target(route.target),
                    pixels: route.pixels.map(|span| types::PixelSpan {
                        start: span.start,
                        count: span.count,
                    }),
                    reverse: route.reverse,
                    controller: self.object_reference(ObjectKind::Controller, route.controller)?,
                    port: types::PortName::new(port.name.clone()),
                    start: match route.start {
                        RouteStart::Next => types::RouteStart::Next,
                        RouteStart::Channel(channel) => types::RouteStart::Channel { channel },
                    },
                    encoding: match route.encoding {
                        PixelEncoding::Rgb { order } => types::Encoding::Rgb {
                            order: (order[0], order[1], order[2]),
                        },
                        PixelEncoding::Rgbw { order } => types::Encoding::Rgbw {
                            order: (order[0], order[1], order[2], order[3]),
                        },
                        PixelEncoding::Channels { colors } => types::Encoding::Channels {
                            colors: colors
                                .as_slice()
                                .iter()
                                .map(|&[red, green, blue]| donder_runtime_types::Color {
                                    red: f32::from(red) / 255.0,
                                    green: f32::from(green) / 255.0,
                                    blue: f32::from(blue) / 255.0,
                                })
                                .collect(),
                        },
                    },
                    gamma: route.gamma,
                    brightness: route.brightness,
                    dither: route.dither,
                })
            })
            .collect()
    }

    fn layer_name(layers: &[Layer], id: &donder_model::LayerId) -> Name {
        let Some(layer) = layers.iter().find(|layer| &layer.id == id) else {
            unreachable!("validation proved every layer reference");
        };
        name(&layer.name)
    }

    /// An operator node's name, or `root`: what edges reach and automation
    /// binds.
    fn node_name(graph: &CompositionGraph, id: &donder_model::CompositionGraphNodeId) -> Name {
        match graph
            .nodes
            .iter()
            .find(|node| &node.id == id)
            .map(|node| &node.kind)
        {
            Some(CompositionGraphNodeKind::Operator(operator)) => name(&operator.name),
            Some(CompositionGraphNodeKind::Root) => name(&donder_language::object_name(
                donder_language::data::ROOT_NODE,
            )),
            Some(CompositionGraphNodeKind::Layer { .. }) | None => {
                unreachable!("validation proved edges and bindings reach operators and the root")
            }
        }
    }

    /// Where an edge leaves: `layers.layer` for a layer node, else its node name.
    fn edge_source(
        layers: &[Layer],
        graph: &CompositionGraph,
        id: &donder_model::CompositionGraphNodeId,
    ) -> Reference {
        let layer = graph.nodes.iter().find_map(|node| match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } if &node.id == id => Some(layer_id),
            _ => None,
        });
        Reference::new(match layer {
            Some(layer) => vec![
                donder_language::object_name(donder_language::data::LAYERS_SCOPE),
                Self::layer_name(layers, layer).0.value,
            ],
            None => vec![Self::node_name(graph, id).0.value],
        })
    }

    /// The layers of a sequence or console.
    fn layers(layers: &[Layer]) -> Vec<types::Layer> {
        layers
            .iter()
            .map(|layer| types::Layer {
                name: name(&layer.name),
                description: layer.description.clone(),
                color: layer.color,
                enabled: layer.enabled,
            })
            .collect()
    }

    /// The composition graph of a sequence or console.
    fn graph(
        &self,
        layers: &[Layer],
        marks: &MarkNames<'_>,
        graph: &CompositionGraph,
    ) -> Result<types::Graph, ExportProjectError> {
        Ok(types::Graph {
            nodes: graph
                .nodes
                .iter()
                .map(|node| {
                    let position = (node.position.x, node.position.y);
                    Ok(match &node.kind {
                        CompositionGraphNodeKind::Layer { layer_id } => types::Node::LayerNode {
                            layer: Self::layer_name(layers, layer_id),
                            position,
                        },
                        CompositionGraphNodeKind::Operator(operator) => {
                            let definition = &operator.operator;
                            let declarations = self
                                .session
                                .project
                                .reusable::<donder_model::ScriptDefinition>()[definition]
                                .params();
                            types::Node::OperatorNode {
                                name: name(&operator.name),
                                operator: self.object_reference(ObjectKind::Script, *definition)?,
                                params: self.params(declarations, marks, &operator.params)?,
                                position,
                            }
                        }
                        CompositionGraphNodeKind::Root => types::Node::RootNode { position },
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            edges: graph
                .edges
                .iter()
                .map(|edge| {
                    let mut segments = vec![Self::node_name(graph, &edge.to).0.value];
                    segments.extend(edge.input.clone());
                    types::Edge {
                        from: Self::edge_source(layers, graph, &edge.from),
                        to: Reference::new(segments),
                    }
                })
                .collect(),
        })
    }

    /// An effect's parameter declarations.
    fn effect_declarations(&self, definition: &donder_model::ScriptId) -> &[ParamDecl] {
        self.session
            .project
            .reusable::<donder_model::ScriptDefinition>()[definition]
            .params()
    }

    /// `device.control`: a control's device reference and the control's name.
    fn control(&self, input: &ControlRef) -> Result<Reference, ExportProjectError> {
        let mut reference = self.object_reference(ObjectKind::InputDevice, input.device)?;
        let Some(control) = self
            .session
            .project
            .get::<donder_model::InputDevice>(input.device)
            .and_then(|device| device.control(input.control))
        else {
            unreachable!("validation proved every console input");
        };
        reference.segments.push(Spanned::new(
            control.name.clone(),
            donder_language::data::NO_SPAN,
        ));
        Ok(reference)
    }

    pub(crate) fn console(&self, console: &Console) -> Result<types::Console, ExportProjectError> {
        let marks = mark_names(&console.composition.marks);
        let clip_name = |id: &ClipId| {
            let Some(clip) = console.composition.clip(*id) else {
                unreachable!("validation proved every console clip reference");
            };
            name(&clip.name)
        };
        Ok(types::Console {
            description: console.description.clone(),
            frame_rate: console.frame_rate,
            marks: mark_collections(&console.composition.marks),
            layers: Self::layers(&console.composition.layers),
            clips: console
                .composition
                .clips
                .iter()
                .map(|clip| self.clip(&console.composition, clip))
                .collect::<Result<_, ExportProjectError>>()?,
            always: console
                .always
                .iter()
                .map(|always| {
                    Ok(types::Always {
                        clip: clip_name(&always.clip),
                        length: always.length.0,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            events: console
                .events
                .iter()
                .map(|on| {
                    Ok(types::On {
                        happening: types::Happening(on.happening),
                        control: self.control(&on.control)?,
                        actions: on
                            .actions
                            .iter()
                            .map(|action| {
                                Ok(match action {
                                    Action::Play(play) => types::Action::Play {
                                        clip: clip_name(&play.clip),
                                        copies: play.copies,
                                        length: match &play.length {
                                            PlayLength::Fixed(duration) => {
                                                types::PlayLength::Fixed {
                                                    duration: duration.0,
                                                }
                                            }
                                            PlayLength::Scaled {
                                                by,
                                                shortest,
                                                longest,
                                            } => types::PlayLength::Scaled {
                                                by: match by {
                                                    LengthSource::Velocity => {
                                                        types::LengthSource::Velocity
                                                    }
                                                    LengthSource::Axis(control) => {
                                                        types::LengthSource::Axis {
                                                            control: self.control(control)?,
                                                        }
                                                    }
                                                },
                                                shortest: shortest.0,
                                                longest: longest.0,
                                            },
                                        },
                                    },
                                    Action::Start { clip, length } => types::Action::Start {
                                        clip: clip_name(clip),
                                        length: length.0,
                                    },
                                    Action::Stop { clip } => types::Action::Stop {
                                        clip: clip_name(clip),
                                    },
                                    Action::Toggle { clip, length } => types::Action::Toggle {
                                        clip: clip_name(clip),
                                        length: length.0,
                                    },
                                    Action::Mark(id) => types::Action::Mark {
                                        into: name(marks.get(id).unwrap_or_else(|| {
                                            unreachable!(
                                                "validation proved every live mark collection"
                                            )
                                        })),
                                    },
                                    Action::Set { target, window } => types::Action::Set {
                                        param: Self::binding(
                                            &console.composition.graph,
                                            target,
                                            |clip| {
                                                console
                                                    .composition
                                                    .clip(clip)
                                                    .map(|clip| name(&clip.name))
                                            },
                                        ),
                                        window: window_document(*window),
                                    },
                                })
                            })
                            .collect::<Result<_, ExportProjectError>>()?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            graph: self.graph(
                &console.composition.layers,
                &marks,
                &console.composition.graph,
            )?,
        })
    }

    /// One clip of a sequence or console. An effect clip's text depends only on
    /// the clip, its composition's layers and mark collections, the layouts and
    /// definitions it names, and the imports of the document being written; an
    /// envelope's also on the names of what it binds.
    pub(crate) fn clip(
        &self,
        composition: &Composition,
        clip: &donder_model::Clip,
    ) -> Result<types::Clip, ExportProjectError> {
        Ok(types::Clip {
            name: name(&clip.name),
            description: clip.description.clone(),
            targets: clip
                .targets
                .iter()
                .map(|&target| self.fixture_target(target))
                .collect(),
            placement: match &clip.placement {
                Placement::Timed { start, duration } => types::Placement::Timed {
                    start: start.0,
                    duration: duration.0,
                },
                Placement::Played(values) => types::Placement::Played {
                    values: values
                        .iter()
                        .map(|value| types::PlayedValue {
                            param: name(&value.param),
                            from: types::PlayedQuantity(value.from),
                            window: window_document(value.window),
                        })
                        .collect(),
                },
            },
            content: match &clip.content {
                ClipContent::Effect(effect) => types::ClipContent::Effect {
                    layer: Self::layer_name(&composition.layers, &effect.layer),
                    scope: types::Scope(effect.scope),
                    effect: self.object_reference(ObjectKind::Script, effect.definition)?,
                    params: self.params(
                        self.effect_declarations(&effect.definition),
                        &mark_names(&composition.marks),
                        &effect.params,
                    )?,
                },
                ClipContent::Envelope(envelope) => types::ClipContent::Envelope {
                    curve: curve_value(&envelope.curve),
                    bindings: envelope
                        .bindings
                        .iter()
                        .map(|bound| types::Bound {
                            binding: Self::binding(&composition.graph, &bound.target, |clip| {
                                composition.clip(clip).map(|clip| name(&clip.name))
                            }),
                            window: window_document(bound.window),
                        })
                        .collect(),
                },
            },
        })
    }

    /// `sequence` without its clips, which are encoded one by one with
    /// [`Self::clip`] and printed into the `clips` field separately.
    pub(crate) fn sequence(
        &self,
        sequence: &Sequence,
    ) -> Result<types::Sequence, ExportProjectError> {
        Ok(types::Sequence {
            description: sequence.description.clone(),
            duration: sequence.duration.0,
            frame_rate: sequence.frame_rate,
            audio: match &sequence.audio {
                SequenceAudio::None => None,
                SequenceAudio::Asset(id) => Some(self.asset_path(id)?),
            },
            marks: mark_collections(&sequence.composition.marks),
            layers: Self::layers(&sequence.composition.layers),
            clips: Vec::new(),
            graph: self.graph(
                &sequence.composition.layers,
                &sequence_marks(sequence),
                &sequence.composition.graph,
            )?,
        })
    }

    /// A sequence's or console's automation binding; `clip_name` names the
    /// clip a clip binding sets.
    fn binding(
        graph: &donder_model::CompositionGraph,
        target: &AutomationTarget,
        clip_name: impl FnOnce(ClipId) -> Option<Name>,
    ) -> types::Binding {
        match target {
            AutomationTarget::ClipParam { clip_id, param } => types::Binding::ClipParam {
                clip: clip_name(*clip_id)
                    .unwrap_or_else(|| unreachable!("validation proved every automated clip")),
                param: name(param),
            },
            AutomationTarget::CompositionNodeParam { node_id, param } => {
                types::Binding::NodeParam {
                    node: Self::node_name(graph, node_id),
                    param: name(param),
                }
            }
        }
    }

    /// Parameter overrides in the definition's declaration order.
    fn params(
        &self,
        declarations: &[ParamDecl],
        marks: &MarkNames<'_>,
        values: &IndexMap<Identifier, ParamValue>,
    ) -> Result<Params, ExportProjectError> {
        Ok(Params(
            declarations
                .iter()
                .filter_map(|declaration| {
                    values
                        .get(&declaration.name)
                        .map(|value| (declaration, value))
                })
                .map(|(declaration, value)| {
                    Ok((
                        Spanned::new(declaration.name.clone(), donder_language::data::NO_SPAN),
                        spanned(self.param_value(marks, value)?),
                    ))
                })
                .collect::<Result<_, ExportProjectError>>()?,
        ))
    }

    fn param_value(
        &self,
        marks: &MarkNames<'_>,
        value: &ParamValue,
    ) -> Result<DataValue, ExportProjectError> {
        use donder_language::data::Data;
        Ok(match value {
            ParamValue::Int(value) => DataValue::Integer(i64::from(*value)),
            ParamValue::Float(value) => DataValue::Float(*value),
            ParamValue::Bool(value) => DataValue::Bool(*value),
            ParamValue::Color(value) => DataValue::Color(*value),
            ParamValue::Enum { option, fields } if fields.is_empty() => {
                donder_language::data::variant(option.as_str())
            }
            ParamValue::Enum { option, fields } => donder_language::data::record(
                option.as_str(),
                fields
                    .iter()
                    .map(|(name, value)| {
                        Ok(donder_language::data::field(
                            name.as_str(),
                            self.param_value(marks, value)?,
                        ))
                    })
                    .collect::<Result<_, ExportProjectError>>()?,
            ),
            ParamValue::Marks(Some(id)) => Reference::new(vec![
                (*marks
                    .get(id)
                    .unwrap_or_else(|| unreachable!("validation proved every mark collection")))
                .clone(),
            ])
            .encode(),
            ParamValue::Marks(None) => DataValue::None,
            ParamValue::Curve(CurveValue::Linked(id)) => {
                self.object_reference(ObjectKind::Curve, *id)?.encode()
            }
            ParamValue::Curve(CurveValue::Literal(curve)) => curve_value(curve).encode(),
            ParamValue::Gradient(GradientValue::Linked(id)) => {
                self.object_reference(ObjectKind::Gradient, *id)?.encode()
            }
            ParamValue::Gradient(GradientValue::Literal(gradient)) => types::GradientValue {
                space: types::GradientSpace(gradient.space),
                stops: gradient_stops(gradient),
            }
            .encode(),
            ParamValue::Array(items) => DataValue::List(
                items
                    .iter()
                    .map(|item| self.param_value(marks, item).map(spanned))
                    .collect::<Result<_, _>>()?,
            ),
            ParamValue::Text(text) => DataValue::String(text.clone()),
            ParamValue::Font(Some(face)) => types::FontValue {
                file: self.asset_path(&face.file)?,
                height: face.height,
            }
            .encode(),
            ParamValue::Font(None) => DataValue::None,
            ParamValue::Image(Some(image)) => types::ImageValue {
                file: self.asset_path(&image.file)?,
                size: image.size,
            }
            .encode(),
            ParamValue::Image(None) => DataValue::None,
        })
    }
}

pub(crate) fn input_device_document(device: &InputDevice) -> types::InputDevice {
    types::InputDevice {
        description: device.description.clone(),
        driver: match &device.driver {
            InputDriver::Keyboard => types::InputDriver::Keyboard,
            InputDriver::Midi { port } => types::InputDriver::Midi { port: port.clone() },
            InputDriver::Osc { port } => types::InputDriver::Osc { port: *port },
        },
        controls: device
            .controls
            .iter()
            .map(|control| {
                let name = name(&control.name);
                let description = control.description.clone();
                match &control.kind {
                    InputControlKind::Button(input) => types::InputControl::Button {
                        name,
                        description,
                        input: match input {
                            ButtonInput::Key(code) => types::ButtonInput::Key {
                                code: types::KeyCode(*code),
                            },
                            ButtonInput::Notes { notes, channel } => types::ButtonInput::Notes {
                                low: notes.low,
                                high: notes.high,
                                channel: channel.map(|channel| channel.0),
                            },
                            ButtonInput::ControlChange { number, channel } => {
                                types::ButtonInput::ControlChange {
                                    number: *number,
                                    channel: channel.map(|channel| channel.0),
                                }
                            }
                            ButtonInput::Address(path) => types::ButtonInput::Address {
                                path: path.0.clone(),
                            },
                        },
                    },
                    InputControlKind::Axis(input) => types::InputControl::Axis {
                        name,
                        description,
                        input: match input {
                            AxisInput::ControlChange { number, channel } => {
                                types::AxisInput::ControlChange {
                                    number: *number,
                                    channel: channel.map(|channel| channel.0),
                                }
                            }
                            AxisInput::PitchBend { channel } => types::AxisInput::PitchBend {
                                channel: channel.map(|channel| channel.0),
                            },
                            AxisInput::Address { path, low, high } => types::AxisInput::Address {
                                path: path.0.clone(),
                                low: *low,
                                high: *high,
                            },
                        },
                    },
                }
            })
            .collect(),
    }
}

pub(crate) fn controller_document(controller: &Controller) -> types::Controller {
    types::Controller {
        description: controller.description.clone(),
        protocol: match &controller.protocol {
            ControllerProtocol::E131(config) => types::Protocol::E131 {
                sender_name: config.sender_name.clone(),
                bind_address: types::Spelled(config.bind_address),
                priority: config.priority,
                mode: match config.mode {
                    E131Mode::Multicast => types::E131Mode::Multicast,
                    E131Mode::Unicast { destination } => types::E131Mode::Unicast {
                        destination: types::Spelled(destination),
                    },
                },
                start_universe: config.universes.start,
                universes: config.universes.count,
                channels: config.universes.channels,
                fseq_channel: config.fseq_channel.map(NonZeroU32::get),
                ports: port_documents(&config.ports),
            },
            ControllerProtocol::ArtNet(config) => types::Protocol::ArtNet {
                bind_address: types::Spelled(config.bind_address),
                destination: types::Spelled(config.destination),
                mode: types::ArtNetMode(config.mode),
                start_universe: config.universes.start,
                universes: config.universes.count,
                channels: config.universes.channels,
                fseq_channel: config.fseq_channel.map(NonZeroU32::get),
                ports: port_documents(&config.ports),
            },
            ControllerProtocol::Donder(config) => types::Protocol::Donder {
                device: config.device.as_str().to_string(),
                ports: config
                    .ports
                    .iter()
                    .map(|port| types::DonderPort {
                        name: types::PortName::new(port.port.name.clone()),
                        description: port.port.description.clone(),
                        output: port.output,
                        slots: port.slot_count,
                    })
                    .collect(),
            },
        },
    }
}

fn port_documents(ports: &[donder_model::ControllerPort]) -> Vec<types::Port> {
    ports
        .iter()
        .map(|port| types::Port {
            name: types::PortName::new(port.name.clone()),
            description: port.description.clone(),
        })
        .collect()
}

pub(crate) fn fixture_definition_document(
    definition: &FixtureDefinition,
) -> types::FixtureDefinition {
    types::FixtureDefinition {
        description: definition.description.clone(),
        shapes: definition
            .elements
            .iter()
            .map(|element| types::Shape {
                name: name(&element.name),
                diameter: Meters(i64::from(element.diameter.micrometers)),
                reverse: element.reverse,
                transform: transform_document(&element.transform),
                geometry: match &element.shape {
                    FixtureShape::Pixel => types::Geometry::Pixel,
                    FixtureShape::Line { length, count } => types::Geometry::Line {
                        length: *length,
                        count: *count,
                    },
                    FixtureShape::Polyline { points, count } => types::Geometry::Polyline {
                        points: points.iter().map(point).collect(),
                        count: *count,
                    },
                    FixtureShape::Arc {
                        radius,
                        start_degrees,
                        sweep_degrees,
                        count,
                        closed,
                    } => types::Geometry::Arc {
                        radius: *radius,
                        start_degrees: *start_degrees,
                        sweep_degrees: *sweep_degrees,
                        count: *count,
                        closed: *closed,
                    },
                    FixtureShape::Grid {
                        columns,
                        rows,
                        width,
                        height,
                        axis,
                        corner,
                        serpentine,
                    } => types::Geometry::Grid {
                        columns: *columns,
                        rows: *rows,
                        width: *width,
                        height: *height,
                        axis: types::GridAxis(*axis),
                        corner: types::GridCorner(*corner),
                        serpentine: *serpentine,
                    },
                },
            })
            .collect(),
    }
}

fn transform_document(transform: &FixtureTransform) -> types::Transform {
    types::Transform {
        position: point(&transform.position),
        rotation: (
            transform.rotation.x,
            transform.rotation.y,
            transform.rotation.z,
        ),
        scale: (transform.scale.x, transform.scale.y, transform.scale.z),
    }
}

fn curve_points(curve: &Curve) -> Vec<(f32, f32)> {
    curve
        .points
        .iter()
        .map(|point| (point.position, point.value))
        .collect()
}

fn curve_value(curve: &Curve) -> types::CurveValue {
    types::CurveValue {
        points: curve_points(curve),
    }
}

fn gradient_stops(gradient: &Gradient) -> Vec<(f32, donder_runtime_types::Color)> {
    gradient
        .stops
        .iter()
        .map(|stop| (stop.position, stop.color))
        .collect()
}

pub(crate) fn curve_document(definition: &donder_model::CurveDefinition) -> types::Curve {
    types::Curve {
        description: definition.description.clone(),
        points: curve_points(&definition.curve),
    }
}

pub(crate) fn gradient_document(definition: &donder_model::GradientDefinition) -> types::Gradient {
    types::Gradient {
        description: definition.description.clone(),
        space: types::GradientSpace(definition.gradient.space),
        stops: gradient_stops(&definition.gradient),
    }
}
