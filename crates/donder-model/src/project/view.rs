//! Objects of a validated project, with the references they hold followed.
use super::{DonderProject, ProjectObject};
use crate::controller::Controller;
use crate::input_device::InputDevice;
use crate::layout::Layout;
use crate::patch::Patch;
use crate::setup::Setup;

/// An object of a validated project. The references it holds resolve, so
/// following them never fails.
pub struct View<'p, T: ProjectObject> {
    project: &'p DonderProject,
    id: T::Id,
    object: &'p T,
}

impl<T: ProjectObject> Clone for View<'_, T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T: ProjectObject> Copy for View<'_, T> {}

impl<T: ProjectObject> std::ops::Deref for View<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.object
    }
}

impl<'p, T: ProjectObject> View<'p, T> {
    pub fn id(&self) -> T::Id {
        self.id
    }

    pub fn project(&self) -> &'p DonderProject {
        self.project
    }

    /// The object, borrowed for as long as the project.
    pub fn object(&self) -> &'p T {
        self.object
    }
}

impl DonderProject {
    /// The object `id` names, when the project has it: for ids from outside
    /// the project (requests, stale UI state, command-line flags).
    pub fn view<T: ProjectObject>(&self, id: T::Id) -> Option<View<'_, T>> {
        let object = self.get::<T>(id)?;
        Some(View {
            project: self,
            id,
            object,
        })
    }

    /// The object a reference the project holds names. Validation proves
    /// every reference resolves; ownership validation proves those of the
    /// root and setups before any other check reads them.
    pub(crate) fn referenced<T: ProjectObject>(&self, id: T::Id) -> View<'_, T> {
        self.view(id)
            .unwrap_or_else(|| unreachable!("validation proved the reference {id:?} resolves"))
    }

    /// The setup the project plays.
    pub fn active_setup(&self) -> View<'_, Setup> {
        self.referenced(*self.root.setup.id())
    }

    pub fn active_layout(&self) -> View<'_, Layout> {
        self.active_setup().layout()
    }

    pub fn active_patch(&self) -> View<'_, Patch> {
        self.active_setup().patch()
    }
}

impl<'p> View<'p, Setup> {
    pub fn layout(&self) -> View<'p, Layout> {
        self.project.referenced(*self.object.layout.id())
    }

    pub fn patch(&self) -> View<'p, Patch> {
        self.project.referenced(*self.object.patch.id())
    }

    pub fn controllers(&self) -> impl Iterator<Item = View<'p, Controller>> + use<'p> {
        let project = self.project;
        self.object
            .controllers
            .iter()
            .map(move |controller| project.referenced(*controller.id()))
    }

    pub fn input_devices(&self) -> impl Iterator<Item = View<'p, InputDevice>> + use<'p> {
        let project = self.project;
        self.object
            .input_devices
            .iter()
            .map(move |device| project.referenced(*device.id()))
    }
}
