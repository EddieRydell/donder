use crate::controller::{Controller, ControllerId};
use crate::effect::{CurveDefinitionStore, EffectDefinitionStore, GradientDefinitionStore};
use crate::fixture::FixtureDefinitions;
use crate::identity::SourceIdentity;
use crate::layout::{Layout, LayoutId};
use crate::operator::OperatorDefinitionStore;
use crate::patch::{Patch, PatchId};
use crate::sequence::Sequence;
use crate::setup::{Setup, SetupId};
use indexmap::IndexMap;

#[derive(Clone, Debug, PartialEq)]
pub struct DawnProject {
    pub root: ProjectRoot,
    pub setups: IndexMap<SetupId, Setup>,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub patches: IndexMap<PatchId, Patch>,
    pub controllers: IndexMap<ControllerId, Controller>,
    pub sequences: IndexMap<crate::sequence::SequenceId, Sequence>,
    pub definitions: ProjectDefinitionStores,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProjectId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub setup: SetupId,
    pub sequences: Vec<crate::sequence::SequenceId>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectDefinitionStores {
    pub effects: EffectDefinitionStore,
    pub fixtures: FixtureDefinitions,
    pub curves: CurveDefinitionStore,
    pub gradients: GradientDefinitionStore,
    pub operators: OperatorDefinitionStore,
}
