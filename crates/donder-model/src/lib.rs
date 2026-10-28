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
mod validation;

pub use controller::{
    ArtNetConfig, ArtNetMode, Controller, ControllerPort, ControllerPortAddress, ControllerPortId,
    ControllerProtocol, ControllerValidationError, DonderConfig, DonderDeviceId, E131Config,
    E131Mode, OwnedController,
};
pub use effect::{
    Clip, ClipId, ClipScope, CurveDefinition, EffectDefinition, GradientDefinition, OwnedCurve,
    OwnedGradient, ParamValue,
};
pub use fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureElement, FixtureElementId,
    FixtureGeometryError, FixtureShape, FixtureTransform, GridAxis, GridCorner, OwnedFixture,
    fixture_pixel_counts,
};
pub use geometry::{
    InvalidFixtureElement, PreparedFixtureDefinitions, PreparedFixtureInstance, PreparedLayout,
    PreparedPixel, element_handles, element_pixels, fixture_transform, prepare_geometry,
};
pub use identity::{
    ControllerId, CurveId, EffectDefinitionId, FixtureDefinitionId, GradientId, InputDeviceId,
    LayoutId, ObjectId, ObjectKind, OperatorDefinitionId, PatchId, ProjectId, RigId, SequenceId,
    SetupId,
};
pub use input_device::{
    ButtonInput, ControlInput, ControlRef, DialInput, InputControl, InputControlId,
    InputControlKind, InputDevice, InputDeviceValidationError, InputDriver, KeyCode, KeyRange,
    MAX_MIDI_CONTROL_CHANGE, MAX_MIDI_NOTE, OscAddress, OwnedInputDevice,
};
pub use layout::{
    FixtureInstanceId, FixtureTarget, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    OwnedLayout,
};
pub use operator::{
    GraphOperatorNode, GraphValidationError, OperatorDefinition, OperatorPortDefinition,
    composition_graph_root_dependencies, param_matches_type, validate_composition_graph,
};
pub use ownership::edit::{
    OwnershipSite, add_rig, add_sequence, duplicate_layout_fixture, make_independent,
    make_reusable, use_existing,
};
pub use ownership::{Identified, Owned, OwnedMember};
pub use patch::{OwnedPatch, Patch, PixelRoute, PixelRouteId, PixelSpan};
pub use project::{
    AcceptedClipInputs, AcceptedOperatorInputs, AcceptedRig, AcceptedRigClip, AcceptedSequence,
    DonderProject, EditError, EditRejection, Object, ProjectData, ProjectEdit, ProjectObject,
    ProjectRoot, RIG_SESSION, Reusable, RigBinding, RigClipOrigin,
};
pub use rig::{
    ControlTarget, LiveMarks, MAX_STACKED_VOICES, Mapping, MappingAction, OwnedRig, PlayedQuantity,
    PlayedValue, Rig, RigClip, RigMappingId, RigSwitchId, RigVoiceId, Switch, Trigger,
    TriggerLength, Voice, VoicePolicy, When,
};
pub use sequence::{
    AssetId, AutomationBinding, AutomationClip, AutomationClipId, AutomationEnvelope,
    AutomationTarget, CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind,
    DetachedAutomationBinding, EffectGraphEdge, GraphNodePosition, GraphPortId, Mark,
    MarkCollection, MarkCollectionKey, OwnedSequence, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceLayer, SequenceLayerId, automation_curve_is_normalized,
    automation_value_at,
};
pub use setup::authoring::{
    attach_controller, attach_input_device, detach_controller, detach_input_device,
};
pub use setup::{OwnedSetup, Setup};
pub use validation::{
    AdmissionProblem, AutomationProblem, ClipProblem, CompositionProblem, InputProblem,
    MappingProblem, Member, MemberPart, NameProblem, OwnershipProblem, ParamProblem, ProblemSite,
    ProjectValidationError, RegionProblem, RigValidationError, RouteProblem,
    SequenceValidationError, SetupProblem, TimingProblem, ValidationProblem,
    automation_target_param, validate_project, validate_rig, validate_sequence,
};
