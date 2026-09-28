use super::*;
use donder_language::identity::{ObjectIdentity, OwnedObjectSlot};

impl Loader {
    pub(crate) fn resolve_object_reference(
        &self,
        document: &DocumentId,
        value: &Value,
        expected: SourceObjectKind,
    ) -> Result<ObjectIdentity, LoadProjectError> {
        let error = |message: &str| LoadProjectError::InvalidDocument {
            path: document.path().to_owned(),
            range: source_range_for_value(document.path(), value),
            message: message.into(),
        };
        if let Some(reference) = value.as_str() {
            let object = self.resolve_reference(document, reference)?;
            if object.source_kind() != expected {
                return Err(error("Reference has the wrong object type."));
            }
            return Ok(object.source_identity().clone().into());
        }
        parse_mapping(document.path(), value, "owned object address", |fields| {
            let root = self.resolve_reference(document, fields.string("owner")?)?;
            let mut kind = root.source_kind();
            let mut identity = ObjectIdentity::from(root.source_identity().clone());
            let path = fields.sequence("path")?;
            if path.is_empty() {
                return Err(error("Owned object addresses require a non-empty path."));
            }
            for part in path {
                let slot = match part.as_str() {
                    Some("setup") => OwnedObjectSlot::Setup,
                    Some("layout") => OwnedObjectSlot::Layout,
                    Some("patch") => OwnedObjectSlot::Patch,
                    Some(_) => return Err(error("Unknown ownership slot.")),
                    None => parse_mapping(
                        document.path(),
                        part,
                        "collection ownership slot",
                        |fields| {
                            Ok(match fields.string("type")? {
                                "controller" => OwnedObjectSlot::Controller(fields.u32("id")?),
                                "sequence" => OwnedObjectSlot::Sequence(fields.u32("id")?),
                                "fixture" => OwnedObjectSlot::Fixture(fields.u32("id")?),
                                _ => return Err(error("Unknown collection ownership slot.")),
                            })
                        },
                    )?,
                };
                kind = kind
                    .owned_child_kind(&slot)
                    .ok_or_else(|| error("Invalid ownership path for this object type."))?;
                identity = identity.owned(slot);
            }
            if kind != expected {
                return Err(error("Owned address has the wrong object type."));
            }
            Ok(identity)
        })
    }
}

pub(crate) fn write_object_reference(
    session: &ProjectSession,
    from: &DocumentId,
    kind: SourceObjectKind,
    identity: &ObjectIdentity,
) -> Result<Value, ExportProjectError> {
    if let Some(source) = identity.source() {
        return write_source_reference(session, from, kind, source).map(Value::String);
    }
    let root = identity.root_source();
    let error = || ExportProjectError::InvalidReference {
        path: from.path().to_owned(),
        reference: root.object().into(),
        message: "Invalid owned object address.".into(),
    };
    let root_kind = session
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
        .ok_or_else(error)?;
    let mut resolved_kind = root_kind.clone();
    let mut path = Vec::new();
    for slot in identity.owned_path() {
        resolved_kind = resolved_kind.owned_child_kind(slot).ok_or_else(error)?;
        path.push(match slot {
            OwnedObjectSlot::Setup => Value::String("setup".into()),
            OwnedObjectSlot::Layout => Value::String("layout".into()),
            OwnedObjectSlot::Patch => Value::String("patch".into()),
            OwnedObjectSlot::Controller(id)
            | OwnedObjectSlot::Sequence(id)
            | OwnedObjectSlot::Fixture(id) => {
                let name = match slot {
                    OwnedObjectSlot::Controller(_) => "controller",
                    OwnedObjectSlot::Sequence(_) => "sequence",
                    _ => "fixture",
                };
                Value::Mapping(Mapping::from_iter([
                    (Value::String("type".into()), Value::String(name.into())),
                    (Value::String("id".into()), Value::Number((*id).into())),
                ]))
            }
        });
    }
    if resolved_kind != kind || !session.owned_object_exists(&kind, identity) {
        return Err(error());
    }
    Ok(Value::Mapping(Mapping::from_iter([
        (
            Value::String("owner".into()),
            Value::String(write_source_reference(session, from, root_kind, root)?),
        ),
        (Value::String("path".into()), Value::Sequence(path)),
    ])))
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
