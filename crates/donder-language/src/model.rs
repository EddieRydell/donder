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
use std::sync::Arc;

mod editing;
pub use editing::ProjectEdit;
mod accepted;
pub use accepted::{AcceptedEffectInputs, AcceptedOperatorInputs, AcceptedSequence};

/// Unvalidated input assembled by the loader. Admission consumes this value;
/// accepted projects never expose a mutable assembly view.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectData {
    pub root: ProjectRoot,
    pub setups: IndexMap<SetupId, Setup>,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub patches: IndexMap<PatchId, Patch>,
    pub controllers: IndexMap<ControllerId, Controller>,
    pub sequences: IndexMap<crate::sequence::SequenceId, Sequence>,
    pub definitions: ProjectDefinitionStores,
}

/// Internal copy-on-write storage keeps checked edits local to changed stores.
/// It is deliberately unavailable through the accepted project's public API.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shared<T>(Arc<T>);

impl<T> From<T> for Shared<T> {
    fn from(value: T) -> Self {
        Self(Arc::new(value))
    }
}
impl<T> std::ops::Deref for Shared<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.0
    }
}
impl<T: Clone> std::ops::DerefMut for Shared<T> {
    fn deref_mut(&mut self) -> &mut T {
        Arc::make_mut(&mut self.0)
    }
}

#[derive(Clone, Debug)]
pub struct DonderProject {
    pub(crate) root: Shared<ProjectRoot>,
    pub(crate) setups: Shared<IndexMap<SetupId, Setup>>,
    pub(crate) layouts: Shared<IndexMap<LayoutId, Layout>>,
    pub(crate) patches: Shared<IndexMap<PatchId, Patch>>,
    pub(crate) controllers: Shared<IndexMap<ControllerId, Controller>>,
    pub(crate) sequences: Shared<IndexMap<crate::sequence::SequenceId, Sequence>>,
    pub(crate) definitions: Shared<ProjectDefinitionStores>,
    accepted_inputs: Arc<accepted::ProjectInputs>,
}

impl PartialEq for DonderProject {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.setups == other.setups
            && self.layouts == other.layouts
            && self.patches == other.patches
            && self.controllers == other.controllers
            && self.sequences == other.sequences
            && self.definitions == other.definitions
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ProjectId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub description: Option<String>,
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
    pub fn try_new(data: ProjectData) -> Result<Self, crate::validation::ProjectValidationError> {
        let mut project = Self {
            root: data.root.into(),
            setups: data.setups.into(),
            layouts: data.layouts.into(),
            patches: data.patches.into(),
            controllers: data.controllers.into(),
            sequences: data.sequences.into(),
            definitions: data.definitions.into(),
            accepted_inputs: Arc::default(),
        };
        crate::validation::validate_project(&project)?;
        project.accepted_inputs = Arc::new(accepted::ProjectInputs::admit(&project, None)?);
        Ok(project)
    }

    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }
    pub fn definitions(&self) -> &ProjectDefinitionStores {
        &self.definitions
    }
    pub fn reusable_setups(&self) -> &IndexMap<SetupId, Setup> {
        &self.setups
    }
    pub fn reusable_layouts(&self) -> &IndexMap<LayoutId, Layout> {
        &self.layouts
    }
    pub fn reusable_patches(&self) -> &IndexMap<PatchId, Patch> {
        &self.patches
    }
    pub fn reusable_controllers(&self) -> &IndexMap<ControllerId, Controller> {
        &self.controllers
    }
    pub fn reusable_sequences(&self) -> &IndexMap<crate::sequence::SequenceId, Sequence> {
        &self.sequences
    }

    /// A private transaction boundary for domain operations. Only changed stores
    /// detach from the original; rejection drops the candidate without mutation.
    pub(crate) fn checked_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut candidate = self.clone();
        let result = edit(&mut candidate)?;
        crate::validation::validate_project(&candidate).map_err(|error| error.to_string())?;
        candidate.accepted_inputs = Arc::new(
            accepted::ProjectInputs::admit(&candidate, Some(self))
                .map_err(|error| error.to_string())?,
        );
        *self = candidate;
        Ok(result)
    }

    /// Iterate each authored object once, including values nested beneath owners.
    pub fn setups(&self) -> impl Iterator<Item = &Setup> {
        self.setups
            .values()
            .chain(self.root.setup.inline().map(Box::as_ref))
    }

    pub(crate) fn setups_mut(&mut self) -> impl Iterator<Item = &mut Setup> {
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

    pub(crate) fn layouts_mut(&mut self) -> impl Iterator<Item = &mut Layout> {
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

    pub(crate) fn patches_mut(&mut self) -> impl Iterator<Item = &mut Patch> {
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

    pub(crate) fn controllers_mut(&mut self) -> impl Iterator<Item = &mut Controller> {
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

    pub(crate) fn sequences_mut(&mut self) -> impl Iterator<Item = &mut Sequence> {
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
    pub(crate) fn setup_mut(&mut self, id: &SetupId) -> Option<&mut Setup> {
        if self.setups.contains_key(id) {
            return self.setups.get_mut(id);
        }
        if self.root.setup.id() != id {
            return None;
        }
        self.root.setup.inline_mut().map(Box::as_mut)
    }
    pub fn layout(&self, id: &LayoutId) -> Option<&Layout> {
        self.layouts().find(|value| &value.id == id)
    }
    pub(crate) fn layout_mut(&mut self, id: &LayoutId) -> Option<&mut Layout> {
        if self.layouts.contains_key(id) {
            return self.layouts.get_mut(id);
        }
        let owner = self
            .setups()
            .find(|setup| setup.layout.id() == id)?
            .id
            .clone();
        self.setup_mut(&owner)?.layout.inline_mut().map(Box::as_mut)
    }
    pub fn patch(&self, id: &PatchId) -> Option<&Patch> {
        self.patches().find(|value| &value.id == id)
    }
    pub(crate) fn patch_mut(&mut self, id: &PatchId) -> Option<&mut Patch> {
        if self.patches.contains_key(id) {
            return self.patches.get_mut(id);
        }
        let owner = self
            .setups()
            .find(|setup| setup.patch.id() == id)?
            .id
            .clone();
        self.setup_mut(&owner)?.patch.inline_mut().map(Box::as_mut)
    }
    pub fn controller(&self, id: &ControllerId) -> Option<&Controller> {
        self.controllers().find(|value| &value.id == id)
    }
    pub(crate) fn controller_mut(&mut self, id: &ControllerId) -> Option<&mut Controller> {
        if self.controllers.contains_key(id) {
            return self.controllers.get_mut(id);
        }
        let (owner, index) = self.setups().find_map(|setup| {
            setup
                .controllers
                .iter()
                .position(|source| source.id() == id)
                .map(|index| (setup.id.clone(), index))
        })?;
        self.setup_mut(&owner)?.controllers[index]
            .inline_mut()
            .map(Box::as_mut)
    }
    pub fn sequence(&self, id: &crate::sequence::SequenceId) -> Option<&Sequence> {
        self.sequences().find(|value| &value.id == id)
    }
    pub(crate) fn sequence_mut(
        &mut self,
        id: &crate::sequence::SequenceId,
    ) -> Option<&mut Sequence> {
        if self.sequences.contains_key(id) {
            return self.sequences.get_mut(id);
        }
        let index = self
            .root
            .sequences
            .iter()
            .position(|source| source.id() == id)?;
        self.root.sequences[index].inline_mut().map(Box::as_mut)
    }
}
