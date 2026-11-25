use super::*;

#[derive(Debug)]
pub enum GuiMutationError {
    /// The edit cannot be made, for the reason the message gives.
    Invalid(String),
    /// A value the edit sets at a field of the open editor cannot be made,
    /// for the reason the message gives.
    At(crate::dto::GuiSite, String),
    /// The model refused the edit.
    Rejected(donder_model::EditRejection),
    /// The edited project breaks a project rule.
    Project(donder_model::ProjectValidationError),
}

impl GuiMutationError {
    /// This error at `site`, unless it already names where it lies.
    pub(crate) fn at(self, site: crate::dto::GuiSite) -> Self {
        match self {
            Self::Invalid(message) => Self::At(site, message),
            Self::Rejected(rejection) => Self::At(site, rejection.to_string()),
            error => error,
        }
    }
}

impl From<donder_model::EditRejection> for GuiMutationError {
    fn from(rejection: donder_model::EditRejection) -> Self {
        Self::Rejected(rejection)
    }
}

impl From<donder_model::EditError> for GuiMutationError {
    fn from(error: donder_model::EditError) -> Self {
        match error {
            donder_model::EditError::Rejected(rejection) => Self::Rejected(rejection),
            donder_model::EditError::Invalid(error) => Self::Project(error),
        }
    }
}

impl From<donder_project_io::SessionEditError> for GuiMutationError {
    fn from(error: donder_project_io::SessionEditError) -> Self {
        match error {
            donder_project_io::SessionEditError::Model(error) => error.into(),
            donder_project_io::SessionEditError::Source(message) => Self::Invalid(message),
        }
    }
}

pub fn blocked(reason: impl Into<String>, diagnostics: Vec<IoDiagnostic>) -> GuiDocument {
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
            return blocked(message, Vec::new());
        }
    };
    use donder_model::ObjectKind as Kind;
    let mut gui = match resolved.kind {
        Kind::Project => project_root(session, &resolved),
        Kind::Sequence => project_sequence(session, &resolved),
        Kind::Controller => super::controller::project_document(session, &resolved),
        Kind::InputDevice => super::input_device::project_document(session, &resolved),
        Kind::Console => super::console::project_console(session, &resolved),
        Kind::Layout => project_layout(session, &resolved),
        Kind::FixtureDefinition => project_fixture(session, &resolved),
        Kind::Curve => super::library::project_curve(session, &resolved),
        Kind::Gradient => super::library::project_gradient(session, &resolved),
        Kind::Script => {
            unreachable!("requests resolve only to objects with a GUI editor")
        }
    };
    match &mut gui {
        GuiDocument::Project { document } => document.path.clone_from(&request.path),
        GuiDocument::Layout { document } => document.path.clone_from(&request.path),
        GuiDocument::Fixture { document } => document.path.clone_from(&request.path),
        GuiDocument::Curve { document } => document.path.clone_from(&request.path),
        GuiDocument::Gradient { document } => document.path.clone_from(&request.path),
        GuiDocument::Controller { document } => document.path.clone_from(&request.path),
        GuiDocument::InputDevice { document } => document.path.clone_from(&request.path),
        GuiDocument::Sequence { .. }
        | GuiDocument::Console { .. }
        | GuiDocument::Blocked { .. } => {}
    }
    gui
}

/// The open document after an edit from `before` to `after`. A sequence edit
/// that changed no store besides sequences sends only the clips that are new
/// allocations in `after`.
pub fn project_gui_document_change(
    before: &ProjectSession,
    after: &ProjectSession,
    request: &GuiDocumentRequest,
) -> GuiDocumentChange {
    if after.project().only_sequences_differ_from(before.project())
        && let Ok(resolved) = resolve_request(after, request)
        && let Ok(id) = SequenceId::try_from(resolved.object)
        && let (Some(previous), Some(sequence)) = (
            before.project().get::<donder_model::Sequence>(id),
            after.project().get::<donder_model::Sequence>(id),
        )
    {
        let previous = previous
            .composition
            .clips
            .iter()
            .map(|clip| (clip.id.0, clip))
            .collect::<std::collections::HashMap<_, _>>();
        let changed = |clip: &std::sync::Arc<Clip>| {
            !previous
                .get(&clip.id.0)
                .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, clip))
        };
        if let GuiDocument::Sequence { document } =
            super::projection::project_sequence_clips(after, &resolved, &changed)
        {
            return GuiDocumentChange::SequenceClips {
                document,
                clip_ids: sequence
                    .composition
                    .clips
                    .iter()
                    .map(|clip| clip.id.0)
                    .collect(),
            };
        }
    }
    GuiDocumentChange::Document {
        document: project_gui_document(Some(after), request),
    }
}

/// The inspector's view of `clip_ids` in a sequence or console request's document.
pub fn sequence_clip_details(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    clip_ids: &[u32],
) -> Result<Vec<SequenceClipDetails>, String> {
    let resolved = resolve_request(session, request)?;
    if !matches!(
        resolved.kind,
        donder_model::ObjectKind::Sequence | donder_model::ObjectKind::Console
    ) {
        return Err("Clip details belong to sequence and console documents.".into());
    }
    super::projection::sequence_clip_details(session, &resolved, clip_ids)
}

pub fn affected_paths(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<BTreeSet<String>, GuiMutationError> {
    let resolved = resolve_request(session, request).map_err(GuiMutationError::Invalid)?;
    use donder_model::ObjectKind as Kind;
    if matches!(
        resolved.kind,
        Kind::Project
            | Kind::Controller
            | Kind::InputDevice
            | Kind::FixtureDefinition
            | Kind::Layout
    ) {
        return Ok(session
            .source()
            .documents()
            .keys()
            .map(|document| document.path().to_string())
            .collect());
    }
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

#[derive(Clone)]
pub struct ResolvedGuiObject {
    /// The object's declaration.
    pub(crate) identity: SourceIdentity,
    pub(crate) kind: donder_model::ObjectKind,
    /// The object.
    pub object: donder_model::ObjectId,
}

impl ResolvedGuiObject {
    /// The kind of the resolved object.
    pub fn kind(&self) -> donder_model::ObjectKind {
        self.kind
    }

    /// The resolved object as the id its editor works on. Requests resolve
    /// only to objects their view edits.
    pub(crate) fn id<I: TryFrom<donder_model::ObjectId>>(&self) -> I {
        I::try_from(self.object)
            .unwrap_or_else(|_| unreachable!("resolved objects match their editor"))
    }

    /// The resolved object in `session`, the session it resolved against:
    /// its inventory declares exactly its project's objects.
    pub(crate) fn view<'s, T: donder_model::ProjectObject>(
        &self,
        session: &'s ProjectSession,
    ) -> donder_model::View<'s, T> {
        session
            .project()
            .view(self.id())
            .unwrap_or_else(|| unreachable!("resolved objects are in their session"))
    }

    /// The document the resolved object is written in.
    pub(crate) fn document_id(&self) -> &DocumentId {
        self.identity.document_id()
    }

    pub fn source_ref(&self) -> GuiObjectRef {
        GuiObjectRef {
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: self.kind.into(),
        }
    }
}

pub fn resolve_request(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
) -> Result<ResolvedGuiObject, String> {
    let path = Utf8Path::new(&request.path);
    let document_id = crate::source_documents::document_for_editor_path(session, path)
        .ok_or("No matching GUI document was found for this request.")?;
    let document = &session.source().documents()[&document_id];
    let declared = document
        .objects()
        .iter()
        .find(|object| object.name().as_str() == request.object_key)
        .ok_or("No matching GUI object was found for this request.")?;
    let resolved = ResolvedGuiObject {
        identity: SourceIdentity::from_document(
            document_id.clone(),
            declared.name().as_str().to_string(),
        ),
        kind: declared.kind(),
        object: declared.id(),
    };
    if gui_editor_order(resolved.kind).is_none() {
        return Err("This object has no GUI editor; edit it as text.".into());
    }
    Ok(resolved)
}

/// Where a GUI editor of an object of `kind` is offered among a document's
/// objects; `None` for scripts, which are edited only as text.
pub fn gui_editor_order(kind: donder_model::ObjectKind) -> Option<u8> {
    use donder_model::ObjectKind as Kind;
    Some(match kind {
        Kind::Project => 0,
        Kind::Sequence => 1,
        Kind::Console => 2,
        Kind::Layout => 3,
        Kind::FixtureDefinition => 4,
        Kind::Controller => 5,
        Kind::InputDevice => 6,
        Kind::Curve => 7,
        Kind::Gradient => 8,
        Kind::Script => return None,
    })
}
