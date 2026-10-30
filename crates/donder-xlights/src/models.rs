//! A model's settings, read as xLights reads them, and its nodes: where
//! xLights draws each light in the model's own coordinates, and the channel
//! each node starts at.
//!
//! Each supported type reads its own settings into a typed struct, the only
//! code that reads its attributes, and lays its nodes out from that struct.
//! Every type, supported or not, also has its channel span from its
//! settings alone, so models whose start channel follows it still resolve.

mod arches;
mod canes;
mod circles;
mod custom;
mod icicles;
mod lines;
mod matrices;
mod multipoint;
mod polyline;
mod spinner;
mod star;
mod window;

use crate::kind::ModelType;
use crate::placement::{Placement, RenderSize};
use crate::read::{self, Name, Reader, Table, Use};
use crate::report::SkipReason;
use crate::xml::Element;
use donder_runtime_types::PixelEncoding;
use glam::DVec3;

/// One node in xLights' node order.
pub(crate) struct Node {
    /// Channels after the model's first that the node starts at.
    pub(crate) channel: u32,
    /// Where xLights draws each of its lights, in model coordinates.
    pub(crate) lights: Vec<DVec3>,
    /// The node's place on one of the model's ellipses, for drawing it as
    /// an arc.
    pub(crate) arc: Option<ArcPoint>,
}

/// A node on one of the model's ellipses, at `degrees` along it.
#[derive(Clone, Copy)]
pub(crate) struct ArcPoint {
    pub(crate) arc: usize,
    pub(crate) degrees: f64,
}

/// The points `center + u cos φ + v sin φ`.
#[derive(Clone, Copy)]
pub(crate) struct Ellipse {
    pub(crate) center: DVec3,
    pub(crate) u: DVec3,
    pub(crate) v: DVec3,
}

/// A model laid out in its own coordinates.
pub(crate) struct Local {
    pub(crate) nodes: Vec<Node>,
    pub(crate) ellipses: Vec<Ellipse>,
    pub(crate) size: RenderSize,
    pub(crate) placement: Placement,
}

/// How xLights sends a node's colors, by `StringType`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct StringType {
    /// xLights: `Model::GetNodeChannelCount`.
    pub(crate) channels: u32,
    /// xLights: `Model::HasSingleNode`: one node holds a whole string.
    pub(crate) single_node: bool,
    /// xLights: `HasSingleChannel`: a whole string takes one channel.
    pub(crate) single_channel: bool,
    /// The channels each node xLights makes holds, which can differ from
    /// the channels between nodes. xLights: the node classes
    /// `Model::SetNodeCount` makes.
    pub(crate) node_channels: u32,
    /// The same for a node made with no lights, as Custom models make
    /// theirs. xLights: `SetNodeCount(1, 0, ...)`.
    pub(crate) empty_node_channels: u32,
    /// The pixels Donder sends, for individual RGB or RGBW pixels.
    pub(crate) encoding: Option<PixelEncoding>,
}

impl StringType {
    /// xLights: `Model::GetNodeChannelCount`, `HasSingleNode` and
    /// `HasSingleChannel`, with `superstring` the number of
    /// `SuperStringColour` attributes.
    fn new(text: &str, superstring: u32) -> Self {
        let bytes = text.as_bytes();
        let channels = if text.starts_with("Single Color")
            || text == "Strobes White 3fps"
            || text == "Strobes"
        {
            1
        } else if text == "4 Channel RGBW" || text == "4 Channel WRGB" {
            4
        } else if text == "RGBWW Nodes" {
            5
        } else if bytes.first() == Some(&b'W') || bytes.get(3) == Some(&b'W') {
            4
        } else if text == "Superstring" {
            superstring.max(1)
        } else if text == "Node Single Color" {
            1
        } else {
            3
        };
        let single_node =
            text != "Node Single Color" && (text == "Superstring" || !text.contains(" Nodes"));
        let single_channel = channels == 1 && text != "Node Single Color";
        let single_class = match text {
            "Single Color Red"
            | "Single Color Green"
            | "Single Color Blue"
            | "Single Color White"
            | "Strobes White 3fps"
            | "Strobes"
            | "Single Color Custom"
            | "Single Color Intensity" => 1,
            "Superstring" => superstring,
            "4 Channel RGBW" | "4 Channel WRGB" => 4,
            _ => 3,
        };
        let node_channels = if single_node {
            single_class
        } else if bytes.get(3) == Some(&b' ') {
            3
        } else if text == "Node Single Color" {
            1
        } else if text == "RGBWW Nodes" {
            5
        } else {
            4
        };
        // xLights: Model::FourChannelNodes and FiveChannelNodes.
        let four = text.contains('W')
            && [
                "RGBW", "WRGB", "WRBG", "RBGW", "WGRB", "GRBW", "WGBR", "GBRW", "WBRG", "BRGW",
                "WBGR", "BGRW",
            ]
            .iter()
            .any(|order| text.contains(order));
        let empty_node_channels = if single_node {
            single_class
        } else if text == "Node Single Color" {
            1
        } else if text.contains("RGBWW") {
            5
        } else if four {
            4
        } else {
            3
        };
        Self {
            channels,
            single_node,
            single_channel,
            node_channels,
            empty_node_channels,
            encoding: (!single_node).then(|| pixel_encoding(text)).flatten(),
        }
    }
}

/// The pixel order of `RGB Nodes` and its permutations, and of the four
/// letter RGBW orders.
fn pixel_encoding(text: &str) -> Option<PixelEncoding> {
    let letters = text.strip_suffix(" Nodes")?;
    let order: Vec<u8> = letters
        .chars()
        .map(|letter| match letter {
            'R' => Some(0),
            'G' => Some(1),
            'B' => Some(2),
            'W' => Some(3),
            _ => None,
        })
        .collect::<Option<_>>()?;
    let encoding = match order.as_slice() {
        [a, b, c] => PixelEncoding::Rgb {
            order: [*a, *b, *c],
        },
        // xLights: Model::GetNodeChannelCount counts four channels only
        // with the white first or last.
        [a, b, c, d] if *a == 3 || *d == 3 => PixelEncoding::Rgbw {
            order: [*a, *b, *c, *d],
        },
        _ => return None,
    };
    encoding.is_valid().then_some(encoding)
}

/// How a model's output is dimmed, when it is exactly a brightness and a
/// gamma.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Dimming {
    /// Output scale: 1 leaves it as it is.
    pub(crate) brightness: f32,
    pub(crate) gamma: f32,
}

/// What every model has, read as xLights reads it.
pub(crate) struct Common<'a> {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    /// xLights: `BaseObject::SetActive(as_int(1))`.
    pub(crate) active: bool,
    /// xLights: `ShadowModelFor`, cleared by `ModelManager::LoadModels`
    /// when it names no model.
    pub(crate) shadow_of: Option<String>,
    pub(crate) layout_group: &'a str,
    pub(crate) start_channel: &'a str,
    /// xLights: `Controller`, trimmed, with "Use Start Channel" as none.
    pub(crate) controller: String,
    pub(crate) string_type: StringType,
    /// xLights: `Model::IsLtoR`: `Dir` is not "R".
    pub(crate) left_to_right: bool,
    /// xLights: `Model::IsBtoT`: `StartSide` is "B", or missing.
    pub(crate) bottom_to_top: bool,
    /// xLights: `ControllerConnection` `ts`, the smart receiver's strings
    /// per port, which spaces each string's channels.
    pub(crate) ts: i32,
    /// The model's dimming, or why it is refused.
    pub(crate) dimming: Result<Option<Dimming>, SkipReason>,
}

/// Attributes every model may have. xLights: `CommonDeserializeSteps`.
const COMMON: Table = &[
    (Name::Exact("name"), Use::Modeled),
    (Name::Exact("DisplayAs"), Use::Modeled),
    (Name::Exact("DmxStyle"), Use::Modeled),
    (Name::Exact("LayoutGroup"), Use::Modeled),
    (Name::Exact("Active"), Use::Modeled),
    (Name::Exact("StartSide"), Use::Modeled),
    (Name::Exact("Dir"), Use::Modeled),
    (Name::Exact("StringType"), Use::Modeled),
    (Name::Numbered("SuperStringColour"), Use::Modeled),
    (Name::Exact("ShadowModelFor"), Use::Modeled),
    (Name::Exact("Description"), Use::Modeled),
    (Name::Exact("Controller"), Use::Modeled),
    (Name::Exact("StartChannel"), Use::Modeled),
    (Name::Exact("ModelBrightness"), Use::Modeled),
    (Name::Exact("WorldPosX"), Use::Modeled),
    (Name::Exact("WorldPosY"), Use::Modeled),
    (Name::Exact("WorldPosZ"), Use::Modeled),
    (Name::Exact("SourceVersion"), Use::Modeled),
    (Name::Exact("versionNumber"), Use::Modeled),
    (Name::Exact("Advanced"), Use::RefusedUnlessDefault),
    (Name::Exact("RGBWHandling"), Use::RefusedUnlessDefault),
    (
        Name::Numbered("String"),
        Use::Irrelevant("a string's own start channel, read only with Advanced, which is refused"),
    ),
    (
        Name::Exact("FromBase"),
        Use::Irrelevant("marks a model merged from a base show folder"),
    ),
    (
        Name::Exact("Antialias"),
        Use::Irrelevant("how xLights draws the model's lights"),
    ),
    (
        Name::Exact("PixelSize"),
        Use::Irrelevant("how xLights draws the model's lights"),
    ),
    (
        Name::Exact("Transparency"),
        Use::Irrelevant("how xLights draws the model's lights"),
    ),
    (
        Name::Exact("BlackTransparency"),
        Use::Irrelevant("how xLights draws the model's lights"),
    ),
    (
        Name::Exact("LowDefinition"),
        Use::Irrelevant("the size of xLights' effect buffer"),
    ),
    (
        Name::Exact("TagColour"),
        Use::Irrelevant("a color xLights tags the model with"),
    ),
    (
        Name::Exact("NodeNames"),
        Use::Irrelevant("names in xLights' sequencer"),
    ),
    (
        Name::Exact("StrandNames"),
        Use::Irrelevant("names in xLights' sequencer"),
    ),
    (
        Name::Exact("CustomColor"),
        Use::Irrelevant("the color of single color strings, which are not imported"),
    ),
    (
        Name::Exact("ModelChain"),
        Use::Irrelevant(
            "orders models on a controller port for xLights' auto layout, which loading a show does not run",
        ),
    ),
    (
        Name::Exact("PixelSpacing"),
        Use::Irrelevant("a description of the lights"),
    ),
    (
        Name::Exact("PixelCount"),
        Use::Irrelevant("a description of the lights"),
    ),
    (
        Name::Exact("PixelType"),
        Use::Irrelevant("a description of the lights"),
    ),
    (
        Name::Exact("Locked"),
        Use::Irrelevant("stops the model being moved in xLights"),
    ),
    (
        Name::Exact("PreviewRotation"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("PreviewScaleX"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("PreviewScaleY"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("offsetXpct"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("offsetYpct"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("X1"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("Y1"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("Z1"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Exact("ControllerConnection"),
        Use::Irrelevant("not read by current xLights"),
    ),
    (
        Name::Numbered("parm"),
        Use::Irrelevant("read only by the types that list it"),
    ),
];

/// Child elements every model may have. xLights:
/// `DeserializeCommonModelChildElements`.
const CHILDREN: Table = &[
    (Name::Exact("ControllerConnection"), Use::Modeled),
    (Name::Exact("dimmingCurve"), Use::Modeled),
    (
        Name::Exact("subModel"),
        Use::Irrelevant("a named part of the model's nodes; groups that name one leave it out"),
    ),
    (
        Name::Exact("faceInfo"),
        Use::Irrelevant("named node sets for xLights' face effects"),
    ),
    (
        Name::Exact("stateInfo"),
        Use::Irrelevant("named node sets for xLights' state effects"),
    ),
    (
        Name::Exact("Aliases"),
        Use::Irrelevant("other names for the model in xLights' sequences"),
    ),
    (
        Name::Exact("modelGroup"),
        Use::Irrelevant("read only when importing a model file"),
    ),
    (
        Name::Exact("shadowmodels"),
        Use::Irrelevant("read only when importing a model file"),
    ),
    (
        Name::Exact("associatedmodels"),
        Use::Irrelevant("read only when importing a model file"),
    ),
    (
        Name::Exact("dimensions"),
        Use::Irrelevant("read only when importing a model file"),
    ),
];

/// The placement attributes of a boxed model: `BoxedScreenLocation`.
const BOXED: Table = &[
    (Name::Exact("ScaleX"), Use::Modeled),
    (Name::Exact("ScaleY"), Use::Modeled),
    (Name::Exact("ScaleZ"), Use::Modeled),
    (Name::Exact("RotateX"), Use::Modeled),
    (Name::Exact("RotateY"), Use::Modeled),
    (Name::Exact("RotateZ"), Use::Modeled),
];

/// The placement attributes of a two-point model: `TwoPointScreenLocation`,
/// which takes no scale or rotation.
const TWO_POINT: Table = &[
    (Name::Exact("X2"), Use::Modeled),
    (Name::Exact("Y2"), Use::Modeled),
    (Name::Exact("Z2"), Use::Modeled),
    (
        Name::Exact("ScaleX"),
        Use::Irrelevant("a two-point model is sized by its points"),
    ),
    (
        Name::Exact("ScaleY"),
        Use::Irrelevant("a two-point model is sized by its points"),
    ),
    (
        Name::Exact("ScaleZ"),
        Use::Irrelevant("a two-point model is sized by its points"),
    ),
    (
        Name::Exact("RotateX"),
        Use::Irrelevant("a two-point model is turned by its points"),
    ),
    (
        Name::Exact("RotateY"),
        Use::Irrelevant("a two-point model is turned by its points"),
    ),
    (
        Name::Exact("RotateZ"),
        Use::Irrelevant("a two-point model is turned by its points"),
    ),
];

/// The placement attributes of a three-point model:
/// `ThreePointScreenLocation`.
const THREE_POINT: Table = &[
    (Name::Exact("X2"), Use::Modeled),
    (Name::Exact("Y2"), Use::Modeled),
    (Name::Exact("Z2"), Use::Modeled),
    (Name::Exact("RotateX"), Use::Modeled),
    (Name::Exact("Height"), Use::Modeled),
    (Name::Exact("Angle"), Use::Modeled),
    (
        Name::Exact("ScaleX"),
        Use::Irrelevant("a three-point model is sized by its points"),
    ),
    (
        Name::Exact("ScaleY"),
        Use::Irrelevant("a three-point model is sized by its points"),
    ),
    (
        Name::Exact("ScaleZ"),
        Use::Irrelevant("a three-point model is sized by its points"),
    ),
    (
        Name::Exact("RotateY"),
        Use::Irrelevant("a three-point model is turned by its points"),
    ),
    (
        Name::Exact("RotateZ"),
        Use::Irrelevant("a three-point model is turned by its points"),
    ),
];

/// Attributes of a `ControllerConnection`. xLights:
/// `DeserializeControllerConnection`.
const CONNECTION: Table = &[
    (
        Name::Exact("Protocol"),
        Use::Irrelevant("used by xLights' auto layout, which loading a show does not run"),
    ),
    (
        Name::Exact("Port"),
        Use::Irrelevant("used by xLights' auto layout, which loading a show does not run"),
    ),
    (Name::Exact("ts"), Use::Modeled),
    (
        Name::Exact("channel"),
        Use::Irrelevant("a DMX address xLights uploads to the controller"),
    ),
    (
        Name::Exact("Speed"),
        Use::Irrelevant("a serial speed xLights uploads to the controller"),
    ),
    (
        Name::Exact("LORPortPixels"),
        Use::Irrelevant("uploaded to the controller"),
    ),
    (
        Name::Exact("Brightness"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("brightness"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("gamma"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("nullNodes"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("endNullNodes"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("colorOrder"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("groupCount"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("Reverse"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("reverse"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("ZigZag"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("zigZag"),
        Use::Irrelevant("uploaded to the controller, which applies it"),
    ),
    (
        Name::Exact("SmartRemote"),
        Use::Irrelevant("uploaded to the controller"),
    ),
    (
        Name::Exact("SRMaxCascade"),
        Use::Irrelevant("uploaded to the controller"),
    ),
    (
        Name::Exact("SRCascadeOnPort"),
        Use::Irrelevant("uploaded to the controller"),
    ),
    (
        Name::Exact("SmartRemoteType"),
        Use::Irrelevant("uploaded to the controller"),
    ),
];

/// The node counts that set a model's channels. xLights: `GetNumStrings`,
/// `NodesPerString` and `SetNodeCount`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Strings {
    pub(crate) count: u32,
    /// Nodes on each string, with `ts`, which sets where each string starts.
    pub(crate) nodes_per_string: u32,
    /// Nodes the model has.
    pub(crate) nodes: u32,
}

/// The channels a model takes, after its start channel: its first is
/// `first` channels on, and its last is the one before `end`. xLights:
/// `Model::GetFirstChannel` and `GetLastChannel`, less the start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Span {
    pub(crate) first: u64,
    pub(crate) end: u64,
}

/// The most lights a model may have, and so the most of anything the
/// importer counts or lays out for one: as many pixels as a Donder fixture
/// holds.
pub(crate) const MOST_NODES: u64 = 1_000_000;

/// Refuse a model whose nodes have more lights between them than
/// `MOST_NODES`.
pub(crate) fn lights(nodes: u64, per_node: u64) -> Result<(), SkipReason> {
    if nodes.saturating_mul(per_node) > MOST_NODES {
        return Err(SkipReason::TooManyPixels);
    }
    Ok(())
}

impl Strings {
    /// `count` strings of `nodes_per_string` nodes, the strings spaced by
    /// `ts` times as many.
    pub(crate) fn uniform(count: i32, nodes_per_string: i32, ts: i32) -> Result<Self, SkipReason> {
        let count = u32::try_from(count)
            .map_err(|_| SkipReason::Unplaceable("a negative number of strings"))?;
        let per = u32::try_from(nodes_per_string)
            .map_err(|_| SkipReason::Unplaceable("a negative number of nodes"))?;
        let nodes = u64::from(count) * u64::from(per);
        if nodes > MOST_NODES {
            return Err(SkipReason::TooManyPixels);
        }
        Ok(Self {
            count,
            nodes_per_string: if ts <= 1 {
                per
            } else {
                per.saturating_mul(ts as u32)
            },
            nodes: nodes as u32,
        })
    }

    /// Channels from the start of one string to the next. xLights:
    /// `Model::CalcChannelsPerString`.
    pub(crate) fn channels_per_string(&self, string_type: &StringType) -> u32 {
        if string_type.single_channel {
            1
        } else if string_type.single_node {
            string_type.channels
        } else {
            self.nodes_per_string.saturating_mul(string_type.channels)
        }
    }

    /// The channel the `index`th node of `string` starts at, after the
    /// model's first.
    pub(crate) fn channel(&self, string_type: &StringType, string: u32, index: u32) -> u32 {
        string * self.channels_per_string(string_type) + index * string_type.channels
    }

    /// Channels from the model's first to its last. xLights:
    /// `Model::GetLastChannel` less the first, plus one, for a model whose
    /// strings are each full.
    pub(crate) fn span(&self, string_type: &StringType) -> Span {
        Span {
            first: 0,
            end: self.end(string_type),
        }
    }

    /// One past the last node's last channel: its start, the last string's
    /// start and its nodes before it, plus the channels its node holds.
    fn end(&self, string_type: &StringType) -> u64 {
        if self.count == 0 || self.nodes == 0 {
            return 0;
        }
        let per_string = u64::from(self.channels_per_string(string_type));
        let before = if string_type.single_node {
            0
        } else {
            u64::from(self.nodes / self.count - 1) * u64::from(string_type.channels)
        };
        u64::from(self.count - 1) * per_string + before + u64::from(string_type.node_channels)
    }
}

/// A model of any type, its settings read.
pub(crate) struct Model<'a> {
    pub(crate) common: Common<'a>,
    /// Channels from its first to its last, or why that is unknown.
    pub(crate) span: Result<Span, SkipReason>,
    /// Its nodes, or why it is not imported.
    pub(crate) local: Result<Local, SkipReason>,
}

/// A model element's name, as `ModelManager::LoadModels` reads it: models
/// with an empty one are not loaded.
pub(crate) fn name(element: &Element) -> String {
    read::trim(element.attribute("name").unwrap_or_default()).to_string()
}

/// xLights: `DeserializeCommonModelAttributes`, `DeserializeBaseObjectAttributes`
/// and the parts of `DeserializeControllerConnection` and
/// `DeserializeDimmingCurve` that matter here.
fn common<'a>(reader: &Reader<'a>) -> Common<'a> {
    let superstring = (0..)
        .take_while(|index| reader.has(&format!("SuperStringColour{index}")))
        .count() as u32;
    let string_type = StringType::new(reader.as_string("StringType", "RGB Nodes"), superstring);
    let controller = read::trim(reader.as_string("Controller", ""));
    let connection = reader.element.child("ControllerConnection");
    let connection_reader = connection.map(Reader::new);
    let dimming = dimming(reader);
    Common {
        name: name(reader.element),
        description: reader
            .raw("Description")
            .filter(|text| !text.trim().is_empty())
            .map(str::to_string),
        active: reader.as_int("Active", 1) != 0,
        shadow_of: reader
            .raw("ShadowModelFor")
            .filter(|text| !text.is_empty())
            .map(str::to_string),
        layout_group: reader.as_string("LayoutGroup", "Unassigned"),
        start_channel: reader.as_string("StartChannel", "1"),
        controller: if controller == "Use Start Channel" {
            String::new()
        } else {
            controller.to_string()
        },
        string_type,
        left_to_right: !reader.equals("Dir", "L", "R"),
        bottom_to_top: reader.raw("StartSide").is_none_or(|side| side == "B"),
        ts: connection_reader.map_or(0, |connection| connection.as_int("ts", 0)),
        dimming,
    }
}

/// The model's dimming curve, when it is one brightness and gamma for all
/// its colors. xLights: `ModelBrightness` (`DeserializeCommonModelAttributes`),
/// replaced by a `dimmingCurve` element (`DeserializeDimmingCurve`), made
/// into a curve by `DimmingCurve::createFromInfo`.
fn dimming(reader: &Reader) -> Result<Option<Dimming>, SkipReason> {
    /// xLights: `BasicDimmingCurve`: `(brightness + 100)% * x^gamma`,
    /// gamma at most 50, and no curve when that is the identity.
    fn curve(brightness: i32, gamma: f32) -> Option<Dimming> {
        let gamma = gamma.min(50.0);
        (brightness != 0 || gamma != 1.0).then(|| Dimming {
            brightness: (brightness as f32 + 100.0) / 100.0,
            gamma,
        })
    }
    if let Some(element) = reader.element.child("dimmingCurve") {
        let refuse = || SkipReason::Setting("dimming curve other than one brightness and gamma");
        // xLights uses `all` over any color's own curve; one beside it is
        // refused all the same, as a curve meant for each color.
        if element.children.iter().any(|child| child.name != "all") {
            return Err(refuse());
        }
        let Some(all) = element.child("all") else {
            return Ok(None);
        };
        let all = Reader::new(all);
        if all.has("filename") {
            return Err(refuse());
        }
        let validate = |text: &'_ str, default: &'static str| {
            if text.is_empty() { default } else { text }.to_string()
        };
        let brightness = read::strtol(&validate(all.as_string("brightness", "100"), "0")) as i32;
        let gamma = read::strtod(&validate(all.as_string("gamma", "1.0"), "1.0"))
            .ok_or_else(|| all.invalid("gamma"))? as f32;
        return Ok(curve(brightness, gamma));
    }
    if reader.has("ModelBrightness") {
        let text = reader.as_string("ModelBrightness", "0");
        let brightness = read::strtol(if text.is_empty() { "0" } else { text }) as i32;
        if brightness != 0 {
            return Ok(curve(brightness, 1.0));
        }
    }
    Ok(None)
}

/// Refusals every model shares, before its type's own.
fn refused(reader: &Reader, common: &Common) -> Result<(), SkipReason> {
    if reader.as_int("Advanced", 0) != 0 {
        return Err(SkipReason::Setting("individual string start channels"));
    }
    // xLights: Model::SetRGBWHandling; only RGBW nodes are affected.
    if common.string_type.channels == 4
        && ["RGB Only", "White Only", "Advanced", "White On All"]
            .contains(&reader.as_string("RGBWHandling", ""))
    {
        return Err(SkipReason::Setting(
            "RGBW handling other than R=G=B to white",
        ));
    }
    common.dimming.clone()?;
    Ok(())
}

/// Read a model element of type `kind`.
pub(crate) fn read(element: &Element, kind: ModelType) -> Model<'_> {
    let reader = Reader::new(element);
    let common = common(&reader);
    let (span, local) = match kind {
        ModelType::SingleLine => lines::single_line(&reader, &common),
        ModelType::PolyLine => polyline::poly_line(&reader, &common),
        ModelType::Arches => arches::arches(&reader, &common),
        ModelType::Circle => circles::circle(&reader, &common),
        ModelType::Wreath => circles::wreath(&reader, &common),
        ModelType::Star => star::star(&reader, &common),
        ModelType::Matrix => matrices::matrix(&reader, &common),
        ModelType::Tree => matrices::tree(&reader, &common),
        ModelType::Sphere => matrices::sphere(&reader, &common),
        ModelType::Icicles => icicles::icicles(&reader, &common),
        ModelType::WindowFrame => window::window_frame(&reader, &common),
        ModelType::Custom => custom::custom(&reader, &common),
        ModelType::MultiPoint => multipoint::multi_point(&reader, &common),
        ModelType::Spinner => spinner::spinner(&reader, &common),
        ModelType::CandyCanes => canes::candy_canes(&reader, &common),
        other => (
            unsupported_span(&reader, &common, other),
            Err(SkipReason::Type(other.name())),
        ),
    };
    // xLights spaces strings by ts inconsistently: with it, some models'
    // channels start before their start channel.
    let (span, local) = if common.ts > 1 {
        let strided = matches!(
            kind,
            ModelType::SingleLine
                | ModelType::Matrix
                | ModelType::Tree
                | ModelType::Sphere
                | ModelType::Circle
                | ModelType::Wreath
                | ModelType::Star
        );
        (
            if strided {
                Err(SkipReason::UnknownSpan)
            } else {
                span
            },
            Err(SkipReason::Setting("smart receiver strings per port (ts)")),
        )
    } else {
        (span, local)
    };
    let local = local.and_then(|local| {
        // xLights places some lights at no number, such as a sphere one
        // light tall or a short spiral tree's.
        if local
            .nodes
            .iter()
            .flat_map(|node| &node.lights)
            .any(|light| !light.is_finite())
        {
            return Err(SkipReason::Unplaceable(
                "a light xLights places at no number",
            ));
        }
        refused(&reader, &common)?;
        if common.string_type.encoding.is_none() {
            return Err(SkipReason::StringType(
                reader.as_string("StringType", "RGB Nodes").to_string(),
            ));
        }
        Ok(local)
    });
    Model {
        common,
        span,
        local,
    }
}

/// The span of a type that is not imported, from its settings. xLights:
/// each type's `InitModel` node count and channels.
fn unsupported_span(reader: &Reader, common: &Common, kind: ModelType) -> Result<Span, SkipReason> {
    let end = |count: i32, channels: u32| -> Result<Span, SkipReason> {
        let count = u64::try_from(count).map_err(|_| SkipReason::UnknownSpan)?;
        Ok(Span {
            first: 0,
            end: count * u64::from(channels),
        })
    };
    match kind {
        // xLights: DmxModel::InitModel, a node a channel. It makes its
        // string type single color white after Setup has decided, from the
        // model's own string type, whether its nodes hold a whole string;
        // when they do not, each is a four channel RGBW node.
        kind if kind.is_dmx() => {
            let count = reader.parm("DmxChannelCount", "parm1", "1");
            let node = if common.string_type.single_node { 1 } else { 4 };
            u64::try_from(count)
                .ok()
                .filter(|count| *count > 0)
                .map(|count| Span {
                    first: 0,
                    end: count - 1 + node,
                })
                .ok_or(SkipReason::UnknownSpan)
        }
        // xLights: ChannelBlockModel, one single channel string a channel.
        ModelType::ChannelBlock => end(reader.parm("NumChannels", "parm1", "1"), 1),
        // xLights: ImageModel and LabelModel, one node.
        ModelType::Image | ModelType::Label => end(1, common.string_type.channels),
        _ => Err(SkipReason::UnknownSpan),
    }
}

/// Check that a type's tables account for every attribute and child.
pub(crate) fn account(
    reader: &Reader,
    placement: Table,
    own: &[Table],
    children: Table,
) -> Result<(), SkipReason> {
    let attributes: Vec<Table> = [COMMON, placement]
        .into_iter()
        .chain(own.iter().copied())
        .collect();
    read::unhandled(reader.element, &attributes, &[CHILDREN, children])?;
    if let Some(connection) = reader.element.child("ControllerConnection") {
        read::unhandled(connection, &[CONNECTION], &[])?;
    }
    Ok(())
}

pub(crate) const NO_CHILDREN: Table = &[];
pub(crate) const BOXED_PLACEMENT: Table = BOXED;
pub(crate) const TWO_POINT_PLACEMENT: Table = TWO_POINT;
pub(crate) const THREE_POINT_PLACEMENT: Table = THREE_POINT;
