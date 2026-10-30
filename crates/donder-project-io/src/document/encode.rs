//! Typed project state to document types. Every typed object has exactly one
//! encoding, and references are written relative to the document holding them.
use super::types;
use crate::ExportProjectError;
use crate::address::{DocumentId, ObjectAddress, OwnedObjectSlot, SourceIdentity};
use crate::source::ProjectSession;
use donder_language::Point3;
use donder_language::compiler::ParamDecl;
use donder_language::data::{DataValue, Spanned};
use donder_language::data::{Meters, Name, OwnedNamedRecord, OwnedRecord, Params, Path, Reference};
use donder_model::ObjectKind;
use donder_model::Owned;
use donder_model::Patch;
use donder_model::Setup;
use donder_model::{
    Action, Console, ConsoleClipId, InputEvent, LengthSource, LiveAutomationId, PlayLength,
    PlayedValue,
};
use donder_model::{ArtNetMode, Controller, ControllerPortAddress, ControllerProtocol, E131Mode};
use donder_model::{AutomationTarget, CompositionGraphNodeKind, Sequence, SequenceAudio};
use donder_model::{
    AxisInput, ButtonInput, ControlRef, InputControlKind, InputDevice, InputDriver, PlayedQuantity,
    SequenceCompositionGraph, SequenceLayer,
};
use donder_model::{FixtureDefinition, FixtureShape, FixtureTransform, OwnedFixture};
use donder_model::{FixtureInstanceId, FixtureTarget, Layout, LayoutFixture, LayoutFixtureKind};
use donder_model::{ObjectId, OwnedMember};
use donder_model::{OwnedCurve, OwnedGradient, ParamValue};
use donder_runtime_types::Identifier;
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;

pub(crate) struct Encoder<'a> {
    pub(crate) session: &'a ProjectSession,
    /// Every object's address, from [`ProjectSession::addresses`].
    pub(crate) addresses: &'a std::collections::HashMap<ObjectId, ObjectAddress>,
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

    /// A declared object, or one owned by a declared object: the owner's
    /// reference followed by the owning fields, `show.setup.controllers.main`.
    fn object_reference(
        &self,
        kind: ObjectKind,
        id: impl Into<ObjectId>,
    ) -> Result<Reference, ExportProjectError> {
        let id = id.into();
        let identity = self
            .addresses
            .get(&id)
            .ok_or_else(|| self.error("", "the referenced object is missing"))?;
        if let Some(source) = identity.source() {
            return self.source_reference(kind, source);
        }
        let root = identity.root_source();
        let root_kind = self
            .session
            .declared_id(root)
            .map(ObjectId::kind)
            .ok_or_else(|| self.error(root.object(), "the owner of this object is missing"))?;
        let mut reference = self.source_reference(root_kind, root)?;
        let mut resolved = root_kind;
        let segment = |text: &str| {
            Identifier::new(text.to_string())
                .map(|identifier| Spanned::new(identifier, donder_language::data::NO_SPAN))
                .map_err(|_| self.error(text, "invalid reference segment"))
        };
        for slot in identity.owned_path() {
            resolved = slot
                .child_kind(resolved)
                .ok_or_else(|| self.error(root.object(), "invalid owned object address"))?;
            match slot {
                OwnedObjectSlot::Setup => reference.segments.push(segment("setup")?),
                OwnedObjectSlot::Layout => reference.segments.push(segment("layout")?),
                OwnedObjectSlot::Patch => reference.segments.push(segment("patch")?),
                OwnedObjectSlot::Controller(member) => {
                    reference.segments.push(segment("controllers")?);
                    reference.segments.push(segment(member.as_str())?);
                }
                OwnedObjectSlot::InputDevice(member) => {
                    reference.segments.push(segment("input_devices")?);
                    reference.segments.push(segment(member.as_str())?);
                }
                OwnedObjectSlot::Sequence(member) => {
                    reference.segments.push(segment("sequences")?);
                    reference.segments.push(segment(member.as_str())?);
                }
                OwnedObjectSlot::Console(member) => {
                    reference.segments.push(segment("consoles")?);
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
        if resolved != kind || id.kind() != kind {
            return Err(self.error(root.object(), "invalid owned object address"));
        }
        Ok(reference)
    }

    /// `layout.fixture`: the layout's reference and the fixture or group name.
    fn fixture_target(&self, target: &FixtureTarget) -> Result<Reference, ExportProjectError> {
        let mut reference = self.object_reference(ObjectKind::Layout, target.layout)?;
        let fixture = self
            .session
            .project
            .get::<donder_model::Layout>(target.layout)
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
        kind: ObjectKind,
        source: &Owned<V, I>,
        inline: impl FnOnce(&V) -> Result<T, ExportProjectError>,
    ) -> Result<OwnedRecord<T>, ExportProjectError>
    where
        I: Into<ObjectId> + Copy,
    {
        Ok(match source {
            Owned::Reference(id) => OwnedRecord::Reference(self.object_reference(kind, *id)?),
            Owned::Inline(value) => OwnedRecord::Inline(inline(value)?),
        })
    }

    /// An owned collection member is named by its ownership slot.
    fn named_source<V, I, T>(
        &self,
        kind: ObjectKind,
        source: &OwnedMember<V, I>,
        inline: impl FnOnce(&V) -> Result<T, ExportProjectError>,
    ) -> Result<OwnedNamedRecord<T>, ExportProjectError>
    where
        I: Into<ObjectId> + Copy,
    {
        Ok(match source {
            OwnedMember::Reference(id) => {
                OwnedNamedRecord::Reference(self.object_reference(kind, *id)?)
            }
            OwnedMember::Inline {
                name: member,
                value,
            } => OwnedNamedRecord::Inline(name(member), inline(value)?),
        })
    }

    pub(crate) fn project(&self) -> Result<types::Project, ExportProjectError> {
        let root = self.session.project.root();
        let metadata = &self.session.source.workspace.metadata;
        Ok(types::Project {
            format: metadata.format_version,
            description: root.description.clone(),
            setup: self.source(ObjectKind::Setup, &root.setup, |setup| self.setup(setup))?,
            sequences: root
                .sequences
                .iter()
                .map(|source| {
                    self.named_source(ObjectKind::Sequence, source, |sequence| {
                        self.sequence(sequence)
                    })
                })
                .collect::<Result<_, _>>()?,
            consoles: root
                .consoles
                .iter()
                .map(|source| {
                    self.named_source(ObjectKind::Console, source, |console| self.console(console))
                })
                .collect::<Result<_, _>>()?,
        })
    }

    pub(crate) fn setup(&self, setup: &Setup) -> Result<types::Setup, ExportProjectError> {
        Ok(types::Setup {
            description: setup.description.clone(),
            layout: self.source(ObjectKind::Layout, &setup.layout, |layout| {
                self.layout(layout)
            })?,
            patch: self.source(ObjectKind::Patch, &setup.patch, |patch| self.patch(patch))?,
            controllers: setup
                .controllers
                .iter()
                .map(|source| {
                    self.named_source(ObjectKind::Controller, source, |controller| {
                        Ok(controller_document(controller))
                    })
                })
                .collect::<Result<_, _>>()?,
            input_devices: setup
                .input_devices
                .iter()
                .map(|source| {
                    self.named_source(ObjectKind::InputDevice, source, |device| {
                        Ok(input_device_document(device))
                    })
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
                    OwnedFixture::Reference(id) => OwnedRecord::Reference(
                        self.object_reference(ObjectKind::FixtureDefinition, *id)?,
                    ),
                    OwnedFixture::Inline(value) => {
                        OwnedRecord::Inline(fixture_definition_document(value))
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
                        .get::<donder_model::Controller>(route.controller)
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
                            .object_reference(ObjectKind::Controller, route.controller)?,
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

    fn layer_name(
        &self,
        layers: &[SequenceLayer],
        id: &donder_model::SequenceLayerId,
    ) -> Result<Name, ExportProjectError> {
        layers
            .iter()
            .find(|layer| &layer.id == id)
            .map(|layer| name(&layer.name))
            .ok_or_else(|| self.error("", "a layer is missing"))
    }

    /// A graph node's name: its operator's, its layer's, or `root`.
    fn node_name(
        &self,
        layers: &[SequenceLayer],
        graph: &SequenceCompositionGraph,
        id: &donder_model::CompositionGraphNodeId,
    ) -> Result<Name, ExportProjectError> {
        let node = graph
            .nodes
            .iter()
            .find(|node| &node.id == id)
            .ok_or_else(|| self.error("", "a graph node is missing"))?;
        match &node.kind {
            CompositionGraphNodeKind::Layer { layer_id } => self.layer_name(layers, layer_id),
            CompositionGraphNodeKind::Operator(operator) => Ok(name(&operator.name)),
            CompositionGraphNodeKind::Root => Ok(name(&donder_language::object_name("root"))),
        }
    }

    /// The layers of a sequence or console.
    fn layers(layers: &[SequenceLayer]) -> Vec<types::Layer> {
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
        layers: &[SequenceLayer],
        graph: &SequenceCompositionGraph,
    ) -> Result<types::Graph, ExportProjectError> {
        Ok(types::Graph {
            nodes: graph
                .nodes
                .iter()
                .map(|node| {
                    let position = (node.position.x, node.position.y);
                    Ok(match &node.kind {
                        CompositionGraphNodeKind::Layer { layer_id } => types::Node::LayerNode {
                            layer: self.layer_name(layers, layer_id)?,
                            position,
                        },
                        CompositionGraphNodeKind::Operator(operator) => {
                            let definition = &operator.operator;
                            let declarations = self
                                .session
                                .project
                                .reusable::<donder_model::OperatorDefinition>()
                                .get(definition)
                                .map(|definition| definition.params().to_vec())
                                .ok_or_else(|| self.error("", "the operator is missing"))?;
                            types::Node::OperatorNode {
                                name: name(&operator.name),
                                operator: self.object_reference(
                                    ObjectKind::OperatorDefinition,
                                    *definition,
                                )?,
                                params: self.params(&declarations, &operator.params)?,
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
                    let to = graph
                        .nodes
                        .iter()
                        .find(|node| node.id == edge.to)
                        .ok_or_else(|| self.error("", "an edge's node is missing"))?;
                    let mut segments = vec![self.node_name(layers, graph, &edge.to)?.0.value];
                    if !matches!(to.kind, CompositionGraphNodeKind::Root) {
                        segments.push(
                            Identifier::new(edge.to_port.0.clone())
                                .map_err(|_| self.error(&edge.to_port.0, "invalid input name"))?,
                        );
                    }
                    Ok(types::Edge {
                        from: self.node_name(layers, graph, &edge.from)?,
                        to: Reference::new(segments),
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
        })
    }

    /// An effect's parameter declarations.
    fn effect_declarations(
        &self,
        definition: &donder_model::EffectDefinitionId,
    ) -> Result<Vec<ParamDecl>, ExportProjectError> {
        self.session
            .project
            .reusable::<donder_model::EffectDefinition>()
            .get(definition)
            .map(|definition| definition.params().to_vec())
            .ok_or_else(|| self.error("", "the effect definition is missing"))
    }

    /// `device.control`: a control's device reference and the control's name.
    fn control(&self, input: &ControlRef) -> Result<Reference, ExportProjectError> {
        let mut reference = self.object_reference(ObjectKind::InputDevice, input.device)?;
        let control = self
            .session
            .project
            .get::<donder_model::InputDevice>(input.device)
            .and_then(|device| device.control(input.control))
            .ok_or_else(|| self.error(&reference.text(), "the input control is missing"))?;
        reference.segments.push(Spanned::new(
            control.name.clone(),
            donder_language::data::NO_SPAN,
        ));
        Ok(reference)
    }

    pub(crate) fn console(&self, console: &Console) -> Result<types::Console, ExportProjectError> {
        let clip_name = |id: &ConsoleClipId| {
            console
                .clip(*id)
                .map(|clip| name(&clip.name))
                .ok_or_else(|| self.error("", "a console clip is missing"))
        };
        let automation_name = |id: &LiveAutomationId| {
            console
                .automation(*id)
                .map(|automation| name(&automation.name))
                .ok_or_else(|| self.error("", "a live automation is missing"))
        };
        let values = |values: &[PlayedValue]| {
            values
                .iter()
                .map(|value| types::PlayedValue {
                    param: name(&value.param),
                    from: match value.from {
                        PlayedQuantity::Velocity => types::PlayedQuantity::Velocity,
                        PlayedQuantity::Pitch => types::PlayedQuantity::Pitch,
                        PlayedQuantity::PitchClass => types::PlayedQuantity::PitchClass,
                        PlayedQuantity::Fifths => types::PlayedQuantity::Fifths,
                    },
                })
                .collect()
        };
        Ok(types::Console {
            description: console.description.clone(),
            frame_rate: console.frame_rate,
            marks: console
                .live_marks
                .iter()
                .map(|marks| types::LiveMarks {
                    name: name(&marks.key.name),
                    description: marks.description.clone(),
                    color: marks.display_color,
                })
                .collect(),
            automations: console
                .automations
                .iter()
                .map(|automation| {
                    Ok(types::LiveAutomation {
                        name: name(&automation.name),
                        description: automation.description.clone(),
                        value: automation.value,
                        bindings: automation
                            .bindings
                            .iter()
                            .map(|binding| {
                                self.binding(
                                    &console.layers,
                                    &console.composition_graph,
                                    binding,
                                    |clip| console.clip(*clip).map(|clip| name(&clip.name)),
                                )
                            })
                            .collect::<Result<_, ExportProjectError>>()?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            layers: Self::layers(&console.layers),
            clips: console
                .clips
                .iter()
                .map(|clip| {
                    let definition = &clip.definition;
                    let declarations = self.effect_declarations(definition)?;
                    Ok(types::ConsoleClip {
                        name: name(&clip.name),
                        description: clip.description.clone(),
                        layer: self.layer_name(&console.layers, &clip.layer_id)?,
                        target: self.fixture_target(&clip.target)?,
                        scope: scope_document(&clip.scope),
                        effect: self.object_reference(ObjectKind::EffectDefinition, *definition)?,
                        params: self.params(&declarations, &clip.param_overrides)?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            always: console
                .always
                .iter()
                .map(clip_name)
                .collect::<Result<_, ExportProjectError>>()?,
            events: console
                .events
                .iter()
                .map(|on| {
                    let control = self.control(on.event.control())?;
                    Ok(types::On {
                        event: match on.event {
                            InputEvent::Press(_) => types::InputEvent::Press { control },
                            InputEvent::Release(_) => types::InputEvent::Release { control },
                            InputEvent::Move(_) => types::InputEvent::Move { control },
                        },
                        actions: on
                            .actions
                            .iter()
                            .map(|action| {
                                Ok(match action {
                                    Action::Play(play) => types::Action::Play {
                                        clip: clip_name(&play.clip)?,
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
                                                    LengthSource::Automation(id) => {
                                                        types::LengthSource::Automation {
                                                            automation: automation_name(id)?,
                                                        }
                                                    }
                                                },
                                                shortest: shortest.0,
                                                longest: longest.0,
                                            },
                                        },
                                        values: values(&play.values),
                                    },
                                    Action::Start {
                                        clip,
                                        values: played,
                                    } => types::Action::Start {
                                        clip: clip_name(clip)?,
                                        values: values(played),
                                    },
                                    Action::Stop { clip } => types::Action::Stop {
                                        clip: clip_name(clip)?,
                                    },
                                    Action::Toggle {
                                        clip,
                                        values: played,
                                    } => types::Action::Toggle {
                                        clip: clip_name(clip)?,
                                        values: values(played),
                                    },
                                    Action::Mark(key) => types::Action::Mark {
                                        into: name(&key.name),
                                    },
                                    Action::Set(id) => types::Action::Set {
                                        into: automation_name(id)?,
                                    },
                                })
                            })
                            .collect::<Result<_, ExportProjectError>>()?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
            graph: self.graph(&console.layers, &console.composition_graph)?,
        })
    }

    /// One clip of `sequence`. Its text depends only on the clip, the
    /// sequence's layers and mark collections, the layouts and definitions it
    /// names, and the imports of the document being written.
    pub(crate) fn clip(
        &self,
        sequence: &Sequence,
        clip: &donder_model::Clip,
    ) -> Result<types::Clip, ExportProjectError> {
        let definition = &clip.definition;
        let declarations = self.effect_declarations(definition)?;
        Ok(types::Clip {
            name: name(&clip.name),
            description: clip.description.clone(),
            layer: self.layer_name(&sequence.layers, &clip.layer_id)?,
            start: clip.start.0,
            duration: clip.duration.0,
            target: self.fixture_target(&clip.target)?,
            scope: scope_document(&clip.scope),
            effect: self.object_reference(ObjectKind::EffectDefinition, *definition)?,
            params: self.params(&declarations, &clip.param_overrides)?,
        })
    }

    /// `sequence` without its clips, which are encoded one by one with
    /// [`Self::clip`] and printed into the `clips` field separately.
    pub(crate) fn sequence(
        &self,
        sequence: &Sequence,
    ) -> Result<types::Sequence, ExportProjectError> {
        let binding = |target: &AutomationTarget| {
            self.binding(
                &sequence.layers,
                &sequence.composition_graph,
                target,
                |clip_id| {
                    sequence
                        .clips
                        .iter()
                        .find(|clip| &clip.id == clip_id)
                        .map(|clip| name(&clip.name))
                },
            )
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
                    times: collection
                        .marks
                        .iter()
                        .map(|mark| types::MarkTime {
                            time: mark.time.0,
                            label: mark.label.clone(),
                        })
                        .collect(),
                })
                .collect(),
            layers: Self::layers(&sequence.layers),
            clips: Vec::new(),
            graph: self.graph(&sequence.layers, &sequence.composition_graph)?,
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
                                })
                            })
                            .collect::<Result<_, ExportProjectError>>()?,
                    })
                })
                .collect::<Result<_, ExportProjectError>>()?,
        })
    }

    /// A sequence's or console's automation binding; `clip_name` names the
    /// clip a clip binding sets.
    fn binding<Clip>(
        &self,
        layers: &[donder_model::SequenceLayer],
        graph: &donder_model::SequenceCompositionGraph,
        target: &AutomationTarget<Clip>,
        clip_name: impl FnOnce(&Clip) -> Option<Name>,
    ) -> Result<types::Binding, ExportProjectError> {
        Ok(match target {
            AutomationTarget::ClipParam { clip_id, param } => types::Binding::ClipParam {
                clip: clip_name(clip_id)
                    .ok_or_else(|| self.error("", "an automated clip is missing"))?,
                param: name(param),
            },
            AutomationTarget::CompositionNodeParam { node_id, param } => {
                types::Binding::NodeParam {
                    node: self.node_name(layers, graph, node_id)?,
                    param: name(param),
                }
            }
        })
    }

    /// Parameter overrides in the definition's declaration order.
    fn params(
        &self,
        declarations: &[ParamDecl],
        values: &IndexMap<Identifier, ParamValue>,
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

    fn param_value(&self, value: &ParamValue) -> Result<DataValue, ExportProjectError> {
        use donder_language::data::Data;
        Ok(match value {
            ParamValue::Int(value) => DataValue::Integer(i64::from(*value)),
            ParamValue::Float(value) => DataValue::Float(*value),
            ParamValue::Bool(value) => DataValue::Bool(*value),
            ParamValue::Color(value) => DataValue::Color(*value),
            ParamValue::Enum(option) => donder_language::data::variant(option.as_str()),
            ParamValue::Marks(Some(key)) => Reference::new(vec![key.name.clone()]).encode(),
            ParamValue::Marks(None) => DataValue::None,
            ParamValue::Curve(OwnedCurve::Reference(id)) => {
                self.object_reference(ObjectKind::Curve, *id)?.encode()
            }
            ParamValue::Curve(OwnedCurve::Inline(curve)) => curve_points(curve).encode(),
            ParamValue::Gradient(OwnedGradient::Reference(id)) => {
                self.object_reference(ObjectKind::Gradient, *id)?.encode()
            }
            ParamValue::Gradient(OwnedGradient::Inline(gradient)) => {
                gradient_stops(gradient).encode()
            }
            ParamValue::Array(items) => DataValue::List(
                items
                    .iter()
                    .map(|item| self.param_value(item).map(spanned))
                    .collect::<Result<_, _>>()?,
            ),
        })
    }
}

fn scope_document(scope: &donder_model::ClipScope) -> types::Scope {
    match scope {
        donder_model::ClipScope::PerFixture => types::Scope::PerFixture,
        donder_model::ClipScope::WholeTarget => types::Scope::WholeTarget,
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

fn curve_points(curve: &Curve) -> Vec<(f32, f32)> {
    curve
        .points
        .iter()
        .map(|point| (point.position, point.value))
        .collect()
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
        stops: gradient_stops(&definition.gradient),
    }
}
