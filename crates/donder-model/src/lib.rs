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

mod clock;
mod composition;
mod console;
mod controller;
mod derived;
mod effect;
mod fixture;
mod geometry;
mod identity;
mod input_device;
mod layout;
mod operator;
mod patch;
mod project;
mod sequence;
mod site;
mod validation;

pub use clock::{sample_duration, sample_time};
pub use composition::{
    Composition, CompositionGraph, CompositionGraphNode, CompositionGraphNodeId,
    CompositionGraphNodeKind, EffectGraphEdge, GraphNodePosition, Layer, LayerId, Mark,
    MarkCollection, MarkCollectionId,
};
pub use console::{
    Action, Always, ClipUse, Console, LengthSource, MAX_COPIES, On, OnId, Play, PlayLength,
    PlayedValue, needs_note,
};
pub use controller::{
    ArtNetConfig, ArtNetMode, ChannelPlace, Controller, ControllerOutput, ControllerPort,
    ControllerPortId, ControllerProtocol, ControllerValidationError, DonderConfig, DonderDeviceId,
    DonderPort, E131Config, E131Mode, UniverseChannel, Universes,
};
pub use derived::{AudioAnalysis, AudioFeature, Derivation, Derived, FontRaster, Number};
pub use effect::{
    Clip, ClipContent, ClipId, CurveDefinition, CurveValue, DefinitionProblem, Effect, FontFace,
    GradientDefinition, GradientValue, ImageSource, ParamValue, Placement, ScriptDefinition,
};
pub use fixture::{
    FixtureDefinition, FixtureDefinitionError, FixtureElement, FixtureElementId,
    FixtureGeometryError, FixtureShape, FixtureTransform, GridAxis, GridCorner,
    fixture_pixel_counts,
};
pub use geometry::{
    InvalidFixtureElement, PreparedFixtureDefinitions, PreparedFixtureInstance, PreparedLayout,
    PreparedPixel, element_handles, element_pixels, fixture_rotation, fixture_transform,
    prepare_geometry,
};
pub use identity::{
    ConsoleId, ControllerId, CurveId, FixtureDefinitionId, GradientId, InputDeviceId, LayoutId,
    ObjectId, ObjectKind, ProjectId, ScriptId, SequenceId, TypedId,
};
pub use input_device::{
    AxisInput, ButtonInput, ControlInput, ControlRef, InputControl, InputControlId,
    InputControlKind, InputDevice, InputDeviceValidationError, InputDriver, KeyCode,
    MAX_MIDI_CHANNEL, MAX_MIDI_CONTROL_CHANGE, MAX_MIDI_NOTE, MidiChannel, NoteRange, OscAddress,
};
pub use layout::{
    FixtureCopies, FixtureId, GroupMember, Layout, LayoutError, LayoutFixture, LayoutFixtureKind,
    TargetPart, duplicate_layout_fixture,
};
pub use operator::{
    GraphOperatorNode, GraphValidationError, composition_graph_root_dependencies, param_matches,
    validate_composition_graph,
};
pub use patch::{
    PixelRoute, PixelRouteId, PixelSpan, RoutePlacement, RouteStart, patched_level,
    route_placements,
};
pub use project::{
    AcceptedClip, AcceptedOperatorInputs, AcceptedShow, DonderProject, EditError, EditRejection,
    LiveBinding, MemberRef, MemberValue, Object, PlaybackRoute, ProjectData, ProjectEdit,
    ProjectObject, ProjectRoot, ShowId, View,
};
pub use sequence::{
    AssetId, AutomationEnvelope, AutomationTarget, Binding, Envelope, Sequence, SequenceAudio,
    Window,
};
pub use site::{Field, Member, Site};
pub use validation::{
    AdmissionProblem, BindingProblem, ClipProblem, CompositionProblem, ConsoleValidationError,
    EnvelopeProblem, EventProblem, GiveWay, InputProblem, NameProblem, ParamProblem, PlayedProblem,
    ProjectProblem, ProjectValidationError, RegionProblem, Repair, RouteProblem, TimingProblem,
    ValidationProblem, automation_target_param, validate_project, validate_sequence,
};
