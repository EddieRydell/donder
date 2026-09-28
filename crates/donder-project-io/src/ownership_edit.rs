//! Source/import bookkeeping after typed ownership edits. No serialization or reload.
use crate::{ExportProjectError, ProjectSession, ensure_document_can_reference_object};
use donder_language::{
    effect::{EffectParamValue, EffectRef},
    identity::{DocumentId, ObjectIdentity},
    layout::LayoutFixtureKind,
    operator::OperatorRef,
    ownership::ValueSource,
    sequence::{CompositionGraphNodeKind, SequenceAudio},
};
use std::collections::{BTreeMap, BTreeSet};

/// Preserve the visibility of typed references after values change owners.
/// The caller owns the candidate session and transaction boundary.
pub fn maintain_ownership_sources(session: &mut ProjectSession) -> Result<(), ExportProjectError> {
    let mut references: BTreeMap<DocumentId, BTreeSet<ObjectIdentity>> = BTreeMap::new();
    let project = &session.project;
    let mut add = |from: &DocumentId, target: ObjectIdentity| {
        if session.source.is_project_owned(from) {
            references.entry(from.clone()).or_default().insert(target);
        }
    };
    let owner = project.root.id.0.document_id();
    add(owner, project.root.setup.id().0.clone());
    for source in &project.root.sequences {
        add(owner, source.id().0.clone());
    }
    for setup in project.setups() {
        let owner = setup.id.0.document_id();
        add(owner, setup.layout.id().0.clone());
        add(owner, setup.patch.id().0.clone());
        for source in &setup.controllers {
            add(owner, source.id().0.clone());
        }
    }
    for layout in project.layouts() {
        for fixture in layout.iter_fixtures() {
            if let LayoutFixtureKind::Fixture {
                definition: ValueSource::Reference(id),
                ..
            } = &fixture.kind
            {
                add(layout.id.0.document_id(), id.0.clone().into());
            }
        }
    }
    for patch in project.patches() {
        for route in &patch.routes {
            add(patch.id.0.document_id(), route.target.layout.0.clone());
            add(patch.id.0.document_id(), route.controller.0.clone());
        }
    }
    for sequence in project.sequences() {
        let owner = sequence.id.0.document_id();
        for effect in &sequence.effects {
            add(owner, effect.target.layout.0.clone());
            if let EffectRef::Custom(id) = &effect.definition {
                add(owner, id.0.clone().into());
            }
            for value in effect.param_overrides.values() {
                param_references(value, &mut |id| add(owner, id));
            }
        }
        for node in &sequence.composition_graph.nodes {
            if let CompositionGraphNodeKind::Operator(operator) = &node.kind {
                if let OperatorRef::Custom(id) = &operator.operator {
                    add(owner, id.0.clone().into());
                }
                for value in operator.params.values() {
                    param_references(value, &mut |id| add(owner, id));
                }
            }
        }
    }
    for (from, targets) in references {
        for target in targets {
            crate::imports::inherit_relocated_reference_import(
                session,
                &from,
                target.document_id(),
            )?;
            ensure_document_can_reference_object(session, &from, &target)?;
        }
    }
    // Asset identity is retained when a sequence moves or is copied; membership
    // records the documents that currently use it, rather than its old owners.
    for asset in &mut session.source.referenced_assets {
        asset.referenced_by.clear();
    }
    for sequence in session.project.sequences() {
        if let SequenceAudio::Asset(id) = &sequence.audio {
            let asset = session
                .source
                .referenced_assets
                .iter_mut()
                .find(|asset| &asset.id == id)
                .ok_or_else(|| ExportProjectError::InvalidReference {
                    path: sequence.id.0.document().to_owned(),
                    reference: id.0.to_string(),
                    message: "Sequence audio asset is missing.".into(),
                })?;
            asset
                .referenced_by
                .insert(sequence.id.0.document_id().clone());
            if asset.module_id != sequence.id.0.module_id()
                && !session
                    .source
                    .source_graph
                    .module(sequence.id.0.module_id())
                    .is_ok_and(|module| {
                        module
                            .dependencies
                            .values()
                            .any(|id| *id == asset.module_id)
                    })
            {
                return Err(ExportProjectError::InvalidReference {
                    path: sequence.id.0.document().to_owned(),
                    reference: asset.relative_path.to_string(),
                    message: "Audio source requires a declared dependency on its package.".into(),
                });
            }
        }
    }
    Ok(())
}

fn param_references(value: &EffectParamValue, add: &mut impl FnMut(ObjectIdentity)) {
    match value {
        EffectParamValue::Curve(ValueSource::Reference(id)) => add(id.0.clone().into()),
        EffectParamValue::Gradient(ValueSource::Reference(id)) => add(id.0.clone().into()),
        EffectParamValue::Array(values) => {
            for value in values {
                param_references(value, add);
            }
        }
        EffectParamValue::Int(_)
        | EffectParamValue::Float(_)
        | EffectParamValue::Bool(_)
        | EffectParamValue::Color(_)
        | EffectParamValue::Enum(_)
        | EffectParamValue::Marks(_)
        | EffectParamValue::Curve(ValueSource::Inline(_))
        | EffectParamValue::Gradient(ValueSource::Inline(_)) => {}
    }
}
