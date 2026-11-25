use crate::console::{Console, ConsoleId};
use crate::controller::{Controller, ControllerId};
use crate::effect::{
    CurveDefinition, CurveId, GradientDefinition, GradientId, ScriptDefinition, ScriptId,
};
use crate::fixture::{FixtureDefinition, FixtureDefinitionId};
pub(crate) use crate::identity::ProjectId;
use crate::identity::{ObjectId, ObjectKind};
use crate::input_device::{InputDevice, InputDeviceId};
use crate::layout::{Layout, LayoutId};
use crate::sequence::{Sequence, SequenceId};
use indexmap::IndexMap;
use std::sync::Arc;

mod editing;
pub use editing::{Object, ProjectEdit};
mod members;
pub(crate) use members::MemberMut;
pub use members::{MemberRef, MemberValue};
mod objects;
pub use objects::ProjectObject;
mod accepted;
mod view;
pub use view::View;
pub(crate) mod references;
pub use accepted::{
    AcceptedClip, AcceptedOperatorInputs, AcceptedShow, LiveBinding, PlaybackRoute, ShowId,
};

/// Unvalidated input assembled by the loader. Admission consumes this value;
/// accepted projects never expose a mutable assembly view. Each store holds
/// the objects of one kind by id.
#[derive(Clone, Debug)]
pub struct ProjectData {
    pub root: ProjectRoot,
    pub layouts: IndexMap<LayoutId, Layout>,
    pub controllers: IndexMap<ControllerId, Controller>,
    pub input_devices: IndexMap<InputDeviceId, InputDevice>,
    pub sequences: IndexMap<SequenceId, Sequence>,
    pub consoles: IndexMap<ConsoleId, Console>,
    pub fixtures: IndexMap<FixtureDefinitionId, FixtureDefinition>,
    pub curves: IndexMap<CurveId, CurveDefinition>,
    pub gradients: IndexMap<GradientId, GradientDefinition>,
    pub scripts: IndexMap<ScriptId, ScriptDefinition>,
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
    fn same_allocation(&self, other: &Self) -> bool {
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
    pub(crate) layouts: Shared<IndexMap<LayoutId, Layout>>,
    pub(crate) controllers: Shared<IndexMap<ControllerId, Controller>>,
    pub(crate) input_devices: Shared<IndexMap<InputDeviceId, InputDevice>>,
    pub(crate) sequences: Shared<IndexMap<SequenceId, Sequence>>,
    pub(crate) consoles: Shared<IndexMap<ConsoleId, Console>>,
    pub(crate) fixtures: Shared<IndexMap<FixtureDefinitionId, FixtureDefinition>>,
    curves: Shared<IndexMap<CurveId, CurveDefinition>>,
    gradients: Shared<IndexMap<GradientId, GradientDefinition>>,
    scripts: Shared<IndexMap<ScriptId, ScriptDefinition>>,
    next_id: u32,
    accepted_inputs: Arc<accepted::ProjectInputs>,
    /// Data derived from asset files: not part of the project's documents
    /// or equality.
    derived: Arc<crate::derived::DerivedStore>,
    /// The repairs edits made since [`DonderProject::take_repairs`]; not
    /// part of the project.
    repairs: Vec<crate::validation::Repair>,
}

impl PartialEq for DonderProject {
    fn eq(&self, other: &Self) -> bool {
        self.root == other.root
            && self.layouts == other.layouts
            && self.controllers == other.controllers
            && self.input_devices == other.input_devices
            && self.sequences == other.sequences
            && self.consoles == other.consoles
            && self.fixtures == other.fixtures
            && self.curves == other.curves
            && self.gradients == other.gradients
            && self.scripts == other.scripts
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectRoot {
    pub id: ProjectId,
    pub description: Option<String>,
    /// The project's one layout.
    pub layout: LayoutId,
    /// How the layout's fixtures are wired to the controllers.
    pub routes: Vec<crate::patch::PixelRoute>,
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
    Missing(ObjectKind),
    MissingItem,
    MissingMember,
    NotInSourceGroup,
    NotInGroup,
    InsertionTargetOutside,
    NoFreeIds,
}

impl std::fmt::Display for EditRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProjectIdentity => f.write_str("A project keeps its identity."),
            Self::ProjectRemoval => f.write_str("A project cannot be removed."),
            Self::Missing(kind) => write!(f, "The {kind} was not found."),
            Self::MissingItem => f.write_str("The fixture or group was not found."),
            Self::MissingMember => f.write_str("That part of the object was not found."),
            Self::NotInSourceGroup => f.write_str("The item is not in the source group."),
            Self::NotInGroup => f.write_str("The item is not in that group."),
            Self::InsertionTargetOutside => {
                f.write_str("The insertion target is not in the destination group.")
            }
            Self::NoFreeIds => f.write_str("No fixture instance IDs remain."),
        }
    }
}

impl DonderProject {
    pub fn try_new(data: ProjectData) -> Result<Self, crate::validation::ProjectValidationError> {
        let mut project = Self {
            root: data.root.into(),
            layouts: data.layouts.into(),
            controllers: data.controllers.into(),
            input_devices: data.input_devices.into(),
            sequences: data.sequences.into(),
            consoles: data.consoles.into(),
            fixtures: data.fixtures.into(),
            curves: data.curves.into(),
            gradients: data.gradients.into(),
            scripts: data.scripts.into(),
            next_id: data.next_id,
            accepted_inputs: Arc::default(),
            derived: Arc::default(),
            repairs: Vec::new(),
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

    /// The repairs edits made since the last call, oldest first.
    pub fn take_repairs(&mut self) -> Vec<crate::validation::Repair> {
        std::mem::take(&mut self.repairs)
    }

    pub fn root(&self) -> &ProjectRoot {
        &self.root
    }

    /// The object of kind `T` with `id`.
    pub fn get<T: ProjectObject>(&self, id: T::Id) -> Option<&T> {
        T::store(self).get(&id)
    }

    /// Every object of kind `T`, in the order the project lists them.
    pub fn objects<T: ProjectObject>(&self) -> impl Iterator<Item = (T::Id, &T)> {
        T::store(self).iter().map(|(id, object)| (*id, object))
    }

    /// Each fixture definition's pixel count.
    pub fn fixture_pixel_counts(&self) -> IndexMap<FixtureDefinitionId, u32> {
        crate::fixture::fixture_pixel_counts(&self.fixtures)
            .unwrap_or_else(|_| unreachable!("validation proved every fixture definition"))
    }

    /// The objects of kind `T`, by id.
    pub fn reusable<T: ProjectObject>(&self) -> &IndexMap<T::Id, T> {
        T::store(self)
    }

    pub(crate) fn find_mut<T: objects::Stored>(&mut self, id: T::Id) -> Option<&mut T> {
        T::store_mut(self).get_mut(&id)
    }

    /// Whether `other` shares every store with this project except sequences:
    /// nothing besides sequences was edited between the two.
    pub fn only_sequences_differ_from(&self, other: &Self) -> bool {
        self.root.same_allocation(&other.root)
            && self.layouts.same_allocation(&other.layouts)
            && self.controllers.same_allocation(&other.controllers)
            && self.input_devices.same_allocation(&other.input_devices)
            && self.consoles.same_allocation(&other.consoles)
            && self.definitions_same_allocation(other)
            && self.fixtures.same_allocation(&other.fixtures)
    }

    /// Whether the definitions clips are admitted against, the scripts,
    /// curves and gradients, are unchanged from `other`.
    fn definitions_same_allocation(&self, other: &Self) -> bool {
        self.scripts.same_allocation(&other.scripts)
            && self.curves.same_allocation(&other.curves)
            && self.gradients.same_allocation(&other.gradients)
    }

    /// What `derivation` derived, or `None` while it runs.
    pub fn derived(&self, derivation: &crate::Derivation) -> Option<&crate::Derived> {
        self.derived.get(derivation)
    }

    /// Each derivation the sequences' and consoles' clips and operators read
    /// that has no result yet, once.
    pub fn pending_derivations(&self) -> Vec<crate::Derivation> {
        let sequences = self
            .objects::<crate::Sequence>()
            .map(|(_, sequence)| self.sequence_derivations(sequence));
        let consoles = self.objects::<crate::Console>().map(|(_, console)| {
            let clips = console
                .composition
                .clips
                .iter()
                .filter_map(|clip| clip.effect())
                .map(|effect| (&effect.definition, &effect.params));
            self.derivations(
                accepted::MarkOrigin::Live,
                clips,
                &console.composition.graph,
            )
        });
        let mut pending = Vec::new();
        for derivation in sequences.chain(consoles).flatten() {
            if !self.derived.contains_key(&derivation) && !pending.contains(&derivation) {
                pending.push(derivation);
            }
        }
        pending
    }

    /// What `sequence`'s clips and operators read, once.
    pub fn sequence_derivations(&self, sequence: &crate::Sequence) -> Vec<crate::Derivation> {
        let clips = sequence
            .composition
            .clips
            .iter()
            .filter_map(|clip| clip.effect())
            .map(|effect| (&effect.definition, &effect.params));
        let origin = accepted::MarkOrigin::Authored(sequence);
        self.derivations(origin, clips, &sequence.composition.graph)
    }

    /// What the clips and operators of a sequence or console read, once.
    fn derivations<'a>(
        &'a self,
        origin: accepted::MarkOrigin<'a>,
        clips: impl Iterator<
            Item = (
                &'a crate::ScriptId,
                &'a IndexMap<donder_runtime_types::Identifier, crate::ParamValue>,
            ),
        >,
        graph: &'a crate::CompositionGraph,
    ) -> Vec<crate::Derivation> {
        let clips = clips.map(|(definition, overrides)| {
            (
                self.reusable::<crate::ScriptDefinition>()[definition].compiled(),
                overrides,
            )
        });
        let operators = graph.nodes.iter().filter_map(|node| match &node.kind {
            crate::CompositionGraphNodeKind::Operator(operator) => Some((
                self.reusable::<crate::ScriptDefinition>()[&operator.operator].compiled(),
                &operator.params,
            )),
            _ => None,
        });
        let mut derivations = Vec::new();
        for (definition, overrides) in clips.chain(operators) {
            for derivation in accepted::derivations(self, origin, definition, overrides) {
                if !derivations.contains(&derivation) {
                    derivations.push(derivation);
                }
            }
        }
        derivations
    }

    /// The project with `derived` for `derivation`; sequences and consoles
    /// are admitted again, so their clips read it.
    pub fn with_derived(
        &self,
        derivation: crate::Derivation,
        derived: crate::Derived,
    ) -> Result<Self, crate::validation::ProjectValidationError> {
        let mut candidate = self.clone();
        Arc::make_mut(&mut candidate.derived).insert(derivation, derived);
        candidate.accepted_inputs =
            Arc::new(accepted::ProjectInputs::admit(&candidate, Some(self))?);
        Ok(candidate)
    }

    /// Whether both projects hold the same analyzed samples.
    pub(crate) fn same_derived(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.derived, &other.derived)
    }

    /// A private transaction boundary for domain operations. Only changed stores
    /// detach from the original; rejection drops the candidate without mutation.
    /// Members the edit left alone give way to it by [`crate::Repair`]s, which
    /// collect until [`Self::take_repairs`].
    pub(crate) fn checked_edit<T>(
        &mut self,
        edit: impl FnOnce(&mut Self) -> Result<T, EditRejection>,
    ) -> Result<T, EditError> {
        let mut candidate = self.clone();
        let result = edit(&mut candidate).map_err(EditError::Rejected)?;
        let repairs =
            crate::validation::repair(&mut candidate, self).map_err(EditError::Invalid)?;
        candidate.repairs.extend(repairs);
        candidate.accepted_inputs = Arc::new(
            accepted::ProjectInputs::admit(&candidate, Some(self)).map_err(EditError::Invalid)?,
        );
        *self = candidate;
        Ok(result)
    }
}
