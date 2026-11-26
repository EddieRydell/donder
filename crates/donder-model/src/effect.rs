use crate::composition::{LayerId, MarkCollectionId};
use crate::console::PlayedValue;
pub(crate) use crate::identity::{CurveId, GradientId, ScriptId};
use crate::sequence::AssetId;
use crate::sequence::Envelope;
use donder_language::compiler::{
    CompiledDefinition, DeclarationKind, OperatorInputDecl, ParamDecl, ParamRange,
};
use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;
use donder_runtime_types::TargetScope;
use donder_runtime_types::Type;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;

/// What a sequence or console plays over a fixture or group of the layout:
/// an effect on one of its layers, or an envelope that drives parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Clip {
    pub id: ClipId,
    /// Unique among its composition's clips; envelopes and actions name it.
    pub name: Identifier,
    pub description: Option<String>,
    /// What the clip plays over, each target on its own in the effect's
    /// scope; an envelope's rows. Never empty, and never repeats a target.
    pub targets: Vec<crate::layout::FixtureId>,
    pub placement: Placement,
    pub content: ClipContent,
}

/// What a clip plays.
#[derive(Clone, Debug, PartialEq)]
pub enum ClipContent {
    Effect(Effect),
    Envelope(Envelope),
}

/// An effect played on a layer.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub layer: LayerId,
    pub scope: TargetScope,
    pub definition: ScriptId,
    /// Overrides of the effect's parameters; the rest follow its defaults.
    pub params: IndexMap<Identifier, ParamValue>,
}

/// When a clip plays.
#[derive(Clone, Debug, PartialEq)]
pub enum Placement {
    /// From `start` for `duration` on the sequence's or session's clock.
    Timed {
        start: DonderTime,
        duration: DonderDuration,
    },
    /// When a console's actions play it, each copy taking these parameters
    /// from the press that started it.
    Played(Vec<PlayedValue>),
}

impl Clip {
    /// Its start and duration, when it is timed.
    pub fn timing(&self) -> Option<(&DonderTime, &DonderDuration)> {
        match &self.placement {
            Placement::Timed { start, duration } => Some((start, duration)),
            Placement::Played(_) => None,
        }
    }

    /// The parameters each copy takes from its press; none for a timed clip.
    pub fn played(&self) -> &[PlayedValue] {
        match &self.placement {
            Placement::Played(values) => values,
            Placement::Timed { .. } => &[],
        }
    }

    /// The effect it plays, when it is not an envelope.
    pub fn effect(&self) -> Option<&Effect> {
        match &self.content {
            ClipContent::Effect(effect) => Some(effect),
            ClipContent::Envelope(_) => None,
        }
    }

    pub fn effect_mut(&mut self) -> Option<&mut Effect> {
        match &mut self.content {
            ClipContent::Effect(effect) => Some(effect),
            ClipContent::Envelope(_) => None,
        }
    }

    pub fn envelope(&self) -> Option<&Envelope> {
        match &self.content {
            ClipContent::Envelope(envelope) => Some(envelope),
            ClipContent::Effect(_) => None,
        }
    }

    pub fn envelope_mut(&mut self) -> Option<&mut Envelope> {
        match &mut self.content {
            ClipContent::Envelope(envelope) => Some(envelope),
            ClipContent::Effect(_) => None,
        }
    }

    /// The effect a timed effect clip plays: what an envelope can bind.
    pub fn bindable(&self) -> Option<&Effect> {
        self.timing().and(self.effect())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct ClipId(pub u32);

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
    Marks(Option<MarkCollectionId>),
    Curve(CurveValue),
    Gradient(GradientValue),
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
            Type::Curve => Some(Self::Curve(CurveValue::Literal(Curve {
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
            Type::Gradient => Some(Self::Gradient(GradientValue::Literal(Gradient {
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

/// A curve parameter: a curve written in the value, or a link to a declared one.
#[derive(Clone, Debug, PartialEq)]
pub enum CurveValue {
    Literal(Curve),
    Linked(CurveId),
}

/// A gradient parameter: a gradient written in the value, or a link to a
/// declared one.
#[derive(Clone, Debug, PartialEq)]
pub enum GradientValue {
    Literal(Gradient),
    Linked(GradientId),
}

/// An effect or operator declared in a script document. Which one it is
/// matters only where it is referenced: clips play effects and graph nodes
/// run operators.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptDefinition {
    compiled: CompiledDefinition,
}

impl ScriptDefinition {
    pub fn new(compiled: CompiledDefinition) -> Self {
        Self { compiled }
    }
    pub fn kind(&self) -> DeclarationKind {
        self.compiled.kind()
    }
    pub fn name(&self) -> &str {
        self.compiled.name().as_str()
    }
    pub fn params(&self) -> &[ParamDecl] {
        self.compiled.params()
    }
    /// The operator's inputs, in declaration order; an effect has none.
    pub fn inputs(&self) -> impl Iterator<Item = &Identifier> {
        self.compiled
            .inputs()
            .iter()
            .map(|OperatorInputDecl { name }| name)
    }
    pub(crate) fn compiled(&self) -> &CompiledDefinition {
        &self.compiled
    }
    pub fn description(&self) -> Option<&str> {
        self.compiled.description()
    }
}

/// Why a reference to a script definition does not name one of the kind it
/// needs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefinitionProblem {
    Missing,
    NotAnEffect,
    NotAnOperator,
}

/// The definition of `definitions` that `id` names, when it is of `kind`.
pub(crate) fn script_of_kind(
    definitions: &IndexMap<ScriptId, ScriptDefinition>,
    id: ScriptId,
    kind: DeclarationKind,
) -> Result<&ScriptDefinition, DefinitionProblem> {
    let definition = definitions.get(&id).ok_or(DefinitionProblem::Missing)?;
    if definition.kind() == kind {
        Ok(definition)
    } else {
        Err(match kind {
            DeclarationKind::Effect => DefinitionProblem::NotAnEffect,
            DeclarationKind::Operator => DefinitionProblem::NotAnOperator,
        })
    }
}

impl crate::project::DonderProject {
    /// The definition `id` names, when it is of `kind`.
    pub(crate) fn script(
        &self,
        id: ScriptId,
        kind: DeclarationKind,
    ) -> Result<&ScriptDefinition, DefinitionProblem> {
        script_of_kind(self.reusable::<ScriptDefinition>(), id, kind)
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
