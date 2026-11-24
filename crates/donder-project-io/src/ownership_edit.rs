//! Source/import bookkeeping after typed ownership edits. No serialization or reload.
use crate::address::{DocumentId, ObjectAddress};
use crate::imports::ensure_document_can_reference_address;
use crate::{ExportProjectError, ProjectSession};
use donder_model::ObjectId;
use std::collections::{BTreeMap, BTreeSet};

/// Preserve the visibility of typed references after values change owners.
/// The caller owns the candidate session and transaction boundary.
pub fn maintain_ownership_sources(session: &mut ProjectSession) -> Result<(), ExportProjectError> {
    let addresses = session.addresses();
    let address = |id: ObjectId| {
        addresses
            .get(&id)
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: crate::PROJECT_ROOT_FILE.into(),
                reference: format!("{:?}", id.kind()),
                message: "An object is missing from the source inventory.".into(),
            })
    };
    // Each object and the objects it refers to.
    let mut references: Vec<(ObjectId, ObjectId)> = Vec::new();
    session
        .project
        .for_each_reference(&mut |from, target| references.push((from, target)));
    let mut visible: BTreeMap<DocumentId, BTreeSet<ObjectAddress>> = BTreeMap::new();
    for (from, target) in references {
        visible
            .entry(address(from)?.document_id().clone())
            .or_default()
            .insert(address(target)?.clone());
    }
    for (from, targets) in visible {
        for target in targets {
            ensure_document_can_reference_address(session, &from, &target)?;
        }
    }
    // Asset identity is retained when a sequence moves or is copied; membership
    // records the documents that currently use it, rather than its old owners.
    let mut uses = Vec::new();
    session
        .project
        .for_each_asset(&mut |user, asset| uses.push((user, asset.clone())));
    for asset in &mut session.source.referenced_assets {
        asset.referenced_by.clear();
    }
    for (user, id) in uses {
        let document = address(user)?.document_id().clone();
        let asset = session
            .source
            .referenced_assets
            .iter_mut()
            .find(|asset| asset.id == id)
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: document.path().to_owned(),
                reference: id.0.to_string(),
                message: "An asset is missing.".into(),
            })?;
        asset.referenced_by.insert(document);
    }
    Ok(())
}

/// Remove `documents` and every object they declare. What other documents
/// held that referred to those objects gives way as the model repairs it, and
/// the other documents stop importing them; the repairs are left in the
/// project for [`donder_model::DonderProject::take_repairs`]. The caller owns
/// the candidate session, and the files.
pub fn remove_documents(
    session: &mut ProjectSession,
    documents: &[DocumentId],
) -> Result<(), donder_model::EditError> {
    let objects = documents
        .iter()
        .filter_map(|document| session.source.documents.get(document))
        .flat_map(|document| document.objects().iter().map(|object| object.id()))
        .collect::<Vec<_>>();
    session
        .project
        .apply_edits(objects.into_iter().map(donder_model::ProjectEdit::Remove))?;
    session
        .source
        .documents
        .retain(|document, _| !documents.contains(document));
    for source in session.source.documents.values_mut() {
        for edge in &mut source.imports {
            edge.targets.retain(|target| !documents.contains(target));
        }
        source.imports.retain(|edge| !edge.targets.is_empty());
    }
    for asset in &mut session.source.referenced_assets {
        asset
            .referenced_by
            .retain(|document| !documents.contains(document));
    }
    session
        .source
        .referenced_assets
        .retain(|asset| !asset.referenced_by.is_empty());
    Ok(())
}
