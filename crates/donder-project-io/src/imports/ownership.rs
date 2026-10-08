use super::*;
use donder_model::{ObjectIdentity, OwnedObjectSlot};

impl Loader {
    /// A declared object, or an object it owns: the owner's reference
    /// followed by the owning fields, `show.setup.controllers.main`.
    pub(crate) fn resolve_object_reference(
        &self,
        document: &DocumentId,
        reference: &Reference,
        expected: SourceObjectKind,
    ) -> Result<ObjectIdentity, LoadProjectError> {
        let (root, mut rest) = self.resolve_declared(document, reference)?;
        let mut kind = root.source_kind();
        let mut identity = ObjectIdentity::from(root.source_identity().clone());
        while let Some((field, after)) = rest.split_first() {
            let (slot, after) = match field.value.as_str() {
                "setup" => (OwnedObjectSlot::Setup, after),
                "layout" => (OwnedObjectSlot::Layout, after),
                "patch" => (OwnedObjectSlot::Patch, after),
                "controllers" | "sequences" => {
                    let (member, after) = after
                        .split_first()
                        .ok_or_else(|| self.unresolved(document, reference))?;
                    let member = member.value.clone();
                    if field.value.as_str() == "controllers" {
                        (OwnedObjectSlot::Controller(member), after)
                    } else {
                        (OwnedObjectSlot::Sequence(member), after)
                    }
                }
                _ => return Err(self.unresolved(document, reference)),
            };
            let member = matches!(
                slot,
                OwnedObjectSlot::Controller(_) | OwnedObjectSlot::Sequence(_)
            );
            kind = kind
                .owned_child_kind(&slot)
                .ok_or_else(|| self.unresolved(document, reference))?;
            identity = identity.owned(slot);
            if member
                && let Some(segment) = rest.get(1)
                && let Some(target) = self.member_span(&identity)
            {
                self.link(document, segment.span, target);
            }
            rest = after;
        }
        if kind != expected {
            return Err(self.invalid(
                document,
                reference.span,
                format!(
                    "`{}` is {}, not {}",
                    reference.text(),
                    kind_name(&kind),
                    kind_name(&expected)
                ),
            ));
        }
        Ok(identity)
    }
}

pub fn ensure_document_can_reference_object(
    session: &mut ProjectSession,
    from: &DocumentId,
    identity: &ObjectIdentity,
) -> Result<(), ExportProjectError> {
    let root = identity.root_source();
    let kind = session
        .source
        .documents
        .get(root.document_id())
        .and_then(|document| {
            document
                .objects
                .iter()
                .find(|object| object.id == root.object())
        })
        .map(|object| object.kind.clone())
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from.path().to_owned(),
            reference: root.object().into(),
            message: "Object owner is missing.".into(),
        })?;
    ensure_document_can_reference_source(session, from, kind, root)
}

/// Named local sources available for an explicit GUI link choice.
pub fn available_reusable_sources(
    session: &ProjectSession,
    owner: &DocumentId,
    kinds: &[SourceObjectKind],
) -> Vec<(SourceObjectKind, SourceIdentity)> {
    session
        .source
        .documents
        .iter()
        .filter(|(id, _)| id.module_id() == owner.module_id())
        .flat_map(|(id, document)| {
            document
                .objects
                .iter()
                .filter(|object| kinds.contains(&object.kind))
                .map(move |object| {
                    (
                        object.kind.clone(),
                        SourceIdentity::from_document(id.clone(), object.id.clone()),
                    )
                })
        })
        .collect()
}

/// Explicitly selecting an existing source establishes its document import.
/// This differs from ordinary reference validation, which never infers scope.
pub fn link_reusable_source(
    session: &mut ProjectSession,
    owner: &DocumentId,
    kind: SourceObjectKind,
    source: &SourceIdentity,
) -> Result<(), ExportProjectError> {
    validate_reference_target(session, owner, &kind, source)?;
    ensure_document_can_reference_source(session, owner, kind, source)
}
