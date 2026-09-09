use std::collections::BTreeMap;

use indexmap::IndexMap;

use crate::effect::{
    CurveId, CurveSource, EffectDefinitionId, EffectParamValue, EffectRef, GradientId,
    GradientSource,
};
use crate::element::{ElementNodeKind, ElementSelection, ElementTreeId};
use crate::fixture_profile::FixtureProfileId;
use crate::identity::{DocumentId, SourceIdentity};
use crate::model::{DawnProject, ProjectId};
use crate::operator::{OperatorDefinitionId, OperatorRef};
use crate::patch::PatchNode;
use crate::preview::{PreviewLayoutId, PropDefinitionId};
use crate::sequence::CompositionGraphNodeKind;
use crate::setup::SetupId;

/// Rewrites document-backed identities throughout a typed project.
///
/// The map is keyed by the complete old document identity so paths in other
/// modules are unaffected. Object keys remain stable.
pub fn remap_document_paths(project: &mut DawnProject, remaps: &BTreeMap<DocumentId, DocumentId>) {
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
        setup.elements = ElementTreeId(remap_identity(&setup.elements.0, remaps));
        setup.preview = PreviewLayoutId(remap_identity(&setup.preview.0, remaps));
        setup.patch = crate::patch::PatchId(remap_identity(&setup.patch.0, remaps));
        setup.controllers = setup
            .controllers
            .iter()
            .map(|id| crate::controller::ControllerId(remap_identity(&id.0, remaps)))
            .collect();
    }

    project.element_trees = remap_index_map(&project.element_trees, |id| {
        ElementTreeId(remap_identity(&id.0, remaps))
    });
    for tree in project.element_trees.values_mut() {
        tree.id = ElementTreeId(remap_identity(&tree.id.0, remaps));
        for node in tree.nodes.values_mut() {
            if let ElementNodeKind::Fixture { profile } = &mut node.kind {
                *profile = FixtureProfileId(remap_identity(&profile.0, remaps));
            }
        }
    }

    project.preview_layouts = remap_index_map(&project.preview_layouts, |id| {
        PreviewLayoutId(remap_identity(&id.0, remaps))
    });
    for layout in project.preview_layouts.values_mut() {
        layout.id = PreviewLayoutId(remap_identity(&layout.id.0, remaps));
        layout.element_tree = ElementTreeId(remap_identity(&layout.element_tree.0, remaps));
        for prop in &mut layout.props {
            prop.definition = PropDefinitionId(remap_identity(&prop.definition.0, remaps));
        }
    }

    project.patches = remap_index_map(&project.patches, |id| {
        crate::patch::PatchId(remap_identity(&id.0, remaps))
    });
    for patch in project.patches.values_mut() {
        patch.id = crate::patch::PatchId(remap_identity(&patch.id.0, remaps));
        for node in patch.nodes.values_mut() {
            if let Some(profile) = node.fixture_profile_mut() {
                *profile = FixtureProfileId(remap_identity(&profile.0, remaps));
            }
            match node {
                PatchNode::Source(source) => remap_selection(&mut source.selection, remaps),
                PatchNode::Filter(_) => {}
                PatchNode::Sink(sink) => {
                    sink.controller =
                        crate::controller::ControllerId(remap_identity(&sink.controller.0, remaps));
                }
            }
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
            remap_selection(&mut effect.target, remaps);
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
        for clip in &mut sequence.control_clips {
            remap_selection(clip.target.selection_mut(), remaps);
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

    project.definitions.props.definitions =
        remap_index_map(&project.definitions.props.definitions, |id| {
            PropDefinitionId(remap_identity(&id.0, remaps))
        });

    project.definitions.fixture_profiles.definitions =
        remap_index_map(&project.definitions.fixture_profiles.definitions, |id| {
            FixtureProfileId(remap_identity(&id.0, remaps))
        });
    for profile in project
        .definitions
        .fixture_profiles
        .definitions
        .values_mut()
    {
        profile.id = FixtureProfileId(remap_identity(&profile.id.0, remaps));
    }

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

fn remap_selection(selection: &mut ElementSelection, remaps: &BTreeMap<DocumentId, DocumentId>) {
    selection.tree = ElementTreeId(remap_identity(&selection.tree.0, remaps));
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
