use super::objects::Stored;
use super::*;

/// Any project object, as an edit writes it, under its id.
#[derive(Clone, Debug)]
pub enum Object {
    Project(ProjectRoot),
    Layout(LayoutId, Layout),
    Controller(ControllerId, Controller),
    InputDevice(InputDeviceId, InputDevice),
    Sequence(SequenceId, Sequence),
    Console(ConsoleId, Box<Console>),
    FixtureDefinition(FixtureDefinitionId, FixtureDefinition),
    Curve(CurveId, CurveDefinition),
    Gradient(GradientId, GradientDefinition),
    Script(ScriptId, ScriptDefinition),
}

macro_rules! object_from {
    ($($variant:ident($id:ty, $kind:ty)),*) => {$(
        impl From<($id, $kind)> for Object {
            fn from((id, value): ($id, $kind)) -> Self {
                Self::$variant(id, value.into())
            }
        }
    )*};
}
object_from!(
    Layout(LayoutId, Layout),
    Controller(ControllerId, Controller),
    InputDevice(InputDeviceId, InputDevice),
    Sequence(SequenceId, Sequence),
    Console(ConsoleId, Console),
    FixtureDefinition(FixtureDefinitionId, FixtureDefinition),
    Curve(CurveId, CurveDefinition),
    Gradient(GradientId, GradientDefinition),
    Script(ScriptId, ScriptDefinition)
);

impl From<ProjectRoot> for Object {
    fn from(value: ProjectRoot) -> Self {
        Self::Project(value)
    }
}

/// Detached authored changes. Every batch is checked as one operation, allowing
/// related values to change together without exposing a mutable project draft.
#[derive(Clone, Debug)]
pub enum ProjectEdit {
    /// Write an object under its id: in place of the object with that id, or
    /// as a new object.
    Put(Object),
    /// Remove an object.
    Remove(ObjectId),
    /// Write a member in place of the member with its id the site names.
    PutMember(crate::Site, super::MemberValue),
    /// Remove the member the site names, or the part of it its field names.
    RemoveMember(crate::Site),
}

fn put<T: Stored>(project: &mut DonderProject, id: T::Id, value: T) {
    T::store_mut(project).insert(id, value);
}

fn remove<T: Stored>(project: &mut DonderProject, id: T::Id) -> Result<(), EditRejection> {
    T::store_mut(project)
        .shift_remove(&id)
        .map(drop)
        .ok_or(EditRejection::Missing(<T::Id as crate::TypedId>::KIND))
}

impl DonderProject {
    pub fn apply_edits(
        &mut self,
        edits: impl IntoIterator<Item = ProjectEdit>,
    ) -> Result<(), crate::EditError> {
        self.checked_edit(|project| {
            for edit in edits {
                match edit {
                    ProjectEdit::Put(object) => match object {
                        Object::Project(value) => {
                            if value.id != project.root.id {
                                return Err(EditRejection::ProjectIdentity);
                            }
                            project.root = value.into();
                        }
                        Object::Layout(id, value) => put(project, id, value),
                        Object::Controller(id, value) => put(project, id, value),
                        Object::InputDevice(id, value) => put(project, id, value),
                        Object::Sequence(id, value) => put(project, id, value),
                        Object::Console(id, value) => put(project, id, *value),
                        Object::FixtureDefinition(id, value) => put(project, id, value),
                        Object::Curve(id, value) => put(project, id, value),
                        Object::Gradient(id, value) => put(project, id, value),
                        Object::Script(id, value) => put(project, id, value),
                    },
                    ProjectEdit::Remove(id) => match id {
                        ObjectId::Project(_) => {
                            return Err(EditRejection::ProjectRemoval);
                        }
                        ObjectId::Layout(id) => remove::<Layout>(project, id)?,
                        ObjectId::Controller(id) => remove::<Controller>(project, id)?,
                        ObjectId::InputDevice(id) => remove::<InputDevice>(project, id)?,
                        ObjectId::Sequence(id) => remove::<Sequence>(project, id)?,
                        ObjectId::Console(id) => remove::<Console>(project, id)?,
                        ObjectId::FixtureDefinition(id) => {
                            remove::<FixtureDefinition>(project, id)?
                        }
                        ObjectId::Curve(id) => remove::<CurveDefinition>(project, id)?,
                        ObjectId::Gradient(id) => remove::<GradientDefinition>(project, id)?,
                        ObjectId::Script(id) => remove::<ScriptDefinition>(project, id)?,
                    },
                    ProjectEdit::PutMember(site, value) => project.put_member(&site, value)?,
                    ProjectEdit::RemoveMember(site) => {
                        if !project.remove_at(&site) {
                            return Err(EditRejection::MissingMember);
                        }
                    }
                }
            }
            Ok(())
        })
    }

    /// Write one object; see [`ProjectEdit::Put`].
    pub fn put(&mut self, object: impl Into<Object>) -> Result<(), crate::EditError> {
        self.apply_edits([ProjectEdit::Put(object.into())])
    }
}
