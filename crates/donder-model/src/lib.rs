//! The Donder domain model: the typed, validated project that every editor,
//! loader and preparation step works on.
#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

mod controller;
mod effect;
mod fixture;
mod geometry;
mod identity;
mod input_device;
mod layout;
mod operator;
mod ownership;
mod patch;
mod project;
mod rig;
mod sequence;
mod setup;
mod source_remap;
mod validation;

pub use controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerId, ControllerPort, ControllerPortAddress,
    ControllerPortId, ControllerProtocol, ControllerSource, ControllerValidationError,
    DonderConfig, DonderDeviceId, E131Config, E131Mode,
};
pub use effect::{
    CurveDefinition, CurveDefinitionStore, CurveId, CurveSource, EffectDefinition,
    EffectDefinitionId, EffectDefinitionStore, EffectImplementation, EffectInst, EffectInstId,
    EffectParamValue, EffectRef, EffectScope, GradientDefinition, GradientDefinitionStore,
    GradientId, GradientSource,
};
pub use fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureDefinitionId, FixtureDefinitions,
    FixtureElement, FixtureElementId, FixtureGeometryError, FixtureShape, FixtureSource,
    FixtureTransform, GridAxis, GridCorner,
};
pub use geometry::{
    InvalidFixtureElement, PreparedFixtureDefinitions, PreparedFixtureInstance, PreparedLayout,
    PreparedPixel, element_handles, element_pixels, fixture_transform, prepare_geometry,
};
pub use identity::{DocumentId, ObjectIdentity, OwnedObjectSlot, SourceIdentity};
pub use input_device::{
    ButtonInput, ControlRef, DialInput, InputControl, InputControlId, InputControlKind,
    InputDevice, InputDeviceId, InputDeviceSource, InputDeviceValidationError, InputDriver,
    InputSource, KeyCode, KeyRange, MAX_MIDI_CONTROL_CHANGE, MAX_MIDI_NOTE, OscAddress,
};
pub use layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    LayoutId, LayoutSource,
};
pub use operator::{
    GraphOperatorNode, GraphValidationError, OperatorDefinition, OperatorDefinitionId,
    OperatorDefinitionStore, OperatorImplementation, OperatorPortDefinition, OperatorRef,
    composition_graph_output_dependencies, custom_operator_definition, effect_param_matches_type,
    validate_composition_graph,
};
pub use ownership::edit::{
    OwnershipSite, add_rig, add_sequence, duplicate_layout_fixture, make_independent,
    make_reusable, use_existing,
};
pub use ownership::{Identified, ValueSource};
pub use patch::{Patch, PatchId, PatchSource, PixelRoute, PixelRouteId, PixelSpan};
pub use project::{
    AcceptedEffectInputs, AcceptedOperatorInputs, AcceptedRig, AcceptedRigEffect, AcceptedSequence,
    DonderProject, ProjectData, ProjectDefinitionStores, ProjectEdit, ProjectId, ProjectRoot,
    RIG_SESSION, RigBinding, RigEffectOrigin,
};
pub use rig::{
    ControlTarget, LiveMarks, MAX_STACKED_VOICES, Mapping, MappingAction, PlayedSource,
    PlayedValue, Rig, RigEffect, RigId, RigMappingId, RigSource, RigSwitchId, RigVoiceId, Switch,
    Trigger, TriggerLength, Voice, VoicePolicy, When,
};
pub use sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationDetachmentReason,
    AutomationEnvelope, AutomationTarget, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition,
    GraphPortId, Mark, MarkCollection, MarkCollectionKey, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceId, SequenceLayer, SequenceLayerId, SequenceSource,
    automation_curve_is_normalized, automation_value_at,
};
pub use setup::authoring::{
    attach_controller, attach_input_device, detach_controller, detach_input_device,
};
pub use setup::{Setup, SetupId, SetupSource};
pub use source_remap::{remap_document_paths, remap_identity, remap_object_identity};
pub use validation::{
    ProjectValidationError, SequenceValidationError, automation_target_param, validate_project,
    validate_rig, validate_sequence,
};
