//! The authored schema of data documents. Each type's Rust name and fields
//! are its language name and fields (`docs/project_language.md`), so these
//! definitions are the schema: `#[derive(Data)]` generates parsing, printing
//! and the shapes the generated reference and the language server read.
use donder_data_derive::Data;
use donder_language::data::{
    Data, DataValue, Decoder, Meters, Name, Params, Path, Reference, Schema, Spanned, spanned,
};
use donder_runtime_types::Color;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// A choice of bare variants that is a model enum, named in the language by
/// the model's variant names. The one list of variants maps both ways, and
/// encoding matches the model enum exhaustively.
macro_rules! model_choice {
    ($(#[$meta:meta])* $name:ident($model:path) { $($variant:ident),+ $(,)? }) => {
        $(#[$meta])*
        #[derive(Clone, Debug, PartialEq)]
        pub(crate) struct $name(pub $model);

        impl donder_language::data::Choice for $name {
            const TYPE: &'static str = stringify!($name);
        }

        impl Data for $name {
            fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
                type Model = $model;
                if let DataValue::Variant(name) = &value.value {
                    $(if name.value.as_str() == stringify!($variant) {
                        return Some(Self(Model::$variant));
                    })+
                }
                let variants: &[&str] = &[$(concat!("`", stringify!($variant), "`")),+];
                decoder.mismatch(value, &format!("one of {}", variants.join(", ")))
            }
            fn encode(&self) -> DataValue {
                type Model = $model;
                donder_language::data::variant(match &self.0 {
                    $(Model::$variant => stringify!($variant),)+
                })
            }
            fn shape(schema: &mut Schema) -> donder_language::data::Shape {
                schema.named(stringify!($name), |_| {
                    donder_language::data::Definition::Variants(vec![
                        $((stringify!($variant), None)),+
                    ])
                })
            }
        }
    };
}

/// A value written as a string in its one canonical spelling, the one its
/// `Display` prints, so saving a loaded document rewrites no byte of it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Spelled<T>(pub T);

/// A port's free-text name, with where it is written for links and
/// diagnostics.
#[derive(Clone, Debug)]
pub(crate) struct PortName(pub Spanned<String>);

impl PortName {
    pub(crate) fn new(name: String) -> Self {
        Self(Spanned::new(name, donder_language::data::NO_SPAN))
    }
}

impl PartialEq for PortName {
    fn eq(&self, other: &Self) -> bool {
        self.0.value == other.0.value
    }
}

impl Data for PortName {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        String::decode(value, decoder).map(|name| Self(Spanned::new(name, value.span)))
    }
    fn encode(&self) -> DataValue {
        self.0.value.encode()
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        String::shape(schema)
    }
}

/// A value with a canonical string spelling.
pub(crate) trait Spelling: std::str::FromStr + std::fmt::Display {
    /// What a string of this type is, for diagnostics.
    const WHAT: &'static str;
}

impl Spelling for IpAddr {
    const WHAT: &'static str = "an IP address like `10.0.0.2`";
}

impl Spelling for SocketAddr {
    const WHAT: &'static str = "a socket address like `10.0.0.2:6454`";
}

impl<T: Spelling> Data for Spelled<T> {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        let text = String::decode(value, decoder)?;
        match text.parse::<T>() {
            Ok(parsed) if parsed.to_string() == text => Some(Self(parsed)),
            Ok(parsed) => {
                decoder.error(
                    value.span,
                    format!("write `\"{parsed}\"`, the canonical spelling of this address"),
                );
                None
            }
            Err(_) => {
                decoder.error(value.span, format!("`{text}` is not {}", T::WHAT));
                None
            }
        }
    }
    fn encode(&self) -> DataValue {
        self.0.to_string().encode()
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        String::shape(schema)
    }
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Project {
    pub format: u8,
    pub description: Option<String>,
    #[refers(Declaration("Layout"))]
    pub layout: Reference,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Controller {
    pub description: Option<String>,
    pub protocol: Protocol,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Protocol {
    E131 {
        sender_name: String,
        bind_address: Spelled<IpAddr>,
        priority: u8,
        mode: E131Mode,
        start_universe: u16,
        universes: u16,
        channels: u16,
        /// Where FSEQ files place its first channel, from 1.
        fseq_channel: Option<u32>,
        /// Its physical outputs, in the order its channels fill them.
        ports: Vec<Port>,
    },
    ArtNet {
        bind_address: Spelled<SocketAddr>,
        destination: Spelled<SocketAddr>,
        mode: ArtNetMode,
        /// The 15-bit port-address of the first universe.
        start_universe: u16,
        universes: u16,
        channels: u16,
        /// Where FSEQ files place its first channel, from 1.
        fseq_channel: Option<u32>,
        /// Its physical outputs, in the order its channels fill them.
        ports: Vec<Port>,
    },
    Donder {
        device: String,
        ports: Vec<DonderPort>,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum E131Mode {
    Multicast,
    Unicast { destination: Spelled<IpAddr> },
}

model_choice!(ArtNetMode(donder_model::ArtNetMode) { Unicast, Broadcast });

/// A physical output, named as its controller names it.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Port {
    pub name: PortName,
    pub description: Option<String>,
}

/// A Donder controller's port, its own channel stream.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct DonderPort {
    pub name: PortName,
    pub description: Option<String>,
    /// The controller's physical output, from 1.
    pub output: u8,
    pub slots: u16,
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
    /// Members are fixtures or groups of this layout, or slices of its
    /// fixtures; a member may belong to several groups.
    Group {
        name: Name,
        description: Option<String>,
        members: Vec<GroupMember>,
    },
    Fixture {
        name: Name,
        description: Option<String>,
        #[refers(Declaration("FixtureDefinition"))]
        definition: Reference,
        transform: Transform,
    },
}

/// A group member: a fixture or group by name, `roof`, or a run of a
/// fixture's pixels, `Slice { fixture: roof, start: 0, count: 25 }`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum GroupMember {
    Item(Name),
    Slice(Slice),
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Slice {
    pub fixture: Name,
    pub start: u32,
    pub count: u32,
}

impl Data for GroupMember {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Record(..) => {
                donder_language::data::decode_record(value, decoder).map(Self::Slice)
            }
            _ => Name::decode(value, decoder).map(Self::Item),
        }
    }
    fn encode(&self) -> DataValue {
        match self {
            Self::Item(name) => name.encode(),
            Self::Slice(slice) => slice.encode(),
        }
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        donder_language::data::Shape::OneOf(vec![Name::shape(schema), Slice::shape(schema)])
    }
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

model_choice!(GridAxis(donder_model::GridAxis) { Rows, Columns });

model_choice!(GridCorner(donder_model::GridCorner) {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
});

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Route {
    /// A fixture or group of the project's layout, by its name.
    #[refers(Fixture)]
    pub target: Reference,
    pub pixels: Option<PixelSpan>,
    /// Sends the pixels last first.
    pub reverse: bool,
    #[refers(Declaration("Controller"))]
    pub controller: Reference,
    /// The name of one of the `controller`'s ports.
    pub port: PortName,
    pub start: RouteStart,
    pub encoding: Encoding,
    pub gamma: f32,
    pub brightness: f32,
    pub dither: bool,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum RouteStart {
    /// Right after the route before it on its port.
    Next,
    /// From 0: across an E1.31 or Art-Net controller's universes, or in the
    /// Donder port.
    Channel { channel: u32 },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct PixelSpan {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Encoding {
    Rgb {
        order: (u8, u8, u8),
    },
    Rgbw {
        order: (u8, u8, u8, u8),
    },
    /// One channel per color, each lighting that color.
    Channels {
        colors: Vec<Color>,
    },
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
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct MarkCollection {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
    pub times: Vec<MarkTime>,
}

/// A mark: its time and duration, `(12.5s, 0s)`, or with a label,
/// `(12.5s, 2s, "chorus")`.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkTime {
    pub time: Duration,
    pub duration: Duration,
    pub label: Option<String>,
}

impl Data for MarkTime {
    fn decode(value: &Spanned<DataValue>, decoder: &mut Decoder) -> Option<Self> {
        match &value.value {
            DataValue::Tuple(items) if items.len() == 3 => {
                let (time, duration, label) =
                    <(Duration, Duration, String)>::decode(value, decoder)?;
                Some(Self {
                    time,
                    duration,
                    label: Some(label),
                })
            }
            _ => <(Duration, Duration)>::decode(value, decoder).map(|(time, duration)| Self {
                time,
                duration,
                label: None,
            }),
        }
    }
    fn encode(&self) -> DataValue {
        let mut items = vec![spanned(self.time.encode()), spanned(self.duration.encode())];
        if let Some(label) = &self.label {
            items.push(spanned(label.clone().encode()));
        }
        DataValue::Tuple(items)
    }
    fn shape(schema: &mut Schema) -> donder_language::data::Shape {
        donder_language::data::Shape::OneOf(vec![
            <(Duration, Duration)>::shape(schema),
            <(Duration, Duration, String)>::shape(schema),
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
    /// A fixture or group of the project's layout, by its name: what an
    /// effect plays over, or the row an envelope is drawn in.
    #[refers(Fixture)]
    pub target: Reference,
    pub placement: Placement,
    pub content: ClipContent,
}

/// What a clip plays: an effect on a layer, or an envelope that drives the
/// parameters it binds.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum ClipContent {
    Effect {
        #[refers(Layer)]
        layer: Name,
        scope: Scope,
        #[refers(Effect)]
        effect: Reference,
        /// Overrides of the effect's parameters; the rest follow its defaults.
        params: Params,
    },
    /// Each binding uses the window `(low, high)` of the curve's `0..1`.
    Envelope {
        curve: CurveValue,
        bindings: Vec<Bound>,
    },
}

/// When a clip plays: at a time, or when a console's actions play it.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Placement {
    Timed {
        start: Duration,
        duration: Duration,
    },
    /// Each copy takes `values` from the press that started it.
    Played {
        values: Vec<PlayedValue>,
    },
}

model_choice!(Scope(donder_runtime_types::TargetScope) { PerFixture, WholeTarget });

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
    /// Named [`ROOT_NODE`](donder_language::data::ROOT_NODE).
    RootNode { position: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Edge {
    /// `layers.layer`, or an operator node, whose single output the edge
    /// carries.
    #[refers(Node)]
    pub from: Reference,
    /// `operator.input`, or the root.
    #[refers(Node)]
    pub to: Reference,
}

/// A binding with the window of the curve's `0..1` it uses, as `(low, high)`.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Bound {
    pub binding: Binding,
    pub window: (f32, f32),
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
pub(crate) struct Console {
    pub description: Option<String>,
    pub frame_rate: u32,
    /// Each starts empty every session; `Mark` actions add to it.
    pub marks: Vec<MarkCollection>,
    pub layers: Vec<Layer>,
    pub clips: Vec<Clip>,
    /// Clips that play from the session's start.
    pub always: Vec<Always>,
    pub events: Vec<On>,
    pub graph: Graph,
}

/// A clip that plays from the session's start, repeating every `length`.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Always {
    #[refers(Clip)]
    pub clip: Name,
    pub length: Duration,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct On {
    pub happening: Happening,
    /// A control of an input device: `device.control`.
    #[refers(Control)]
    pub control: Reference,
    pub actions: Vec<Action>,
}

model_choice!(
    /// What happens to a control: a button pressed or released, or an axis moved.
    Happening(donder_runtime_types::InputHappening) { Press, Release, Move }
);

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) enum Action {
    Play {
        #[refers(Clip)]
        clip: Name,
        copies: u8,
        length: PlayLength,
    },
    /// Plays one copy until stopped, repeating every `length`.
    Start {
        #[refers(Clip)]
        clip: Name,
        length: Duration,
    },
    Stop {
        #[refers(Clip)]
        clip: Name,
    },
    Toggle {
        #[refers(Clip)]
        clip: Name,
        length: Duration,
    },
    Mark {
        #[refers(Marks)]
        into: Name,
    },
    /// Writes the event's value, through `window` as `(low, high)`, into a
    /// parameter.
    Set { param: Binding, window: (f32, f32) },
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

/// A parameter a copy takes from how the press that started it was played.
#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct PlayedValue {
    pub param: Name,
    pub from: PlayedQuantity,
    pub window: (f32, f32),
}

model_choice!(PlayedQuantity(donder_runtime_types::PlayedSource) {
    Velocity,
    PitchClass,
});

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Curve {
    pub description: Option<String>,
    pub points: Vec<(f32, f32)>,
}

/// A font parameter's font: a font file drawn at a line height in pixels.
#[derive(Clone, Debug, PartialEq, Data)]
#[data(name = "Font")]
pub(crate) struct FontValue {
    pub file: Path,
    pub height: u32,
}

/// An image parameter's image: an image file and the longest side, in
/// pixels, it is scaled to.
#[derive(Clone, Debug, PartialEq, Data)]
#[data(name = "Image")]
pub(crate) struct ImageValue {
    pub file: Path,
    pub size: u32,
}

/// A curve written in place, where only its value matters: a declaration's
/// fields without its description.
#[derive(Clone, Debug, PartialEq, Data)]
#[data(name = "Curve")]
pub(crate) struct CurveValue {
    pub points: Vec<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub(crate) struct Gradient {
    pub description: Option<String>,
    pub space: GradientSpace,
    pub stops: Vec<(f32, Color)>,
}

/// A gradient written in place, where only its value matters: a declaration's
/// fields without its description.
#[derive(Clone, Debug, PartialEq, Data)]
#[data(name = "Gradient")]
pub(crate) struct GradientValue {
    pub space: GradientSpace,
    pub stops: Vec<(f32, Color)>,
}

model_choice!(
    /// The color space a gradient interpolates in.
    GradientSpace(donder_runtime_types::GradientSpace) { LinearRgb, Hsv, Oklab }
);
