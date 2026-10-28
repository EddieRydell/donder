use super::*;
use crate::address::{ObjectAddress, OwnedObjectSlot};
use donder_model::ObjectKind;

impl Loader {
    /// A declared object, or an object it owns: the owner's reference
    /// followed by the owning fields, `show.setup.controllers.main`.
    pub(crate) fn resolve_object_reference(
        &self,
        document: &DocumentId,
        reference: &Reference,
        expected: ObjectKind,
    ) -> Result<ObjectAddress, LoadError> {
        let (root, mut rest) = self.resolve_declared(document, reference)?;
        let mut kind = root.kind();
        let mut identity = self.address(root);
        while let Some((field, after)) = rest.split_first() {
            let (slot, after) = match field.value.as_str() {
                "setup" => (OwnedObjectSlot::Setup, after),
                "layout" => (OwnedObjectSlot::Layout, after),
                "patch" => (OwnedObjectSlot::Patch, after),
                collection => {
                    let slot: fn(Identifier) -> OwnedObjectSlot = match collection {
                        "controllers" => OwnedObjectSlot::Controller,
                        "input_devices" => OwnedObjectSlot::InputDevice,
                        "sequences" => OwnedObjectSlot::Sequence,
                        "rigs" => OwnedObjectSlot::Rig,
                        _ => return Err(self.unresolved(document, reference)),
                    };
                    let (member, after) = after
                        .split_first()
                        .ok_or_else(|| self.unresolved(document, reference))?;
                    (slot(member.value.clone()), after)
                }
            };
            let member = matches!(
                slot,
                OwnedObjectSlot::Controller(_)
                    | OwnedObjectSlot::InputDevice(_)
                    | OwnedObjectSlot::Sequence(_)
                    | OwnedObjectSlot::Rig(_)
            );
            kind = slot
                .child_kind(kind)
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

/// Make `id`, a declared object or one owned by a declared object, visible
/// from `from`: its owner's document is imported when it is another one.
pub fn ensure_document_can_reference_object(
    session: &mut ProjectSession,
    from: &DocumentId,
    id: ObjectId,
) -> Result<(), ExportProjectError> {
    let address = session
        .address_of(id)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from.path().to_owned(),
            reference: String::new(),
            message: "Object is missing from the source inventory.".into(),
        })?;
    ensure_document_can_reference_address(session, from, &address)
}

pub(crate) fn ensure_document_can_reference_address(
    session: &mut ProjectSession,
    from: &DocumentId,
    address: &ObjectAddress,
) -> Result<(), ExportProjectError> {
    let root = address.root_source();
    let kind = session
        .declared_id(root)
        .map(ObjectId::kind)
        .ok_or_else(|| ExportProjectError::InvalidReference {
            path: from.path().to_owned(),
            reference: root.object().into(),
            message: "Object owner is missing.".into(),
        })?;
    ensure_document_can_reference_source(session, from, kind, root)
}

/// Declared objects available for an explicit GUI link choice.
pub fn available_declarations(session: &ProjectSession, kinds: &[ObjectKind]) -> Vec<ObjectId> {
    session
        .source
        .documents
        .values()
        .flat_map(|document| &document.objects)
        .filter(|object| kinds.contains(&object.kind()))
        .map(|object| object.id)
        .collect()
}

/// Explicitly selecting an existing declared object establishes its document
/// import. This differs from ordinary reference validation, which never
/// infers scope.
pub fn link_declaration(
    session: &mut ProjectSession,
    owner: &DocumentId,
    source: ObjectId,
) -> Result<(), ExportProjectError> {
    let identity =
        session
            .source_identity(source)
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: owner.path().to_owned(),
                reference: String::new(),
                message: "Owned values cannot be linked.".into(),
            })?;
    ensure_document_can_reference_source(session, owner, source.kind(), &identity)
}
