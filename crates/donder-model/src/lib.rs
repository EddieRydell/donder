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

mod console;
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
mod sequence;
mod setup;
mod validation;

pub use console::{
    Action, ClipUse, Console, ConsoleAutomationTarget, ConsoleClip, ConsoleClipId, InputEvent,
    LengthSource, LiveMarks, MAX_COPIES, On, OnId, OwnedConsole, Play, PlayLength, PlayedQuantity,
    PlayedValue, sample_duration,
};
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
    ConsoleId, ControllerId, CurveId, EffectDefinitionId, FixtureDefinitionId, GradientId,
    InputDeviceId, LayoutId, ObjectId, ObjectKind, OperatorDefinitionId, PatchId, ProjectId,
    SequenceId, SetupId,
};
pub use input_device::{
    AxisInput, ButtonInput, ControlInput, ControlRef, InputControl, InputControlId,
    InputControlKind, InputDevice, InputDeviceValidationError, InputDriver, KeyCode,
    MAX_MIDI_CHANNEL, MAX_MIDI_CONTROL_CHANGE, MAX_MIDI_NOTE, MidiChannel, NoteRange, OscAddress,
    OwnedInputDevice,
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
    OwnershipSite, add_console, add_sequence, duplicate_layout_fixture, make_independent,
    make_reusable, use_existing,
};
pub use ownership::{Identified, Owned, OwnedMember};
pub use patch::{OwnedPatch, Patch, PixelRoute, PixelRouteId, PixelSpan};
pub use project::{
    AcceptedClipInputs, AcceptedConsole, AcceptedConsoleClip, AcceptedOperatorInputs,
    AcceptedSequence, CONSOLE_SESSION, ConsoleBinding, DonderProject, EditError, EditRejection,
    Object, ProjectData, ProjectEdit, ProjectObject, ProjectRoot, Reusable,
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
    AdmissionProblem, AutomationProblem, ClipProblem, CompositionProblem, ConsoleValidationError,
    EventProblem, InputProblem, Member, MemberPart, NameProblem, OwnershipProblem, ParamProblem,
    ProblemSite, ProjectValidationError, RegionProblem, RouteProblem, SequenceValidationError,
    SetupProblem, TimingProblem, ValidationProblem, automation_target_param, validate_console,
    validate_project, validate_sequence,
};
