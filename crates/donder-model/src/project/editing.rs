use super::objects::Stored;
use super::*;

/// Any project object, as an edit writes it. Definitions that may also be
/// written in place carry no id of their own, so theirs is given beside them.
#[derive(Clone, Debug)]
pub enum Object {
    Project(ProjectRoot),
    Setup(Setup),
    Layout(Layout),
    Patch(Patch),
    Controller(Controller),
    InputDevice(InputDevice),
    Sequence(Sequence),
    Rig(Rig),
    FixtureDefinition(FixtureDefinitionId, FixtureDefinition),
    Curve(CurveId, CurveDefinition),
    Gradient(GradientId, GradientDefinition),
    EffectDefinition(EffectDefinition),
    OperatorDefinition(OperatorDefinition),
}

impl Object {
    pub fn id(&self) -> ObjectId {
        match self {
            Self::Project(value) => value.id.into(),
            Self::Setup(value) => value.id.into(),
            Self::Layout(value) => value.id.into(),
            Self::Patch(value) => value.id.into(),
            Self::Controller(value) => value.id.into(),
            Self::InputDevice(value) => value.id.into(),
            Self::Sequence(value) => value.id.into(),
            Self::Rig(value) => value.id.into(),
            Self::FixtureDefinition(id, _) => (*id).into(),
            Self::Curve(id, _) => (*id).into(),
            Self::Gradient(id, _) => (*id).into(),
            Self::EffectDefinition(value) => (*value.id()).into(),
            Self::OperatorDefinition(value) => (*value.id()).into(),
        }
    }
}

macro_rules! object_from {
    ($($kind:ident),*) => {$(
        impl From<$kind> for Object {
            fn from(value: $kind) -> Self {
                Self::$kind(value)
            }
        }
    )*};
}
object_from!(Setup, Layout, Patch, Controller, InputDevice, Sequence, Rig);

impl From<ProjectRoot> for Object {
    fn from(value: ProjectRoot) -> Self {
        Self::Project(value)
    }
}
impl From<EffectDefinition> for Object {
    fn from(value: EffectDefinition) -> Self {
        Self::EffectDefinition(value)
    }
}
impl From<OperatorDefinition> for Object {
    fn from(value: OperatorDefinition) -> Self {
        Self::OperatorDefinition(value)
    }
}

/// Detached authored changes. Every batch is checked as one operation, allowing
/// related values to change together without exposing a mutable project draft.
#[derive(Clone, Debug)]
pub enum ProjectEdit {
    /// Write an object under its id: in place of the object with that id,
    /// reusable or owned, or as a new reusable object.
    Put(Object),
    /// Remove a reusable object. An owned object goes with its owner's edit.
    Remove(ObjectId),
}

fn put<T: Stored>(project: &mut DonderProject, id: T::Id, value: T) {
    match T::find_mut(project, id) {
        Some(slot) => *slot = value,
        None => {
            T::store_mut(project).insert(id, value);
        }
    }
}

fn remove<T: Stored>(project: &mut DonderProject, id: T::Id) -> Result<(), EditRejection> {
    T::store_mut(project)
        .shift_remove(&id)
        .map(drop)
        .ok_or(EditRejection::NotReusable)
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
                        Object::Setup(value) => put(project, value.id, value),
                        Object::Layout(value) => put(project, value.id, value),
                        Object::Patch(value) => put(project, value.id, value),
                        Object::Controller(value) => put(project, value.id, value),
                        Object::InputDevice(value) => put(project, value.id, value),
                        Object::Sequence(value) => put(project, value.id, value),
                        Object::Rig(value) => put(project, value.id, value),
                        Object::FixtureDefinition(id, value) => put(project, id, value),
                        Object::Curve(id, value) => put(project, id, value),
                        Object::Gradient(id, value) => put(project, id, value),
                        Object::EffectDefinition(value) => put(project, *value.id(), value),
                        Object::OperatorDefinition(value) => put(project, *value.id(), value),
                    },
                    ProjectEdit::Remove(id) => match id {
                        ObjectId::Project(_) => {
                            return Err(EditRejection::ProjectRemoval);
                        }
                        ObjectId::Setup(id) => remove::<Setup>(project, id)?,
                        ObjectId::Layout(id) => remove::<Layout>(project, id)?,
                        ObjectId::Patch(id) => remove::<Patch>(project, id)?,
                        ObjectId::Controller(id) => remove::<Controller>(project, id)?,
                        ObjectId::InputDevice(id) => remove::<InputDevice>(project, id)?,
                        ObjectId::Sequence(id) => remove::<Sequence>(project, id)?,
                        ObjectId::Rig(id) => remove::<Rig>(project, id)?,
                        ObjectId::FixtureDefinition(id) => {
                            remove::<FixtureDefinition>(project, id)?
                        }
                        ObjectId::Curve(id) => remove::<CurveDefinition>(project, id)?,
                        ObjectId::Gradient(id) => remove::<GradientDefinition>(project, id)?,
                        ObjectId::EffectDefinition(id) => remove::<EffectDefinition>(project, id)?,
                        ObjectId::OperatorDefinition(id) => {
                            remove::<OperatorDefinition>(project, id)?
                        }
                    },
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
