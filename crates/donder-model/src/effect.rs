pub(crate) use crate::identity::{CurveId, EffectDefinitionId, GradientId};
use crate::layout::FixtureTarget;
use crate::sequence::{AssetId, MarkCollectionKey, SequenceLayerId};
use donder_language::compiler::{CompiledDefinition, ParamDecl, ParamRange};
use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;
use donder_runtime_types::Type;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: ClipId,
    /// Unique among the sequence's clips; automation bindings use it.
    pub name: Identifier,
    pub description: Option<String>,
    pub layer_id: SequenceLayerId,
    pub start: DonderTime,
    pub duration: DonderDuration,
    pub target: FixtureTarget,
    pub scope: ClipScope,
    pub definition: EffectDefinitionId,
    pub param_overrides: IndexMap<Identifier, ParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ClipId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ClipScope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(donder_runtime_types::Color),
    /// An enum option and its fields' values, every field of the option in
    /// declaration order; a plain option has none.
    Enum {
        option: Identifier,
        fields: IndexMap<Identifier, ParamValue>,
    },
    /// `None` is no collection: an empty set of marks, which never triggers.
    Marks(Option<MarkCollectionKey>),
    Curve(OwnedCurve),
    Gradient(OwnedGradient),
    Array(Vec<ParamValue>),
    /// At most [`donder_runtime_types::TEXT_LENGTH`] characters, none NUL.
    Text(String),
    /// `None` is no font: every character is blank.
    Font(Option<FontFace>),
    /// `None` is no image: every point is black.
    Image(Option<ImageSource>),
}

/// A font file drawn at a line height in pixels.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct FontFace {
    pub file: AssetId,
    pub height: u32,
}

impl FontFace {
    /// The tallest line a font is drawn at.
    pub const MAX_HEIGHT: u32 = 256;
}

/// An image file, a picture or an animation, scaled so its longer side is
/// at most `size` pixels.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ImageSource {
    pub file: AssetId,
    pub size: u32,
}

impl ImageSource {
    /// The longest side an image is scaled to.
    pub const MAX_SIZE: u32 = 256;
}

impl ParamValue {
    /// The asset files the value names.
    pub fn assets(&self) -> Vec<&AssetId> {
        match self {
            Self::Font(Some(face)) => vec![&face.file],
            Self::Image(Some(image)) => vec![&image.file],
            Self::Array(values) => values.iter().flat_map(Self::assets).collect(),
            Self::Enum { fields, .. } => fields.values().flat_map(Self::assets).collect(),
            _ => Vec::new(),
        }
    }

    /// Initial authored values must be valid resources inside the declared
    /// range, unlike the VM's empty storage defaults. Numbers start at zero
    /// held inside the range, a curve falls across it (from 1 to 0, as curves
    /// are stored normalized), and an enum starts at
    /// its first option. The caller supplies the user-facing initial color.
    pub fn initial_for_param(
        param: &ParamDecl,
        color: donder_runtime_types::Color,
    ) -> Option<Self> {
        match &param.ty {
            Type::Enum(options) => Some(Self::option(param, options.first()?.clone())),
            ty => Self::initial_for_type(ty, param.range, color),
        }
    }

    /// Option `option` of the enum parameter `param`, its fields at their
    /// defaults.
    pub fn option(param: &ParamDecl, option: Identifier) -> Self {
        let fields = param
            .fields_of(&option)
            .iter()
            .map(|field| {
                let value = field
                    .default
                    .as_ref()
                    .and_then(Self::from_default)
                    .unwrap_or_else(|| unreachable!("every field has a plain default"));
                (field.name.clone(), value)
            })
            .collect();
        Self::Enum { option, fields }
    }

    /// A declared default as an authored value: an int, float, bool, color
    /// or plain enum option, the defaults a field may have.
    pub fn from_default(value: &donder_runtime_types::Value) -> Option<Self> {
        use donder_runtime_types::Value;
        Some(match value {
            Value::Int(value) => Self::Int(*value),
            Value::Float(value) => Self::Float(*value),
            Value::Bool(value) => Self::Bool(*value),
            Value::Color(value) => Self::Color(*value),
            Value::Enum(option) => Self::Enum {
                option: option.clone(),
                fields: IndexMap::new(),
            },
            _ => return None,
        })
    }

    fn initial_for_type(
        ty: &Type,
        range: Option<ParamRange>,
        color: donder_runtime_types::Color,
    ) -> Option<Self> {
        let (min, max) = match range {
            Some(ParamRange::Float { min, max }) => (min, max),
            Some(ParamRange::Int { .. }) | None => (0.0, 1.0),
        };
        match ty {
            Type::Int => Some(Self::Int(match range {
                Some(ParamRange::Int { min, max }) => 0.clamp(min, max),
                _ => 0,
            })),
            Type::Float => Some(Self::Float(0.0_f32.clamp(min, max))),
            Type::Bool => Some(Self::Bool(false)),
            Type::Color => Some(Self::Color(color)),
            Type::Curve => Some(Self::Curve(OwnedCurve::Inline(Curve {
                points: vec![
                    donder_runtime_types::CurvePoint {
                        position: 0.0,
                        value: 1.0,
                    },
                    donder_runtime_types::CurvePoint {
                        position: 1.0,
                        value: 0.0,
                    },
                ],
            }))),
            Type::Gradient => Some(Self::Gradient(OwnedGradient::Inline(Gradient {
                space: donder_runtime_types::GradientSpace::LinearRgb,
                stops: vec![donder_runtime_types::GradientStop {
                    position: 0.0,
                    color,
                }],
            }))),
            Type::Array(element) => Some(Self::Array(vec![Self::initial_for_type(
                element, range, color,
            )?])),
            Type::Enum(options) => options.first().map(|option| Self::Enum {
                option: option.clone(),
                fields: IndexMap::new(),
            }),
            Type::Marks => Some(Self::Marks(None)),
            Type::Text => Some(Self::Text(String::new())),
            Type::Font => Some(Self::Font(None)),
            Type::Image => Some(Self::Image(None)),
            // Audio samples are bound by preparation, never authored.
            Type::Void | Type::Track => None,
        }
    }
}

pub type OwnedCurve = crate::ownership::Owned<Curve, CurveId>;
pub type OwnedGradient = crate::ownership::Owned<Gradient, GradientId>;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDefinition {
    pub(crate) id: EffectDefinitionId,
    compiled: CompiledDefinition,
}

impl EffectDefinition {
    pub fn new(id: EffectDefinitionId, compiled: CompiledDefinition) -> Self {
        Self { id, compiled }
    }
    pub(crate) fn id(&self) -> &EffectDefinitionId {
        &self.id
    }
    pub fn name(&self) -> &str {
        self.compiled.name().as_str()
    }
    pub fn params(&self) -> &[ParamDecl] {
        self.compiled.params()
    }
    pub(crate) fn compiled(&self) -> &CompiledDefinition {
        &self.compiled
    }
    pub fn description(&self) -> Option<&str> {
        self.compiled.description()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CurveDefinition {
    pub description: Option<String>,
    pub curve: Curve,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GradientDefinition {
    pub description: Option<String>,
    pub gradient: Gradient,
}
