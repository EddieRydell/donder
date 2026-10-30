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
    /// Controllers that are not imported, by name, with their protocol.
    pub skipped_controllers: Vec<(String, String)>,
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
    Type(String),
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
    /// Placed by an xLights from before 2019, whose layout positions current
    /// xLights no longer reads.
    OldLayout,
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
    UnknownUniverse(String),
    UnknownController(String),
    /// Some channels are not on any controller output.
    OutsideOutputs,
    /// Some channels are on a controller whose protocol Donder does not send.
    UnsupportedController(String),
    /// A pixel's channels continue from one output into the next.
    SplitPixel,
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
            Self::StringType(kind) => {
                write!(f, "its lights are {kind}, not RGB or RGBW pixels")
            }
            Self::Setting(setting) => write!(f, "its {setting} setting is not imported yet"),
            Self::Attribute { name, value } => write!(f, "its {name} is `{value}`"),
            Self::NoPixels => write!(f, "it has no pixels"),
            Self::TooManyPixels => write!(f, "it has more pixels than a Donder fixture holds"),
            Self::Unplaceable(what) => write!(f, "{what}, which leaves its pixels no place"),
            Self::OldLayout => write!(
                f,
                "it was placed by an xLights from before 2019; move it in a current xLights and save"
            ),
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
                // Through a chain of unpatched models, to the one that starts it.
                let (mut first, mut cause) = (model, reason.as_ref());
                while let Self::Follows { model, reason } = cause {
                    (first, cause) = (model, reason.as_ref());
                }
                if first == model {
                    write!(
                        f,
                        "its start channel follows {model}, which is not patched: {cause}"
                    )
                } else {
                    write!(
                        f,
                        "its start channel follows {model}, which follows {first}, which is not patched: {cause}"
                    )
                }
            }
            Self::UnknownSpan(model) => write!(
                f,
                "its start channel follows {model}, whose number of channels is unknown"
            ),
            Self::OutOfRange(text) => write!(f, "its start channel `{text}` is out of range"),
            Self::ChainLoop => write!(f, "its start channel follows itself"),
            Self::UnknownUniverse(text) => {
                write!(f, "no controller sends the universe of `{text}`")
            }
            Self::UnknownController(name) => write!(f, "there is no controller {name}"),
            Self::OutsideOutputs => write!(f, "some of its channels are on no controller"),
            Self::UnsupportedController(protocol) => {
                write!(f, "some of its channels are on a {protocol} controller")
            }
            Self::SplitPixel => write!(f, "a pixel's channels run on into the next universe"),
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
        for (name, protocol) in &self.skipped_controllers {
            writeln!(
                f,
                "Left out controller {name}: Donder does not send {protocol}."
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
