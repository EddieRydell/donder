//! The model types xLights loads, by `DisplayAs`.

/// xLights: the type dispatch of `XmlDeserializingModelFactory::Deserialize`
/// after `MigrateLegacyModelNode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelType {
    Arches,
    CandyCanes,
    ChannelBlock,
    Circle,
    Cube,
    Custom,
    DmxMovingHead,
    DmxMovingHeadAdv,
    DmxFloodArea,
    DmxFloodlight,
    DmxGeneral,
    DmxServo,
    DmxServo3d,
    DmxSkull,
    Icicles,
    Image,
    Label,
    /// `Matrix`, `Horiz Matrix`, `Vert Matrix`: any type naming Matrix.
    Matrix,
    ModelGroup,
    /// Any type naming MultiPoint.
    MultiPoint,
    SingleLine,
    PolyLine,
    Sphere,
    Spinner,
    Star,
    /// `Tree` and the older `Tree 360`, `Tree Flat` and so on: any type
    /// naming Tree.
    Tree,
    WindowFrame,
    Wreath,
}

impl ModelType {
    /// The type of a `DisplayAs`, or none for one xLights does not load.
    /// xLights: `XmlDeserializingModelFactory::Deserialize`, which also
    /// rewrites `DmxMovingHead3D` and `DMX` before dispatching
    /// (`MigrateLegacyModelNode`).
    pub(crate) fn from_display(display: &str, dmx_style: &str) -> Option<Self> {
        let migrated = match display {
            "DmxMovingHead3D" => "DmxMovingHeadAdv",
            "DMX" => match dmx_style {
                "Flood Light" => "DmxFloodlight",
                "Skulltronix Skull" => "DmxSkull",
                _ => "DmxMovingHead",
            },
            other => other,
        };
        Some(match migrated {
            "Arches" => Self::Arches,
            "Candy Canes" => Self::CandyCanes,
            "Channel Block" => Self::ChannelBlock,
            "Circle" => Self::Circle,
            "Cube" => Self::Cube,
            "Custom" => Self::Custom,
            "DmxMovingHead" => Self::DmxMovingHead,
            "DmxMovingHeadAdv" => Self::DmxMovingHeadAdv,
            "DmxFloodArea" => Self::DmxFloodArea,
            "DmxFloodlight" => Self::DmxFloodlight,
            "DmxGeneral" => Self::DmxGeneral,
            "DmxServo" => Self::DmxServo,
            "DmxServo3d" | "DmxServo3Axis" => Self::DmxServo3d,
            "DmxSkull" => Self::DmxSkull,
            "Icicles" => Self::Icicles,
            "Image" => Self::Image,
            "Label" => Self::Label,
            text if text.contains("Matrix") => Self::Matrix,
            "ModelGroup" => Self::ModelGroup,
            text if text.contains("MultiPoint") => Self::MultiPoint,
            "Single Line" => Self::SingleLine,
            "Poly Line" => Self::PolyLine,
            "Sphere" => Self::Sphere,
            "Spinner" => Self::Spinner,
            "Star" => Self::Star,
            text if text.contains("Tree") => Self::Tree,
            "Window Frame" => Self::WindowFrame,
            "Wreath" => Self::Wreath,
            _ => return None,
        })
    }

    /// The type's name, as xLights writes it.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Arches => "Arches",
            Self::CandyCanes => "Candy Canes",
            Self::ChannelBlock => "Channel Block",
            Self::Circle => "Circle",
            Self::Cube => "Cube",
            Self::Custom => "Custom",
            Self::DmxMovingHead => "DmxMovingHead",
            Self::DmxMovingHeadAdv => "DmxMovingHeadAdv",
            Self::DmxFloodArea => "DmxFloodArea",
            Self::DmxFloodlight => "DmxFloodlight",
            Self::DmxGeneral => "DmxGeneral",
            Self::DmxServo => "DmxServo",
            Self::DmxServo3d => "DmxServo3d",
            Self::DmxSkull => "DmxSkull",
            Self::Icicles => "Icicles",
            Self::Image => "Image",
            Self::Label => "Label",
            Self::Matrix => "Matrix",
            Self::ModelGroup => "ModelGroup",
            Self::MultiPoint => "MultiPoint",
            Self::SingleLine => "Single Line",
            Self::PolyLine => "Poly Line",
            Self::Sphere => "Sphere",
            Self::Spinner => "Spinner",
            Self::Star => "Star",
            Self::Tree => "Tree",
            Self::WindowFrame => "Window Frame",
            Self::Wreath => "Wreath",
        }
    }

    pub(crate) fn is_dmx(self) -> bool {
        matches!(
            self,
            Self::DmxMovingHead
                | Self::DmxMovingHeadAdv
                | Self::DmxFloodArea
                | Self::DmxFloodlight
                | Self::DmxGeneral
                | Self::DmxServo
                | Self::DmxServo3d
                | Self::DmxSkull
        )
    }
}
