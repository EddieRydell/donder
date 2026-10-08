//! The authored schema of data documents. Each type's Rust name and fields
//! are its language name and fields (`docs/project_language.md`), so these
//! definitions are the schema: `#[derive(Data)]` generates parsing, printing
//! and the shapes the generated reference and the language server read.
use donder_data_derive::Data;
use donder_language::data::{Meters, Name, NamedSource, Params, Path, Reference, Source};
use donder_runtime_types::Color;
use std::time::Duration;

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Project {
    pub format: u8,
    pub id: String,
    pub description: Option<String>,
    pub setup: Source<Setup>,
    pub sequences: Vec<NamedSource<Sequence>>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Setup {
    pub description: Option<String>,
    pub layout: Source<Layout>,
    pub patch: Source<Patch>,
    pub controllers: Vec<NamedSource<Controller>>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Controller {
    pub description: Option<String>,
    pub protocol: Protocol,
    pub ports: Vec<Port>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum Protocol {
    E131 {
        source_name: String,
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
pub enum E131Mode {
    Multicast,
    Unicast { destination: String },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum ArtNetMode {
    Unicast,
    Broadcast,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Port {
    pub name: Name,
    pub address: PortAddress,
    pub slots: u16,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum PortAddress {
    Universe { universe: u16 },
    ArtNetPort { port_address: u16 },
    Output { output: u8 },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Layout {
    pub description: Option<String>,
    /// Top-level members, by name, in display order.
    pub root: Vec<Name>,
    /// Every fixture and group once; fixture order is placement order.
    pub items: Vec<LayoutItem>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum LayoutItem {
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
        definition: Source<FixtureDefinition>,
        transform: Transform,
    },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct FixtureDefinition {
    pub description: Option<String>,
    pub shapes: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Shape {
    pub name: Name,
    pub diameter: Meters,
    pub reverse: bool,
    pub transform: Transform,
    pub geometry: Geometry,
}

/// Position in meters, rotation in degrees, and scale factors.
#[derive(Clone, Debug, PartialEq, Data)]
pub struct Transform {
    pub position: (Meters, Meters, Meters),
    pub rotation: (f32, f32, f32),
    pub scale: (f32, f32, f32),
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum Geometry {
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
pub enum GridAxis {
    Rows,
    Columns,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum GridCorner {
    BottomLeft,
    BottomRight,
    TopLeft,
    TopRight,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Patch {
    pub description: Option<String>,
    pub routes: Vec<Route>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Route {
    /// A fixture or group: `layout.fixture`.
    pub target: Reference,
    pub pixels: Option<PixelSpan>,
    pub controller: Reference,
    /// A port of `controller`.
    pub port: Name,
    pub start_slot: u16,
    pub encoding: Encoding,
    pub gamma: f32,
    pub brightness: f32,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct PixelSpan {
    pub start: u32,
    pub count: u32,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum Encoding {
    Rgb { order: (u8, u8, u8) },
    Rgbw { order: (u8, u8, u8, u8) },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Sequence {
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
pub struct MarkCollection {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
    pub times: Vec<Duration>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Layer {
    pub name: Name,
    pub description: Option<String>,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Clip {
    pub name: Name,
    pub description: Option<String>,
    pub layer: Name,
    pub start: Duration,
    pub duration: Duration,
    /// A fixture or group: `layout.fixture`.
    pub target: Reference,
    pub scope: Scope,
    pub effect: Reference,
    /// Overrides of the effect's parameters; the rest follow its defaults.
    pub params: Params,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum Scope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

#[derive(Clone, Debug, PartialEq, Data)]
#[expect(
    clippy::enum_variant_names,
    reason = "variant names are the language's type names, and `Layer` names the layer record"
)]
pub enum Node {
    /// Named by its layer.
    LayerNode { layer: Name, position: (f32, f32) },
    OperatorNode {
        name: Name,
        operator: Reference,
        params: Params,
        position: (f32, f32),
    },
    /// Named `output`.
    OutputNode { position: (f32, f32) },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Edge {
    /// A node, whose single output the edge carries.
    pub from: Name,
    /// `operator.input`, or `output`.
    pub to: Reference,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct AutomationClip {
    /// The fixture or group whose automation row holds the clip.
    pub row: Reference,
    pub start: Duration,
    pub duration: Duration,
    pub curve: Vec<(f32, f32)>,
    pub bindings: Vec<Binding>,
    pub detached: Vec<Detached>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum Binding {
    ClipParam { clip: Name, param: Name },
    NodeParam { node: Name, param: Name },
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Detached {
    pub binding: Binding,
    pub reason: DetachReason,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub enum DetachReason {
    TargetDeleted,
    DefinitionChanged,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Curve {
    pub description: Option<String>,
    pub points: Vec<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq, Data)]
pub struct Gradient {
    pub description: Option<String>,
    pub stops: Vec<(f32, Color)>,
}
