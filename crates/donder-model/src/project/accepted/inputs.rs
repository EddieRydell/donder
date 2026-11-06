//! Playback invocations derived during the same admission transaction as
//! authored state; they cannot be edited independently afterwards.
use super::parameters::{ClipTiming, MarkOrigin, prepare_params};
use super::*;
use crate::effect::ParamValue;
use crate::sequence::AutomationTarget;
use crate::sequence::CompositionGraphNodeId;
use crate::validation::{AdmissionProblem, Member, ProjectValidationError, ValidationProblem, at};
use donder_language::compiler::{Invocation, ParamDecl};
use donder_runtime_types::SampleTime;
use donder_runtime_types::SequenceTiming;
use donder_runtime_types::Value;
use indexmap::IndexMap;
use std::num::NonZeroU32;

#[derive(Debug, Default)]
pub(in crate::project) struct ProjectInputs {
    pub(super) sequences: IndexMap<SequenceId, SequenceInputs>,
    pub(super) consoles: IndexMap<crate::console::ConsoleId, super::console::ConsoleInputs>,
    pub(super) layouts: IndexMap<crate::layout::LayoutId, super::geometry::LayoutGeometry>,
    pub(super) patches: IndexMap<crate::patch::PatchId, super::patch::PatchEncodings>,
}

#[derive(Debug)]
pub(super) struct SequenceInputs {
    pub(super) timing: SequenceTiming,
    pub(super) clips: Box<[std::sync::Arc<Invocation>]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, Invocation>,
}

impl ProjectInputs {
    pub(in crate::project) fn admit(
        project: &DonderProject,
        previous: Option<&DonderProject>,
    ) -> Result<Self, ProjectValidationError> {
        let mut sequences = IndexMap::new();
        for sequence in project.objects::<crate::Sequence>() {
            let reusable = reusable_effect_inputs(project, previous, sequence);
            let mut clips = Vec::with_capacity(sequence.clips.len());
            for clip in &sequence.clips {
                if let Some(invocation) =
                    reusable.get(&clip.id).and_then(|(previous, invocation)| {
                        std::sync::Arc::ptr_eq(previous, clip)
                            .then(|| std::sync::Arc::clone(invocation))
                    })
                {
                    clips.push(invocation);
                    continue;
                }
                let id = &clip.definition;
                let definition = &project.reusable::<crate::EffectDefinition>()[id];
                let values = resolve(
                    project,
                    sequence,
                    definition.params(),
                    &clip.param_overrides,
                    ClipTiming {
                        start: crate::clock::sample_time(&clip.start),
                        duration: crate::clock::sample_duration(&clip.duration),
                    },
                )
                .map_err(at(sequence.id))?;
                let compiled = definition.compiled();
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::ClipParam { clip_id, .. } if clip_id == &clip.id),
                ).map_err(at(sequence.id))?;
                let invocation =
                    compiled
                        .invoke(values.into_vec(), automation)
                        .map_err(|error| {
                            at(sequence.id)(invalid(AdmissionProblem::Binding(
                                Member::Clip(clip.id.clone()),
                                error,
                            )))
                        })?;
                clips.push(std::sync::Arc::new(invocation));
            }
            let mut operators = IndexMap::new();
            for node in &sequence.composition_graph.nodes {
                let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
                    continue;
                };
                let id = &operator.operator;
                let definition = &project.reusable::<crate::OperatorDefinition>()[id];
                let values = resolve(
                    project,
                    sequence,
                    definition.params(),
                    &operator.params,
                    ClipTiming {
                        start: SampleTime::from_ticks(0),
                        duration: crate::clock::sample_duration(&sequence.duration),
                    },
                )
                .map_err(at(sequence.id))?;
                let compiled = definition.compiled();
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == &node.id),
                ).map_err(at(sequence.id))?;
                let invocation =
                    compiled
                        .invoke(values.into_vec(), automation)
                        .map_err(|error| {
                            at(sequence.id)(invalid(AdmissionProblem::Binding(
                                Member::Node(node.id.clone()),
                                error,
                            )))
                        })?;
                operators.insert(node.id.clone(), invocation);
            }
            sequences.insert(
                sequence.id,
                SequenceInputs {
                    timing: admit_timing(sequence),
                    clips: clips.into(),
                    operators,
                },
            );
        }
        Ok(Self {
            sequences,
            consoles: super::console::admit(project)?,
            layouts: super::geometry::admit(project, previous)?,
            patches: super::patch::admit(project)?,
        })
    }
}

/// Validation proved the sequence's timing fits the playback clock.
fn admit_timing(sequence: &Sequence) -> SequenceTiming {
    fn proven<T>() -> T {
        unreachable!("validation proves the sequence's timing")
    }
    let frame_count = u32::try_from(sequence.frame_count()).unwrap_or_else(|_| proven());
    SequenceTiming::new(
        NonZeroU32::new(sequence.frame_rate).unwrap_or_else(proven),
        NonZeroU32::new(frame_count).unwrap_or_else(proven),
        NonZeroU32::new(crate::clock::sample_duration(&sequence.duration).as_ticks())
            .unwrap_or_else(proven),
    )
}

fn resolve(
    project: &DonderProject,
    sequence: &Sequence,
    declarations: &[ParamDecl],
    overrides: &IndexMap<donder_runtime_types::Identifier, ParamValue>,
    timing: ClipTiming,
) -> Result<Box<[Value]>, ValidationProblem> {
    resolve_values(
        project,
        MarkOrigin::Authored(&sequence.mark_collections),
        declarations,
        overrides,
        timing,
    )
}

/// Every declared parameter's value: its override, or its default.
pub(super) fn resolve_values(
    project: &DonderProject,
    marks: MarkOrigin<'_>,
    declarations: &[ParamDecl],
    overrides: &IndexMap<donder_runtime_types::Identifier, ParamValue>,
    timing: ClipTiming,
) -> Result<Box<[Value]>, ValidationProblem> {
    let values = prepare_params(project, marks, declarations, overrides, timing);
    declarations
        .iter()
        .map(|param| {
            values
                .get(&param.name)
                .cloned()
                .ok_or_else(|| unreachable!("validation proves every parameter has a value"))
        })
        .collect()
}

/// The previous admission's invocations of `sequence`'s clips, when every
/// input of a clip's invocation other than the clip itself is unchanged: the
/// definitions, the sequence's mark collections and its automation. A clip
/// that is still the same allocation then admits to the same invocation.
fn reusable_effect_inputs<'a>(
    project: &DonderProject,
    previous: Option<&'a DonderProject>,
    sequence: &Sequence,
) -> std::collections::HashMap<
    &'a crate::effect::ClipId,
    (
        &'a std::sync::Arc<crate::effect::Clip>,
        &'a std::sync::Arc<Invocation>,
    ),
> {
    let Some(previous) = previous else {
        return Default::default();
    };
    let (Some(before), Some(inputs)) = (
        previous.get::<crate::Sequence>(sequence.id),
        previous.accepted_inputs.sequences.get(&sequence.id),
    ) else {
        return Default::default();
    };
    if !project.definitions_same_allocation(previous)
        || before.mark_collections != sequence.mark_collections
        || before.automation_clips != sequence.automation_clips
    {
        return Default::default();
    }
    before
        .clips
        .iter()
        .zip(inputs.clips.iter())
        .map(|(clip, invocation)| (&clip.id, (clip, invocation)))
        .collect()
}

pub(super) fn invalid(problem: AdmissionProblem) -> ValidationProblem {
    ValidationProblem::Admission(problem)
}
