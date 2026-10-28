//! Decoded declarations to typed state. Session identities are assigned here
//! in document order, and names resolve to them. Identities inside another
//! object (fixtures, ports) follow from that object's declaration alone, so a
//! reference resolves without the referenced object being built first.
use donder_model::ObjectKind;
use std::sync::Arc;

use crate::address::{DocumentId, ObjectAddress, OwnedObjectSlot, SourceIdentity};
use camino::Utf8PathBuf;
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
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
use donder_model::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationTarget,
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition, GraphPortId, Mark,
    MarkCollection, MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph,
    SequenceId, SequenceLayer, SequenceLayerId,
};
use donder_model::{
    ButtonInput, ControlRef, DialInput, InputControl, InputControlId, InputControlKind,
    InputDevice, InputDeviceId, InputDriver, KeyRange, OscAddress, PlayedQuantity, PlayedValue,
};
use donder_model::{
    Clip, ClipId, ClipScope, EffectDefinitionId, OwnedCurve, OwnedGradient, ParamValue,
};
use donder_model::{
    ControlTarget, LiveMarks, Mapping, MappingAction, Rig, RigClip, RigId, RigMappingId,
    RigSwitchId, RigVoiceId, Switch, Trigger, TriggerLength, Voice, VoicePolicy, When,
};
use donder_model::{
    FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
    FixtureTransform, GridAxis, GridCorner, OwnedFixture,
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
use crate::source::ReferencedAsset;

pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut ProjectData,
}

/// Each layer of a sequence or rig by name, with its declaring name's span.
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

pub(super) fn gradient(stops: &[(f32, donder_runtime_types::Color)]) -> Gradient {
    Gradient {
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

    pub(super) fn owned_setup(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedRecord<types::Setup>,
    ) -> Result<donder_model::OwnedSetup, LoadError> {
        match record {
            OwnedRecord::Reference(reference) => {
                let ObjectId::Setup(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Setup)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_setup(&id)?;
                Ok(Owned::Reference(id))
            }
            OwnedRecord::Inline(setup) => {
                let id = SetupId(self.loader.intern(owner.owned(OwnedObjectSlot::Setup)));
                Ok(Owned::Inline(Box::new(self.setup(&id, document, setup)?)))
            }
        }
    }

    fn owned_layout(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedRecord<types::Layout>,
    ) -> Result<donder_model::OwnedLayout, LoadError> {
        match record {
            OwnedRecord::Reference(reference) => {
                let ObjectId::Layout(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Layout)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_layout(&id)?;
                Ok(Owned::Reference(id))
            }
            OwnedRecord::Inline(layout) => {
                let id = LayoutId(self.loader.intern(owner.owned(OwnedObjectSlot::Layout)));
                Ok(Owned::Inline(Box::new(self.layout(&id, document, layout)?)))
            }
        }
    }

    fn owned_patch(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedRecord<types::Patch>,
    ) -> Result<donder_model::OwnedPatch, LoadError> {
        match record {
            OwnedRecord::Reference(reference) => {
                let ObjectId::Patch(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Patch)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_patch(&id)?;
                Ok(Owned::Reference(id))
            }
            OwnedRecord::Inline(patch) => {
                let id = PatchId(self.loader.intern(owner.owned(OwnedObjectSlot::Patch)));
                Ok(Owned::Inline(Box::new(self.patch(&id, document, patch)?)))
            }
        }
    }

    fn owned_controller(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedNamedRecord<types::Controller>,
    ) -> Result<donder_model::OwnedController, LoadError> {
        match record {
            OwnedNamedRecord::Reference(reference) => {
                let ObjectId::Controller(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Controller)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_controller(&id)?;
                Ok(OwnedMember::Reference(id))
            }
            OwnedNamedRecord::Inline(member, controller) => {
                let id = ControllerId(
                    self.loader
                        .intern(owner.owned(OwnedObjectSlot::Controller(name(member)))),
                );
                Ok(OwnedMember::Inline {
                    name: name(member),
                    value: Box::new(self.controller(&id, document, member.0.span, controller)?),
                })
            }
        }
    }

    fn owned_input_device(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedNamedRecord<types::InputDevice>,
    ) -> Result<donder_model::OwnedInputDevice, LoadError> {
        match record {
            OwnedNamedRecord::Reference(reference) => {
                let ObjectId::InputDevice(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::InputDevice)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_input_device(&id)?;
                Ok(OwnedMember::Reference(id))
            }
            OwnedNamedRecord::Inline(member, device) => {
                let id = InputDeviceId(
                    self.loader
                        .intern(owner.owned(OwnedObjectSlot::InputDevice(name(member)))),
                );
                Ok(OwnedMember::Inline {
                    name: name(member),
                    value: Box::new(Self::input_device(&id, device)),
                })
            }
        }
    }

    pub(super) fn owned_rig(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedNamedRecord<types::Rig>,
    ) -> Result<donder_model::OwnedRig, LoadError> {
        match record {
            OwnedNamedRecord::Reference(reference) => {
                let ObjectId::Rig(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Rig)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_rig(&id)?;
                Ok(OwnedMember::Reference(id))
            }
            OwnedNamedRecord::Inline(member, rig) => {
                let id = RigId(
                    self.loader
                        .intern(owner.owned(OwnedObjectSlot::Rig(name(member)))),
                );
                Ok(OwnedMember::Inline {
                    name: name(member),
                    value: Box::new(self.rig(&id, document, rig)?),
                })
            }
        }
    }

    pub(super) fn owned_sequence(
        &mut self,
        document: &DocumentId,
        owner: &ObjectAddress,
        record: &OwnedNamedRecord<types::Sequence>,
    ) -> Result<donder_model::OwnedSequence, LoadError> {
        match record {
            OwnedNamedRecord::Reference(reference) => {
                let ObjectId::Sequence(id) =
                    self.loader
                        .resolve_reference(document, reference, ObjectKind::Sequence)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_sequence(&id)?;
                Ok(OwnedMember::Reference(id))
            }
            OwnedNamedRecord::Inline(member, sequence) => {
                let id = SequenceId(
                    self.loader
                        .intern(owner.owned(OwnedObjectSlot::Sequence(name(member)))),
                );
                Ok(OwnedMember::Inline {
                    name: name(member),
                    value: Box::new(self.sequence(&id, document, member.0.span, sequence)?),
                })
            }
        }
    }

    // Declared objects.

    pub(super) fn resolve_setup(&mut self, id: &SetupId) -> Result<(), LoadError> {
        if self.project.setups.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Setup(setup) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.setup(id, source.document_id(), setup)?;
        self.project.setups.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_controller(&mut self, id: &ControllerId) -> Result<(), LoadError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::Controller(controller) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.controller(id, source.document_id(), span, controller)?;
        self.project.controllers.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_input_device(&mut self, id: &InputDeviceId) -> Result<(), LoadError> {
        if self.project.input_devices.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::InputDevice(device) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = Self::input_device(id, device);
        self.project.input_devices.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_rig(&mut self, id: &RigId) -> Result<(), LoadError> {
        if self.project.rigs.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Rig(rig) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.rig(id, source.document_id(), rig)?;
        self.project.rigs.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_layout(&mut self, id: &LayoutId) -> Result<(), LoadError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Layout(layout) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.layout(id, source.document_id(), layout)?;
        self.project.layouts.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_patch(&mut self, id: &PatchId) -> Result<(), LoadError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Patch(patch) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.patch(id, source.document_id(), patch)?;
        self.project.patches.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_fixture(&mut self, id: &FixtureDefinitionId) -> Result<(), LoadError> {
        if self.project.fixtures.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::FixtureDefinition(definition) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.fixture_definition(source.document_id(), definition)?;
        self.project.fixtures.insert(*id, value);
        Ok(())
    }

    pub(super) fn resolve_sequence(&mut self, id: &SequenceId) -> Result<(), LoadError> {
        if self.project.sequences.contains_key(id) {
            return Ok(());
        }
        let address = self.loader.address(*id);
        let source = address.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::Sequence(sequence) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.sequence(id, source.document_id(), span, sequence)?;
        self.project.sequences.insert(*id, value);
        Ok(())
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
        let layout = self.owned_layout(document, &address, &setup.layout)?;
        let controllers = setup
            .controllers
            .iter()
            .map(|record| self.owned_controller(document, &address, record))
            .collect::<Result<_, _>>()?;
        let input_devices = setup
            .input_devices
            .iter()
            .map(|record| self.owned_input_device(document, &address, record))
            .collect::<Result<_, _>>()?;
        let patch = self.owned_patch(document, &address, &setup.patch)?;
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
                            types::ButtonInput::Note { note } => ButtonInput::Note(*note),
                            types::ButtonInput::ControlChange { number } => {
                                ButtonInput::ControlChange(*number)
                            }
                            types::ButtonInput::Address { path } => {
                                ButtonInput::Address(OscAddress(path.clone()))
                            }
                        }),
                    ),
                    types::InputControl::Dial {
                        name,
                        description,
                        input,
                    } => (
                        name,
                        description,
                        InputControlKind::Dial(match input {
                            types::DialInput::ControlChange { number } => {
                                DialInput::ControlChange(*number)
                            }
                            types::DialInput::PitchBend => DialInput::PitchBend,
                            types::DialInput::Address { path, low, high } => DialInput::Address {
                                path: OscAddress(path.clone()),
                                low: *low,
                                high: *high,
                            },
                        }),
                    ),
                    types::InputControl::Keys {
                        name,
                        description,
                        input,
                    } => (
                        name,
                        description,
                        InputControlKind::Keys(KeyRange {
                            low: input.low,
                            high: input.high,
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

    /// A trigger's played values, each linked to its parameter.
    fn played_values(
        &self,
        document: &DocumentId,
        effect: &Reference,
        values: &[types::PlayedValue],
    ) -> Result<Vec<PlayedValue>, LoadError> {
        let definition = self.effect_definition(document, effect)?;
        let declarations = self.definition_params(&definition);
        Ok(values
            .iter()
            .map(|value| {
                let param = &value.param;
                if declarations
                    .iter()
                    .any(|declaration| declaration.name == param.0.value)
                {
                    self.loader.link(
                        document,
                        param.0.span,
                        self.script(definition, ScriptMember::Param(param.as_str().to_string())),
                    );
                }
                PlayedValue {
                    param: name(param),
                    from: match value.from {
                        types::PlayedQuantity::Velocity => PlayedQuantity::Velocity,
                        types::PlayedQuantity::Pitch => PlayedQuantity::Pitch,
                        types::PlayedQuantity::PitchClass => PlayedQuantity::PitchClass,
                        types::PlayedQuantity::Fifths => PlayedQuantity::Fifths,
                    },
                }
            })
            .collect())
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
        fn parse<T: std::str::FromStr>(
            resolver: &DomainResolver<'_>,
            document: &DocumentId,
            span: TextSpan,
            text: &str,
            what: &str,
        ) -> Result<T, LoadError> {
            text.parse()
                .map_err(|_| resolver.invalid(document, span, format!("`{text}` is not {what}")))
        }
        let address = |text: &str, what: &str| parse(self, document, span, text, what);
        let socket = |text: &str, what: &str| parse(self, document, span, text, what);
        let protocol = match &controller.protocol {
            types::Protocol::E131 {
                sender_name,
                bind_address,
                priority,
                mode,
            } => ControllerProtocol::E131(E131Config {
                sender_name: sender_name.clone(),
                bind_address: address(bind_address, "an IP address")?,
                priority: *priority,
                mode: match mode {
                    types::E131Mode::Multicast => E131Mode::Multicast,
                    types::E131Mode::Unicast { destination } => E131Mode::Unicast {
                        destination: address(destination, "an IP address")?,
                    },
                },
            }),
            types::Protocol::ArtNet {
                bind_address,
                destination,
                mode,
            } => ControllerProtocol::ArtNet(ArtNetConfig {
                bind_address: socket(bind_address, "a socket address like `0.0.0.0:6454`")?,
                destination: socket(destination, "a socket address like `10.0.0.2:6454`")?,
                mode: match mode {
                    types::ArtNetMode::Unicast => ArtNetMode::Unicast,
                    types::ArtNetMode::Broadcast => ArtNetMode::Broadcast,
                },
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
                        OwnedRecord::Reference(reference) => {
                            let ObjectId::FixtureDefinition(definition) =
                                self.loader.resolve_reference(
                                    document,
                                    reference,
                                    ObjectKind::FixtureDefinition,
                                )?
                            else {
                                return Err(self.loader.unresolved(document, reference));
                            };
                            self.resolve_fixture(&definition)?;
                            OwnedFixture::Reference(definition)
                        }
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
                            axis: match axis {
                                types::GridAxis::Rows => GridAxis::Rows,
                                types::GridAxis::Columns => GridAxis::Columns,
                            },
                            corner: match corner {
                                types::GridCorner::BottomLeft => GridCorner::BottomLeft,
                                types::GridCorner::BottomRight => GridCorner::BottomRight,
                                types::GridCorner::TopLeft => GridCorner::TopLeft,
                                types::GridCorner::TopRight => GridCorner::TopRight,
                            },
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

    /// The layers of a sequence or rig, numbered from 0, the default layer.
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

    /// The composition graph of a sequence or rig, and its nodes by name.
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
                        .unwrap_or_default();
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
                    donder_language::object_name("root"),
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
                        (to, true, _) => (to, "input".to_string()),
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
                            (to, input.value.as_str().to_string())
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
                Ok(EffectGraphEdge {
                    from,
                    from_port: GraphPortId("output".to_string()),
                    to,
                    to_port: GraphPortId(input),
                })
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

    fn scope(scope: &types::Scope) -> ClipScope {
        match scope {
            types::Scope::PerFixture => ClipScope::PerFixture,
            types::Scope::WholeTarget => ClipScope::WholeTarget,
        }
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
                scope: Self::scope(&clip.scope),
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
        let binding = |binding: &types::Binding| -> Result<AutomationTarget, LoadError> {
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
                    self.loader.link(
                        document,
                        param.0.span,
                        self.script(effect, ScriptMember::Param(param.as_str().to_string())),
                    );
                    AutomationTarget::ClipParam {
                        clip_id,
                        param: name(param),
                    }
                }
                types::Binding::NodeParam {
                    node: target,
                    param,
                } => {
                    let (node_id, _, operator) = self.node(document, &node_ids, &target.0)?;
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
        };
        let mut automation_clips = Vec::with_capacity(sequence.automation.len());
        for (index, clip) in sequence.automation.iter().enumerate() {
            let bindings = clip
                .bindings
                .iter()
                .map(|target| binding(target).map(|target| AutomationBinding { target }))
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
                curve: curve(&clip.curve),
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

    /// A rig clip: a sequence clip without timing.
    #[expect(
        clippy::too_many_arguments,
        reason = "the fields a voice and a trigger share"
    )]
    fn rig_clip(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        layer: &Name,
        target: &Reference,
        scope: &types::Scope,
        effect: &Reference,
        params: &Params,
    ) -> Result<RigClip, LoadError> {
        let definition = self.effect_definition(document, effect)?;
        let declarations = self.definition_params(&definition);
        Ok(RigClip {
            layer_id: self.layer(document, layer_ids, layer)?,
            target: self.fixture_target(document, target)?,
            scope: Self::scope(scope),
            param_overrides: self.params(
                document,
                definition.into(),
                &declarations,
                marks,
                params,
            )?,
            definition: (definition),
        })
    }

    fn rig(&self, id: &RigId, document: &DocumentId, rig: &types::Rig) -> Result<Rig, LoadError> {
        let mut mark_names = IndexMap::new();
        for collection in &rig.marks {
            mark_names
                .entry(name(&collection.name))
                .or_insert(collection.name.0.span);
        }
        let marks = mark_names.clone().into_iter().collect::<Vec<_>>();
        let (layers, layer_ids) = Self::layers(&rig.layers);
        // Voices and mappings share one namespace; `Control` names either.
        let mut controlled =
            IndexMap::<Identifier, (ControlTarget, TextSpan, Option<EffectDefinitionId>)>::new();
        let mut voices = Vec::with_capacity(rig.scene.len());
        for (voice, index) in rig.scene.iter().zip(1..) {
            let clip = self.rig_clip(
                document,
                &layer_ids,
                &marks,
                &voice.layer,
                &voice.target,
                &voice.scope,
                &voice.effect,
                &voice.params,
            )?;
            controlled.entry(name(&voice.name)).or_insert((
                ControlTarget::Voice(RigVoiceId(index)),
                voice.name.0.span,
                Some(clip.definition),
            ));
            voices.push(Voice {
                id: RigVoiceId(index),
                name: name(&voice.name),
                description: voice.description.clone(),
                clip,
            });
        }
        let mut switch_ids = IndexMap::new();
        let mut switches = Vec::with_capacity(rig.switches.len());
        for (switch, index) in rig.switches.iter().zip(1..) {
            switch_ids
                .entry(name(&switch.name))
                .or_insert((RigSwitchId(index), switch.name.0.span));
            switches.push(Switch {
                id: RigSwitchId(index),
                name: name(&switch.name),
                description: switch.description.clone(),
                input: self.control_ref(document, &switch.input)?,
            });
        }
        // Mapping names come first, so a `Control` may name a later trigger.
        for (mapping, index) in rig.mappings.iter().zip(1..) {
            let definition = match &mapping.action {
                types::MappingAction::Trigger { effect, .. } => {
                    Some(self.effect_definition(document, effect)?)
                }
                types::MappingAction::Mark { .. } | types::MappingAction::Control { .. } => None,
            };
            controlled.entry(name(&mapping.name)).or_insert((
                ControlTarget::Trigger(RigMappingId(index)),
                mapping.name.0.span,
                definition,
            ));
        }
        let switch = |switch: &Name| {
            let (id, span) = switch_ids.get(&switch.0.value).copied().ok_or_else(|| {
                self.invalid(
                    document,
                    switch.0.span,
                    format!("no switch `{}`", switch.as_str()),
                )
            })?;
            self.link_local(document, switch.0.span, span);
            Ok::<_, LoadError>(id)
        };
        let mut mappings = Vec::with_capacity(rig.mappings.len());
        for (mapping, index) in rig.mappings.iter().zip(1..) {
            let action = match &mapping.action {
                types::MappingAction::Trigger {
                    layer,
                    target,
                    scope,
                    effect,
                    params,
                    length,
                    voices,
                    values,
                } => MappingAction::Trigger(Box::new(Trigger {
                    clip: self.rig_clip(
                        document, &layer_ids, &marks, layer, target, scope, effect, params,
                    )?,
                    values: self.played_values(document, effect, values)?,
                    length: match length {
                        types::TriggerLength::Fixed { duration } => {
                            TriggerLength::Fixed(DonderDuration(*duration))
                        }
                        types::TriggerLength::Held => TriggerLength::Held,
                        types::TriggerLength::Latched => TriggerLength::Latched,
                    },
                    voices: match voices {
                        types::VoicePolicy::Retrigger => VoicePolicy::Retrigger,
                        types::VoicePolicy::Stack { count } => VoicePolicy::Stack { count: *count },
                        types::VoicePolicy::Mono => VoicePolicy::Mono,
                    },
                })),
                types::MappingAction::Mark { marks } => {
                    if let Some(span) = mark_names.get(&marks.0.value) {
                        self.link_local(document, marks.0.span, *span);
                    }
                    MappingAction::Mark(MarkCollectionKey { name: name(marks) })
                }
                types::MappingAction::Control { voice, param } => {
                    let (target, span, definition) =
                        controlled.get(&voice.0.value).copied().ok_or_else(|| {
                            self.invalid(
                                document,
                                voice.0.span,
                                format!("no voice or trigger `{}`", voice.as_str()),
                            )
                        })?;
                    self.link_local(document, voice.0.span, span);
                    if let Some(definition) = definition
                        && self
                            .definition_params(&definition)
                            .iter()
                            .any(|declaration| declaration.name == param.0.value)
                    {
                        self.loader.link(
                            document,
                            param.0.span,
                            self.script(
                                definition,
                                ScriptMember::Param(param.as_str().to_string()),
                            ),
                        );
                    }
                    MappingAction::Control {
                        target,
                        param: name(param),
                    }
                }
            };
            mappings.push(Mapping {
                id: RigMappingId(index),
                name: name(&mapping.name),
                description: mapping.description.clone(),
                input: self.control_ref(document, &mapping.input)?,
                when: match &mapping.when {
                    None => None,
                    Some(types::When::On { switch: on }) => Some(When::On(switch(on)?)),
                    Some(types::When::Off { switch: off }) => Some(When::Off(switch(off)?)),
                },
                action,
            });
        }
        let (composition_graph, _) = self.graph(document, &layer_ids, &marks, &rig.graph)?;
        Ok(Rig {
            id: *id,
            description: rig.description.clone(),
            frame_rate: rig.frame_rate,
            live_marks: rig
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
            voices,
            switches,
            mappings,
            composition_graph,
        })
    }

    fn definition_params(&self, definition: &EffectDefinitionId) -> Vec<ParamDecl> {
        self.project
            .effects
            .get(definition)
            .map(|definition| definition.params().to_vec())
            .unwrap_or_default()
    }

    fn audio(
        &mut self,
        document: &DocumentId,
        span: TextSpan,
        path: &str,
    ) -> Result<AssetId, LoadError> {
        crate::validate_relative_path(path)
            .map_err(|message| self.invalid(document, span, message))?;
        let unresolved = self.loader.workspace.root.join(path);
        let absolute = unresolved
            .canonicalize_utf8()
            .map_err(|source| crate::diagnostics::read_error(unresolved, &source))?;
        if !absolute.is_file() || !absolute.starts_with(&self.loader.workspace.root) {
            return Err(self.invalid(
                document,
                span,
                format!("audio asset does not exist inside the project: {path}"),
            ));
        }
        let relative = Utf8PathBuf::from(path);
        if let Some(existing) = self
            .loader
            .referenced_assets
            .iter_mut()
            .find(|asset| asset.relative_path == relative)
        {
            existing.referenced_by.insert(document.clone());
            return Ok(existing.id.clone());
        }
        let id = AssetId(self.loader.next_asset_id);
        self.loader.next_asset_id += 1;
        self.loader.referenced_assets.push(ReferencedAsset {
            id: id.clone(),
            relative_path: relative,
            absolute_path: absolute,
            referenced_by: std::collections::BTreeSet::from([document.clone()]),
        });
        Ok(id)
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
            (Type::Curve, DataValue::List(_)) => ParamValue::Curve(OwnedCurve::Inline(curve(
                &self.decode::<Vec<(f32, f32)>>(document, value)?,
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
            (Type::Gradient, DataValue::List(_)) => ParamValue::Gradient(OwnedGradient::Inline(
                gradient(&self.decode::<Vec<(f32, donder_runtime_types::Color)>>(document, value)?),
            )),
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
        Type::Curve => "a curve: a reference or `[(position, value), ...]`".into(),
        Type::Gradient => "a gradient: a reference or `[(position, #color), ...]`".into(),
        Type::Array(item) => format!("a list of {}", describe(item, marks)),
        Type::Void => "no value".into(),
    }
}
