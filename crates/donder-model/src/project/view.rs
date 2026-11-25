//! Objects of a validated project, with the references they hold followed.
use super::{DonderProject, ProjectObject};
use crate::controller::Controller;
use crate::input_device::InputDevice;
use crate::layout::Layout;

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
    /// every reference resolves; the root.s layout before any other check
    /// reads it.
    pub(crate) fn referenced<T: ProjectObject>(&self, id: T::Id) -> View<'_, T> {
        self.view(id)
            .unwrap_or_else(|| unreachable!("validation proved the reference {id:?} resolves"))
    }

    /// The layout the project plays on.
    pub fn layout(&self) -> View<'_, Layout> {
        self.referenced(self.root.layout)
    }

    /// The controllers the project plays through, in order.
    pub fn controllers(&self) -> impl Iterator<Item = View<'_, Controller>> {
        self.controllers.keys().map(|&id| self.referenced(id))
    }

    /// The input devices the project reads, in order.
    pub fn input_devices(&self) -> impl Iterator<Item = View<'_, InputDevice>> {
        self.input_devices.keys().map(|&id| self.referenced(id))
    }
}
