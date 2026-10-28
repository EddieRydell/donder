use crate::controller::{Controller, ControllerId};
use crate::effect::{
    CurveDefinition, CurveId, EffectDefinition, EffectDefinitionId, GradientDefinition, GradientId,
};
use crate::fixture::{FixtureDefinition, FixtureDefinitionId};
pub(crate) use crate::identity::ProjectId;
use crate::identity::{ObjectId, ObjectKind};
use crate::input_device::{InputDevice, InputDeviceId};
use crate::layout::{Layout, LayoutId};
use crate::operator::{OperatorDefinition, OperatorDefinitionId};
use crate::patch::{Patch, PatchId};
use crate::rig::{Rig, RigId};
use crate::sequence::{Sequence, SequenceId};
use crate::setup::{Setup, SetupId};
use indexmap::IndexMap;
use std::sync::Arc;

mod editing;
pub use editing::{Object, ProjectEdit};
mod objects;
pub use objects::{ProjectObject, Reusable};
mod accepted;
pub(crate) mod references;
pub use accepted::{
    AcceptedEffectInputs, AcceptedOperatorInputs, AcceptedRig, AcceptedRigEffect, AcceptedSequence,
    RIG_SESSION, RigBinding, RigEffectOrigin,
};

/// Unvalidated input assembled by the loader. Admission consumes this value;
/// accepted projects never expose a mutable assembly view. Each store holds
/// the reusable objects of one kind by id; owned objects live in their owner.
#[derive(Clone, Debug)]
pub struct ProjectData {
    pub root: ProjectRoot,
    pub setups: IndexMap<SetupId, Setup>,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub patches: IndexMap<PatchId, Patch>,
    pub controllers: IndexMap<ControllerId, Controller>,
    pub input_devices: IndexMap<InputDeviceId, InputDevice>,
    pub sequences: IndexMap<SequenceId, Sequence>,
    pub rigs: IndexMap<RigId, Rig>,
    pub fixtures: IndexMap<FixtureDefinitionId, FixtureDefinition>,
    pub curves: IndexMap<CurveId, CurveDefinition>,
    pub gradients: IndexMap<GradientId, GradientDefinition>,
    pub effects: IndexMap<EffectDefinitionId, EffectDefinition>,
    pub operators: IndexMap<OperatorDefinitionId, OperatorDefinition>,
    /// The next unassigned object id; every id in the data is below it.
    pub next_id: u32,
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
impl<T> Shared<T> {
    /// Whether both values are one allocation: neither side has been edited
    /// since they were shared.
    pub(crate) fn same_allocation(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
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
    pub(crate) input_devices: Shared<IndexMap<InputDeviceId, InputDevice>>,
    pub(crate) sequences: Shared<IndexMap<SequenceId, Sequence>>,
    pub(crate) rigs: Shared<IndexMap<RigId, Rig>>,
    pub(crate) fixtures: Shared<IndexMap<FixtureDefinitionId, FixtureDefinition>>,
    pub(crate) curves: Shared<IndexMap<CurveId, CurveDefinition>>,
    pub(crate) gradients: Shared<IndexMap<GradientId, GradientDefinition>>,
    pub(crate) effects: Shared<IndexMap<EffectDefinitionId, EffectDefinition>>,
    pub(crate) operators: Shared<IndexMap<OperatorDefinitionId, OperatorDefinition>>,
    next_id: u32,
    accepted_inputs: Arc<accepted::ProjectInputs>,
}

impl PartialEq for DonderProject {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.setups == other.setups
            && self.layouts == other.layouts
            && self.patches == other.patches
            && self.controllers == other.controllers
            && self.input_devices == other.input_devices
            && self.sequences == other.sequences
            && self.rigs == other.rigs
            && self.fixtures == other.fixtures
            && self.curves == other.curves
            && self.gradients == other.gradients
            && self.effects == other.effects
            && self.operators == other.operators
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub description: Option<String>,
    pub setup: crate::setup::SetupSource,
    pub sequences: Vec<crate::sequence::SequenceSource>,
    pub rigs: Vec<crate::rig::RigSource>,
}

/// Why an edit of a project failed: the edit itself was refused, or the edited
/// project breaks a project rule.
#[derive(Clone, Debug, PartialEq)]
pub enum EditError {
    Rejected(EditRejection),
    Invalid(crate::validation::ProjectValidationError),
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Rejected(rejection) => rejection.fmt(f),
            Self::Invalid(error) => error.fmt(f),
        }
    }
}

/// An edit the model refuses before checking the edited project.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EditRejection {
    ProjectIdentity,
    ProjectRemoval,
    NotReusable,
    Missing(ObjectKind),
    MissingItem,
    MissingOutput,
    MemberNotInOwner,
    NoMembers,
    Unnamed,
    AlreadyReusable,
    AlreadyIndependent(ObjectKind),
    NonPositiveTiming,
    ChooseReusable(ObjectKind),
    ChooseMatchingKind,
    AlreadyInSetup(ObjectKind),
    NotInSetup(ObjectKind),
    ControllerHasOutputs(usize),
    SharedPatch,
    NotAGroup,
    NotInSourceGroup,
    NotInGroup,
    InsertionTargetOutside,
    AlreadyInGroup,
    GroupInsideItself,
    NoFreeIds,
}

impl std::fmt::Display for EditRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectIdentity => f.write_str("A project keeps its identity."),
            Self::ProjectRemoval => f.write_str("A project cannot be removed."),
            Self::NotReusable => f.write_str("Only a reusable object can be removed."),
            Self::Missing(kind) => write!(f, "The {kind} was not found."),
            Self::MissingItem => f.write_str("The fixture or group was not found."),
            Self::MissingOutput => f.write_str("The output assignment was not found."),
            Self::MemberNotInOwner => f.write_str("The member was not found in its owner."),
            Self::NoMembers => f.write_str("This object owns no members."),
            Self::Unnamed => f.write_str("The reusable value has no name."),
            Self::AlreadyReusable => f.write_str("This value is already reusable."),
            Self::AlreadyIndependent(kind) => write!(f, "This {kind} is already independent."),
            Self::NonPositiveTiming => {
                f.write_str("Sequence duration and frame rate must be positive.")
            }
            Self::ChooseReusable(kind) => write!(f, "Choose a reusable {kind}."),
            Self::ChooseMatchingKind => f.write_str("Choose a reusable source of the matching kind."),
            Self::AlreadyInSetup(kind) => write!(f, "This {kind} is already in the setup."),
            Self::NotInSetup(kind) => write!(f, "Choose a {kind} in this setup."),
            Self::ControllerHasOutputs(count) => write!(
                f,
                "This controller has {count} output assignments. Remove them first or choose Remove controller and outputs."
            ),
            Self::SharedPatch => f.write_str(
                "Another setup uses this patch. Make the patch independent first so output changes stay in this setup.",
            ),
            Self::NotAGroup => f.write_str("Items can only be placed in groups."),
            Self::NotInSourceGroup => f.write_str("The item is not in the source group."),
            Self::NotInGroup => f.write_str("The item is not in that group."),
            Self::InsertionTargetOutside => {
                f.write_str("The insertion target is not in the destination group.")
            }
            Self::AlreadyInGroup => f.write_str("The item is already in that group."),
            Self::GroupInsideItself => {
                f.write_str("A group cannot contain itself or one of its groups.")
            }
            Self::NoFreeIds => f.write_str("No fixture instance IDs remain."),
        }
    }
}

impl DonderProject {
    pub fn try_new(data: ProjectData) -> Result<Self, crate::validation::ProjectValidationError> {
        let mut project = Self {
            root: data.root.into(),
            setups: data.setups.into(),
            layouts: data.layouts.into(),
            patches: data.patches.into(),
            controllers: data.controllers.into(),
            input_devices: data.input_devices.into(),
            sequences: data.sequences.into(),
            rigs: data.rigs.into(),
            fixtures: data.fixtures.into(),
            curves: data.curves.into(),
            gradients: data.gradients.into(),
            effects: data.effects.into(),
            operators: data.operators.into(),
            next_id: data.next_id,
            accepted_inputs: Arc::default(),
        };
        crate::validation::validate_project(&project)?;
        project.accepted_inputs = Arc::new(accepted::ProjectInputs::admit(&project, None)?);
        Ok(project)
    }

    /// A fresh project-wide object id. Copies and new objects take theirs here.
    pub fn new_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }

    /// The object of kind `T` with `id`, reusable or owned in place.
    pub fn get<T: ProjectObject>(&self, id: T::Id) -> Option<&T> {
        T::find(self, id)
    }

    /// Every object of kind `T` once: the reusable ones, then those owned in
    /// place.
    pub fn objects<T: ProjectObject>(&self) -> impl Iterator<Item = &T> {
        T::all(self)
    }

    /// The reusable objects of kind `T`, by id.
    pub fn reusable<T: Reusable>(&self) -> &IndexMap<T::Id, T> {
        T::store(self)
    }

    pub(crate) fn find_mut<T: objects::Stored>(&mut self, id: T::Id) -> Option<&mut T> {
        T::find_mut(self, id)
    }

    /// Whether `other` shares every store with this project except sequences:
    /// nothing besides sequences was edited between the two.
    pub fn only_sequences_differ_from(&self, other: &Self) -> bool {
        self.root.same_allocation(&other.root)
            && self.setups.same_allocation(&other.setups)
            && self.layouts.same_allocation(&other.layouts)
            && self.patches.same_allocation(&other.patches)
            && self.controllers.same_allocation(&other.controllers)
            && self.input_devices.same_allocation(&other.input_devices)
            && self.rigs.same_allocation(&other.rigs)
            && self.definitions_same_allocation(other)
            && self.fixtures.same_allocation(&other.fixtures)
    }

    /// Whether the definitions clips are admitted against, the effects,
    /// operators, curves and gradients, are unchanged from `other`.
    pub(crate) fn definitions_same_allocation(&self, other: &Self) -> bool {
        self.effects.same_allocation(&other.effects)
            && self.operators.same_allocation(&other.operators)
            && self.curves.same_allocation(&other.curves)
            && self.gradients.same_allocation(&other.gradients)
    }

    /// A private transaction boundary for domain operations. Only changed stores
    /// detach from the original; rejection drops the candidate without mutation.
    pub(crate) fn checked_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> Result<T, EditRejection>,
    ) -> Result<T, EditError> {
        let mut candidate = self.clone();
        let result = edit(&mut candidate).map_err(EditError::Rejected)?;
        crate::validation::validate_project_edit(&candidate, self).map_err(EditError::Invalid)?;
        candidate.accepted_inputs = Arc::new(
            accepted::ProjectInputs::admit(&candidate, Some(self)).map_err(EditError::Invalid)?,
        );
        *self = candidate;
        Ok(result)
    }

    /// The object `id` names, whatever its kind.
    pub fn contains(&self, id: ObjectId) -> bool {
        match id {
            ObjectId::Project(id) => self.get::<ProjectRoot>(id).is_some(),
            ObjectId::Setup(id) => self.get::<Setup>(id).is_some(),
            ObjectId::Layout(id) => self.get::<Layout>(id).is_some(),
            ObjectId::Patch(id) => self.get::<Patch>(id).is_some(),
            ObjectId::Controller(id) => self.get::<Controller>(id).is_some(),
            ObjectId::InputDevice(id) => self.get::<InputDevice>(id).is_some(),
            ObjectId::Sequence(id) => self.get::<Sequence>(id).is_some(),
            ObjectId::Rig(id) => self.get::<Rig>(id).is_some(),
            ObjectId::FixtureDefinition(id) => self.get::<FixtureDefinition>(id).is_some(),
            ObjectId::Curve(id) => self.get::<CurveDefinition>(id).is_some(),
            ObjectId::Gradient(id) => self.get::<GradientDefinition>(id).is_some(),
            ObjectId::EffectDefinition(id) => self.get::<EffectDefinition>(id).is_some(),
            ObjectId::OperatorDefinition(id) => self.get::<OperatorDefinition>(id).is_some(),
        }
    }
}
