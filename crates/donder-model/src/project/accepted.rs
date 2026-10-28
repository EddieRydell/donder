use super::DonderProject;
use crate::effect::Clip;
use crate::operator::OperatorDefinition;
use crate::sequence::{CompositionGraphNode, CompositionGraphNodeKind, Sequence, SequenceId};
mod automation;
mod geometry;
mod inputs;
mod parameters;
mod patch;
mod rig;
use crate::rig::{Mapping, Rig, RigClip, RigId, Trigger, Voice};
use donder_language::compiler::Invocation;
pub(super) use inputs::ProjectInputs;
pub use rig::{RIG_SESSION, RigBinding};

/// A sequence borrowed from the same accepted project as all its dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedSequence<'a> {
    project: &'a DonderProject,
    sequence: &'a Sequence,
}

/// An authored clip paired with its admitted playback invocation.
#[derive(Clone, Copy)]
pub struct AcceptedClipInputs<'a> {
    clip: &'a Clip,
    invocation: &'a Invocation,
}

#[derive(Clone, Copy)]
pub struct AcceptedOperatorInputs<'a> {
    node: &'a CompositionGraphNode,
    definition: &'a OperatorDefinition,
    invocation: &'a Invocation,
}

impl DonderProject {
    /// Encodings belong to this accepted patch snapshot. Once selection resolves
    /// the patch, its authored route IDs index this cache without readmission.
    pub fn playback_patch_encodings(
        &self,
        patch: &crate::patch::PatchId,
    ) -> Option<&indexmap::IndexMap<crate::patch::PixelRouteId, donder_runtime_types::OutputEncoding>>
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
            donder_runtime_types::FixtureGeometry,
        )],
    > {
        self.accepted_inputs.layouts.get(layout).map(AsRef::as_ref)
    }

    pub fn accepted_rig(&self, id: &RigId) -> Option<AcceptedRig<'_>> {
        self.get::<crate::Rig>(*id)
            .map(|rig| AcceptedRig { project: self, rig })
    }

    pub fn accepted_sequence(&self, id: &SequenceId) -> Option<AcceptedSequence<'_>> {
        self.get::<crate::Sequence>(*id)
            .map(|sequence| AcceptedSequence {
                project: self,
                sequence,
            })
    }
}

impl<'a> AcceptedSequence<'a> {
    pub fn timing(self) -> &'a donder_runtime_types::SequenceTiming {
        &self.project.accepted_inputs.sequences[&self.sequence.id].timing
    }

    pub fn sequence(self) -> &'a Sequence {
        self.sequence
    }

    pub fn clips(self) -> impl ExactSizeIterator<Item = AcceptedClipInputs<'a>> + 'a {
        self.sequence
            .clips
            .iter()
            .zip(
                self.project.accepted_inputs.sequences[&self.sequence.id]
                    .clips
                    .iter(),
            )
            .map(|(clip, invocation)| AcceptedClipInputs {
                clip,
                invocation: invocation.as_ref(),
            })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        operator_inputs(
            self.project,
            &self.sequence.composition_graph,
            &self.project.accepted_inputs.sequences[&self.sequence.id].operators,
        )
    }
}

/// A rig borrowed from the same accepted project as all its dependencies.
#[derive(Clone, Copy)]
pub struct AcceptedRig<'a> {
    project: &'a DonderProject,
    rig: &'a Rig,
}

/// Who plays a rig clip: a scene voice, or a trigger mapping's voices.
#[derive(Clone, Copy)]
pub enum RigClipOrigin<'a> {
    Voice(&'a Voice),
    Trigger(&'a Mapping, &'a Trigger),
}

#[derive(Clone, Copy)]
pub struct AcceptedRigClip<'a> {
    origin: RigClipOrigin<'a>,
    clip: &'a RigClip,
    inputs: &'a rig::RigClipInputs,
}

impl<'a> AcceptedRig<'a> {
    pub fn rig(self) -> &'a Rig {
        self.rig
    }

    /// The session clock: the whole playback range, with no authored windows.
    pub fn timing(self) -> &'a donder_runtime_types::SequenceTiming {
        &self.project.accepted_inputs.rigs[&self.rig.id].timing
    }

    /// The scene's voices, then each trigger's clip.
    pub fn clips(self) -> impl Iterator<Item = AcceptedRigClip<'a>> + 'a {
        let origins = self.rig.voices.iter().map(RigClipOrigin::Voice).chain(
            self.rig
                .mappings
                .iter()
                .filter_map(|mapping| match &mapping.action {
                    crate::rig::MappingAction::Trigger(trigger) => {
                        Some(RigClipOrigin::Trigger(mapping, trigger))
                    }
                    crate::rig::MappingAction::Mark(_)
                    | crate::rig::MappingAction::Control { .. } => None,
                }),
        );
        origins
            .zip(self.rig.clips())
            .zip(self.project.accepted_inputs.rigs[&self.rig.id].clips.iter())
            .map(|((origin, clip), inputs)| AcceptedRigClip {
                origin,
                clip,
                inputs,
            })
    }

    pub fn operators(self) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
        operator_inputs(
            self.project,
            &self.rig.composition_graph,
            &self.project.accepted_inputs.rigs[&self.rig.id].operators,
        )
    }
}

impl<'a> AcceptedRigClip<'a> {
    pub fn origin(self) -> RigClipOrigin<'a> {
        self.origin
    }

    pub fn clip(self) -> &'a RigClip {
        self.clip
    }

    pub fn invocation(self) -> &'a Invocation {
        &self.inputs.invocation
    }

    /// What sets each of the invocation's automation bindings.
    pub fn bindings(self) -> &'a [RigBinding] {
        &self.inputs.bindings
    }
}

fn operator_inputs<'a>(
    project: &'a DonderProject,
    graph: &'a crate::sequence::SequenceCompositionGraph,
    invocations: &'a indexmap::IndexMap<crate::sequence::CompositionGraphNodeId, Invocation>,
) -> impl Iterator<Item = AcceptedOperatorInputs<'a>> + 'a {
    graph.nodes.iter().filter_map(move |node| {
        let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
            return None;
        };
        let id = &operator.operator;
        Some(AcceptedOperatorInputs {
            node,
            definition: &project.reusable::<crate::OperatorDefinition>()[id],
            invocation: &invocations[&node.id],
        })
    })
}

impl<'a> AcceptedClipInputs<'a> {
    pub fn invocation(self) -> &'a Invocation {
        self.invocation
    }

    pub fn clip(self) -> &'a Clip {
        self.clip
    }

    /// When the clip starts on the sequence clock; admission checked that it
    /// plays within the sequence.
    pub fn start(self) -> donder_runtime_types::SampleTime {
        donder_runtime_types::SampleTime::from_ticks(self.clip.start.as_micros_rounded() as u32)
    }

    pub fn duration(self) -> donder_runtime_types::SampleDuration {
        donder_runtime_types::SampleDuration::from_ticks(
            self.clip.duration.as_micros_rounded() as u32
        )
    }
}

impl<'a> AcceptedOperatorInputs<'a> {
    pub fn invocation(self) -> &'a Invocation {
        self.invocation
    }

    pub fn node(self) -> &'a CompositionGraphNode {
        self.node
    }
    pub fn definition(self) -> &'a OperatorDefinition {
        self.definition
    }
}
