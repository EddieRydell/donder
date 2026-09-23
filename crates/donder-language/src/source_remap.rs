use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectParamValue, EffectRef, GradientId,
    GradientSource,
};
use crate::fixture::FixtureDefinitionId;
use crate::identity::{DocumentId, SourceIdentity};
use crate::layout::{FixtureTarget, LayoutFixture, LayoutFixtureKind, LayoutId};
use crate::model::{DonderProject, ProjectId};
use crate::operator::{OperatorDefinitionId, OperatorRef};
use crate::sequence::CompositionGraphNodeKind;
use crate::setup::SetupId;

/// Rewrites document-backed identities throughout a typed project.
///
/// The map is keyed by the complete old document identity so paths in other
/// modules are unaffected. Object keys remain stable.
pub fn remap_document_paths(
    project: &mut DonderProject,
    remaps: &BTreeMap<DocumentId, DocumentId>,
) {
    if remaps.is_empty() {
        return;
    }

    project.root.id = ProjectId(remap_identity(&project.root.id.0, remaps));
    project.root.setup = SetupId(remap_identity(&project.root.setup.0, remaps));
    project.root.sequences = project
        .root
        .sequences
        .iter()
        .map(|id| crate::sequence::SequenceId(remap_identity(&id.0, remaps)))
        .collect();

    project.setups = remap_index_map(&project.setups, |id| SetupId(remap_identity(&id.0, remaps)));
    for setup in project.setups.values_mut() {
        setup.id = SetupId(remap_identity(&setup.id.0, remaps));
        setup.layout = LayoutId(remap_identity(&setup.layout.0, remaps));
        setup.patch = crate::patch::PatchId(remap_identity(&setup.patch.0, remaps));
        setup.controllers = setup
            .controllers
            .iter()
            .map(|id| crate::controller::ControllerId(remap_identity(&id.0, remaps)))
            .collect();
    }

    project.layouts = remap_index_map(&project.layouts, |id| {
        LayoutId(remap_identity(&id.0, remaps))
    });
    for layout in project.layouts.values_mut() {
        layout.id = LayoutId(remap_identity(&layout.id.0, remaps));
        remap_layout_fixtures(&mut layout.fixtures, remaps);
    }

    project.patches = remap_index_map(&project.patches, |id| {
        crate::patch::PatchId(remap_identity(&id.0, remaps))
    });
    for patch in project.patches.values_mut() {
        patch.id = crate::patch::PatchId(remap_identity(&patch.id.0, remaps));
        for route in &mut patch.routes {
            remap_target(&mut route.target, remaps);
            route.controller =
                crate::controller::ControllerId(remap_identity(&route.controller.0, remaps));
        }
    }

    project.controllers = remap_index_map(&project.controllers, |id| {
        crate::controller::ControllerId(remap_identity(&id.0, remaps))
    });

    project.sequences = remap_index_map(&project.sequences, |id| {
        crate::sequence::SequenceId(remap_identity(&id.0, remaps))
    });
    for sequence in project.sequences.values_mut() {
        sequence.id = crate::sequence::SequenceId(remap_identity(&sequence.id.0, remaps));
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
        for target in &mut definition.generated_effect_targets {
            if let crate::effect::EffectRef::Custom(identity) = target {
                *identity = EffectDefinitionId(remap_identity(&identity.0, remaps));
            }
        }
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

fn remap_target(target: &mut FixtureTarget, remaps: &BTreeMap<DocumentId, DocumentId>) {
    target.layout = LayoutId(remap_identity(&target.layout.0, remaps));
}

fn remap_layout_fixtures(
    fixtures: &mut [LayoutFixture],
    remaps: &BTreeMap<DocumentId, DocumentId>,
) {
    for fixture in fixtures {
        match &mut fixture.kind {
            LayoutFixtureKind::Fixture { definition, .. } => {
                *definition = FixtureDefinitionId(remap_identity(&definition.0, remaps))
            }
            LayoutFixtureKind::Group { children } => remap_layout_fixtures(children, remaps),
        }
    }
}

fn remap_effect_ref(reference: &mut EffectRef, remaps: &BTreeMap<DocumentId, DocumentId>) {
    if let EffectRef::Custom(id) = reference {
        *id = EffectDefinitionId(remap_identity(&id.0, remaps));
    }
}

fn remap_operator_ref(reference: &mut OperatorRef, remaps: &BTreeMap<DocumentId, DocumentId>) {
    if let OperatorRef::Custom(id) = reference {
        *id = OperatorDefinitionId(remap_identity(&id.0, remaps));
    }
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
