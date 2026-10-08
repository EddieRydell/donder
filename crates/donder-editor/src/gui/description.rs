//! Object descriptions: the prose a data object carries in place of comments,
//! shown and edited with the open object.
use donder_model::ControllerId;
use donder_model::LayoutId;
use donder_model::PatchId;
use donder_model::ProjectEdit;
use donder_model::SequenceId;
use donder_model::SetupId;
use donder_model::{CurveId, GradientId};
use donder_project_io::ProjectSession;

use super::{GuiMutationError, ResolvedGuiObject};
use donder_sequence_api::DocumentViewId;

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
    view: &DocumentViewId,
    description: Option<String>,
) -> Result<(), GuiMutationError> {
    let description = normalized(description);
    let project = &session.project;
    let identity = resolved.object_identity();
    let missing = || GuiMutationError::Invalid("The open object is missing.".into());
    let edit = match view {
        DocumentViewId::Project => {
            let mut root = project.root().clone();
            root.description = description;
            ProjectEdit::ReplaceRoot(root)
        }
        DocumentViewId::Setup => {
            let id = SetupId(identity);
            let mut value = project.setup(&id).cloned().ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::ReplaceSetup { id, value }
        }
        DocumentViewId::Layout => {
            let id = LayoutId(identity);
            let mut value = project.layout(&id).cloned().ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::ReplaceLayout { id, value }
        }
        DocumentViewId::Patch => {
            let id = PatchId(identity);
            let mut value = project.patch(&id).cloned().ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::ReplacePatch { id, value }
        }
        DocumentViewId::Controller => {
            let id = ControllerId(identity);
            let mut value = project.controller(&id).cloned().ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::ReplaceController { id, value }
        }
        DocumentViewId::Sequence => {
            let id = SequenceId(identity);
            let mut value = project.sequence(&id).cloned().ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::ReplaceSequence { id, value }
        }
        DocumentViewId::Curve => {
            let id = CurveId(resolved.identity.clone());
            let mut value = project
                .definitions()
                .curves
                .get(&id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::SetCurveDefinition { id, value }
        }
        DocumentViewId::Gradient => {
            let id = GradientId(resolved.identity.clone());
            let mut value = project
                .definitions()
                .gradients
                .get(&id)
                .cloned()
                .ok_or_else(missing)?;
            value.description = description;
            ProjectEdit::SetGradientDefinition { id, value }
        }
        DocumentViewId::Fixture => {
            return super::fixture::update_fixture_definition(session, resolved, |fixture| {
                fixture.description = description;
                Ok(())
            });
        }
        DocumentViewId::Text => {
            return Err(GuiMutationError::Invalid(
                "Text documents have no description.".into(),
            ));
        }
    };
    session
        .project
        .apply_edits([edit])
        .map_err(GuiMutationError::Invalid)
}
