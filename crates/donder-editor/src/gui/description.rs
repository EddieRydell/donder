//! Object descriptions: the prose a data object carries in place of comments,
//! shown and edited with the open object.
use donder_model::ProjectEdit;
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
    let project = session.project();
    use donder_model::ObjectKind as Kind;
    let edit = match resolved.kind {
        Kind::Project => {
            let mut root = project.root().clone();
            root.description = description;
            ProjectEdit::Put(donder_model::Object::Project(root))
        }
        Kind::Layout => {
            let mut value = donder_model::Layout::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put((resolved.id(), value).into())
        }
        Kind::Controller => {
            let mut value = donder_model::Controller::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put((resolved.id(), value).into())
        }
        Kind::InputDevice => {
            let mut value = donder_model::InputDevice::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put((resolved.id(), value).into())
        }
        Kind::Console => {
            let mut value = donder_model::Console::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put((resolved.id(), value).into())
        }
        Kind::Sequence => {
            let mut value = donder_model::Sequence::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put((resolved.id(), value).into())
        }
        Kind::Curve => {
            let mut value = donder_model::CurveDefinition::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put(donder_model::Object::Curve(resolved.id(), value))
        }
        Kind::Gradient => {
            let mut value = donder_model::GradientDefinition::clone(&resolved.view(session));
            value.description = description;
            ProjectEdit::Put(donder_model::Object::Gradient(resolved.id(), value))
        }
        Kind::FixtureDefinition => {
            return super::fixture::update_fixture_definition(session, resolved, |fixture| {
                fixture.description = description;
                Ok(())
            });
        }
        Kind::Script => {
            return Err(GuiMutationError::Invalid(
                "Scripts describe their definitions in their text.".into(),
            ));
        }
    };
    session.apply_edits([edit]).map_err(GuiMutationError::from)
}
