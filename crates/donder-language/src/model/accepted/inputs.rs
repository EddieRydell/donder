//! Playback invocations derived during the same admission transaction as
//! authored state; they cannot be edited independently afterwards.
use super::parameters::{EffectParamTiming, prepare_params};
use super::*;
use crate::dsl::{
    OperatorDefinition as ProgramOperatorDefinition, OperatorInvocation, SampleDefinition,
    SampleInvocation,
};
use crate::dsl::{ParamDecl, Value};
use crate::effect::{EffectImplementation, EffectParamValue, EffectRef};
use crate::execution::{SequenceTiming, SequenceWindow};
use crate::operator::OperatorImplementation;
use crate::sequence::AutomationTarget;
use crate::sequence::CompositionGraphNodeId;
use crate::validation::ProjectValidationError;
use crate::values::{SampleDuration, SampleTime};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::Arc;

#[derive(Debug, Default)]
pub(in crate::model) struct ProjectInputs {
    pub(super) sequences: IndexMap<SequenceId, SequenceInputs>,
    pub(super) layouts: IndexMap<crate::layout::LayoutId, super::geometry::LayoutGeometry>,
    pub(super) patches: IndexMap<crate::patch::PatchId, super::patch::PatchEncodings>,
}

#[derive(Debug)]
pub(super) struct SequenceInputs {
    pub(super) timing: SequenceTiming,
    pub(super) effects: Box<[SampleInvocation]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, OperatorInvocation>,
}

impl ProjectInputs {
    pub(in crate::model) fn admit(
        project: &DonderProject,
        previous: Option<&DonderProject>,
    ) -> Result<Self, ProjectValidationError> {
        let definitions: IndexMap<_, _> = project
            .definitions()
            .effects
            .definitions
            .iter()
            .map(|(id, definition)| {
                let EffectImplementation::Dsl(compiled) = definition.implementation();
                (
                    id,
                    SampleDefinition::new(Arc::clone(compiled.shared_sample_program())),
                )
            })
            .collect();
        let operator_definitions: IndexMap<_, _> = project
            .definitions()
            .operators
            .definitions
            .iter()
            .map(|(id, definition)| {
                let OperatorImplementation::Dsl(compiled) = definition.implementation();
                (
                    id,
                    ProgramOperatorDefinition::new(Arc::clone(compiled.shared_program())),
                )
            })
            .collect();
        let mut sequences = IndexMap::new();
        for sequence in project.sequences() {
            let mut effects = Vec::with_capacity(sequence.effects.len());
            for effect in &sequence.effects {
                let EffectRef::Custom(id) = &effect.definition;
                let definition = &project.definitions().effects.definitions[id];
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
                )?;
                let EffectImplementation::Dsl(compiled) = definition.implementation();
                compiled
                    .check_values(&values)
                    .map_err(|error| invalid(error.message))?;
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id == &effect.id),
                )?;
                let invocation = definitions[id]
                    .bind(values.into_vec())
                    .and_then(|invocation| invocation.with_automation(automation))
                    .map_err(|error| invalid(error.message))?;
                effects.push(invocation);
            }
            let mut operators = IndexMap::new();
            for node in &sequence.composition_graph.nodes {
                let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
                    continue;
                };
                let OperatorRef::Custom(id) = &operator.operator;
                let definition = &project.definitions().operators.definitions[id];
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
                )?;
                let OperatorImplementation::Dsl(compiled) = definition.implementation();
                compiled
                    .check_values(&values)
                    .map_err(|error| invalid(error.message))?;
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == &node.id),
                )?;
                let invocation = operator_definitions[id]
                    .bind(values.into_vec())
                    .and_then(|invocation| invocation.with_automation(automation))
                    .map_err(|error| invalid(error.message))?;
                operators.insert(node.id.clone(), invocation);
            }
            sequences.insert(
                sequence.id.clone(),
                SequenceInputs {
                    timing: admit_timing(sequence)?,
                    effects: effects.into(),
                    operators,
                },
            );
        }
        Ok(Self {
            sequences,
            layouts: super::geometry::admit(project, previous)?,
            patches: super::patch::admit(project)?,
        })
    }
}

fn admit_timing(sequence: &Sequence) -> Result<SequenceTiming, ProjectValidationError> {
    let admit = || {
        let frame_rate = NonZeroU32::new(sequence.frame_rate)?;
        let frame_count = NonZeroU32::new(u32::try_from(sequence.frame_count()).ok()?)?;
        let duration = NonZeroU32::new(u32::try_from(sequence.duration.as_micros_rounded()).ok()?)?;
        let windows = sequence
            .effects
            .iter()
            .map(|effect| {
                Some(SequenceWindow {
                    start: SampleTime::from_ticks(
                        u32::try_from(effect.start.as_micros_rounded()).ok()?,
                    ),
                    duration: NonZeroU32::new(
                        u32::try_from(effect.duration.as_micros_rounded()).ok()?,
                    )?,
                })
            })
            .collect::<Option<Box<[_]>>>()?;
        SequenceTiming::admit(frame_rate, frame_count, duration, windows)
    };
    admit()
        .ok_or_else(|| invalid("Sequence timing is not representable by the playback clock".into()))
}

fn resolve(
    project: &DonderProject,
    sequence: &Sequence,
    declarations: &[ParamDecl],
    overrides: &IndexMap<crate::dsl::Identifier, EffectParamValue>,
    timing: EffectParamTiming,
) -> Result<Box<[Value]>, ProjectValidationError> {
    let values = prepare_params(project, sequence, declarations, overrides, timing);
    declarations
        .iter()
        .map(|param| {
            values
                .get(&param.name)
                .cloned()
                .ok_or_else(|| invalid(format!("Missing parameter `{}`", param.name.as_str())))
        })
        .collect()
}

fn invalid(message: String) -> ProjectValidationError {
    ProjectValidationError::InvalidRelationship(message)
}
