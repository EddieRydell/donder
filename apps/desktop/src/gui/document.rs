use super::*;

#[derive(Debug)]
pub enum GuiMutationError {
    Blocked(String),
    Invalid(String),
}

impl GuiMutationError {
    pub fn message(&self) -> &str {
        match self {
            Self::Blocked(message) | Self::Invalid(message) => message,
        }
    }
}

pub fn blocked(reason: impl Into<String>, diagnostics: Vec<ProjectDiagnostic>) -> GuiDocument {
    GuiDocument::Blocked {
        reason: reason.into(),
        diagnostics,
    }
}

pub fn project_gui_document(
    session: Option<&ProjectSession>,
    request: &GuiDocumentRequest,
) -> GuiDocument {
    let Some(session) = session else {
        return blocked("No project is loaded.", Vec::new());
    };
    let resolved = match resolve_request(session, request) {
        Ok(resolved) => resolved,
        Err(message) => {
            return blocked(
                message.clone(),
                vec![gui_diagnostic(&request.path, "gui.resolve", &message)],
            );
        }
    };
    let mut gui = match request.view {
        DocumentViewId::Project => project_root(session, &resolved),
        DocumentViewId::Sequence => project_sequence(session, &resolved),
        DocumentViewId::Setup => project_setup(session, &resolved),
        DocumentViewId::Controller => super::controller::project_document(session, &resolved),
        DocumentViewId::Patch => super::patch::project_document(session, &resolved),
        DocumentViewId::Layout => project_layout(session, &resolved),
        DocumentViewId::Fixture => project_fixture(session, &resolved),
        DocumentViewId::Curve => super::library::project_curve(session, &resolved),
        DocumentViewId::Gradient => super::library::project_gradient(session, &resolved),
        DocumentViewId::Text => blocked(
            "Text documents do not have a GUI projection.",
            vec![gui_diagnostic(
                &request.path,
                "gui.view",
                "Text documents do not have a GUI projection.",
            )],
        ),
    };
    match &mut gui {
        GuiDocument::Project { document } => document.path.clone_from(&request.path),
        GuiDocument::Setup { document } => document.path.clone_from(&request.path),
        GuiDocument::Sequence { document } => document.path.clone_from(&request.path),
        GuiDocument::Layout { document } => document.path.clone_from(&request.path),
        GuiDocument::Fixture { document } => document.path.clone_from(&request.path),
        GuiDocument::Curve { document } => document.path.clone_from(&request.path),
        GuiDocument::Gradient { document } => document.path.clone_from(&request.path),
        GuiDocument::Controller { document } => document.path.clone_from(&request.path),
        GuiDocument::Patch { document } => document.path.clone_from(&request.path),
        GuiDocument::Blocked { .. } => {}
    }
    gui
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    ensure_owned_gui_document(session, &resolved)?;
    if matches!(
        request.view,
        DocumentViewId::Setup
            | DocumentViewId::Patch
            | DocumentViewId::Controller
            | DocumentViewId::Fixture
            | DocumentViewId::Layout
    ) {
        return Ok(session
            .source
            .documents
            .keys()
            .filter(|document| session.source.is_project_owned(document))
            .map(|document| document.path().to_string())
            .collect());
    }
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

pub(crate) struct ResolvedGuiObject {
    pub(crate) identity: SourceIdentity,
    pub(crate) kind: SourceObjectKind,
}

impl ResolvedGuiObject {
    pub(crate) fn source_ref(&self) -> GuiObjectRef {
        GuiObjectRef {
            module_id: self.identity.module_id().to_string(),
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: ObjectKind::from(&self.kind),
            id: self.identity.object().to_string(),
        }
    }
}

pub(crate) fn ensure_owned_gui_document(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> Result<(), GuiMutationError> {
    if session
        .source
        .is_project_owned(resolved.identity.document_id())
    {
        Ok(())
    } else {
        Err(GuiMutationError::Blocked(
            "Imported dependency documents are read-only.".to_string(),
        ))
    }
}

pub(crate) fn resolve_request(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, String> {
    let path = Utf8Path::new(&request.path);
    let requested_key = request.object_key.as_deref();
    let document_id = crate::source_documents::document_for_editor_path(session, path)
        .ok_or("No matching GUI document was found for this request.")?;
    let document = &session.source.documents[&document_id];
    let mut matches = document
        .objects()
        .iter()
        .filter(|object| {
            ObjectKind::from(object.kind()).document_view().as_ref() == Some(&request.view)
        })
        .filter(|object| requested_key.is_none_or(|key| object.id() == key));
    let Some(source_id) = matches.next() else {
        return Err("No matching GUI object was found for this request.".to_string());
    };
    if matches.next().is_some() && requested_key.is_none() {
        return Err("GUI request must include an object key for this document.".to_string());
    }
    Ok(ResolvedGuiObject {
        identity: SourceIdentity::from_document(document_id.clone(), source_id.id().to_string()),
        kind: source_id.kind().clone(),
    })
}

pub(crate) fn gui_diagnostic(path: &str, code: &str, message: &str) -> ProjectDiagnostic {
    ProjectDiagnostic {
        path: path.to_string(),
        code: code.to_string(),
        severity: DiagnosticSeverity::Error,
        message: message.to_string(),
        range: None,
        detail: None,
        related: Vec::new(),
    }
}
