pub(crate) use crate::identity::{CurveId, EffectDefinitionId, GradientId};
use crate::layout::FixtureTarget;
use crate::sequence::{MarkCollectionKey, SequenceLayerId};
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
    Enum(Identifier),
    /// `None` is no collection: an empty set of marks, which never triggers.
    Marks(Option<MarkCollectionKey>),
    Curve(OwnedCurve),
    Gradient(OwnedGradient),
    Array(Vec<ParamValue>),
}

impl ParamValue {
    /// Initial authored values must be valid resources inside the declared
    /// range, unlike the VM's empty storage defaults. Numbers start at zero
    /// held inside the range, and a curve rises across it. The caller supplies
    /// the user-facing initial color.
    pub fn initial_for_param(
        param: &ParamDecl,
        color: donder_runtime_types::Color,
    ) -> Option<Self> {
        Self::initial_for_type(&param.ty, param.range, color)
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
                        value: min,
                    },
                    donder_runtime_types::CurvePoint {
                        position: 1.0,
                        value: max,
                    },
                ],
            }))),
            Type::Gradient => Some(Self::Gradient(OwnedGradient::Inline(Gradient {
                stops: vec![donder_runtime_types::GradientStop {
                    position: 0.0,
                    color,
                }],
            }))),
            Type::Array(element) => Some(Self::Array(vec![Self::initial_for_type(
                element, range, color,
            )?])),
            Type::Enum(options) => options.first().cloned().map(Self::Enum),
            Type::Marks => Some(Self::Marks(None)),
            Type::Void => None,
        }
    }
}

pub type OwnedCurve = crate::ownership::Owned<Curve, CurveId>;
pub type OwnedGradient = crate::ownership::Owned<Gradient, GradientId>;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDefinition {
    pub(crate) id: EffectDefinitionId,
    pub(crate) compiled: CompiledDefinition,
}

impl EffectDefinition {
    pub fn new(id: EffectDefinitionId, compiled: CompiledDefinition) -> Self {
        Self { id, compiled }
    }
    pub fn id(&self) -> &EffectDefinitionId {
        &self.id
    }
    pub fn name(&self) -> &str {
        self.compiled.name().as_str()
    }
    pub fn params(&self) -> &[ParamDecl] {
        self.compiled.params()
    }
    pub fn compiled(&self) -> &CompiledDefinition {
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
