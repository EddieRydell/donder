//! The authored schema of data documents. Each type's Rust name and fields
//! are its language name and fields (`docs/project_language.md`), so these
//! definitions are the schema: `#[derive(Data)]` generates parsing, printing
//! and the shapes the generated reference and the language server read.
use donder_data_derive::Data;
use donder_language::data::{
    Data, DataValue, Decoder, Meters, Name, OwnedNamedRecord, OwnedRecord, Params, Path, Reference,
    Schema, Spanned, spanned,
};
use donder_runtime_types::Color;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Project {
    pub format: u8,
    pub description: Option<String>,
    pub setup: OwnedRecord<Setup>,
    pub sequences: Vec<OwnedNamedRecord<Sequence>>,
    pub consoles: Vec<OwnedNamedRecord<Console>>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Setup {
    pub description: Option<String>,
    pub layout: OwnedRecord<Layout>,
    pub patch: OwnedRecord<Patch>,
    pub controllers: Vec<OwnedNamedRecord<Controller>>,
    pub input_devices: Vec<OwnedNamedRecord<InputDevice>>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Controller {
    pub description: Option<String>,
    pub protocol: Protocol,
    pub ports: Vec<Port>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Protocol {
    E131 {
        sender_name: String,
        bind_address: String,
        priority: u8,
        mode: E131Mode,
    },
    ArtNet {
        bind_address: String,
        destination: String,
        mode: ArtNetMode,
    },
    Donder {
        device: String,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum E131Mode {
    Multicast,
    Unicast { destination: String },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum ArtNetMode {
    Unicast,
    Broadcast,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Port {
    pub name: Name,
    pub address: PortAddress,
    pub slots: u16,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum PortAddress {
    Universe { universe: u16 },
    ArtNetPort { port_address: u16 },
    Output { output: u8 },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct InputDevice {
    pub description: Option<String>,
    pub driver: InputDriver,
    pub controls: Vec<InputControl>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum InputDriver {
    Keyboard,
    Midi { port: Option<String> },
    Osc { port: u16 },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum InputControl {
    Button {
        name: Name,
        description: Option<String>,
        input: ButtonInput,
    },
    Axis {
        name: Name,
        description: Option<String>,
        input: AxisInput,
    },
}

impl InputControl {
    pub(crate) fn name(&self) -> &Name {
        match self {
            Self::Button { name, .. } | Self::Axis { name, .. } => name,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum ButtonInput {
    Key {
        code: KeyCode,
    },
    /// MIDI notes from `low` through `high`, each its own press.
    Notes {
        low: u8,
        high: u8,
        channel: Option<u8>,
    },
    ControlChange {
        number: u8,
        channel: Option<u8>,
    },
    Address {
        path: String,
    },
}

/// A MIDI input's `channel` is 1 to 16, or `none` for every channel.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum AxisInput {
    ControlChange { number: u8, channel: Option<u8> },
    PitchBend { channel: Option<u8> },
    Address { path: String, low: f32, high: f32 },
}

/// A key by its `KeyboardEvent.code` name; the names are the model's.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct KeyCode(pub donder_model::KeyCode);

impl Data for KeyCode {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Variant(name) => match donder_model::KeyCode::from_name(name.value.as_str())
            {
                Some(code) => Some(Self(code)),
                None => {
                    decoder.error(
                        name.span,
                        format!(
                            "`{}` is not a key code; use a `KeyboardEvent.code` name such as `KeyA`, `Digit1` or `Space`",
                            name.value.as_str()
                        ),
                    );
                    None
                }
            },
            _ => decoder.mismatch(value, "a key code such as `KeyA`"),
        }
    }
    fn encode(&self) -> DataValue {
        donder_language::data::variant(self.0.name())
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        schema.named("KeyCode", |_| {
            donder_language::data::Definition::Variants(
                donder_model::KeyCode::ALL
                    .iter()
                    .map(|code| (code.name(), None))
                    .collect(),
            )
        })
    }
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Layout {
    pub description: Option<String>,
    /// Top-level members, by name, in display order.
    pub root: Vec<Name>,
    /// Every fixture and group once; fixture order is placement order.
    pub items: Vec<LayoutItem>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum LayoutItem {
    /// Members are fixtures or groups of this layout; a member may belong to
    /// several groups.
    Group {
        name: Name,
        description: Option<String>,
        members: Vec<Name>,
    },
    Fixture {
        name: Name,
        description: Option<String>,
        definition: OwnedRecord<FixtureDefinition>,
        transform: Transform,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct FixtureDefinition {
    pub description: Option<String>,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Shape {
    pub name: Name,
    pub diameter: Meters,
    pub reverse: bool,
    pub transform: Transform,
    pub geometry: Geometry,
}

/// Position in meters, rotation in degrees, and scale factors.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Transform {
    pub position: (Meters, Meters, Meters),
    pub rotation: (f32, f32, f32),
    pub scale: (f32, f32, f32),
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Geometry {
    Pixel,
    Line {
        length: f32,
        count: u32,
    },
    Polyline {
        points: Vec<(Meters, Meters, Meters)>,
        count: u32,
    },
    Arc {
        radius: f32,
        start_degrees: f32,
        sweep_degrees: f32,
        count: u32,
        closed: bool,
    },
    Grid {
        columns: u32,
        rows: u32,
        width: f32,
        height: f32,
        axis: GridAxis,
        corner: GridCorner,
        serpentine: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum GridAxis {
    Rows,
    Columns,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum GridCorner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Patch {
    pub description: Option<String>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Route {
    /// A fixture or group: `layout.fixture`.
    #[refers(Fixture)]
    pub target: Reference,
    pub pixels: Option<PixelSpan>,
    #[refers(Declaration("Controller"))]
    pub controller: Reference,
    /// A port of `controller`.
    pub port: Name,
    pub start_slot: u16,
    pub encoding: Encoding,
    pub gamma: f32,
    pub brightness: f32,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct PixelSpan {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Encoding {
    Rgb { order: (u8, u8, u8) },
    Rgbw { order: (u8, u8, u8, u8) },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Sequence {
    pub description: Option<String>,
    pub duration: Duration,
    pub frame_rate: u32,
    pub audio: Option<Path>,
    pub marks: Vec<MarkCollection>,
    pub layers: Vec<Layer>,
    pub clips: Vec<Clip>,
    pub graph: Graph,
    pub automation: Vec<AutomationClip>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct MarkCollection {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
    pub times: Vec<MarkTime>,
}

/// A mark: its time alone, `12.5s`, or with a label, `(12.5s, "chorus")`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkTime {
    pub time: Duration,
    pub label: Option<String>,
}

impl Data for MarkTime {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Tuple(_) => {
                let (time, label) = <(Duration, String)>::decode(value, decoder)?;
                Some(Self {
                    time,
                    label: Some(label),
                })
            }
            _ => Duration::decode(value, decoder).map(|time| Self { time, label: None }),
        }
    }
    fn encode(&self) -> DataValue {
        match &self.label {
            None => self.time.encode(),
            Some(label) => DataValue::Tuple(vec![
                spanned(self.time.encode()),
                spanned(label.clone().encode()),
            ]),
        }
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        donder_language::data::Shape::OneOf(vec![
            Duration::shape(schema),
            <(Duration, String)>::shape(schema),
        ])
    }
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Layer {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Clip {
    pub name: Name,
    pub description: Option<String>,
    #[refers(Layer)]
    pub layer: Name,
    pub start: Duration,
    pub duration: Duration,
    /// A fixture or group: `layout.fixture`.
    #[refers(Fixture)]
    pub target: Reference,
    pub scope: Scope,
    #[refers(Effect)]
    pub effect: Reference,
    /// Overrides of the effect's parameters; the rest follow its defaults.
    pub params: Params,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Scope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq, Data)]
#[expect(
    clippy::enum_variant_names,
    reason = "variant names are the language's type names, and `Layer` names the layer record"
)]
pub(crate) enum Node {
    /// Named by its layer.
    LayerNode {
        #[refers(Layer)]
        layer: Name,
        position: (f32, f32),
    },
    OperatorNode {
        name: Name,
        #[refers(Operator)]
        operator: Reference,
        params: Params,
        position: (f32, f32),
    },
    /// Named `output`.
    RootNode { position: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Edge {
    /// A node, whose single output the edge carries.
    #[refers(Node)]
    pub from: Name,
    /// `operator.input`, or `output`.
    #[refers(Node)]
    pub to: Reference,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct AutomationClip {
    /// The fixture or group whose automation row holds the clip.
    #[refers(Fixture)]
    pub row: Reference,
    pub start: Duration,
    pub duration: Duration,
    pub curve: Vec<(f32, f32)>,
    pub bindings: Vec<Binding>,
    pub detached: Vec<Detached>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Binding {
    ClipParam {
        #[refers(Clip)]
        clip: Name,
        param: Name,
    },
    NodeParam {
        #[refers(Node)]
        node: Name,
        param: Name,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Detached {
    pub binding: Binding,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Console {
    pub description: Option<String>,
    pub frame_rate: u32,
    pub marks: Vec<LiveMarks>,
    pub layers: Vec<Layer>,
    pub clips: Vec<ConsoleClip>,
    /// Clips that play for the whole session, by name.
    pub always: Vec<Name>,
    pub events: Vec<On>,
    pub graph: Graph,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct LiveMarks {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct ConsoleClip {
    pub name: Name,
    pub description: Option<String>,
    #[refers(Layer)]
    pub layer: Name,
    /// A fixture or group: `layout.fixture`.
    #[refers(Fixture)]
    pub target: Reference,
    pub scope: Scope,
    #[refers(Effect)]
    pub effect: Reference,
    pub params: Params,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct On {
    pub event: InputEvent,
    pub actions: Vec<Action>,
}

/// A control of an input device: `device.control`.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum InputEvent {
    Press {
        #[refers(Control)]
        control: Reference,
    },
    Release {
        #[refers(Control)]
        control: Reference,
    },
    Move {
        #[refers(Control)]
        control: Reference,
    },
}

impl InputEvent {
    pub(crate) fn control(&self) -> &Reference {
        match self {
            Self::Press { control } | Self::Release { control } | Self::Move { control } => control,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Action {
    Play {
        #[refers(Clip)]
        clip: Name,
        copies: u8,
        length: PlayLength,
        values: Vec<PlayedValue>,
    },
    Start {
        #[refers(Clip)]
        clip: Name,
        values: Vec<PlayedValue>,
    },
    Stop {
        #[refers(Clip)]
        clip: Name,
    },
    Toggle {
        #[refers(Clip)]
        clip: Name,
        values: Vec<PlayedValue>,
    },
    Mark {
        #[refers(Marks)]
        into: Name,
    },
    /// Writes the event's value into a parameter.
    Set { param: Binding },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum PlayLength {
    Fixed {
        duration: Duration,
    },
    Scaled {
        by: LengthSource,
        shortest: Duration,
        longest: Duration,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum LengthSource {
    Velocity,
    Axis {
        #[refers(Control)]
        control: Reference,
    },
}

/// A parameter a press sets on the copy it starts.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct PlayedValue {
    pub param: Name,
    pub from: PlayedQuantity,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum PlayedQuantity {
    Velocity,
    Pitch,
    PitchClass,
    Fifths,
}
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Curve {
    pub description: Option<String>,
    pub points: Vec<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Gradient {
    pub description: Option<String>,
    pub stops: Vec<(f32, Color)>,
}
