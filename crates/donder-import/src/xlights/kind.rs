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
    const ALL: [Self; 28] = [
        Self::Arches,
        Self::CandyCanes,
        Self::ChannelBlock,
        Self::Circle,
        Self::Cube,
        Self::Custom,
        Self::DmxMovingHead,
        Self::DmxMovingHeadAdv,
        Self::DmxFloodArea,
        Self::DmxFloodlight,
        Self::DmxGeneral,
        Self::DmxServo,
        Self::DmxServo3d,
        Self::DmxSkull,
        Self::Icicles,
        Self::Image,
        Self::Label,
        Self::Matrix,
        Self::ModelGroup,
        Self::MultiPoint,
        Self::SingleLine,
        Self::PolyLine,
        Self::Sphere,
        Self::Spinner,
        Self::Star,
        Self::Tree,
        Self::WindowFrame,
        Self::Wreath,
    ];

    /// The types xLights loads from any `DisplayAs` holding their name, in
    /// the order it tests them.
    const NAMED_WITHIN: [Self; 3] = [Self::Matrix, Self::MultiPoint, Self::Tree];

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
            "DmxServo3Axis" => "DmxServo3d",
            other => other,
        };
        Self::ALL
            .into_iter()
            .find(|kind| kind.name() == migrated)
            .or_else(|| {
                Self::NAMED_WITHIN
                    .into_iter()
                    .find(|kind| migrated.contains(kind.name()))
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
