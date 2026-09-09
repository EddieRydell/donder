use dawn_project_io::{ProjectSession, SourceObjectKind};

use super::{ResolvedGuiObject, blocked};
use crate::dto::{GuiDocument, GuiObjectRef, ObjectKind, ProjectGuiDocument};

pub(super) fn project_root(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    if session.project.root.id.0 != resolved.identity {
        return blocked("The requested project is missing.", Vec::new());
    }

    GuiDocument::Project {
        document: ProjectGuiDocument {
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            setup: object_ref(&session.project.root.setup.0, SourceObjectKind::Setup),
            sequences: session
                .project
                .root
                .sequences
                .iter()
                .map(|sequence| object_ref(&sequence.0, SourceObjectKind::Sequence))
                .collect(),
        },
    }
}

fn object_ref(
    identity: &dawn_language::identity::SourceIdentity,
    kind: SourceObjectKind,
) -> GuiObjectRef {
    GuiObjectRef {
        module_id: identity.module_id().to_string(),
        path: identity.document().to_string(),
        object_key: identity.object().to_string(),
        kind: ObjectKind::from(&kind),
        id: identity.object().to_string(),
    }
}
