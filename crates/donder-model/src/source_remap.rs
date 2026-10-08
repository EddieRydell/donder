use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectParamValue, EffectRef, GradientId,
    GradientSource,
};
use crate::fixture::FixtureDefinitionId;
use crate::identity::{DocumentId, ObjectIdentity, SourceIdentity};
use crate::layout::{FixtureTarget, LayoutFixture, LayoutFixtureKind, LayoutId};
use crate::model::{DonderProject, ProjectId};
use crate::operator::{OperatorDefinitionId, OperatorRef};
use crate::ownership::ValueSource;
use crate::sequence::CompositionGraphNodeKind;
use crate::setup::SetupId;

/// Rewrites document-backed identities throughout a typed project.
///
/// The map is keyed by the complete old document identity so paths in other
/// modules are unaffected. Object keys remain stable.
pub fn remap_document_paths(
    project: &mut DonderProject,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) -> Result<(), String> {
    project.checked_edit(|project| {
        // Reject collisions before rebuilding maps; collecting colliding keys
        // would otherwise silently discard an authored object.
        let mut documents = std::collections::BTreeSet::new();
        documents.insert(project.root.id.0.document_id().clone());
        for setup in project.setups() {
            documents.insert(setup.id.0.document_id().clone());
        }
        for layout in project.layouts() {
            documents.insert(layout.id.0.document_id().clone());
        }
        for patch in project.patches() {
            documents.insert(patch.id.0.document_id().clone());
        }
        for controller in project.controllers() {
            documents.insert(controller.id.0.document_id().clone());
        }
        for sequence in project.sequences() {
            documents.insert(sequence.id.0.document_id().clone());
        }
        for id in project.definitions.effects.definitions.keys() {
            documents.insert(id.0.document_id().clone());
        }
        for id in project.definitions.operators.definitions.keys() {
            documents.insert(id.0.document_id().clone());
        }
        for id in project.definitions.fixtures.definitions.keys() {
            documents.insert(id.0.document_id().clone());
        }
        for id in project.definitions.curves.definitions.keys() {
            documents.insert(id.0.document_id().clone());
        }
        for id in project.definitions.gradients.definitions.keys() {
            documents.insert(id.0.document_id().clone());
        }
        let mut destinations = std::collections::BTreeSet::new();
        for document in &documents {
            if !destinations.insert(remaps.get(document).unwrap_or(document)) {
                return Err("Document path remapping merges distinct documents.".into());
            }
        }
        remap_candidate(project, remaps);
        Ok(())
    })
}

fn remap_candidate(project: &mut DonderProject, remaps: &BTreeMap<DocumentId, DocumentId>) {
    if remaps.is_empty() {
        return;
    }

    project.root.id = ProjectId(remap_identity(&project.root.id.0, remaps));
    if let ValueSource::Reference(id) = &mut project.root.setup {
        id.0 = remap_object_identity(&id.0, remaps);
    }
    for source in &mut project.root.sequences {
        if let ValueSource::Reference(id) = source {
            id.0 = remap_object_identity(&id.0, remaps);
        }
    }

    project.setups = remap_index_map(&project.setups, |id| {
        SetupId(remap_object_identity(&id.0, remaps))
    })
    .into();
    for setup in project.setups_mut() {
        setup.id.0 = remap_object_identity(&setup.id.0, remaps);
        if let ValueSource::Reference(id) = &mut setup.layout {
            id.0 = remap_object_identity(&id.0, remaps);
        }
        if let ValueSource::Reference(id) = &mut setup.patch {
            id.0 = remap_object_identity(&id.0, remaps);
        }
        for source in &mut setup.controllers {
            if let ValueSource::Reference(id) = source {
                id.0 = remap_object_identity(&id.0, remaps);
            }
        }
    }

    project.layouts = remap_index_map(&project.layouts, |id| {
        LayoutId(remap_object_identity(&id.0, remaps))
    })
    .into();
    for layout in project.layouts_mut() {
        layout.id.0 = remap_object_identity(&layout.id.0, remaps);
        remap_layout_fixtures(&mut layout.fixtures, remaps);
    }

    project.patches = remap_index_map(&project.patches, |id| {
        crate::patch::PatchId(remap_object_identity(&id.0, remaps))
    })
    .into();
    for patch in project.patches_mut() {
        patch.id.0 = remap_object_identity(&patch.id.0, remaps);
        for route in &mut patch.routes {
            remap_target(&mut route.target, remaps);
            route.controller.0 = remap_object_identity(&route.controller.0, remaps);
        }
    }

    project.controllers = remap_index_map(&project.controllers, |id| {
        crate::controller::ControllerId(remap_object_identity(&id.0, remaps))
    })
    .into();
    for controller in project.controllers_mut() {
        controller.id.0 = remap_object_identity(&controller.id.0, remaps);
    }
    project.sequences = remap_index_map(&project.sequences, |id| {
        crate::sequence::SequenceId(remap_object_identity(&id.0, remaps))
    })
    .into();
    for sequence in project.sequences_mut() {
        sequence.id.0 = remap_object_identity(&sequence.id.0, remaps);
        for clip in &mut sequence.automation_clips {
            remap_target(&mut clip.row_target, remaps);
        }
        for effect in &mut sequence.effects {
            remap_target(&mut effect.target, remaps);
            remap_effect_ref(&mut effect.definition, remaps);
            for value in effect.param_overrides.values_mut() {
                remap_param(value, remaps);
            }
        }
        for node in &mut sequence.composition_graph.nodes {
            if let CompositionGraphNodeKind::Operator(operator) = &mut node.kind {
                remap_operator_ref(&mut operator.operator, remaps);
                for value in operator.params.values_mut() {
                    remap_param(value, remaps);
                }
            }
        }
    }

    project.definitions.effects.definitions =
        remap_index_map(&project.definitions.effects.definitions, |id| {
            EffectDefinitionId(remap_identity(&id.0, remaps))
        });
    for definition in project.definitions.effects.definitions.values_mut() {
        remap_effect_ref(&mut definition.id, remaps);
    }

    project.definitions.fixtures.definitions =
        remap_index_map(&project.definitions.fixtures.definitions, |id| {
            FixtureDefinitionId(remap_identity(&id.0, remaps))
        });

    project.definitions.curves.definitions =
        remap_index_map(&project.definitions.curves.definitions, |id| {
            CurveId(remap_identity(&id.0, remaps))
        });
    project.definitions.gradients.definitions =
        remap_index_map(&project.definitions.gradients.definitions, |id| {
            GradientId(remap_identity(&id.0, remaps))
        });

    project.definitions.operators.definitions =
        remap_index_map(&project.definitions.operators.definitions, |id| {
            OperatorDefinitionId(remap_identity(&id.0, remaps))
        });
    for definition in project.definitions.operators.definitions.values_mut() {
        remap_operator_ref(&mut definition.id, remaps);
    }
}

fn remap_index_map<K, V>(source: &IndexMap<K, V>, mut remap: impl FnMut(&K) -> K) -> IndexMap<K, V>
where
    K: Clone + Eq + std::hash::Hash,
    V: Clone,
{
    source
        .iter()
        .map(|(key, value)| (remap(key), value.clone()))
        .collect()
}

pub fn remap_identity(
    identity: &SourceIdentity,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) -> SourceIdentity {
    SourceIdentity::from_document(
        remaps
            .get(identity.document_id())
            .cloned()
            .unwrap_or_else(|| identity.document_id().clone()),
        identity.object().to_string(),
    )
}

pub fn remap_object_identity(
    identity: &ObjectIdentity,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) -> ObjectIdentity {
    identity.with_root_source(remap_identity(identity.root_source(), remaps))
}

fn remap_target(target: &mut FixtureTarget, remaps: &BTreeMap<DocumentId, DocumentId>) {
    target.layout = LayoutId(remap_object_identity(&target.layout.0, remaps));
}

fn remap_layout_fixtures(
    fixtures: &mut [LayoutFixture],
    remaps: &BTreeMap<DocumentId, DocumentId>,
) {
    for fixture in fixtures {
        if let LayoutFixtureKind::Fixture {
            definition: crate::fixture::FixtureSource::Reference(id),
            ..
        } = &mut fixture.kind
        {
            *id = FixtureDefinitionId(remap_identity(&id.0, remaps));
        }
    }
}

fn remap_effect_ref(reference: &mut EffectRef, remaps: &BTreeMap<DocumentId, DocumentId>) {
    let EffectRef::Custom(id) = reference;
    *id = EffectDefinitionId(remap_identity(&id.0, remaps));
}

fn remap_operator_ref(reference: &mut OperatorRef, remaps: &BTreeMap<DocumentId, DocumentId>) {
    let OperatorRef::Custom(id) = reference;
    *id = OperatorDefinitionId(remap_identity(&id.0, remaps));
}

fn remap_param(value: &mut EffectParamValue, remaps: &BTreeMap<DocumentId, DocumentId>) {
    match value {
        EffectParamValue::Curve(CurveSource::Reference(id)) => {
            *id = CurveId(remap_identity(&id.0, remaps));
        }
        EffectParamValue::Gradient(GradientSource::Reference(id)) => {
            *id = GradientId(remap_identity(&id.0, remaps));
        }
        EffectParamValue::Array(values) => {
            for value in values {
                remap_param(value, remaps);
            }
        }
        EffectParamValue::Int(_)
        | EffectParamValue::Float(_)
        | EffectParamValue::Bool(_)
        | EffectParamValue::Color(_)
        | EffectParamValue::Enum(_)
        | EffectParamValue::Marks(_)
        | EffectParamValue::Curve(CurveSource::Inline(_))
        | EffectParamValue::Gradient(GradientSource::Inline(_)) => {}
    }
}
