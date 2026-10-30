use super::*;

#[derive(Debug)]
pub enum GuiMutationError {
    Blocked(String),
    Invalid(String),
    /// The model refused the edit.
    Rejected(donder_model::EditRejection),
    /// The edited project breaks a project rule.
    Project(donder_model::ProjectValidationError),
}

impl GuiMutationError {
    pub fn message(&self) -> String {
        match self {
            Self::Blocked(message) | Self::Invalid(message) => message.clone(),
            Self::Rejected(rejection) => rejection.to_string(),
            Self::Project(error) => error.to_string(),
        }
    }

    /// This error with a broken project rule named by the object it concerns in
    /// `session`, the session the edit was made in.
    pub fn named(self, session: &ProjectSession) -> Self {
        match self {
            Self::Project(error) => {
                Self::Invalid(donder_project_io::validation_message(session, &error))
            }
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
        Kind::Setup => project_setup(session, &resolved),
        Kind::Controller => super::controller::project_document(session, &resolved),
        Kind::InputDevice => super::input_device::project_document(session, &resolved),
        Kind::Console => super::console::project_console(session, &resolved),
        Kind::Patch => super::patch::project_document(session, &resolved),
        Kind::Layout => project_layout(session, &resolved),
        Kind::FixtureDefinition => project_fixture(session, &resolved),
        Kind::Curve => super::library::project_curve(session, &resolved),
        Kind::Gradient => super::library::project_gradient(session, &resolved),
        Kind::EffectDefinition | Kind::OperatorDefinition => {
            unreachable!("requests resolve only to objects with a GUI editor")
        }
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
        GuiDocument::InputDevice { document } => document.path.clone_from(&request.path),
        GuiDocument::Console { document } => document.path.clone_from(&request.path),
        GuiDocument::Blocked { .. } => {}
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
    if after.project.only_sequences_differ_from(&before.project)
        && let Ok(resolved) = resolve_request(after, request)
        && let Ok(id) = SequenceId::try_from(resolved.object)
        && let (Some(previous), Some(sequence)) = (
            before.project.get::<donder_model::Sequence>(id),
            after.project.get::<donder_model::Sequence>(id),
        )
    {
        let previous = previous
            .clips
            .iter()
            .map(|clip| (clip.id.0, clip))
            .collect::<std::collections::HashMap<_, _>>();
        let changed = |clip: &std::sync::Arc<Clip>| {
            !previous
                .get(&clip.id.0)
                .is_some_and(|previous| std::sync::Arc::ptr_eq(previous, clip))
        };
        if let GuiDocument::Sequence { mut document } =
            super::projection::project_sequence_clips(after, &resolved, &changed)
        {
            document.path.clone_from(&request.path);
            return GuiDocumentChange::SequenceClips {
                document,
                clip_ids: sequence.clips.iter().map(|clip| clip.id.0).collect(),
            };
        }
    }
    GuiDocumentChange::Document {
        document: project_gui_document(Some(after), request),
    }
}

/// The inspector's view of `clip_ids` in a sequence request's document.
pub fn sequence_clip_details(
    session: &ProjectSession,
    request: &GuiDocumentRequest,
    clip_ids: &[u32],
) -> Result<Vec<SequenceClipDetails>, String> {
    let resolved = resolve_request(session, request)?;
    if resolved.kind != donder_model::ObjectKind::Sequence {
        return Err("Clip details belong to sequence documents.".into());
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
            | Kind::Setup
            | Kind::Patch
            | Kind::Controller
            | Kind::InputDevice
            | Kind::FixtureDefinition
            | Kind::Layout
    ) {
        return Ok(session
            .source
            .documents
            .keys()
            .map(|document| document.path().to_string())
            .collect());
    }
    Ok(BTreeSet::from([resolved.identity.document().to_string()]))
}

pub struct ResolvedGuiObject {
    pub(crate) owned_path: Vec<crate::dto::OwnedObjectSlot>,
    /// The declared object the address starts at.
    pub(crate) identity: SourceIdentity,
    pub(crate) kind: donder_model::ObjectKind,
    /// The object, or for a fixture definition a layout owns, that layout.
    pub object: donder_model::ObjectId,
}

impl ResolvedGuiObject {
    /// The resolved object's address. Owned paths are checked when the object
    /// is resolved, so every step converts.
    fn object_identity(&self) -> donder_project_io::ObjectAddress {
        self.owned_path.iter().fold(
            self.identity.clone().into(),
            |address: donder_project_io::ObjectAddress, step| {
                address.owned(
                    step.try_into()
                        .unwrap_or_else(|_| unreachable!("resolved owned paths are valid")),
                )
            },
        )
    }

    /// The resolved object as the id its editor works on. Requests resolve
    /// only to objects their view edits.
    pub(crate) fn id<I: TryFrom<donder_model::ObjectId>>(&self) -> I {
        I::try_from(self.object)
            .unwrap_or_else(|_| unreachable!("resolved objects match their editor"))
    }

    /// The document the resolved object is written in.
    pub(crate) fn document_id(&self) -> &DocumentId {
        self.identity.document_id()
    }

    pub fn source_ref(&self) -> GuiObjectRef {
        GuiObjectRef {
            owned_path: self.owned_path.clone(),
            path: self.identity.document().to_string(),
            object_key: self.identity.object().to_string(),
            kind: self.kind.into(),
            id: self.identity.object().to_string(),
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
    let document = &session.source.documents[&document_id];
    let declared = document
        .objects()
        .iter()
        .find(|object| object.name().as_str() == request.object_key)
        .ok_or("No matching GUI object was found for this request.")?;
    let mut resolved = ResolvedGuiObject {
        owned_path: request.owned_path.clone(),
        identity: SourceIdentity::from_document(
            document_id.clone(),
            declared.name().as_str().to_string(),
        ),
        kind: declared.kind(),
        object: declared.id(),
    };
    for step in &request.owned_path {
        resolved.kind = donder_project_io::OwnedObjectSlot::try_from(step)?
            .child_kind(resolved.kind)
            .ok_or("Invalid owned object path.")?;
    }
    if gui_editor_order(resolved.kind).is_none() {
        return Err("This object has no GUI editor; edit it as text.".into());
    }
    if !request.owned_path.is_empty() {
        let missing =
            || "Owned object was not found. Linked objects must be opened at their source.";
        let address = resolved.object_identity();
        resolved.object = match address.owned_path().last() {
            Some(donder_project_io::OwnedObjectSlot::Fixture(fixture)) => {
                let layout = address
                    .parent()
                    .and_then(|parent| session.resolve_address(&parent))
                    .and_then(|id| donder_model::LayoutId::try_from(id).ok())
                    .ok_or_else(missing)?;
                let inline = session
                    .project
                    .get::<donder_model::Layout>(layout)
                    .and_then(|layout| layout.fixture(donder_model::FixtureInstanceId(*fixture)))
                    .is_some_and(|fixture| {
                        matches!(
                            fixture.kind,
                            donder_model::LayoutFixtureKind::Fixture {
                                definition: donder_model::OwnedFixture::Inline(_),
                                ..
                            }
                        )
                    });
                if !inline {
                    return Err(missing().into());
                }
                layout.into()
            }
            _ => session.resolve_address(&address).ok_or_else(missing)?,
        };
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
        Kind::Setup => 3,
        Kind::Layout => 4,
        Kind::FixtureDefinition => 5,
        Kind::Patch => 6,
        Kind::Controller => 7,
        Kind::InputDevice => 8,
        Kind::Curve => 9,
        Kind::Gradient => 10,
        Kind::EffectDefinition | Kind::OperatorDefinition => return None,
    })
}
