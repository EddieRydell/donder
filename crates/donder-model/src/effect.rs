use crate::identity::SourceIdentity;
use crate::layout::FixtureTarget;
use crate::sequence::{MarkCollectionKey, SequenceLayerId};
use donder_language::compiler::{CompiledEffect, ParamDecl};
use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;
use donder_runtime_types::Type;
use donder_runtime_types::{Curve, Gradient};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectInst {
    pub id: EffectInstId,
    /// Unique among the sequence's clips; automation bindings use it.
    pub name: Identifier,
    pub description: Option<String>,
    pub layer_id: SequenceLayerId,
    pub start: DonderTime,
    pub duration: DonderDuration,
    pub target: FixtureTarget,
    pub scope: EffectScope,
    pub definition: EffectRef,
    pub param_overrides: IndexMap<Identifier, EffectParamValue>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectInstId(pub u32);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct EffectDefinitionId(pub SourceIdentity);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum EffectRef {
    Custom(EffectDefinitionId),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EffectScope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectParamValue {
    Int(i32),
    Float(f32),
    Bool(bool),
    Color(donder_runtime_types::Color),
    Enum(Identifier),
    Marks(MarkCollectionKey),
    Curve(CurveSource),
    Gradient(GradientSource),
    Array(Vec<EffectParamValue>),
}

impl EffectParamValue {
    /// Initial authored values must be valid resources, unlike the VM's empty
    /// storage defaults. The caller supplies the user-facing initial color.
    pub fn initial_for_type(ty: &Type, color: donder_runtime_types::Color) -> Option<Self> {
        match ty {
            Type::Int => Some(Self::Int(0)),
            Type::Float => Some(Self::Float(0.0)),
            Type::Bool => Some(Self::Bool(false)),
            Type::Color => Some(Self::Color(color)),
            Type::Curve => Some(Self::Curve(CurveSource::Inline(Curve {
                points: vec![
                    donder_runtime_types::CurvePoint {
                        position: 0.0,
                        value: 0.0,
                    },
                    donder_runtime_types::CurvePoint {
                        position: 1.0,
                        value: 1.0,
                    },
                ],
            }))),
            Type::Gradient => Some(Self::Gradient(GradientSource::Inline(Gradient {
                stops: vec![donder_runtime_types::GradientStop {
                    position: 0.0,
                    color,
                }],
            }))),
            Type::Array(element) => {
                Some(Self::Array(vec![Self::initial_for_type(element, color)?]))
            }
            Type::Enum(options) => options.first().cloned().map(Self::Enum),
            Type::Void | Type::Signal | Type::Marks => None,
        }
    }
}

pub type CurveSource = crate::ownership::ValueSource<Curve, CurveId>;
pub type GradientSource = crate::ownership::ValueSource<Gradient, GradientId>;

#[derive(Clone, Debug, PartialEq)]
pub struct EffectDefinition {
    pub(crate) id: EffectRef,
    pub source_name: String,
    pub display_name: String,
    pub(crate) params: Vec<ParamDecl>,
    pub(crate) implementation: EffectImplementation,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectImplementation {
    Dsl(CompiledEffect),
}

impl EffectDefinition {
    pub fn id(&self) -> &EffectRef {
        &self.id
    }
    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }
    pub fn implementation(&self) -> &EffectImplementation {
        &self.implementation
    }
    pub fn description(&self) -> Option<&str> {
        match &self.implementation {
            EffectImplementation::Dsl(compiled) => compiled.description(),
        }
    }
    pub fn custom(id: EffectDefinitionId, compiled: CompiledEffect) -> Self {
        Self {
            id: EffectRef::Custom(id),
            source_name: compiled.name().as_str().to_string(),
            display_name: compiled.name().as_str().to_string(),
            params: compiled.params().to_vec(),
            implementation: EffectImplementation::Dsl(compiled),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EffectDefinitionStore {
    pub definitions: IndexMap<EffectDefinitionId, EffectDefinition>,
}

impl EffectDefinitionStore {
    pub fn get(&self, key: &EffectDefinitionId) -> Option<&EffectDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: EffectDefinitionId,
        definition: EffectDefinition,
    ) -> Option<EffectDefinition> {
        self.definitions.insert(key, definition)
    }

    pub fn resolve(&self, reference: &EffectRef) -> Option<&EffectDefinition> {
        match reference {
            EffectRef::Custom(id) => self.get(id),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CurveId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct CurveDefinition {
    pub description: Option<String>,
    pub curve: Curve,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GradientId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct GradientDefinition {
    pub description: Option<String>,
    pub gradient: Gradient,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CurveDefinitionStore {
    pub definitions: IndexMap<CurveId, CurveDefinition>,
}

impl CurveDefinitionStore {
    pub fn get(&self, key: &CurveId) -> Option<&CurveDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(&mut self, key: CurveId, curve: CurveDefinition) -> Option<CurveDefinition> {
        self.definitions.insert(key, curve)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct GradientDefinitionStore {
    pub definitions: IndexMap<GradientId, GradientDefinition>,
}

impl GradientDefinitionStore {
    pub fn get(&self, key: &GradientId) -> Option<&GradientDefinition> {
        self.definitions.get(key)
    }

    pub fn insert(
        &mut self,
        key: GradientId,
        gradient: GradientDefinition,
    ) -> Option<GradientDefinition> {
        self.definitions.insert(key, gradient)
    }
}
