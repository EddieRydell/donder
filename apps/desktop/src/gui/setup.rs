use dawn_language::controller::ControllerId;
use dawn_language::setup::SetupId;
use dawn_project_io::{ProjectSession, SourceObjectKind};

use super::model::source_identity_from_gui;
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, SetupGuiDocument, SetupGuiEdit};

pub(crate) mod authoring;
mod copies;
use super::patch;

pub(super) fn project_setup(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let Some(setup) = session
        .project
        .setups
        .get(&SetupId(resolved.identity.clone()))
    else {
        return blocked("The requested setup is missing.", Vec::new());
    };
    let controllers = setup
        .controllers
        .iter()
        .map(|id| {
            session
                .project
                .controllers
                .get(id)
                .map(|controller| super::controller::project_controller(session, id, controller))
                .ok_or_else(|| format!("Controller {} was not found.", id.0.object()))
        })
        .collect::<Result<_, _>>();
    let controllers = match controllers {
        Ok(controllers) => controllers,
        Err(error) => return blocked(error, Vec::new()),
    };
    GuiDocument::Setup {
        document: SetupGuiDocument {
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            layout_ref: patch::object_ref(&setup.layout.0, SourceObjectKind::Layout),
            patch_ref: patch::object_ref(&setup.patch.0, SourceObjectKind::Patch),
            layout_read_only: !session
                .source
                .is_project_owned(setup.layout.0.document_id()),
            patch_read_only: !session.source.is_project_owned(setup.patch.0.document_id()),
            controllers,
            available_controllers: session
                .project
                .controllers
                .iter()
                .filter(|(id, _)| !setup.controllers.contains(id))
                .map(|(id, controller)| {
                    super::controller::project_controller(session, id, controller)
                })
                .collect(),
        },
    }
}

pub(super) fn edit_setup(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SetupGuiEdit,
) -> Result<(), GuiMutationError> {
    let setup = session
        .project
        .setups
        .get(&SetupId(resolved.identity.clone()))
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("The requested setup is missing.".to_string()))?;
    match edit {
        SetupGuiEdit::CopyLayout => copies::copy_layout(session, &setup)?,
        SetupGuiEdit::CopyController { controller } => {
            authoring::copy_controller(session, &setup, controller)?;
        }
        SetupGuiEdit::AttachController { controller } => {
            let identity = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            dawn_language::setup::authoring::attach_controller(
                &mut session.project,
                &setup.id,
                ControllerId(identity.clone()),
            )
            .map_err(GuiMutationError::Invalid)?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.id.0.document_id(),
                dawn_project_io::SourceObjectKind::Controller,
                &identity,
            )
            .map_err(|error| GuiMutationError::Invalid(format!("{error:?}")))?;
        }
        SetupGuiEdit::DetachController {
            controller,
            remove_outputs,
        } => {
            let identity = source_identity_from_gui(
                &controller.module_id,
                &controller.path,
                &controller.object_key,
            )?;
            if remove_outputs {
                ensure_owned_target(session, &setup.patch.0)?;
            }
            dawn_language::setup::authoring::detach_controller(
                &mut session.project,
                &setup.id,
                &ControllerId(identity),
                remove_outputs,
            )
            .map_err(GuiMutationError::Invalid)?;
        }
        SetupGuiEdit::AddController { config, ports } => {
            let controller = super::controller::domain_controller(config, ports)?;
            let identity = create_object_document(
                session,
                dawn_project_io::SourceObjectKind::Controller,
                "controller",
                "controllers",
                "controller",
            )?;
            dawn_project_io::ensure_document_can_reference_source(
                session,
                setup.id.0.document_id(),
                dawn_project_io::SourceObjectKind::Controller,
                &identity,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            let id = ControllerId(identity);
            session.project.controllers.insert(id.clone(), controller);
            session
                .project
                .setups
                .get_mut(&setup.id)
                .ok_or_else(|| GuiMutationError::Invalid("Setup was not found.".into()))?
                .controllers
                .push(id);
        }
    }
    Ok(())
}

pub(super) fn ensure_owned_target(
    session: &ProjectSession,
    identity: &dawn_language::identity::SourceIdentity,
) -> Result<(), GuiMutationError> {
    if session.source.is_project_owned(identity.document_id()) {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(format!(
            "{} belongs to a dependency. Make a project-owned copy before editing it.",
            identity.object()
        )))
    }
}

pub(super) fn source_key(id: &dawn_language::identity::SourceIdentity) -> String {
    format!("{}#{}", id.document(), id.object())
}

pub(super) fn create_object_document(
    session: &mut ProjectSession,
    kind: dawn_project_io::SourceObjectKind,
    name: &str,
    directory: &str,
    suffix: &str,
) -> Result<dawn_language::identity::SourceIdentity, GuiMutationError> {
    let mut key = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>();
    while key.contains("__") {
        key = key.replace("__", "_");
    }
    key = key.trim_matches('_').to_string();
    if key.is_empty() || key.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        key = format!("item_{key}");
    }
    for index in 1_u32.. {
        let stem = if index == 1 {
            key.clone()
        } else {
            format!("{key}_{index}")
        };
        let path = camino::Utf8PathBuf::from(format!("{directory}/{stem}.{suffix}.dawn"));
        let document = session.source.project_document(path.clone());
        if session.source.documents.contains_key(&document)
            || session.source.project_root().join(&path).exists()
        {
            continue;
        }
        return session
            .source
            .add_yaml_document(path, vec![(kind, stem)])
            .map_err(GuiMutationError::Invalid)?
            .into_iter()
            .next()
            .ok_or_else(|| GuiMutationError::Invalid("New document has no object.".into()));
    }
    Err(GuiMutationError::Invalid(
        "No source document names remain.".into(),
    ))
}
