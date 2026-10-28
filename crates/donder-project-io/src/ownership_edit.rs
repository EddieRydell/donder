//! Source/import bookkeeping after typed ownership edits. No serialization or reload.
use crate::address::{DocumentId, ObjectAddress};
use crate::imports::ensure_document_can_reference_address;
use crate::{ExportProjectError, ProjectSession};
use donder_model::{ObjectId, SequenceAudio};
use std::collections::{BTreeMap, BTreeSet, HashMap};

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
    let documents: HashMap<_, _> = session
        .project
        .objects::<donder_model::Sequence>()
        .map(|sequence| {
            address(sequence.id.into()).map(|address| (sequence.id, address.document_id().clone()))
        })
        .collect::<Result<_, _>>()?;
    for asset in &mut session.source.referenced_assets {
        asset.referenced_by.clear();
    }
    for sequence in session.project.objects::<donder_model::Sequence>() {
        if let SequenceAudio::Asset(id) = &sequence.audio {
            let document = &documents[&sequence.id];
            let asset = session
                .source
                .referenced_assets
                .iter_mut()
                .find(|asset| &asset.id == id)
                .ok_or_else(|| ExportProjectError::InvalidReference {
                    path: document.path().to_owned(),
                    reference: id.0.to_string(),
                    message: "Sequence audio asset is missing.".into(),
                })?;
            asset.referenced_by.insert(document.clone());
        }
    }
    Ok(())
}
