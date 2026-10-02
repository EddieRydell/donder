use super::DonderProject;
use crate::effect::EffectInst;
use crate::operator::{OperatorDefinition, OperatorRef};
use crate::sequence::{CompositionGraphNode, CompositionGraphNodeKind, Sequence, SequenceId};
mod automation;
mod geometry;
mod inputs;
mod parameters;
mod patch;
use donder_runtime::{OperatorInvocation, SampleInvocation};
pub(super) use inputs::ProjectInputs;

/// A sequence borrowed from the same accepted project as all its dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedSequence<'a> {
    project: &'a DonderProject,
    sequence: &'a Sequence,
}

/// An authored instance paired with its admitted playback invocation.
#[derive(Clone, Copy)]
pub struct AcceptedEffectInputs<'a> {
    instance: &'a EffectInst,
    execution: &'a SampleInvocation,
}

#[derive(Clone, Copy)]
pub struct AcceptedOperatorInputs<'a> {
    node: &'a CompositionGraphNode,
    definition: &'a OperatorDefinition,
    invocation: &'a OperatorInvocation,
}

impl DonderProject {
    /// Encodings belong to this accepted patch snapshot. Once selection resolves
    /// the patch, its authored route IDs index this cache without readmission.
    pub fn playback_patch_encodings(
        &self,
        patch: &crate::patch::PatchId,
    ) -> Option<&indexmap::IndexMap<crate::patch::PixelRouteId, donder_runtime::OutputEncoding>>
    {
        self.accepted_inputs
            .patches
            .get(patch)
            .map(|patch| &patch.routes)
    }

    pub fn playback_geometry(
        &self,
        layout: &crate::layout::LayoutId,
    ) -> Option<
        &[(
            crate::layout::FixtureInstanceId,
            donder_runtime::FixtureGeometry,
        )],
    > {
        self.accepted_inputs.layouts.get(layout).map(AsRef::as_ref)
    }

    pub fn accepted_sequence(&self, id: &SequenceId) -> Option<AcceptedSequence<'_>> {
        self.sequence(id).map(|sequence| AcceptedSequence {
            project: self,
            sequence,
        })
    }
}

impl<'a> AcceptedSequence<'a> {
    pub fn timing(self) -> &'a donder_runtime::SequenceTiming {
        &self.project.accepted_inputs.sequences[&self.sequence.id].timing
    }

    pub fn sequence(self) -> &'a Sequence {
        self.sequence
    }

    pub fn effects(self) -> impl ExactSizeIterator<Item = AcceptedEffectInputs<'a>> + 'a {
        self.sequence
            .effects
            .iter()
            .zip(
                self.project.accepted_inputs.sequences[&self.sequence.id]
                    .effects
                    .iter(),
            )
            .map(|(instance, execution)| AcceptedEffectInputs {
                instance,
                execution,
            })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        self.sequence
            .composition_graph
            .nodes
            .iter()
            .filter_map(move |node| {
                let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
                    return None;
                };
                let OperatorRef::Custom(id) = &operator.operator;
                Some(AcceptedOperatorInputs {
                    node,
                    definition: &self.project.definitions.operators.definitions[id],
                    invocation: &self.project.accepted_inputs.sequences[&self.sequence.id]
                        .operators[&node.id],
                })
            })
    }
}

impl<'a> AcceptedEffectInputs<'a> {
    pub fn execution(self) -> &'a SampleInvocation {
        self.execution
    }

    pub fn instance(self) -> &'a EffectInst {
        self.instance
    }
}

impl<'a> AcceptedOperatorInputs<'a> {
    pub fn invocation(self) -> &'a donder_runtime::OperatorInvocation {
        self.invocation
    }

    pub fn node(self) -> &'a CompositionGraphNode {
        self.node
    }
    pub fn definition(self) -> &'a OperatorDefinition {
        self.definition
    }
}
