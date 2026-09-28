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
pub struct DonderProject {
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
    pub setup: crate::setup::SetupSource,
    pub sequences: Vec<crate::sequence::SequenceSource>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProjectDefinitionStores {
    pub effects: EffectDefinitionStore,
    pub fixtures: FixtureDefinitions,
    pub curves: CurveDefinitionStore,
    pub gradients: GradientDefinitionStore,
    pub operators: OperatorDefinitionStore,
}

impl DonderProject {
    /// Iterate each authored object once, including values nested beneath owners.
    pub fn setups(&self) -> impl Iterator<Item = &Setup> {
        self.setups
            .values()
            .chain(self.root.setup.inline().map(Box::as_ref))
    }

    pub fn setups_mut(&mut self) -> impl Iterator<Item = &mut Setup> {
        self.setups
            .values_mut()
            .chain(self.root.setup.inline_mut().map(Box::as_mut))
    }

    pub fn layouts(&self) -> impl Iterator<Item = &Layout> {
        self.layouts.values().chain(
            self.setups()
                .filter_map(|setup| setup.layout.inline().map(Box::as_ref)),
        )
    }

    pub fn layouts_mut(&mut self) -> impl Iterator<Item = &mut Layout> {
        let Self {
            root,
            setups,
            layouts,
            ..
        } = self;
        layouts.values_mut().chain(
            setups
                .values_mut()
                .chain(root.setup.inline_mut().map(Box::as_mut))
                .filter_map(|setup| setup.layout.inline_mut().map(Box::as_mut)),
        )
    }

    pub fn patches(&self) -> impl Iterator<Item = &Patch> {
        self.patches.values().chain(
            self.setups()
                .filter_map(|setup| setup.patch.inline().map(Box::as_ref)),
        )
    }

    pub fn patches_mut(&mut self) -> impl Iterator<Item = &mut Patch> {
        let Self {
            root,
            setups,
            patches,
            ..
        } = self;
        patches.values_mut().chain(
            setups
                .values_mut()
                .chain(root.setup.inline_mut().map(Box::as_mut))
                .filter_map(|setup| setup.patch.inline_mut().map(Box::as_mut)),
        )
    }

    pub fn controllers(&self) -> impl Iterator<Item = &Controller> {
        self.controllers.values().chain(
            self.setups()
                .flat_map(|setup| setup.controllers.iter())
                .filter_map(|source| source.inline().map(Box::as_ref)),
        )
    }

    pub fn controllers_mut(&mut self) -> impl Iterator<Item = &mut Controller> {
        let Self {
            root,
            setups,
            controllers,
            ..
        } = self;
        controllers.values_mut().chain(
            setups
                .values_mut()
                .chain(root.setup.inline_mut().map(Box::as_mut))
                .flat_map(|setup| setup.controllers.iter_mut())
                .filter_map(|source| source.inline_mut().map(Box::as_mut)),
        )
    }

    pub fn sequences(&self) -> impl Iterator<Item = &Sequence> {
        self.sequences.values().chain(
            self.root
                .sequences
                .iter()
                .filter_map(|source| source.inline().map(Box::as_ref)),
        )
    }

    pub fn sequences_mut(&mut self) -> impl Iterator<Item = &mut Sequence> {
        self.sequences.values_mut().chain(
            self.root
                .sequences
                .iter_mut()
                .filter_map(|source| source.inline_mut().map(Box::as_mut)),
        )
    }

    pub fn setup(&self, id: &SetupId) -> Option<&Setup> {
        self.setups().find(|value| &value.id == id)
    }
    pub fn setup_mut(&mut self, id: &SetupId) -> Option<&mut Setup> {
        self.setups_mut().find(|value| &value.id == id)
    }
    pub fn layout(&self, id: &LayoutId) -> Option<&Layout> {
        self.layouts().find(|value| &value.id == id)
    }
    pub fn layout_mut(&mut self, id: &LayoutId) -> Option<&mut Layout> {
        self.layouts_mut().find(|value| &value.id == id)
    }
    pub fn patch(&self, id: &PatchId) -> Option<&Patch> {
        self.patches().find(|value| &value.id == id)
    }
    pub fn patch_mut(&mut self, id: &PatchId) -> Option<&mut Patch> {
        self.patches_mut().find(|value| &value.id == id)
    }
    pub fn controller(&self, id: &ControllerId) -> Option<&Controller> {
        self.controllers().find(|value| &value.id == id)
    }
    pub fn controller_mut(&mut self, id: &ControllerId) -> Option<&mut Controller> {
        self.controllers_mut().find(|value| &value.id == id)
    }
    pub fn sequence(&self, id: &crate::sequence::SequenceId) -> Option<&Sequence> {
        self.sequences().find(|value| &value.id == id)
    }
    pub fn sequence_mut(&mut self, id: &crate::sequence::SequenceId) -> Option<&mut Sequence> {
        self.sequences_mut().find(|value| &value.id == id)
    }
}
