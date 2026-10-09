//! Typed project state to document types. Every typed object has exactly one
//! encoding, and references are written relative to the document holding them.
use super::types;
use crate::ExportProjectError;
use crate::source::{ProjectSession, SourceObjectKind};
use donder_language::Point3;
use donder_language::compiler::ParamDecl;
use donder_language::data::{DataValue, Spanned};
use donder_language::data::{Meters, Name, NamedSource, Params, Path, Reference, Source};
use donder_model::Patch;
use donder_model::Setup;
use donder_model::ValueSource;
use donder_model::{ArtNetMode, Controller, ControllerPortAddress, ControllerProtocol, E131Mode};
use donder_model::{
    AutomationDetachmentReason, AutomationTarget, CompositionGraphNodeKind, Sequence, SequenceAudio,
};
use donder_model::{CurveSource, EffectParamValue, GradientSource};
use donder_model::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
use donder_model::{FixtureDefinition, FixtureShape, FixtureSource, FixtureTransform};
use donder_model::{FixtureInstanceId, FixtureTarget, Layout, LayoutFixture, LayoutFixtureKind};
use donder_runtime_types::Identifier;
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;

pub(crate) struct Encoder<'a> {
    pub(crate) session: &'a ProjectSession,
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

fn point(point: &Point3) -> (Meters, Meters, Meters) {
    (
        meters(point.x.micrometers),
        meters(point.y.micrometers),
        meters(point.z.micrometers),
    )
}

impl Encoder<'_> {
    fn error(&self, reference: &str, message: &str) -> ExportProjectError {
        invalid(self.document, reference, message)
    }

    /// `name` or `alias.name` for a declared object.
    pub(crate) fn source_reference(
        &self,
        kind: SourceObjectKind,
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

    /// A declared object, or one owned by a declared object: the owner's
    /// reference followed by the owning fields, `show.setup.controllers.main`.
    pub(crate) fn object_reference(
        &self,
        kind: SourceObjectKind,
        identity: &ObjectIdentity,
    ) -> Result<Reference, ExportProjectError> {
        if let Some(source) = identity.source() {
            return self.source_reference(kind, source);
        }
        let root = identity.root_source();
        let root_kind = self
            .session
            .source
            .documents
            .get(root.document_id())
            .and_then(|document| {
                document
                    .objects()
                    .iter()
                    .find(|object| object.id() == root.object())
            })
            .map(|object| object.kind().clone())
            .ok_or_else(|| self.error(root.object(), "the owner of this object is missing"))?;
        let mut reference = self.source_reference(root_kind.clone(), root)?;
        let mut resolved = root_kind;
        let segment = |text: &str| {
            Identifier::new(text.to_string())
                .map(|identifier| Spanned::new(identifier, donder_language::data::NO_SPAN))
                .map_err(|_| self.error(text, "invalid reference segment"))
        };
        for slot in identity.owned_path() {
            resolved = resolved
                .owned_child_kind(slot)
                .ok_or_else(|| self.error(root.object(), "invalid owned object address"))?;
            match slot {
                OwnedObjectSlot::Setup => reference.segments.push(segment("setup")?),
                OwnedObjectSlot::Layout => reference.segments.push(segment("layout")?),
                OwnedObjectSlot::Patch => reference.segments.push(segment("patch")?),
                OwnedObjectSlot::Controller(member) => {
                    reference.segments.push(segment("controllers")?);
                    reference.segments.push(segment(member.as_str())?);
                }
                OwnedObjectSlot::Sequence(member) => {
                    reference.segments.push(segment("sequences")?);
                    reference.segments.push(segment(member.as_str())?);
                }
                OwnedObjectSlot::Fixture(_) => {
                    return Err(self.error(
                        root.object(),
                        "owned fixture definitions are not referenced",
                    ));
                }
            }
        }
        if resolved != kind || !self.session.owned_object_exists(&kind, identity) {
            return Err(self.error(root.object(), "invalid owned object address"));
        }
        Ok(reference)
    }

    /// `layout.fixture`: the layout's reference and the fixture or group name.
    fn fixture_target(&self, target: &FixtureTarget) -> Result<Reference, ExportProjectError> {
        let mut reference = self.object_reference(SourceObjectKind::Layout, &target.layout.0)?;
        let fixture = self
            .session
            .project
            .layout(&target.layout)
            .and_then(|layout| layout.fixture(target.fixture))
            .ok_or_else(|| self.error(&reference.text(), "the target fixture is missing"))?;
        reference.segments.push(Spanned::new(
            fixture.name.clone(),
            donder_language::data::NO_SPAN,
        ));
        Ok(reference)
    }

    fn source<V, I, T>(
        &self,
        kind: SourceObjectKind,
        source: &ValueSource<V, I>,
        inline: impl FnOnce(&V) -> Result<T, ExportProjectError>,
    ) -> Result<Source<T>, ExportProjectError>
    where
        I: AsRef<ObjectIdentity>,
    {
        Ok(match source {
            ValueSource::Reference(id) => {
                Source::Reference(self.object_reference(kind, id.as_ref())?)
            }
            ValueSource::Inline(value) => Source::Inline(inline(value)?),
        })
    }

    /// An owned collection member is named by its ownership slot.
    fn named_source<V, I, T>(
        &self,
        kind: SourceObjectKind,
        source: &ValueSource<Box<V>, I>,
        identity: impl Fn(&V) -> &ObjectIdentity,
        inline: impl FnOnce(&V) -> Result<T, ExportProjectError>,
    ) -> Result<NamedSource<T>, ExportProjectError>
    where
        I: AsRef<ObjectIdentity>,
    {
        Ok(match source {
            ValueSource::Reference(id) => {
                NamedSource::Reference(self.object_reference(kind, id.as_ref())?)
            }
            ValueSource::Inline(value) => {
                let member = match identity(value).owned_path().last() {
                    Some(
                        OwnedObjectSlot::Controller(member) | OwnedObjectSlot::Sequence(member),
                    ) => member.clone(),
                    _ => return Err(self.error("", "an owned member has no name")),
                };
                NamedSource::Inline(name(&member), inline(value)?)
            }
        })
    }

    pub(crate) fn project(&self) -> Result<types::Project, ExportProjectError> {
        let root = self.session.project.root();
        let metadata = &self.session.source.workspace.metadata;
        Ok(types::Project {
            format: metadata.format_version,
            id: metadata.project_id.to_string(),
            description: root.description.clone(),
            setup: self.source(SourceObjectKind::Setup, &root.setup, |setup| {
                self.setup(setup)
            })?,
            sequences: root
                .sequences
                .iter()
                .map(|source| {
                    self.named_source(
                        SourceObjectKind::Sequence,
                        source,
                        |sequence| &sequence.id.0,
                        |sequence| self.sequence(sequence),
                    )
                })
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn setup(&self, setup: &Setup) -> Result<types::Setup, ExportProjectError> {
        Ok(types::Setup {
            description: setup.description.clone(),
            layout: self.source(SourceObjectKind::Layout, &setup.layout, |layout| {
                self.layout(layout)
            })?,
            patch: self.source(SourceObjectKind::Patch, &setup.patch, |patch| {
                self.patch(patch)
            })?,
            controllers: setup
                .controllers
                .iter()
                .map(|source| {
                    self.named_source(
                        SourceObjectKind::Controller,
                        source,
                        |controller| &controller.id.0,
                        |controller| Ok(controller_document(controller)),
                    )
                })
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn layout(&self, layout: &Layout) -> Result<types::Layout, ExportProjectError> {
        let member_names = |members: &[FixtureInstanceId]| {
            members
                .iter()
                .map(|&member| {
                    layout
                        .fixture(member)
                        .map(|fixture| name(&fixture.name))
                        .ok_or_else(|| {
                            self.error(&member.0.to_string(), "the layout member is missing")
                        })
                })
                .collect::<Result<Vec<_>, _>>()
        };
        Ok(types::Layout {
            description: layout.description.clone(),
            root: member_names(&layout.root)?,
            items: layout
                .fixtures
                .iter()
                .map(|fixture| self.layout_item(fixture, &member_names))
                .collect::<Result<_, _>>()?,
        })
    }

    fn layout_item(
        &self,
        fixture: &LayoutFixture,
        member_names: &impl Fn(&[FixtureInstanceId]) -> Result<Vec<Name>, ExportProjectError>,
    ) -> Result<types::LayoutItem, ExportProjectError> {
        Ok(match &fixture.kind {
            LayoutFixtureKind::Group { members } => types::LayoutItem::Group {
                name: name(&fixture.name),
                description: fixture.description.clone(),
                members: member_names(members)?,
            },
            LayoutFixtureKind::Fixture {
                definition,
                transform,
            } => types::LayoutItem::Fixture {
                name: name(&fixture.name),
                description: fixture.description.clone(),
                definition: match definition {
                    FixtureSource::Reference(id) => Source::Reference(
                        self.source_reference(SourceObjectKind::FixtureDefinition, &id.0)?,
                    ),
                    FixtureSource::Inline(value) => {
                        Source::Inline(fixture_definition_document(value))
                    }
                },
                transform: transform_document(transform),
            },
        })
    }

    pub(crate) fn patch(&self, patch: &Patch) -> Result<types::Patch, ExportProjectError> {
        Ok(types::Patch {
            description: patch.description.clone(),
            routes: patch
                .routes
                .iter()
                .map(|route| {
                    let controller = self
                        .session
                        .project
                        .controller(&route.controller)
                        .ok_or_else(|| self.error("", "a route's controller is missing"))?;
                    let port = controller
                        .ports
                        .iter()
                        .find(|port| port.id == route.port)
                        .ok_or_else(|| self.error("", "a route's port is missing"))?;
                    Ok(types::Route {
                        target: self.fixture_target(&route.target)?,
                        pixels: route.pixels.map(|span| types::PixelSpan {
                            start: span.start,
                            count: span.count,
                        }),
                        controller: self
                            .object_reference(SourceObjectKind::Controller, &route.controller.0)?,
                        port: name(&port.name),
                        start_slot: route.start_slot,
                        encoding: match route.encoding {
                            PixelEncoding::Rgb { order } => types::Encoding::Rgb {
                                order: (order[0], order[1], order[2]),
                            },
                            PixelEncoding::Rgbw { order } => types::Encoding::Rgbw {
                                order: (order[0], order[1], order[2], order[3]),
                            },
                        },
                        gamma: route.gamma,
                        brightness: route.brightness,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
        })
    }

    pub(crate) fn sequence(
        &self,
        sequence: &Sequence,
    ) -> Result<types::Sequence, ExportProjectError> {
        let layer_name = |id: &donder_model::SequenceLayerId| {
            sequence
                .layers
                .iter()
                .find(|layer| &layer.id == id)
                .map(|layer| name(&layer.name))
                .ok_or_else(|| self.error("", "a layer is missing"))
        };
        let node_name = |id: &donder_model::CompositionGraphNodeId| {
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| &node.id == id)
                .ok_or_else(|| self.error("", "a graph node is missing"))?;
            match &node.kind {
                CompositionGraphNodeKind::Layer { layer_id } => layer_name(layer_id),
                CompositionGraphNodeKind::Operator(operator) => Ok(name(&operator.name)),
                CompositionGraphNodeKind::Output => {
                    Ok(name(&donder_language::object_name("output")))
                }
            }
        };
        let binding = |target: &AutomationTarget| -> Result<types::Binding, ExportProjectError> {
            Ok(match target {
                AutomationTarget::EffectParam { effect_id, param } => types::Binding::ClipParam {
                    clip: sequence
                        .effects
                        .iter()
                        .find(|effect| &effect.id == effect_id)
                        .map(|effect| name(&effect.name))
                        .ok_or_else(|| self.error("", "an automated clip is missing"))?,
                    param: name(param),
                },
                AutomationTarget::CompositionNodeParam { node_id, param } => {
                    types::Binding::NodeParam {
                        node: node_name(node_id)?,
                        param: name(param),
                    }
                }
            })
        };
        Ok(types::Sequence {
            description: sequence.description.clone(),
            duration: sequence.duration.0,
            frame_rate: sequence.frame_rate,
            audio: match &sequence.audio {
                SequenceAudio::None => None,
                SequenceAudio::Asset(id) => Some(Path(
                    self.session
                        .source
                        .referenced_assets
                        .iter()
                        .find(|asset| &asset.id == id)
                        .ok_or_else(|| self.error("", "the sequence's audio asset is missing"))?
                        .relative_path
                        .to_string(),
                )),
            },
            marks: sequence
                .mark_collections
                .iter()
                .map(|collection| types::MarkCollection {
                    name: name(&collection.key.name),
                    description: collection.description.clone(),
                    color: collection.display_color,
                    times: collection.marks.iter().map(|mark| mark.0).collect(),
                })
                .collect(),
            layers: sequence
                .layers
                .iter()
                .map(|layer| types::Layer {
                    name: name(&layer.name),
                    description: layer.description.clone(),
                    color: layer.color,
                    enabled: layer.enabled,
                })
                .collect(),
            clips: sequence
                .effects
                .iter()
                .map(|effect| {
                    let donder_model::EffectRef::Custom(definition) = &effect.definition;
                    let declarations = self
                        .session
                        .project
                        .definitions()
                        .effects
                        .definitions
                        .get(definition)
                        .map(|definition| definition.params().to_vec())
                        .ok_or_else(|| {
                            self.error(definition.0.object(), "the effect is missing")
                        })?;
                    Ok(types::Clip {
                        name: name(&effect.name),
                        description: effect.description.clone(),
                        layer: layer_name(&effect.layer_id)?,
                        start: effect.start.0,
                        duration: effect.duration.0,
                        target: self.fixture_target(&effect.target)?,
                        scope: match effect.scope {
                            donder_model::EffectScope::PerFixture => types::Scope::PerFixture,
                            donder_model::EffectScope::WholeTarget => types::Scope::WholeTarget,
                        },
                        effect: self
                            .source_reference(SourceObjectKind::EffectDefinition, &definition.0)?,
                        params: self.params(&declarations, &effect.param_overrides)?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            graph: types::Graph {
                nodes: sequence
                    .composition_graph
                    .nodes
                    .iter()
                    .map(|node| {
                        let position = (node.position.x, node.position.y);
                        Ok(match &node.kind {
                            CompositionGraphNodeKind::Layer { layer_id } => {
                                types::Node::LayerNode {
                                    layer: layer_name(layer_id)?,
                                    position,
                                }
                            }
                            CompositionGraphNodeKind::Operator(operator) => {
                                let donder_model::OperatorRef::Custom(definition) =
                                    &operator.operator;
                                let declarations = self
                                    .session
                                    .project
                                    .definitions()
                                    .operators
                                    .definitions
                                    .get(definition)
                                    .map(|definition| definition.params().to_vec())
                                    .ok_or_else(|| {
                                        self.error(definition.0.object(), "the operator is missing")
                                    })?;
                                types::Node::OperatorNode {
                                    name: name(&operator.name),
                                    operator: self.source_reference(
                                        SourceObjectKind::OperatorDefinition,
                                        &definition.0,
                                    )?,
                                    params: self.params(&declarations, &operator.params)?,
                                    position,
                                }
                            }
                            CompositionGraphNodeKind::Output => {
                                types::Node::OutputNode { position }
                            }
                        })
                    })
                    .collect::<Result<_, ExportProjectError>>()?,
                edges: sequence
                    .composition_graph
                    .edges
                    .iter()
                    .map(|edge| {
                        let to = sequence
                            .composition_graph
                            .nodes
                            .iter()
                            .find(|node| node.id == edge.to)
                            .ok_or_else(|| self.error("", "an edge's node is missing"))?;
                        let mut segments = vec![node_name(&edge.to)?.0.value];
                        if !matches!(to.kind, CompositionGraphNodeKind::Output) {
                            segments.push(
                                Identifier::new(edge.to_port.0.clone()).map_err(|_| {
                                    self.error(&edge.to_port.0, "invalid input name")
                                })?,
                            );
                        }
                        Ok(types::Edge {
                            from: node_name(&edge.from)?,
                            to: Reference::new(segments),
                        })
                    })
                    .collect::<Result<_, ExportProjectError>>()?,
            },
            automation: sequence
                .automation_clips
                .iter()
                .map(|clip| {
                    Ok(types::AutomationClip {
                        row: self.fixture_target(&clip.row_target)?,
                        start: clip.start.0,
                        duration: clip.duration.0,
                        curve: curve_points(&clip.curve),
                        bindings: clip
                            .bindings
                            .iter()
                            .map(|binding_value| binding(&binding_value.target))
                            .collect::<Result<_, _>>()?,
                        detached: clip
                            .detached_bindings
                            .iter()
                            .map(|detached| {
                                Ok(types::Detached {
                                    binding: binding(&detached.target)?,
                                    reason: match detached.reason {
                                        AutomationDetachmentReason::DefinitionChanged => {
                                            types::DetachReason::DefinitionChanged
                                        }
                                    },
                                })
                            })
                            .collect::<Result<_, ExportProjectError>>()?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
        })
    }

    /// Parameter overrides in the definition's declaration order.
    fn params(
        &self,
        declarations: &[ParamDecl],
        values: &IndexMap<Identifier, EffectParamValue>,
    ) -> Result<Params, ExportProjectError> {
        if let Some(unknown) = values.keys().find(|key| {
            !declarations
                .iter()
                .any(|declaration| &declaration.name == *key)
        }) {
            return Err(self.error(unknown.as_str(), "a parameter value has no declaration"));
        }
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
                        spanned(self.param_value(value)?),
                    ))
                })
                .collect::<Result<_, ExportProjectError>>()?,
        ))
    }

    fn param_value(&self, value: &EffectParamValue) -> Result<DataValue, ExportProjectError> {
        use donder_language::data::Data;
        Ok(match value {
            EffectParamValue::Int(value) => DataValue::Integer(i64::from(*value)),
            EffectParamValue::Float(value) => DataValue::Float(*value),
            EffectParamValue::Bool(value) => DataValue::Bool(*value),
            EffectParamValue::Color(value) => DataValue::Color(*value),
            EffectParamValue::Enum(option) => donder_language::data::variant(option.as_str()),
            EffectParamValue::Marks(Some(key)) => Reference::new(vec![key.name.clone()]).encode(),
            EffectParamValue::Marks(None) => DataValue::None,
            EffectParamValue::Curve(CurveSource::Reference(id)) => self
                .source_reference(SourceObjectKind::Curve, &id.0)?
                .encode(),
            EffectParamValue::Curve(CurveSource::Inline(curve)) => curve_points(curve).encode(),
            EffectParamValue::Gradient(GradientSource::Reference(id)) => self
                .source_reference(SourceObjectKind::Gradient, &id.0)?
                .encode(),
            EffectParamValue::Gradient(GradientSource::Inline(gradient)) => {
                gradient_stops(gradient).encode()
            }
            EffectParamValue::Array(items) => DataValue::List(
                items
                    .iter()
                    .map(|item| self.param_value(item).map(spanned))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }
}

pub(crate) fn controller_document(controller: &Controller) -> types::Controller {
    types::Controller {
        description: controller.description.clone(),
        protocol: match &controller.protocol {
            ControllerProtocol::E131(config) => types::Protocol::E131 {
                source_name: config.source_name.clone(),
                bind_address: config.bind_address.to_string(),
                priority: config.priority,
                mode: match config.mode {
                    E131Mode::Multicast => types::E131Mode::Multicast,
                    E131Mode::Unicast { destination } => types::E131Mode::Unicast {
                        destination: destination.to_string(),
                    },
                },
            },
            ControllerProtocol::ArtNet(config) => types::Protocol::ArtNet {
                bind_address: config.bind_address.to_string(),
                destination: config.destination.to_string(),
                mode: match config.mode {
                    ArtNetMode::Unicast => types::ArtNetMode::Unicast,
                    ArtNetMode::Broadcast => types::ArtNetMode::Broadcast,
                },
            },
            ControllerProtocol::Donder(config) => types::Protocol::Donder {
                device: config.device.as_str().to_string(),
            },
        },
        ports: controller
            .ports
            .iter()
            .map(|port| types::Port {
                name: name(&port.name),
                address: match port.address {
                    ControllerPortAddress::E131Universe(universe) => {
                        types::PortAddress::Universe { universe }
                    }
                    ControllerPortAddress::ArtNetPort(port_address) => {
                        types::PortAddress::ArtNetPort { port_address }
                    }
                    ControllerPortAddress::DonderOutput(output) => {
                        types::PortAddress::Output { output }
                    }
                },
                slots: port.slot_count,
            })
            .collect(),
    }
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
                        axis: match axis {
                            donder_model::GridAxis::Rows => types::GridAxis::Rows,
                            donder_model::GridAxis::Columns => types::GridAxis::Columns,
                        },
                        corner: match corner {
                            donder_model::GridCorner::BottomLeft => types::GridCorner::BottomLeft,
                            donder_model::GridCorner::BottomRight => types::GridCorner::BottomRight,
                            donder_model::GridCorner::TopLeft => types::GridCorner::TopLeft,
                            donder_model::GridCorner::TopRight => types::GridCorner::TopRight,
                        },
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

pub(crate) fn curve_points(curve: &Curve) -> Vec<(f32, f32)> {
    curve
        .points
        .iter()
        .map(|point| (point.position, point.value))
        .collect()
}

pub(crate) fn gradient_stops(gradient: &Gradient) -> Vec<(f32, donder_runtime_types::Color)> {
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
        stops: gradient_stops(&definition.gradient),
    }
}
