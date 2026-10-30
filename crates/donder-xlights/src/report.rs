use std::fmt;

/// Real pixels on a string sit this far apart, 3 inches: the scale an
/// xLights layout takes when it has no ruler.
pub const TYPICAL_PIXEL_SPACING: f64 = 0.075;

#[derive(Clone, Debug, Default)]
pub struct ImportOptions {
    /// How far apart pixels are in meters, which sets the layout's size; the
    /// layout's ruler sets it when this is none, and [`TYPICAL_PIXEL_SPACING`]
    /// when the layout has no ruler.
    pub pixel_spacing: Option<f64>,
}

/// How the layout's size was found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Scale {
    /// The layout's ruler.
    Ruler,
    /// The typical distance between neighbouring pixels was taken to be this
    /// many meters.
    PixelSpacing(f64),
}

#[derive(Clone, Debug, Default)]
pub struct ImportReport {
    pub fixtures: usize,
    pub groups: usize,
    pub controllers: usize,
    pub routes: usize,
    pub pixels: usize,
    /// Meters for one unit of the xLights layout, and where that came from.
    pub meters_per_unit: f64,
    pub scale: Option<Scale>,
    /// Models that are not in the layout.
    pub skipped: Vec<Skipped>,
    /// Models in the layout that are not patched, or not completely.
    pub unrouted: Vec<Unrouted>,
    /// Controllers that are not imported.
    pub skipped_controllers: Vec<SkippedController>,
    /// Group members left out, by group.
    pub dropped_members: Vec<DroppedMember>,
}

#[derive(Clone, Debug)]
pub struct Skipped {
    pub model: String,
    pub reason: SkipReason,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SkipReason {
    Inactive,
    /// A second model of lights another model already describes.
    Shadow,
    /// A model type Donder has no pixels for, or that is not imported yet.
    Type(&'static str),
    /// A `DisplayAs` xLights does not load.
    UnknownType(String),
    /// An attribute the importer does not account for, which may change
    /// where the model's lights are or which channels they take.
    UnhandledAttribute(String),
    /// A child element the importer does not account for.
    UnhandledElement(String),
    /// In another of xLights' previews, whose coordinates are not the
    /// default preview's.
    OtherPreview(String),
    /// The model's channel span cannot be computed from its settings.
    UnknownSpan,
    /// Another model has the same name; xLights keeps whichever it loads
    /// last, loading them in parallel.
    DuplicateName,
    /// A string type whose lights are not individual RGB or RGBW pixels.
    StringType(String),
    /// A setting of this model type that is not imported yet.
    Setting(&'static str),
    Attribute {
        name: String,
        value: String,
    },
    NoPixels,
    TooManyPixels,
    /// A model xLights itself cannot lay out, as this describes it.
    Unplaceable(&'static str),
    /// The model's channels are already patched to the named model.
    SharesChannels(String),
}

#[derive(Clone, Debug)]
pub struct Unrouted {
    pub model: String,
    pub reason: UnroutedReason,
}

#[derive(Clone, Debug, PartialEq)]
pub enum UnroutedReason {
    StartChannel(String),
    /// The start channel counts from a model that is not in the layout.
    UnknownModel(String),
    /// The start channel counts from a model that is not patched, for its
    /// own reason.
    Follows {
        model: String,
        reason: Box<UnroutedReason>,
    },
    /// The start channel counts on from a model whose channel count is
    /// unknown.
    UnknownSpan(String),
    /// The start channel is past the last channel xLights can number.
    OutOfRange(String),
    /// The start channel counts from itself, through other models.
    ChainLoop,
    /// The start channel counts from a model xLights resolves after it,
    /// so xLights cannot compute it.
    ChainOrder(String),
    /// The model, or the one its start channel follows with `>`, is on
    /// "No Controller".
    NoController(String),
    /// Another model has the same name.
    Duplicate(String),
    /// The start channel counts from a group, which xLights cannot count
    /// from.
    FollowsGroup(String),
    UnknownUniverse(String),
    UnknownController(String),
    /// Some channels are not on any controller output.
    OutsideOutputs,
    /// Some channels are on a controller that is not imported, by name.
    UnsupportedController(String),
    /// A pixel's channels continue from one output into the next.
    SplitPixel,
}

#[derive(Clone, Debug)]
pub struct SkippedController {
    pub controller: String,
    pub reason: ControllerSkip,
}

/// Why a controller is not created in Donder. Its outputs still take their
/// channels, as they do in xLights.
#[derive(Clone, Debug, PartialEq)]
pub enum ControllerSkip {
    /// It sends a protocol Donder has no output for, by xLights' name.
    Protocol(&'static str),
    /// Its protocol is not one xLights sends, as written.
    UnknownProtocol(String),
    /// A serial controller.
    Serial,
    /// xLights' NULL controller, which sends nothing.
    Null,
    /// It only plays sequences in xLights' player.
    PlayerOnly,
    Inactive,
    /// Its address is not an IP address.
    Address(String),
    /// An E1.31 priority Donder does not send.
    Priority(i32),
    /// A universe Donder cannot address.
    Universe(i32),
    /// Two of its outputs send the same universe.
    RepeatedUniverse(i32),
    /// Its outputs send another protocol than the controller.
    MixedOutputs,
    /// An output with no channels.
    EmptyOutput,
}

#[derive(Clone, Debug)]
pub struct DroppedMember {
    pub group: String,
    pub member: String,
    pub reason: DropReason,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DropReason {
    /// Donder groups hold whole fixtures, not part of one.
    Submodel,
    /// Not a model or group of the layout, or one that was not imported.
    Missing,
    /// The group would contain itself.
    Loop,
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inactive => write!(f, "it is not active"),
            Self::Shadow => write!(f, "it is a shadow of another model"),
            Self::Type(kind) => write!(f, "{kind} models are not imported"),
            Self::UnknownType(kind) => write!(f, "xLights does not load `{kind}` models"),
            Self::UnhandledAttribute(name) => {
                write!(f, "its {name} setting is not understood by the importer")
            }
            Self::UnhandledElement(name) => {
                write!(f, "its {name} element is not understood by the importer")
            }
            Self::OtherPreview(name) => {
                write!(f, "it is in the {name} preview, not the default one")
            }
            Self::UnknownSpan => write!(f, "its number of channels is unknown"),
            Self::DuplicateName => write!(
                f,
                "another model has the same name, and xLights keeps either at random"
            ),
            Self::StringType(kind) => {
                write!(f, "its lights are {kind}, not RGB or RGBW pixels")
            }
            Self::Setting(setting) => write!(f, "the importer does not import its {setting}"),
            Self::Attribute { name, value } => write!(f, "its {name} is `{value}`"),
            Self::NoPixels => write!(f, "it has no pixels"),
            Self::TooManyPixels => write!(f, "it has more pixels than a Donder fixture holds"),
            Self::Unplaceable(what) => write!(f, "{what}, which leaves its pixels no place"),
            Self::SharesChannels(other) => {
                write!(f, "its channels are already patched to {other}")
            }
        }
    }
}

impl fmt::Display for UnroutedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StartChannel(text) => write!(f, "its start channel `{text}` is not understood"),
            Self::UnknownModel(model) => {
                write!(
                    f,
                    "its start channel follows {model}, which is not a model of the layout"
                )
            }
            Self::Follows { model, reason } => {
                // Every link of a chain of unpatched models, to the one that
                // starts it.
                write!(f, "its start channel follows {model}")?;
                let mut cause = reason.as_ref();
                while let Self::Follows { model, reason } = cause {
                    write!(f, ", which follows {model}")?;
                    cause = reason.as_ref();
                }
                write!(f, ", which is not patched: {cause}")
            }
            Self::UnknownSpan(model) => write!(
                f,
                "its start channel follows {model}, whose number of channels is unknown"
            ),
            Self::OutOfRange(text) => write!(f, "its start channel `{text}` is out of range"),
            Self::ChainLoop => write!(f, "its start channel follows itself"),
            Self::ChainOrder(model) => write!(
                f,
                "its start channel counts from {model}, which xLights resolves after it"
            ),
            Self::NoController(model) => write!(f, "{model} is on No Controller"),
            Self::Duplicate(model) => write!(f, "another model is also named {model}"),
            Self::FollowsGroup(group) => write!(
                f,
                "its start channel follows the group {group}, which xLights cannot count from"
            ),
            Self::UnknownUniverse(text) => {
                write!(f, "no controller sends the universe of `{text}`")
            }
            Self::UnknownController(name) => write!(f, "there is no controller {name}"),
            Self::OutsideOutputs => write!(f, "some of its channels are on no controller"),
            Self::UnsupportedController(controller) => write!(
                f,
                "some of its channels are on controller {controller}, which is not imported"
            ),
            Self::SplitPixel => write!(f, "a pixel's channels run on into the next universe"),
        }
    }
}

impl fmt::Display for ControllerSkip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Protocol(protocol) => write!(f, "Donder has no {protocol} output"),
            Self::UnknownProtocol(protocol) => {
                write!(f, "its protocol `{protocol}` is not one xLights sends")
            }
            Self::Serial => write!(f, "Donder has no serial outputs"),
            Self::Null => write!(f, "it is xLights' NULL controller, which sends nothing"),
            Self::PlayerOnly => write!(f, "it only plays in xLights' player"),
            Self::Inactive => write!(f, "it is not active"),
            Self::Address(address) => write!(f, "its address `{address}` is not an IP address"),
            Self::Priority(priority) => write!(f, "its E1.31 priority is {priority}"),
            Self::Universe(universe) => write!(f, "universe {universe} is out of range"),
            Self::RepeatedUniverse(universe) => write!(f, "it sends universe {universe} twice"),
            Self::MixedOutputs => write!(f, "its outputs send another protocol than it does"),
            Self::EmptyOutput => write!(f, "one of its outputs has no channels"),
        }
    }
}

impl fmt::Display for DropReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Submodel => write!(f, "Donder groups hold whole fixtures, not submodels"),
            Self::Missing => write!(f, "it is not an imported model or group"),
            Self::Loop => write!(f, "the group would contain itself"),
        }
    }
}

impl fmt::Display for ImportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Imported {} fixtures ({} pixels), {} groups, {} controllers and {} routes.",
            self.fixtures, self.pixels, self.groups, self.controllers, self.routes
        )?;
        match self.scale {
            Some(Scale::Ruler) => writeln!(
                f,
                "The layout's ruler sets its size: {:.4} m to a layout unit.",
                self.meters_per_unit
            )?,
            Some(Scale::PixelSpacing(spacing)) => writeln!(
                f,
                "Neighbouring pixels are taken to be {:.1} cm apart: {:.4} m to a layout unit.",
                spacing * 100.0,
                self.meters_per_unit
            )?,
            None => {}
        }
        for skipped in &self.skipped {
            writeln!(f, "Left out {}: {}.", skipped.model, skipped.reason)?;
        }
        for unrouted in &self.unrouted {
            writeln!(f, "Not patched {}: {}.", unrouted.model, unrouted.reason)?;
        }
        for skipped in &self.skipped_controllers {
            writeln!(
                f,
                "Left out controller {}: {}.",
                skipped.controller, skipped.reason
            )?;
        }
        for dropped in &self.dropped_members {
            writeln!(
                f,
                "Left {} out of group {}: {}.",
                dropped.member, dropped.group, dropped.reason
            )?;
        }
        Ok(())
    }
}
