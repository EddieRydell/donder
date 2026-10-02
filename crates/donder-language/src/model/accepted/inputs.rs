//! Playback invocations derived during the same admission transaction as
//! authored state; they cannot be edited independently afterwards.
use super::parameters::{EffectParamTiming, prepare_params};
use super::*;
use crate::dsl::{EffectProgram, ParamDecl, Value};
use crate::effect::{EffectDefinitionId, EffectImplementation, EffectParamValue, EffectRef};
use crate::operator::OperatorImplementation;
use crate::sequence::AutomationTarget;
use crate::sequence::CompositionGraphNodeId;
use crate::validation::ProjectValidationError;
use crate::values::{SampleDuration, SampleTime};
use donder_runtime::DslBindCache;
use donder_runtime::SampleProgram;
use donder_runtime::{
    GeneratorPlayback, OperatorDefinition as RuntimeOperatorDefinition, OperatorInvocation,
    SampleDefinition, SampleInvocation, SequenceTiming, SequenceWindow,
};
use donder_runtime::{GeneratorTarget, LinkedGenerator};
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
    pub(super) effects: Box<[Execution]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, OperatorInvocation>,
}

#[derive(Debug)]
pub(super) enum Execution {
    Sample(SampleInvocation),
    Generator(GeneratorPlayback),
}

#[derive(Clone)]
enum Definition {
    Sample(Arc<SampleProgram>),
    Generator(Arc<LinkedGenerator>),
}

impl ProjectInputs {
    pub(in crate::model) fn admit(
        project: &DonderProject,
        previous: Option<&DonderProject>,
    ) -> Result<Self, ProjectValidationError> {
        let mut definitions = IndexMap::new();
        for id in project.definitions().effects.definitions.keys() {
            link(project, id, &mut definitions)?;
        }
        let operator_definitions: IndexMap<_, _> = project
            .definitions()
            .operators
            .definitions
            .iter()
            .map(|(id, definition)| {
                let OperatorImplementation::Dsl(compiled) = definition.implementation();
                (
                    id,
                    RuntimeOperatorDefinition::new(compiled.as_ref().clone()),
                )
            })
            .collect();
        let mut bind_cache = DslBindCache::default();
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
                let execution = match &definitions[id] {
                    Definition::Sample(program) => {
                        let automation = super::automation::admit(
                            sequence,
                            definition.params(),
                            |target| matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id == &effect.id),
                        )?;
                        let invocation = SampleDefinition::new(Arc::clone(program))
                            .bind(values.into_vec(), &mut bind_cache)
                            .and_then(|invocation| invocation.with_automation(automation))
                            .map_err(|error| invalid(error.message))?;
                        Execution::Sample(invocation)
                    }
                    Definition::Generator(generator) => {
                        let automation = super::automation::admit(
                            sequence,
                            definition.params(),
                            |target| matches!(target, AutomationTarget::EffectParam { effect_id, .. } if effect_id == &effect.id),
                        )?;
                        Execution::Generator(
                            GeneratorPlayback::admit(
                                Arc::clone(generator),
                                values.into_vec(),
                                automation,
                                &mut bind_cache,
                            )
                            .map_err(|error| invalid(error.message))?,
                        )
                    }
                };
                effects.push(execution);
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
                let automation = super::automation::admit(
                    sequence,
                    definition.params(),
                    |target| matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == &node.id),
                )?;
                let invocation = operator_definitions[id]
                    .bind(values.into_vec(), &mut bind_cache)
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

fn link(
    project: &DonderProject,
    id: &EffectDefinitionId,
    definitions: &mut IndexMap<EffectDefinitionId, Definition>,
) -> Result<Definition, ProjectValidationError> {
    if let Some(definition) = definitions.get(id) {
        return Ok(definition.clone());
    }
    let definition = &project.definitions().effects.definitions[id];
    let EffectImplementation::Dsl(compiled) = definition.implementation();
    let linked = match compiled.program() {
        EffectProgram::Sample(program) => Definition::Sample(Arc::clone(program)),
        EffectProgram::Generator(program) => {
            let targets = definition
                .generated_effect_targets()
                .iter()
                .map(|target| {
                    let EffectRef::Custom(id) = target;
                    Ok(match link(project, id, definitions)? {
                        Definition::Sample(program) => GeneratorTarget::Sample {
                            program,
                            params: project.definitions().effects.definitions[id]
                                .params()
                                .into(),
                        },
                        Definition::Generator(generator) => GeneratorTarget::Generator(generator),
                    })
                })
                .collect::<Result<_, ProjectValidationError>>()?;
            Definition::Generator(
                LinkedGenerator::link(Arc::clone(program), targets).ok_or_else(|| {
                    invalid(format!(
                        "Invalid compiled child bindings in `{}`",
                        id.0.object()
                    ))
                })?,
            )
        }
    };
    definitions.insert(id.clone(), linked.clone());
    Ok(linked)
}

fn invalid(message: String) -> ProjectValidationError {
    ProjectValidationError::InvalidRelationship(message)
}
