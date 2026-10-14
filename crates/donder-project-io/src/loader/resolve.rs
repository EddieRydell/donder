//! Decoded declarations to typed state. Session identities are assigned here
//! in document order, and names resolve to them. Identities inside another
//! object (fixtures, ports) follow from that object's declaration alone, so a
//! reference resolves without the referenced object being built first.
use std::sync::Arc;

use camino::Utf8PathBuf;
use donder_language::compiler::{ParamDecl, TextSpan};
use donder_language::data::{Data, Decoder, Meters, Name, NamedSource, Params, Reference, Source};
use donder_language::data::{DataValue, Spanned};
use donder_language::{
    Distance, DistanceSpan, DonderDuration, DonderTime, Point3, Rotation3, Scale3,
};
use donder_model::ProjectData;
use donder_model::ValueSource;
use donder_model::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
use donder_model::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationDetachmentReason,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition, GraphPortId, Mark,
    MarkCollection, MarkCollectionKey, Sequence, SequenceAudio, SequenceCompositionGraph,
    SequenceId, SequenceLayer, SequenceLayerId, automation_curve_is_normalized,
};
use donder_model::{
    ButtonInput, ButtonRef, InputControl, InputControlId, InputControlKind, InputDevice,
    InputDeviceId, InputDriver,
};
use donder_model::{
    ControlTarget, LiveMarks, MAX_STACKED_VOICES, Mapping, MappingAction, Rig, RigEffect, RigId,
    RigMappingId, RigSwitchId, RigVoiceId, Switch, Trigger, TriggerLength, Voice, VoicePolicy,
    When,
};
use donder_model::{
    CurveSource, EffectDefinitionId, EffectInst, EffectInstId, EffectParamValue, EffectRef,
    EffectScope, GradientSource,
};
use donder_model::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
use donder_model::{
    FixtureDefinition, FixtureDefinitionId, FixtureElement, FixtureElementId, FixtureShape,
    FixtureSource, FixtureTransform, GridAxis, GridCorner,
};
use donder_model::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    LayoutId,
};
use donder_model::{GraphOperatorNode, OperatorRef, validate_composition_graph};
use donder_model::{Patch, PatchId, PixelRoute, PixelRouteId, PixelSpan};
use donder_model::{Setup, SetupId};
use donder_runtime_types::PixelEncoding;
use donder_runtime_types::{Curve, CurvePoint, Gradient, GradientStop};
use donder_runtime_types::{Identifier, Type};
use indexmap::{IndexMap, IndexSet};

use super::{DataDocument, Loader, ResolvedObject};
use crate::LoadProjectError;
use crate::document::{Declaration, types};
use crate::index::{LinkTarget, ScriptMember};
use crate::source::{ReferencedAsset, SourceObjectKind};

pub(super) struct DomainResolver<'a> {
    pub(super) loader: &'a mut Loader,
    pub(super) project: &'a mut ProjectData,
}

/// Each layer of a sequence or rig by name, with its declaring name's span.
type LayerIds = IndexMap<Identifier, (SequenceLayerId, TextSpan)>;

/// Each graph node by name: its identity, whether it is the output, the name
/// it is declared by, and its operator.
type NodeIds = IndexMap<
    Identifier,
    (
        CompositionGraphNodeId,
        bool,
        Option<TextSpan>,
        Option<SourceIdentity>,
    ),
>;

/// A declaration, or an object written in place inside one.
enum Owned<'a> {
    Project(&'a types::Project),
    Setup(&'a types::Setup),
    Controller(&'a types::Controller),
    InputDevice(&'a types::InputDevice),
    Layout(&'a types::Layout),
    Other,
}

fn owned<'a>(declaration: &'a Declaration, path: &[OwnedObjectSlot]) -> Option<Owned<'a>> {
    let mut current = match declaration {
        Declaration::Project(project) => Owned::Project(project),
        Declaration::Setup(setup) => Owned::Setup(setup),
        Declaration::Controller(controller) => Owned::Controller(controller),
        Declaration::InputDevice(device) => Owned::InputDevice(device),
        Declaration::Layout(layout) => Owned::Layout(layout),
        Declaration::Patch(_)
        | Declaration::Sequence(_)
        | Declaration::Rig(_)
        | Declaration::FixtureDefinition(_)
        | Declaration::Curve(_)
        | Declaration::Gradient(_) => Owned::Other,
    };
    fn member<'a, T>(sources: &'a [NamedSource<T>], name: &Identifier) -> Option<&'a T> {
        sources.iter().find_map(|source| match source {
            NamedSource::Inline(member, value) if &member.0.value == name => Some(value),
            _ => None,
        })
    }
    for slot in path {
        current = match (current, slot) {
            (Owned::Project(project), OwnedObjectSlot::Setup) => match &project.setup {
                Source::Inline(setup) => Owned::Setup(setup),
                Source::Reference(_) => return None,
            },
            (Owned::Project(project), OwnedObjectSlot::Sequence(name)) => {
                member(&project.sequences, name)?;
                Owned::Other
            }
            (Owned::Project(project), OwnedObjectSlot::Rig(name)) => {
                member(&project.rigs, name)?;
                Owned::Other
            }
            (Owned::Setup(setup), OwnedObjectSlot::Layout) => match &setup.layout {
                Source::Inline(layout) => Owned::Layout(layout),
                Source::Reference(_) => return None,
            },
            (Owned::Setup(setup), OwnedObjectSlot::Patch) => match &setup.patch {
                Source::Inline(_) => Owned::Other,
                Source::Reference(_) => return None,
            },
            (Owned::Setup(setup), OwnedObjectSlot::Controller(name)) => {
                Owned::Controller(member(&setup.controllers, name)?)
            }
            (Owned::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
                Owned::InputDevice(member(&setup.input_devices, name)?)
            }
            _ => return None,
        };
    }
    Some(current)
}

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

/// The name span of the owned collection member at the end of `path`.
pub(super) fn member_name(declaration: &Declaration, path: &[OwnedObjectSlot]) -> Option<TextSpan> {
    let (last, parent) = path.split_last()?;
    let sources_name = |name: &Identifier, spans: Vec<&Name>| {
        spans
            .into_iter()
            .find(|member| &member.0.value == name)
            .map(|member| member.0.span)
    };
    fn names<T>(sources: &[NamedSource<T>]) -> Vec<&Name> {
        sources
            .iter()
            .filter_map(|source| match source {
                NamedSource::Inline(name, _) => Some(name),
                NamedSource::Reference(_) => None,
            })
            .collect()
    }
    match (owned(declaration, parent)?, last) {
        (Owned::Project(project), OwnedObjectSlot::Sequence(name)) => {
            sources_name(name, names(&project.sequences))
        }
        (Owned::Project(project), OwnedObjectSlot::Rig(name)) => {
            sources_name(name, names(&project.rigs))
        }
        (Owned::Setup(setup), OwnedObjectSlot::Controller(name)) => {
            sources_name(name, names(&setup.controllers))
        }
        (Owned::Setup(setup), OwnedObjectSlot::InputDevice(name)) => {
            sources_name(name, names(&setup.input_devices))
        }
        _ => None,
    }
}

fn script(identity: &SourceIdentity, member: ScriptMember) -> LinkTarget {
    LinkTarget::Script {
        document: identity.document_id().clone(),
        declaration: identity.object().to_string(),
        member,
    }
}

pub(super) fn curve(points: &[(f32, f32)]) -> Result<Curve, String> {
    let curve = Curve {
        points: points
            .iter()
            .map(|&(position, value)| CurvePoint { position, value })
            .collect(),
    };
    curve
        .validate()
        .map_err(|error| format!("invalid curve: {error:?}"))?;
    Ok(curve)
}

pub(super) fn gradient(stops: &[(f32, donder_runtime_types::Color)]) -> Result<Gradient, String> {
    let gradient = Gradient {
        stops: stops
            .iter()
            .map(|&(position, color)| GradientStop { position, color })
            .collect(),
    };
    gradient
        .validate()
        .map_err(|error| format!("invalid gradient: {error:?}"))?;
    Ok(gradient)
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
    fn invalid(
        &self,
        document: &DocumentId,
        span: TextSpan,
        message: impl Into<String>,
    ) -> LoadProjectError {
        self.loader.invalid(document, span, message)
    }

    /// A declared object's document and declaration span.
    fn declared(
        &self,
        identity: &SourceIdentity,
    ) -> Result<(Arc<DataDocument>, TextSpan), LoadProjectError> {
        let data = self.loader.declaration(identity)?;
        let (span, _) = data.declared(identity.object());
        Ok((data, span))
    }

    fn wrong_kind(&self, identity: &SourceIdentity) -> LoadProjectError {
        LoadProjectError::InvalidReference {
            path: identity.document().to_path_buf(),
            range: None,
            reference: identity.object().to_string(),
        }
    }

    // Sources: a declared object, resolved once, or one written in place.

    pub(super) fn setup_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &Source<types::Setup>,
    ) -> Result<donder_model::SetupSource, LoadProjectError> {
        match source {
            Source::Reference(reference) => {
                let ResolvedObject::Setup(id) =
                    self.loader
                        .resolve_reference(document, reference, SourceObjectKind::Setup)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_setup(&id)?;
                Ok(ValueSource::Reference(id))
            }
            Source::Inline(setup) => {
                let id = SetupId(owner.owned(OwnedObjectSlot::Setup));
                Ok(ValueSource::Inline(Box::new(
                    self.setup(&id, document, setup)?,
                )))
            }
        }
    }

    fn layout_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &Source<types::Layout>,
    ) -> Result<donder_model::LayoutSource, LoadProjectError> {
        match source {
            Source::Reference(reference) => {
                let ResolvedObject::Layout(id) =
                    self.loader
                        .resolve_reference(document, reference, SourceObjectKind::Layout)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_layout(&id)?;
                Ok(ValueSource::Reference(id))
            }
            Source::Inline(layout) => {
                let id = LayoutId(owner.owned(OwnedObjectSlot::Layout));
                Ok(ValueSource::Inline(Box::new(
                    self.layout(&id, document, layout)?,
                )))
            }
        }
    }

    fn patch_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &Source<types::Patch>,
    ) -> Result<donder_model::PatchSource, LoadProjectError> {
        match source {
            Source::Reference(reference) => {
                let ResolvedObject::Patch(id) =
                    self.loader
                        .resolve_reference(document, reference, SourceObjectKind::Patch)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_patch(&id)?;
                Ok(ValueSource::Reference(id))
            }
            Source::Inline(patch) => {
                let id = PatchId(owner.owned(OwnedObjectSlot::Patch));
                Ok(ValueSource::Inline(Box::new(
                    self.patch(&id, document, patch)?,
                )))
            }
        }
    }

    fn controller_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &NamedSource<types::Controller>,
    ) -> Result<donder_model::ControllerSource, LoadProjectError> {
        match source {
            NamedSource::Reference(reference) => {
                let ResolvedObject::Controller(id) = self.loader.resolve_reference(
                    document,
                    reference,
                    SourceObjectKind::Controller,
                )?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_controller(&id)?;
                Ok(ValueSource::Reference(id))
            }
            NamedSource::Inline(member, controller) => {
                let id = ControllerId(owner.owned(OwnedObjectSlot::Controller(name(member))));
                Ok(ValueSource::Inline(Box::new(self.controller(
                    &id,
                    document,
                    member.0.span,
                    controller,
                )?)))
            }
        }
    }

    fn input_device_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &NamedSource<types::InputDevice>,
    ) -> Result<donder_model::InputDeviceSource, LoadProjectError> {
        match source {
            NamedSource::Reference(reference) => {
                let ResolvedObject::InputDevice(id) = self.loader.resolve_reference(
                    document,
                    reference,
                    SourceObjectKind::InputDevice,
                )?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_input_device(&id)?;
                Ok(ValueSource::Reference(id))
            }
            NamedSource::Inline(member, device) => {
                let id = InputDeviceId(owner.owned(OwnedObjectSlot::InputDevice(name(member))));
                Ok(ValueSource::Inline(Box::new(self.input_device(
                    &id,
                    document,
                    member.0.span,
                    device,
                )?)))
            }
        }
    }

    /// A rig of the project, whose inputs must be among `setup_devices`.
    pub(super) fn rig_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &NamedSource<types::Rig>,
        setup_devices: &[InputDeviceId],
    ) -> Result<donder_model::RigSource, LoadProjectError> {
        match source {
            NamedSource::Reference(reference) => {
                let ResolvedObject::Rig(id) =
                    self.loader
                        .resolve_reference(document, reference, SourceObjectKind::Rig)?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_rig(&id, Some(setup_devices))?;
                Ok(ValueSource::Reference(id))
            }
            NamedSource::Inline(member, rig) => {
                let id = RigId(owner.owned(OwnedObjectSlot::Rig(name(member))));
                Ok(ValueSource::Inline(Box::new(self.rig(
                    &id,
                    document,
                    member.0.span,
                    rig,
                    Some(setup_devices),
                )?)))
            }
        }
    }

    pub(super) fn sequence_source(
        &mut self,
        document: &DocumentId,
        owner: &ObjectIdentity,
        source: &NamedSource<types::Sequence>,
    ) -> Result<donder_model::SequenceSource, LoadProjectError> {
        match source {
            NamedSource::Reference(reference) => {
                let ResolvedObject::Sequence(id) = self.loader.resolve_reference(
                    document,
                    reference,
                    SourceObjectKind::Sequence,
                )?
                else {
                    return Err(self.loader.unresolved(document, reference));
                };
                self.resolve_sequence(&id)?;
                Ok(ValueSource::Reference(id))
            }
            NamedSource::Inline(member, sequence) => {
                let id = SequenceId(owner.owned(OwnedObjectSlot::Sequence(name(member))));
                Ok(ValueSource::Inline(Box::new(self.sequence(
                    &id,
                    document,
                    member.0.span,
                    sequence,
                )?)))
            }
        }
    }

    // Declared objects.

    pub(super) fn resolve_setup(&mut self, id: &SetupId) -> Result<(), LoadProjectError> {
        if self.project.setups.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Setup(setup) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.setup(id, source.document_id(), setup)?;
        self.project.setups.insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_controller(&mut self, id: &ControllerId) -> Result<(), LoadProjectError> {
        if self.project.controllers.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::Controller(controller) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.controller(id, source.document_id(), span, controller)?;
        self.project.controllers.insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_input_device(
        &mut self,
        id: &InputDeviceId,
    ) -> Result<(), LoadProjectError> {
        if self.project.input_devices.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::InputDevice(device) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.input_device(id, source.document_id(), span, device)?;
        self.project.input_devices.insert(id.clone(), value);
        Ok(())
    }

    /// A declared rig; the project's rigs check their inputs against
    /// `setup_devices`, the input devices of the project's setup.
    pub(super) fn resolve_rig(
        &mut self,
        id: &RigId,
        setup_devices: Option<&[InputDeviceId]>,
    ) -> Result<(), LoadProjectError> {
        if self.project.rigs.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::Rig(rig) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.rig(id, source.document_id(), span, rig, setup_devices)?;
        self.project.rigs.insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_layout(&mut self, id: &LayoutId) -> Result<(), LoadProjectError> {
        if self.project.layouts.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Layout(layout) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.layout(id, source.document_id(), layout)?;
        self.project.layouts.insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_patch(&mut self, id: &PatchId) -> Result<(), LoadProjectError> {
        if self.project.patches.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, _) = self.declared(source)?;
        let Declaration::Patch(patch) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.patch(id, source.document_id(), patch)?;
        self.project.patches.insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_fixture(
        &mut self,
        id: &FixtureDefinitionId,
    ) -> Result<(), LoadProjectError> {
        if self
            .project
            .definitions
            .fixtures
            .definitions
            .contains_key(id)
        {
            return Ok(());
        }
        let (data, span) = self.declared(&id.0)?;
        let Declaration::FixtureDefinition(definition) = data.declared(id.0.object()).1 else {
            return Err(self.wrong_kind(&id.0));
        };
        let value = self.fixture_definition(id.0.document_id(), span, definition)?;
        self.project
            .definitions
            .fixtures
            .definitions
            .insert(id.clone(), value);
        Ok(())
    }

    pub(super) fn resolve_sequence(&mut self, id: &SequenceId) -> Result<(), LoadProjectError> {
        if self.project.sequences.contains_key(id) {
            return Ok(());
        }
        let source = id.0.root_source();
        let (data, span) = self.declared(source)?;
        let Declaration::Sequence(sequence) = data.declared(source.object()).1 else {
            return Err(self.wrong_kind(source));
        };
        let value = self.sequence(id, source.document_id(), span, sequence)?;
        self.project.sequences.insert(id.clone(), value);
        Ok(())
    }

    // Identities inside other objects, from their declarations.

    fn with_owned<R>(
        &self,
        identity: &ObjectIdentity,
        read: impl FnOnce(Owned<'_>) -> Option<R>,
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
    ) -> Result<FixtureTarget, LoadProjectError> {
        let Some((fixture, layout)) = reference.segments.split_last() else {
            return Err(self.loader.unresolved(document, reference));
        };
        let layout_reference = Reference {
            segments: layout.to_vec(),
            span: reference.span,
        };
        let layout = LayoutId(self.loader.resolve_object_reference(
            document,
            &layout_reference,
            SourceObjectKind::Layout,
        )?);
        let (index, item) = self
            .with_owned(&layout.0, |owned| match owned {
                Owned::Layout(layout) => fixture_index(&layout.items, &fixture.value),
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
                document: layout.0.root_source().document_id().clone(),
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
    ) -> Result<ControllerPortId, LoadProjectError> {
        self.with_owned(&controller.0, |owned| match owned {
            Owned::Controller(controller) => controller
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
                    document: controller.0.root_source().document_id().clone(),
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
    ) -> Result<Setup, LoadProjectError> {
        let layout = self.layout_source(document, &id.0, &setup.layout)?;
        let controllers = setup
            .controllers
            .iter()
            .map(|source| self.controller_source(document, &id.0, source))
            .collect::<Result<_, _>>()?;
        let input_devices = setup
            .input_devices
            .iter()
            .map(|source| self.input_device_source(document, &id.0, source))
            .collect::<Result<_, _>>()?;
        let patch = self.patch_source(document, &id.0, &setup.patch)?;
        Ok(Setup {
            id: id.clone(),
            description: setup.description.clone(),
            layout,
            patch,
            controllers,
            input_devices,
        })
    }

    fn input_device(
        &self,
        id: &InputDeviceId,
        document: &DocumentId,
        span: TextSpan,
        device: &types::InputDevice,
    ) -> Result<InputDevice, LoadProjectError> {
        let mut names = IndexMap::new();
        let mut inputs = IndexMap::new();
        let controls = device
            .controls
            .iter()
            .zip(1..)
            .map(|(control, index)| {
                let types::InputControl::Button {
                    name: control_name,
                    description,
                    input,
                } = control;
                if names.insert(name(control_name), ()).is_some() {
                    return Err(self.invalid(
                        document,
                        control_name.0.span,
                        format!("control `{}` is declared twice", control_name.as_str()),
                    ));
                }
                let kind = InputControlKind::Button(match input {
                    types::ButtonInput::Key { code } => ButtonInput::Key(code.0),
                });
                if let Some(other) = inputs.insert(kind, name(control_name)) {
                    let types::ButtonInput::Key { code } = input;
                    return Err(self.invalid(
                        document,
                        control_name.0.span,
                        format!(
                            "control `{}` reads `{}`, which control `{}` already reads",
                            control_name.as_str(),
                            code.0.name(),
                            other.as_str()
                        ),
                    ));
                }
                Ok(InputControl {
                    id: InputControlId(index),
                    name: name(control_name),
                    description: description.clone(),
                    kind,
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let device = InputDevice {
            id: id.clone(),
            description: device.description.clone(),
            driver: match device.driver {
                types::InputDriver::Keyboard => InputDriver::Keyboard,
            },
            controls,
        };
        device.validate().map_err(|error| {
            self.invalid(document, span, format!("invalid input device: {error}"))
        })?;
        Ok(device)
    }

    /// `device.control`: a button of an input device, which must be among
    /// `setup_devices` when they are given.
    fn button(
        &self,
        document: &DocumentId,
        reference: &Reference,
        setup_devices: Option<&[InputDeviceId]>,
    ) -> Result<ButtonRef, LoadProjectError> {
        if self
            .loader
            .resolve_object_reference(document, reference, SourceObjectKind::InputDevice)
            .is_ok()
        {
            return Err(self.invalid(
                document,
                reference.span,
                format!(
                    "`{0}` is an input device; an input is one of its buttons, `{0}.button`",
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
        let device = InputDeviceId(self.loader.resolve_object_reference(
            document,
            &device_reference,
            SourceObjectKind::InputDevice,
        )?);
        let (index, span) = self
            .with_owned(&device.0, |owned| match owned {
                Owned::InputDevice(device) => {
                    device
                        .controls
                        .iter()
                        .zip(1..)
                        .find_map(|(candidate, index)| {
                            let types::InputControl::Button { name, .. } = candidate;
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
                        "`{}` has no button `{}`",
                        device_reference.text(),
                        control.value.as_str()
                    ),
                )
            })?;
        self.loader.link(
            document,
            control.span,
            LinkTarget::Data {
                document: device.0.root_source().document_id().clone(),
                span,
            },
        );
        if let Some(setup_devices) = setup_devices
            && !setup_devices.contains(&device)
        {
            return Err(self.invalid(
                document,
                reference.span,
                format!(
                    "`{}` is not one of the project setup's `input_devices`",
                    device_reference.text()
                ),
            ));
        }
        Ok(ButtonRef {
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
    ) -> Result<Controller, LoadProjectError> {
        fn parse<T: std::str::FromStr>(
            resolver: &DomainResolver<'_>,
            document: &DocumentId,
            span: TextSpan,
            text: &str,
            what: &str,
        ) -> Result<T, LoadProjectError> {
            text.parse()
                .map_err(|_| resolver.invalid(document, span, format!("`{text}` is not {what}")))
        }
        let address = |text: &str, what: &str| parse(self, document, span, text, what);
        let socket = |text: &str, what: &str| parse(self, document, span, text, what);
        let protocol = match &controller.protocol {
            types::Protocol::E131 {
                source_name,
                bind_address,
                priority,
                mode,
            } => ControllerProtocol::E131(E131Config {
                source_name: source_name.clone(),
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
            .enumerate()
            .map(|(index, port)| {
                let address = match (&protocol, &port.address) {
                    (ControllerProtocol::E131(_), types::PortAddress::Universe { universe }) => {
                        ControllerPortAddress::E131Universe(*universe)
                    }
                    (
                        ControllerProtocol::ArtNet(_),
                        types::PortAddress::ArtNetPort { port_address },
                    ) => ControllerPortAddress::ArtNetPort(*port_address),
                    (ControllerProtocol::Donder(_), types::PortAddress::Output { output }) => {
                        ControllerPortAddress::DonderOutput(*output)
                    }
                    _ => {
                        let expected = match &protocol {
                            ControllerProtocol::E131(_) => "Universe",
                            ControllerProtocol::ArtNet(_) => "ArtNetPort",
                            ControllerProtocol::Donder(_) => "Output",
                        };
                        return Err(self.invalid(
                            document,
                            port.name.0.span,
                            format!("this controller's port addresses are `{expected} {{ ... }}`"),
                        ));
                    }
                };
                Ok(ControllerPort {
                    id: ControllerPortId(index as u32 + 1),
                    name: name(&port.name),
                    address,
                    slot_count: port.slots,
                })
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let controller = Controller {
            id: id.clone(),
            description: controller.description.clone(),
            protocol,
            ports,
        };
        controller.validate().map_err(|error| {
            self.invalid(document, span, format!("invalid controller: {error:?}"))
        })?;
        Ok(controller)
    }

    fn layout(
        &mut self,
        id: &LayoutId,
        document: &DocumentId,
        layout: &types::Layout,
    ) -> Result<Layout, LoadProjectError> {
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
            if ids.insert(name(item_name), id).is_some() {
                return Err(self.invalid(
                    document,
                    item_name.0.span,
                    format!(
                        "duplicate layout item name `{}`",
                        item_name.0.value.as_str()
                    ),
                ));
            }
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
        let resolved = Layout {
            id: id.clone(),
            description: layout.description.clone(),
            fixtures,
            root,
        };
        resolved.validate_membership().map_err(|error| {
            let (item, message) = match error {
                LayoutError::DuplicateMember(item) => (item, "is listed twice in one group"),
                LayoutError::Cycle(item) => (item, "contains itself through its members"),
                LayoutError::Unreachable(item) => (item, "is not in `root` or any group"),
                _ => unreachable!("resolved members name existing items"),
            };
            let (types::LayoutItem::Group { name, .. } | types::LayoutItem::Fixture { name, .. }) =
                &layout.items[item.0 as usize - 1];
            self.invalid(
                document,
                name.0.span,
                format!("layout item `{}` {message}", name.0.value.as_str()),
            )
        })?;
        Ok(resolved)
    }

    fn layout_item(
        &mut self,
        document: &DocumentId,
        id: FixtureInstanceId,
        item: &types::LayoutItem,
        members: Vec<FixtureInstanceId>,
    ) -> Result<LayoutFixture, LoadProjectError> {
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
                        Source::Reference(reference) => {
                            let ResolvedObject::FixtureDefinition(definition) =
                                self.loader.resolve_reference(
                                    document,
                                    reference,
                                    SourceObjectKind::FixtureDefinition,
                                )?
                            else {
                                return Err(self.loader.unresolved(document, reference));
                            };
                            self.resolve_fixture(&definition)?;
                            FixtureSource::Reference(definition)
                        }
                        Source::Inline(definition) => FixtureSource::Inline(
                            self.fixture_definition(document, fixture.0.span, definition)?,
                        ),
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
    ) -> Result<FixtureTransform, LoadProjectError> {
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
        let transform = FixtureTransform {
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
        };
        if !transform.is_valid() {
            return Err(self.invalid(document, span, "invalid transform"));
        }
        Ok(transform)
    }

    fn fixture_definition(
        &self,
        document: &DocumentId,
        span: TextSpan,
        definition: &types::FixtureDefinition,
    ) -> Result<FixtureDefinition, LoadProjectError> {
        let elements = definition
            .shapes
            .iter()
            .enumerate()
            .map(|(index, shape)| {
                let span = shape.name.0.span;
                let diameter = u32::try_from(shape.diameter.0)
                    .ok()
                    .filter(|micrometers| (1..=100_000_000).contains(micrometers))
                    .ok_or_else(|| {
                        self.invalid(document, span, "a pixel's diameter is 1µm to 100m")
                    })?;
                let element = FixtureElement {
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
                };
                if !element.is_valid() {
                    return Err(self.invalid(document, span, "invalid shape geometry"));
                }
                Ok(element)
            })
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let definition = FixtureDefinition {
            description: definition.description.clone(),
            elements,
        };
        definition
            .validate_geometry()
            .map_err(|error| self.invalid(document, span, format!("invalid fixture: {error:?}")))?;
        Ok(definition)
    }

    fn patch(
        &mut self,
        id: &PatchId,
        document: &DocumentId,
        patch: &types::Patch,
    ) -> Result<Patch, LoadProjectError> {
        let routes = patch
            .routes
            .iter()
            .enumerate()
            .map(|(index, route)| {
                let controller = ControllerId(self.loader.resolve_object_reference(
                    document,
                    &route.controller,
                    SourceObjectKind::Controller,
                )?);
                // An owned address names a controller only if its owner declares one there.
                if self
                    .with_owned(&controller.0, |owned| {
                        matches!(owned, Owned::Controller(_)).then_some(())
                    })
                    .is_none()
                {
                    return Err(self.loader.unresolved(document, &route.controller));
                }
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
            .collect::<Result<_, LoadProjectError>>()?;
        Ok(Patch {
            id: id.clone(),
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
    fn layers(
        &self,
        document: &DocumentId,
        layers: &[types::Layer],
    ) -> Result<(Vec<SequenceLayer>, LayerIds), LoadProjectError> {
        let mut layer_ids = IndexMap::new();
        let layers = layers
            .iter()
            .enumerate()
            .map(|(index, layer)| {
                // Layer 0, the first, is the default layer.
                let id = SequenceLayerId(index as u32);
                if layer_ids
                    .insert(name(&layer.name), (id.clone(), layer.name.0.span))
                    .is_some()
                {
                    return Err(self.invalid(
                        document,
                        layer.name.0.span,
                        format!("layer `{}` is declared twice", layer.name.as_str()),
                    ));
                }
                Ok(SequenceLayer {
                    id,
                    name: name(&layer.name),
                    description: layer.description.clone(),
                    color: layer.color,
                    enabled: layer.enabled,
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((layers, layer_ids))
    }

    fn layer(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        layer: &Name,
    ) -> Result<SequenceLayerId, LoadProjectError> {
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

    /// A graph node by name: its identity, whether it is the output, and its
    /// operator.
    fn node(
        &self,
        document: &DocumentId,
        node_ids: &NodeIds,
        node: &Spanned<Identifier>,
    ) -> Result<(CompositionGraphNodeId, bool, Option<SourceIdentity>), LoadProjectError> {
        let (id, output, declared, operator) =
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
        Ok((id, output, operator))
    }

    /// The composition graph of a sequence or rig, declared at `span`, and
    /// its nodes by name.
    fn graph(
        &self,
        document: &DocumentId,
        span: TextSpan,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        graph: &types::Graph,
    ) -> Result<(SequenceCompositionGraph, NodeIds), LoadProjectError> {
        let mut node_ids = NodeIds::new();
        let mut nodes = Vec::with_capacity(graph.nodes.len());
        for (index, node) in graph.nodes.iter().enumerate() {
            let id = CompositionGraphNodeId(index as u32 + 1);
            let (node_name, node_span, output, position, kind, declared, operator) = match node {
                types::Node::LayerNode {
                    layer: node_layer,
                    position,
                } => (
                    name(node_layer),
                    node_layer.0.span,
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
                    let ResolvedObject::OperatorDefinition(definition) =
                        self.loader.resolve_reference(
                            document,
                            operator,
                            SourceObjectKind::OperatorDefinition,
                        )?
                    else {
                        return Err(self.loader.unresolved(document, operator));
                    };
                    let declarations = self
                        .project
                        .definitions
                        .operators
                        .definitions
                        .get(&definition)
                        .map(|definition| definition.params().to_vec())
                        .unwrap_or_default();
                    let params =
                        self.params(document, &definition.0, &declarations, marks, params)?;
                    (
                        name(node_name),
                        node_name.0.span,
                        false,
                        position,
                        CompositionGraphNodeKind::Operator(GraphOperatorNode {
                            name: name(node_name),
                            operator: OperatorRef::Custom(definition.clone()),
                            params,
                        }),
                        Some(node_name.0.span),
                        Some(definition.0),
                    )
                }
                types::Node::OutputNode { position } => (
                    donder_language::object_name("output"),
                    span,
                    true,
                    position,
                    CompositionGraphNodeKind::Output,
                    None,
                    None,
                ),
            };
            if node_ids
                .insert(node_name.clone(), (id.clone(), output, declared, operator))
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    node_span,
                    format!("graph node `{}` appears twice", node_name.as_str()),
                ));
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
                                    script(
                                        &operator,
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
                                "an edge to the output is `to: output`",
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
            .collect::<Result<Vec<_>, LoadProjectError>>()?;
        let composition_graph = SequenceCompositionGraph { nodes, edges };
        validate_composition_graph(&composition_graph, &self.project.definitions.operators)
            .map_err(|error| self.invalid(document, span, error.message))?;
        Ok((composition_graph, node_ids))
    }

    fn effect_definition(
        &self,
        document: &DocumentId,
        effect: &Reference,
    ) -> Result<EffectDefinitionId, LoadProjectError> {
        let ResolvedObject::EffectDefinition(definition) =
            self.loader
                .resolve_reference(document, effect, SourceObjectKind::EffectDefinition)?
        else {
            return Err(self.loader.unresolved(document, effect));
        };
        Ok(definition)
    }

    fn scope(scope: &types::Scope) -> EffectScope {
        match scope {
            types::Scope::PerFixture => EffectScope::PerFixture,
            types::Scope::WholeTarget => EffectScope::WholeTarget,
        }
    }

    fn sequence(
        &mut self,
        id: &SequenceId,
        document: &DocumentId,
        span: TextSpan,
        sequence: &types::Sequence,
    ) -> Result<Sequence, LoadProjectError> {
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
        let (layers, layer_ids) = self.layers(document, &sequence.layers)?;
        let mut clip_ids = IndexMap::new();
        let mut effects = Vec::with_capacity(sequence.clips.len());
        for (index, clip) in sequence.clips.iter().enumerate() {
            let id = EffectInstId(index as u32 + 1);
            if clip_ids
                .insert(name(&clip.name), (id.clone(), clip.name.0.span, None))
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    clip.name.0.span,
                    format!("clip `{}` is declared twice", clip.name.as_str()),
                ));
            }
            let definition = self.effect_definition(document, &clip.effect)?;
            let params = self.effect_params(&definition);
            if let Some(entry) = clip_ids.get_mut(&clip.name.0.value) {
                entry.2 = Some(definition.0.clone());
            }
            effects.push(std::sync::Arc::new(EffectInst {
                id,
                name: name(&clip.name),
                description: clip.description.clone(),
                layer_id: self.layer(document, &layer_ids, &clip.layer)?,
                start: DonderTime(clip.start),
                duration: DonderDuration(clip.duration),
                target: self.fixture_target(document, &clip.target)?,
                scope: Self::scope(&clip.scope),
                definition: EffectRef::Custom(definition.clone()),
                param_overrides: self.params(
                    document,
                    &definition.0,
                    &params,
                    &marks,
                    &clip.params,
                )?,
            }));
        }
        let (composition_graph, node_ids) =
            self.graph(document, span, &layer_ids, &marks, &sequence.graph)?;
        let binding = |binding: &types::Binding| -> Result<AutomationTarget, LoadProjectError> {
            Ok(match binding {
                types::Binding::ClipParam { clip, param } => {
                    let (effect_id, span, effect) =
                        clip_ids.get(&clip.0.value).cloned().ok_or_else(|| {
                            self.invalid(
                                document,
                                clip.0.span,
                                format!("no clip `{}`", clip.as_str()),
                            )
                        })?;
                    self.link_local(document, clip.0.span, span);
                    if let Some(effect) = effect {
                        self.loader.link(
                            document,
                            param.0.span,
                            script(&effect, ScriptMember::Param(param.as_str().to_string())),
                        );
                    }
                    AutomationTarget::EffectParam {
                        effect_id,
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
                            script(&operator, ScriptMember::Param(param.as_str().to_string())),
                        );
                    }
                    AutomationTarget::CompositionNodeParam {
                        node_id,
                        param: name(param),
                    }
                }
            })
        };
        let mut automation_targets = IndexSet::new();
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
                        reason: match detached.reason {
                            types::DetachReason::DefinitionChanged => {
                                AutomationDetachmentReason::DefinitionChanged
                            }
                        },
                    })
                })
                .collect::<Result<Vec<_>, LoadProjectError>>()?;
            for target in bindings
                .iter()
                .map(|binding| &binding.target)
                .chain(detached_bindings.iter().map(|binding| &binding.target))
            {
                if !automation_targets.insert(target.clone()) {
                    return Err(self.invalid(
                        document,
                        clip.row.span,
                        "a parameter is automated by one automation clip at most",
                    ));
                }
            }
            let automation_curve = curve(&clip.curve)
                .map_err(|message| self.invalid(document, clip.row.span, message))?;
            if !automation_curve_is_normalized(&automation_curve) {
                return Err(self.invalid(
                    document,
                    clip.row.span,
                    "automation curve values must lie in 0..1",
                ));
            }
            automation_clips.push(AutomationClip {
                id: AutomationClipId(index as u32 + 1),
                start: DonderTime(clip.start),
                duration: DonderDuration(clip.duration),
                row_target: self.fixture_target(document, &clip.row)?,
                curve: automation_curve,
                bindings,
                detached_bindings,
            });
        }
        Ok(Sequence {
            id: id.clone(),
            description: sequence.description.clone(),
            duration: DonderDuration(sequence.duration),
            frame_rate: sequence.frame_rate,
            audio,
            mark_collections,
            layers,
            effects,
            composition_graph,
            automation_clips,
        })
    }

    /// An effect of a rig: a clip without timing.
    #[expect(
        clippy::too_many_arguments,
        reason = "the fields a voice and a trigger share"
    )]
    fn rig_effect(
        &self,
        document: &DocumentId,
        layer_ids: &LayerIds,
        marks: &[(Identifier, TextSpan)],
        layer: &Name,
        target: &Reference,
        scope: &types::Scope,
        effect: &Reference,
        params: &Params,
    ) -> Result<RigEffect, LoadProjectError> {
        let definition = self.effect_definition(document, effect)?;
        let declarations = self.effect_params(&definition);
        Ok(RigEffect {
            layer_id: self.layer(document, layer_ids, layer)?,
            target: self.fixture_target(document, target)?,
            scope: Self::scope(scope),
            param_overrides: self.params(document, &definition.0, &declarations, marks, params)?,
            definition: EffectRef::Custom(definition),
        })
    }

    fn rig(
        &self,
        id: &RigId,
        document: &DocumentId,
        span: TextSpan,
        rig: &types::Rig,
        setup_devices: Option<&[InputDeviceId]>,
    ) -> Result<Rig, LoadProjectError> {
        let mut mark_names = IndexMap::new();
        for collection in &rig.marks {
            if mark_names
                .insert(name(&collection.name), collection.name.0.span)
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    collection.name.0.span,
                    format!(
                        "live mark collection `{}` is declared twice",
                        collection.name.as_str()
                    ),
                ));
            }
        }
        let marks = mark_names.clone().into_iter().collect::<Vec<_>>();
        let (layers, layer_ids) = self.layers(document, &rig.layers)?;
        // Voices and mappings share one namespace; `Control` names either.
        let mut controlled =
            IndexMap::<Identifier, (ControlTarget, TextSpan, Option<SourceIdentity>)>::new();
        let mut voices = Vec::with_capacity(rig.scene.len());
        for (voice, index) in rig.scene.iter().zip(1..) {
            let effect = self.rig_effect(
                document,
                &layer_ids,
                &marks,
                &voice.layer,
                &voice.target,
                &voice.scope,
                &voice.effect,
                &voice.params,
            )?;
            let EffectRef::Custom(definition) = &effect.definition;
            if controlled
                .insert(
                    name(&voice.name),
                    (
                        ControlTarget::Voice(RigVoiceId(index)),
                        voice.name.0.span,
                        Some(definition.0.clone()),
                    ),
                )
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    voice.name.0.span,
                    format!("voice `{}` is declared twice", voice.name.as_str()),
                ));
            }
            voices.push(Voice {
                id: RigVoiceId(index),
                name: name(&voice.name),
                description: voice.description.clone(),
                effect,
            });
        }
        let mut switch_ids = IndexMap::new();
        let mut switches = Vec::with_capacity(rig.switches.len());
        for (switch, index) in rig.switches.iter().zip(1..) {
            if switch_ids
                .insert(name(&switch.name), (RigSwitchId(index), switch.name.0.span))
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    switch.name.0.span,
                    format!("switch `{}` is declared twice", switch.name.as_str()),
                ));
            }
            switches.push(Switch {
                id: RigSwitchId(index),
                name: name(&switch.name),
                description: switch.description.clone(),
                input: self.button(document, &switch.input, setup_devices)?,
            });
        }
        // Mapping names come first, so a `Control` may name a later trigger.
        for (mapping, index) in rig.mappings.iter().zip(1..) {
            let definition = match &mapping.action {
                types::MappingAction::Trigger { effect, .. } => {
                    Some(self.effect_definition(document, effect)?.0)
                }
                types::MappingAction::Mark { .. } | types::MappingAction::Control { .. } => None,
            };
            if controlled
                .insert(
                    name(&mapping.name),
                    (
                        ControlTarget::Trigger(RigMappingId(index)),
                        mapping.name.0.span,
                        definition,
                    ),
                )
                .is_some()
            {
                return Err(self.invalid(
                    document,
                    mapping.name.0.span,
                    format!(
                        "`{}` names a voice or mapping already; they share one namespace",
                        mapping.name.as_str()
                    ),
                ));
            }
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
            Ok::<_, LoadProjectError>(id)
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
                } => MappingAction::Trigger(Box::new(Trigger {
                    effect: self.rig_effect(
                        document, &layer_ids, &marks, layer, target, scope, effect, params,
                    )?,
                    length: match length {
                        types::TriggerLength::Fixed { duration } => {
                            if duration.is_zero() {
                                return Err(self.invalid(
                                    document,
                                    mapping.name.0.span,
                                    format!(
                                        "mapping `{}` has a `Fixed` duration of zero; use a positive duration",
                                        mapping.name.as_str()
                                    ),
                                ));
                            }
                            TriggerLength::Fixed(DonderDuration(*duration))
                        }
                        types::TriggerLength::Held => TriggerLength::Held,
                        types::TriggerLength::Latched => TriggerLength::Latched,
                    },
                    voices: match voices {
                        types::VoicePolicy::Retrigger => VoicePolicy::Retrigger,
                        types::VoicePolicy::Stack { count } => {
                            if !(1..=MAX_STACKED_VOICES).contains(count) {
                                return Err(self.invalid(
                                    document,
                                    mapping.name.0.span,
                                    format!(
                                        "mapping `{}` stacks {count} voices; a stack has 1 to {MAX_STACKED_VOICES}",
                                        mapping.name.as_str()
                                    ),
                                ));
                            }
                            VoicePolicy::Stack { count: *count }
                        }
                        types::VoicePolicy::Mono => VoicePolicy::Mono,
                    },
                })),
                types::MappingAction::Mark { marks } => {
                    let span = mark_names.get(&marks.0.value).copied().ok_or_else(|| {
                        self.invalid(
                            document,
                            marks.0.span,
                            format!("no live mark collection `{}`", marks.as_str()),
                        )
                    })?;
                    self.link_local(document, marks.0.span, span);
                    MappingAction::Mark(MarkCollectionKey { name: name(marks) })
                }
                types::MappingAction::Control { voice, param } => {
                    let (target, span, definition) =
                        controlled.get(&voice.0.value).cloned().ok_or_else(|| {
                            self.invalid(
                                document,
                                voice.0.span,
                                format!("no voice or trigger `{}`", voice.as_str()),
                            )
                        })?;
                    self.link_local(document, voice.0.span, span);
                    let Some(definition) = definition else {
                        return Err(self.invalid(
                            document,
                            voice.0.span,
                            format!("`{}` is not a voice or a `Trigger` mapping", voice.as_str()),
                        ));
                    };
                    let declaration = self
                        .effect_params(&EffectDefinitionId(definition.clone()))
                        .into_iter()
                        .find(|declaration| declaration.name == param.0.value)
                        .ok_or_else(|| {
                            self.invalid(
                                document,
                                param.0.span,
                                format!("no parameter `{}`", param.as_str()),
                            )
                        })?;
                    self.loader.link(
                        document,
                        param.0.span,
                        script(&definition, ScriptMember::Param(param.as_str().to_string())),
                    );
                    if declaration.automation_mapping().is_none() {
                        return Err(self.invalid(
                            document,
                            param.0.span,
                            format!(
                                "`{}` cannot be controlled; a control sets a float, int, bool, enum or curve parameter",
                                param.as_str()
                            ),
                        ));
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
                input: self.button(document, &mapping.input, setup_devices)?,
                when: match &mapping.when {
                    None => None,
                    Some(types::When::On { switch: on }) => Some(When::On(switch(on)?)),
                    Some(types::When::Off { switch: off }) => Some(When::Off(switch(off)?)),
                },
                action,
            });
        }
        let (composition_graph, _) = self.graph(document, span, &layer_ids, &marks, &rig.graph)?;
        Ok(Rig {
            id: id.clone(),
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

    fn effect_params(&self, definition: &EffectDefinitionId) -> Vec<ParamDecl> {
        self.project
            .definitions
            .effects
            .definitions
            .get(definition)
            .map(|definition| definition.params().to_vec())
            .unwrap_or_default()
    }

    fn audio(
        &mut self,
        document: &DocumentId,
        span: TextSpan,
        path: &str,
    ) -> Result<AssetId, LoadProjectError> {
        crate::validate_relative_path(path)
            .map_err(|message| self.invalid(document, span, message))?;
        let module_id = self.loader.workspace.metadata.project_id;
        let missing = || format!("audio asset does not exist inside the project: {path}");
        let absolute = match &self.loader.asset_locations {
            Some(locations) => locations
                .get(camino::Utf8Path::new(path))
                .cloned()
                .ok_or_else(|| self.invalid(document, span, missing()))?,
            None => {
                let unresolved = self.loader.workspace.root.join(path);
                let absolute =
                    unresolved
                        .canonicalize_utf8()
                        .map_err(|source| LoadProjectError::Io {
                            path: unresolved,
                            source,
                        })?;
                if !absolute.is_file() || !absolute.starts_with(&self.loader.workspace.root) {
                    return Err(self.invalid(document, span, missing()));
                }
                absolute
            }
        };
        let relative = Utf8PathBuf::from(path);
        if let Some(existing) = self
            .loader
            .referenced_assets
            .iter_mut()
            .find(|asset| asset.module_id == module_id && asset.relative_path == relative)
        {
            existing.referenced_by.insert(document.clone());
            return Ok(existing.id.clone());
        }
        let id = AssetId(self.loader.next_asset_id);
        self.loader.next_asset_id += 1;
        self.loader.referenced_assets.push(ReferencedAsset {
            id: id.clone(),
            module_id,
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
        definition: &SourceIdentity,
        declarations: &[ParamDecl],
        marks: &[(Identifier, TextSpan)],
        params: &Params,
    ) -> Result<IndexMap<Identifier, EffectParamValue>, LoadProjectError> {
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
                script(
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
        param: (&SourceIdentity, &Identifier),
        ty: &Type,
        marks: &[(Identifier, TextSpan)],
        value: &Spanned<DataValue>,
    ) -> Result<EffectParamValue, LoadProjectError> {
        let reference = |segments: &[Spanned<Identifier>]| Reference {
            segments: segments.to_vec(),
            span: value.span,
        };
        Ok(match (ty, &value.value) {
            (Type::Int, DataValue::Integer(integer)) => EffectParamValue::Int(
                i32::try_from(*integer)
                    .map_err(|_| self.invalid(document, value.span, "this int is out of range"))?,
            ),
            (Type::Float, DataValue::Float(float)) => EffectParamValue::Float(*float),
            (Type::Bool, DataValue::Bool(bool)) => EffectParamValue::Bool(*bool),
            (Type::Color, DataValue::Color(color)) => EffectParamValue::Color(*color),
            (Type::Enum(options), DataValue::Variant(option))
                if options.contains(&option.value) =>
            {
                self.loader.link(
                    document,
                    option.span,
                    script(
                        param.0,
                        ScriptMember::Option {
                            param: param.1.as_str().to_string(),
                            option: option.value.as_str().to_string(),
                        },
                    ),
                );
                EffectParamValue::Enum(option.value.clone())
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
                EffectParamValue::Marks(Some(MarkCollectionKey {
                    name: segment.value.clone(),
                }))
            }
            (Type::Marks, DataValue::None) => EffectParamValue::Marks(None),
            (Type::Curve, DataValue::Reference(segments)) => {
                let ResolvedObject::Curve(curve) = self.loader.resolve_reference(
                    document,
                    &reference(segments),
                    SourceObjectKind::Curve,
                )?
                else {
                    return Err(self.loader.unresolved(document, &reference(segments)));
                };
                EffectParamValue::Curve(CurveSource::Reference(curve))
            }
            (Type::Curve, DataValue::List(_)) => EffectParamValue::Curve(CurveSource::Inline(
                curve(&self.decode::<Vec<(f32, f32)>>(document, value)?)
                    .map_err(|message| self.invalid(document, value.span, message))?,
            )),
            (Type::Gradient, DataValue::Reference(segments)) => {
                let ResolvedObject::Gradient(gradient) = self.loader.resolve_reference(
                    document,
                    &reference(segments),
                    SourceObjectKind::Gradient,
                )?
                else {
                    return Err(self.loader.unresolved(document, &reference(segments)));
                };
                EffectParamValue::Gradient(GradientSource::Reference(gradient))
            }
            (Type::Gradient, DataValue::List(_)) => {
                EffectParamValue::Gradient(GradientSource::Inline(
                    gradient(
                        &self.decode::<Vec<(f32, donder_runtime_types::Color)>>(document, value)?,
                    )
                    .map_err(|message| self.invalid(document, value.span, message))?,
                ))
            }
            (Type::Array(item), DataValue::List(items)) => EffectParamValue::Array(
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
    ) -> Result<T, LoadProjectError> {
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
        Type::Void | Type::Signal => "no value".into(),
    }
}
