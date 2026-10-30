//! Object descriptions: the prose a data object carries in place of comments,
//! shown and edited with the open object.
use donder_model::ControllerId;
use donder_model::LayoutId;
use donder_model::PatchId;
use donder_model::ProjectEdit;
use donder_model::SequenceId;
use donder_model::SetupId;
use donder_model::{ConsoleId, InputDeviceId};
use donder_model::{CurveId, GradientId};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, ResolvedGuiObject};

/// Text as a description: surrounding whitespace is not kept, and empty
/// text is no description.
pub(super) fn normalized(description: Option<String>) -> Option<String> {
    description
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

pub(super) fn edit(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    description: Option<String>,
) -> Result<(), GuiMutationError> {
    let description = normalized(description);
    let project = &session.project;
    let missing = || GuiMutationError::Invalid("The open object is missing.".into());
    use donder_model::ObjectKind as Kind;
    let edit = match resolved.kind {
        Kind::Project => {
            let mut root = project.root().clone();
            root.description = description;
            ProjectEdit::Put(donder_model::Object::Project(root))
        }
        Kind::Setup => {
            let id: SetupId = resolved.id();
            let mut value = project
                .get::<donder_model::Setup>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Layout => {
            let id: LayoutId = resolved.id();
            let mut value = project
                .get::<donder_model::Layout>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Patch => {
            let id: PatchId = resolved.id();
            let mut value = project
                .get::<donder_model::Patch>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Controller => {
            let id: ControllerId = resolved.id();
            let mut value = project
                .get::<donder_model::Controller>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::InputDevice => {
            let id: InputDeviceId = resolved.id();
            let mut value = project
                .get::<donder_model::InputDevice>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Console => {
            let id: ConsoleId = resolved.id();
            let mut value = project
                .get::<donder_model::Console>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Sequence => {
            let id: SequenceId = resolved.id();
            let mut value = project
                .get::<donder_model::Sequence>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(value.into())
        }
        Kind::Curve => {
            let id = resolved.id::<CurveId>();
            let mut value = project
                .get::<donder_model::CurveDefinition>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(donder_model::Object::Curve(id, value))
        }
        Kind::Gradient => {
            let id = resolved.id::<GradientId>();
            let mut value = project
                .get::<donder_model::GradientDefinition>(id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::Put(donder_model::Object::Gradient(id, value))
        }
        Kind::FixtureDefinition => {
            return super::fixture::update_fixture_definition(session, resolved, |fixture| {
                fixture.description = description;
                Ok(())
            });
        }
        Kind::EffectDefinition | Kind::OperatorDefinition => {
            return Err(GuiMutationError::Invalid(
                "Scripts describe their definitions in their text.".into(),
            ));
        }
    };
    session
        .project
        .apply_edits([edit])
        .map_err(GuiMutationError::from)
}
