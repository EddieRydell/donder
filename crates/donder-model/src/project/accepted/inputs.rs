//! Playback invocations derived during the same admission transaction as
//! authored state; they cannot be edited independently afterwards.
use super::parameters::{EffectParamTiming, MarkSource, prepare_params};
use super::*;
use crate::effect::EffectParamValue;
use crate::sequence::AutomationTarget;
use crate::sequence::CompositionGraphNodeId;
use crate::validation::{AdmissionProblem, Member, ProjectValidationError, ValidationProblem, at};
use donder_language::compiler::{Invocation, ParamDecl};
use donder_runtime_types::SequenceTiming;
use donder_runtime_types::Value;
use donder_runtime_types::{SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::num::NonZeroU32;

#[derive(Debug, Default)]
pub(in crate::project) struct ProjectInputs {
    pub(super) sequences: IndexMap<SequenceId, SequenceInputs>,
    pub(super) rigs: IndexMap<crate::rig::RigId, super::rig::RigInputs>,
    pub(super) layouts: IndexMap<crate::layout::LayoutId, super::geometry::LayoutGeometry>,
    pub(super) patches: IndexMap<crate::patch::PatchId, super::patch::PatchEncodings>,
}

#[derive(Debug)]
pub(super) struct SequenceInputs {
    pub(super) timing: SequenceTiming,
    pub(super) effects: Box<[std::sync::Arc<Invocation>]>,
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
            let mut effects = Vec::with_capacity(sequence.effects.len());
            for effect in &sequence.effects {
                if let Some(invocation) =
                    reusable.get(&effect.id).and_then(|(previous, invocation)| {
                        std::sync::Arc::ptr_eq(previous, effect)
                            .then(|| std::sync::Arc::clone(invocation))
                    })
                {
                    effects.push(invocation);
                    continue;
                }
                let id = &effect.definition;
                let definition = &project.reusable::<crate::EffectDefinition>()[id];
                let values = resolve(
                    project,
                    sequence,
                    definition.params(),
                    &effect.param_overrides,
                    EffectParamTiming {
                        start: SampleTime::from_ticks(effect.start.as_micros_rounded() as u32),
                        duration: SampleDuration::from_ticks(
                            effect.duration.as_micros_rounded() as u32
                        ),
                    },
                )
                .map_err(at(sequence.id))?;
                let compiled = definition.compiled();
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id == &effect.id),
                ).map_err(at(sequence.id))?;
                let invocation =
                    compiled
                        .invoke(values.into_vec(), automation)
                        .map_err(|error| {
                            at(sequence.id)(invalid(AdmissionProblem::Binding(
                                Member::Clip(effect.id.clone()),
                                error,
                            )))
                        })?;
                effects.push(std::sync::Arc::new(invocation));
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
                    EffectParamTiming {
                        start: SampleTime::from_ticks(0),
                        duration: SampleDuration::from_ticks(
                            sequence.duration.as_micros_rounded() as u32
                        ),
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
                    timing: admit_timing(sequence).map_err(at(sequence.id))?,
                    effects: effects.into(),
                    operators,
                },
            );
        }
        Ok(Self {
            sequences,
            rigs: super::rig::admit(project)?,
            layouts: super::geometry::admit(project, previous)?,
            patches: super::patch::admit(project)?,
        })
    }
}

fn admit_timing(sequence: &Sequence) -> Result<SequenceTiming, ValidationProblem> {
    let admit = || {
        let frame_rate = NonZeroU32::new(sequence.frame_rate)?;
        let frame_count = NonZeroU32::new(u32::try_from(sequence.frame_count()).ok()?)?;
        let duration = NonZeroU32::new(u32::try_from(sequence.duration.as_micros_rounded()).ok()?)?;
        // Every clip plays for a while within the sequence.
        for effect in &sequence.effects {
            let start = u32::try_from(effect.start.as_micros_rounded()).ok()?;
            let length = u32::try_from(effect.duration.as_micros_rounded()).ok()?;
            if length == 0 || start.checked_add(length)? > duration.get() {
                return None;
            }
        }
        Some(SequenceTiming::new(frame_rate, frame_count, duration))
    };
    admit().ok_or_else(|| invalid(AdmissionProblem::TimingUnrepresentable))
}

fn resolve(
    project: &DonderProject,
    sequence: &Sequence,
    declarations: &[ParamDecl],
    overrides: &IndexMap<donder_runtime_types::Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<Box<[Value]>, ValidationProblem> {
    resolve_values(
        project,
        MarkSource::Authored(&sequence.mark_collections),
        declarations,
        overrides,
        timing,
    )
}

/// Every declared parameter's value: its override, or its default.
pub(super) fn resolve_values(
    project: &DonderProject,
    marks: MarkSource<'_>,
    declarations: &[ParamDecl],
    overrides: &IndexMap<donder_runtime_types::Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<Box<[Value]>, ValidationProblem> {
    let values = prepare_params(project, marks, declarations, overrides, timing);
    declarations
        .iter()
        .map(|param| {
            values
                .get(&param.name)
                .cloned()
                .ok_or_else(|| invalid(AdmissionProblem::MissingParam(param.name.clone())))
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
    &'a crate::effect::EffectInstId,
    (
        &'a std::sync::Arc<crate::effect::EffectInst>,
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
        .effects
        .iter()
        .zip(inputs.effects.iter())
        .map(|(effect, invocation)| (&effect.id, (effect, invocation)))
        .collect()
}

pub(super) fn invalid(problem: AdmissionProblem) -> ValidationProblem {
    ValidationProblem::Admission(problem)
}
