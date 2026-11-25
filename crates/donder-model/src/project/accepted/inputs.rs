//! Playback invocations derived during the same admission transaction as
//! authored state; they cannot be edited independently afterwards.
use super::parameters::{ClipTiming, MarkOrigin, prepare_program_values};
use super::*;
use crate::effect::ParamValue;
use crate::sequence::AutomationTarget;
use crate::site::Member;
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::SampleTime;
use donder_runtime_types::SequenceTiming;
use donder_runtime_types::Value;
use indexmap::IndexMap;
use std::num::NonZeroU32;

#[derive(Debug, Default)]
pub(in crate::project) struct ProjectInputs {
    pub(super) shows: IndexMap<ShowId, CompositionInputs>,
    pub(super) layouts: IndexMap<crate::layout::LayoutId, super::geometry::LayoutGeometry>,
    pub(super) routes: super::patch::AdmittedRoutes,
}

impl ProjectInputs {
    pub(in crate::project) fn admit(
        project: &DonderProject,
        previous: Option<&DonderProject>,
    ) -> Result<Self, ProjectValidationError> {
        let mut shows = IndexMap::new();
        for (sequence_id, sequence) in project.objects::<crate::Sequence>() {
            let reusable = reusable_effect_inputs(project, previous, sequence_id, sequence);
            let mut clips = Vec::with_capacity(sequence.composition.clips.len());
            for clip in &sequence.composition.clips {
                let Some(effect) = clip.effect() else {
                    continue;
                };
                let Some((start, duration)) = clip.timing() else {
                    unreachable!("validation keeps a sequence's clips timed");
                };
                if let Some(invocation) =
                    reusable.get(&clip.id).and_then(|(previous, invocation)| {
                        std::sync::Arc::ptr_eq(previous, clip)
                            .then(|| std::sync::Arc::clone(invocation))
                    })
                {
                    clips.push(invocation);
                    continue;
                }
                let id = &effect.definition;
                let definition = &project.reusable::<crate::ScriptDefinition>()[id];
                let values = resolve_values(
                    project,
                    MarkOrigin::Authored(sequence),
                    definition.compiled(),
                    &effect.params,
                    ClipTiming {
                        start: crate::clock::sample_time(start),
                        duration: crate::clock::sample_duration(duration),
                    },
                );
                let compiled = definition.compiled();
                let automation = super::automation::admit(
                    &sequence.composition,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::ClipParam { clip_id, .. } if *clip_id == clip.id),
                ).map_err(at(sequence_id))?;
                let invocation =
                    compiled
                        .invoke(values.into_vec(), automation)
                        .map_err(|error| {
                            at(sequence_id)(invalid(AdmissionProblem::Binding(
                                Member::Clip(clip.id),
                                error,
                            )))
                        })?;
                clips.push(std::sync::Arc::new(ClipInputs {
                    invocation,
                    live: Box::default(),
                }));
            }
            let mut operators = IndexMap::new();
            for node in &sequence.composition.graph.nodes {
                let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
                    continue;
                };
                let id = &operator.operator;
                let definition = &project.reusable::<crate::ScriptDefinition>()[id];
                let values = resolve_values(
                    project,
                    MarkOrigin::Authored(sequence),
                    definition.compiled(),
                    &operator.params,
                    ClipTiming {
                        start: SampleTime::from_ticks(0),
                        duration: crate::clock::sample_duration(&sequence.duration),
                    },
                );
                let compiled = definition.compiled();
                let automation = super::automation::admit(
                    &sequence.composition,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == &node.id),
                ).map_err(at(sequence_id))?;
                let invocation =
                    compiled
                        .invoke(values.into_vec(), automation)
                        .map_err(|error| {
                            at(sequence_id)(invalid(AdmissionProblem::Binding(
                                Member::Node(node.id.clone()),
                                error,
                            )))
                        })?;
                operators.insert(node.id.clone(), invocation);
            }
            shows.insert(
                ShowId::Sequence(sequence_id),
                CompositionInputs {
                    timing: admit_timing(sequence),
                    clips: clips.into(),
                    operators,
                    operator_automations: IndexMap::new(),
                },
            );
        }
        shows.extend(super::console::admit(project)?);
        Ok(Self {
            shows,
            layouts: super::geometry::admit(project, previous)?,
            routes: super::patch::admit(project)?,
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

/// The program's values: every declared parameter's override or default,
/// then the enum options' fields.
pub(super) fn resolve_values(
    project: &DonderProject,
    marks: MarkOrigin<'_>,
    definition: &donder_language::compiler::CompiledDefinition,
    overrides: &IndexMap<donder_runtime_types::Identifier, ParamValue>,
    timing: ClipTiming,
) -> Box<[Value]> {
    prepare_program_values(project, marks, definition, overrides, timing).into()
}

/// The previous admission's invocations of `sequence`'s clips, when every
/// input of a clip's invocation other than the clip itself is unchanged: the
/// definitions, the sequence's mark collections and its automation. A clip
/// that is still the same allocation then admits to the same invocation.
fn reusable_effect_inputs<'a>(
    project: &DonderProject,
    previous: Option<&'a DonderProject>,
    sequence_id: crate::SequenceId,
    sequence: &Sequence,
) -> std::collections::HashMap<
    crate::effect::ClipId,
    (
        &'a std::sync::Arc<crate::effect::Clip>,
        &'a std::sync::Arc<ClipInputs>,
    ),
> {
    let Some(previous) = previous else {
        return Default::default();
    };
    let (Some(before), Some(inputs)) = (
        previous.get::<crate::Sequence>(sequence_id),
        previous
            .accepted_inputs
            .shows
            .get(&ShowId::Sequence(sequence_id)),
    ) else {
        return Default::default();
    };
    if !project.definitions_same_allocation(previous)
        || before.composition.marks != sequence.composition.marks
        || !before
            .composition
            .clips
            .iter()
            .filter(|clip| clip.envelope().is_some())
            .eq(sequence
                .composition
                .clips
                .iter()
                .filter(|clip| clip.envelope().is_some()))
        || before.audio != sequence.audio
        || !project.same_derived(previous)
    {
        return Default::default();
    }
    before
        .composition
        .clips
        .iter()
        .filter(|clip| clip.effect().is_some())
        .zip(inputs.clips.iter())
        .map(|(clip, inputs)| (clip.id, (clip, inputs)))
        .collect()
}

pub(super) fn invalid(problem: AdmissionProblem) -> ValidationProblem {
    ValidationProblem::Admission(problem)
}
