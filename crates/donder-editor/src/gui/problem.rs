//! Where a refused edit shows: the site of the open editor a broken rule or
//! a refused value concerns, from the model's problem site and the edit.
use donder_model::ProjectValidationError;
use donder_project_io::ProjectSession;

use super::{GuiMutationError, apply_edit, resolve_request};
use crate::dto::{GuiDocumentRequest, GuiEdit, GuiProblem, GuiSite};

/// What a person reads about `error`, raised by an edit of the object
/// `request` opens in `before`, and the site it concerns. `edited` is the
/// session the edit was made in; `edit_site` is the site the edit wrote.
pub fn refusal(
    before: &ProjectSession,
    edited: &ProjectSession,
    request: &GuiDocumentRequest,
    edit_site: Option<GuiSite>,
    error: GuiMutationError,
) -> GuiProblem {
    match error {
        GuiMutationError::Invalid(message) => problem(message, edit_site),
        GuiMutationError::At(site, message) => problem(message, Some(site)),
        GuiMutationError::Rejected(rejection) => problem(rejection.to_string(), edit_site),
        GuiMutationError::Project(rule) => match concerned_site(before, request, &rule) {
            Some(site) => problem(rule.problem.to_string(), Some(merged(site, edit_site))),
            None => problem(
                donder_project_io::validation_message(edited, &rule),
                edit_site,
            ),
        },
    }
}

/// The problem `edit` of the object `request` opens would raise; none when
/// it would be accepted. The session does not change.
pub fn check_edit(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    edit: GuiEdit,
) -> Option<GuiProblem> {
    let site = edit.site();
    let mut draft = session.clone();
    let error = apply_edit(&mut draft, request, edit).err()?;
    Some(refusal(session, &draft, request, site, error))
}

/// A problem whose message starts as a sentence.
pub(crate) fn problem(message: String, site: Option<GuiSite>) -> GuiProblem {
    let mut chars = message.chars();
    let message = chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default();
    GuiProblem { message, site }
}

/// The site of a broken rule in the open editor: the rule concerns the open
/// object or one the edit created.
fn concerned_site(
    before: &ProjectSession,
    request: &GuiDocumentRequest,
    rule: &ProjectValidationError,
) -> Option<GuiSite> {
    let resolved = resolve_request(before, request).ok()?;
    let concerns = rule.object == resolved.object || before.source_identity(rule.object).is_none();
    concerns.then(|| GuiSite::from(&rule.site()))
}

/// The model's site, with the field of the edit when the model names only
/// the member the edit wrote.
fn merged(site: GuiSite, edit_site: Option<GuiSite>) -> GuiSite {
    match edit_site {
        Some(edited) if site.field.is_none() && edited.member == site.member => edited,
        _ => site,
    }
}
