use crate::sequence::effects::generators::{GeneratorExpansion, GeneratorPrepareContext};
use crate::sequence::effects::parameters::{EffectParamTiming, prepare_params};
use crate::sequence::fixtures::PreparedFixture;
use crate::sequence::targets::{
    PreparedTargetCache, PreparedTargetPixel, generator_expansion_targets, prepare_target,
    prepare_target_pixels_cached, sorted_sample_target,
};
use donder_language::dsl::{
    BoundParams, BytecodeProgram, DslBindCache, EffectProgram, Identifier, ParamDecl,
};
use donder_language::effect::{EffectDefinitionId, EffectImplementation, EffectInstId, EffectRef};
use donder_language::layout::FixtureInstanceId;
use donder_language::model::DonderProject;
use donder_language::sequence::{AutomationBinding, AutomationClip, AutomationTarget, Sequence};
use donder_runtime::dsl::RuntimeError;
use donder_runtime::signal::{
    PreparedAutomation, PreparedEffect, PreparedEffectAutomation, PreparedEffectImplementation,
};
use indexmap::IndexMap;
use std::sync::Arc;

pub(crate) struct PrepareEffectContext<'a> {
    pub(crate) project: &'a DonderProject,
    pub(crate) sequence: &'a Sequence,
    pub(crate) fixtures: &'a [PreparedFixture],
    pub(crate) groups: &'a IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
    pub(crate) environments: &'a mut Vec<donder_runtime::bindings::PreparedParameterEnvironment>,
    pub(crate) effects: &'a mut Vec<PreparedEffect>,
    pub(crate) bind_cache: &'a mut DslBindCache,
    pub(crate) sample_programs: &'a mut IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    pub(crate) target_cache: &'a mut PreparedTargetCache,
}

pub(crate) fn prepare_effect_inst(
    context: PrepareEffectContext<'_>,
    effect: &donder_language::effect::EffectInst,
) -> Result<Arc<[PreparedTargetPixel]>, RuntimeError> {
    // Authored timing and references were checked when the project was accepted.
    let start_time =
        donder_language::values::SampleTime::from_ticks(effect.start.as_micros_rounded() as u32);
    let duration = donder_language::values::SampleDuration::from_ticks(
        effect.duration.as_micros_rounded() as u32,
    );
    let EffectRef::Custom(id) = &effect.definition;
    let definition = &context.project.definitions.effects.definitions[id];
    let target_selection = prepare_target(&effect.target, context.groups);
    let target = prepare_target_pixels_cached(
        context.target_cache,
        &target_selection,
        context.fixtures,
        &effect.scope,
    );
    let param_timing = EffectParamTiming {
        start: start_time,
        duration,
    };
    let automation = automation_for_effect(context.sequence, &effect.id, &definition.params);
    let params = prepare_params(
        context.project,
        context.sequence,
        &definition.params,
        &effect.param_overrides,
        param_timing,
    );
    let EffectImplementation::Dsl(compiled) = &definition.implementation;
    match &compiled.program {
        EffectProgram::Sample(bytecode) => {
            let program = prepare_sample_program(context.sample_programs, id, bytecode);
            let implementation = PreparedEffectImplementation::Dsl {
                bound_params: BoundParams::from_values(
                    definition
                        .params
                        .iter()
                        .map(|param| (&param.ty, params[&param.name].clone())),
                    context.bind_cache,
                ),
                program,
            };
            let target = sorted_sample_target(&target);
            let automation = (!automation.is_empty()).then(|| {
                Box::new(PreparedEffectAutomation {
                    workspace_slot: 0,
                    bindings: automation.into_boxed_slice(),
                })
            });
            context.effects.push(PreparedEffect {
                start_time,
                duration,
                target: context.target_cache.sample_target(Arc::clone(&target)),
                implementation,
                automation,
            });
        }
        EffectProgram::Generator(program) => {
            let sequence_duration = donder_language::values::SampleDuration::from_ticks(
                context.sequence.duration.as_micros_rounded() as u32,
            );
            let params = BoundParams::from_values(
                definition
                    .params
                    .iter()
                    .map(|param| (&param.ty, params[&param.name].clone())),
                context.bind_cache,
            );
            let mut inputs = params
                .iter_values()
                .map(super::retained::ParameterInput::Constant)
                .collect::<Vec<_>>();
            if !automation.is_empty() {
                let environment = context.environments.len();
                for binding in &automation {
                    inputs[usize::from(binding.param_index)] =
                        super::retained::ParameterInput::Source(
                            donder_runtime::bindings::ParameterSource {
                                environment,
                                parameter: binding.param_index,
                            },
                        );
                }
                context
                    .environments
                    .push(donder_runtime::bindings::PreparedParameterEnvironment {
                        start_time,
                        duration,
                        params: params.clone(),
                        types: definition
                            .params
                            .iter()
                            .map(|param| param.ty.clone())
                            .collect(),
                        bindings: Box::new([]),
                        automation: automation.clone().into_boxed_slice(),
                        calculation: None,
                        array_capacity: 0,
                        array_width: 0,
                    });
            }
            let mut generator_context = GeneratorPrepareContext {
                environments: context.environments,
                project: context.project,
                sequence_duration,
                effects: context.effects,
                bind_cache: context.bind_cache,
                sample_programs: context.sample_programs,
                target_cache: context.target_cache,
            };
            for expansion_target in generator_expansion_targets(&target, &effect.scope) {
                super::retained::expand(
                    &mut generator_context,
                    definition,
                    program,
                    &inputs,
                    GeneratorExpansion {
                        start_time,
                        duration,
                        target: expansion_target,
                    },
                )?;
            }
        }
    }
    Ok(target)
}

pub(crate) fn automation_for_effect(
    sequence: &Sequence,
    target_effect_id: &EffectInstId,
    params: &[ParamDecl],
) -> Vec<PreparedAutomation> {
    sequence
        .automation_clips
        .iter()
        .flat_map(|clip| {
            clip.bindings
                .iter()
                .filter(move |binding| {
                    matches!(
                        &binding.target,
                        AutomationTarget::EffectParam { effect_id, .. }
                            if effect_id == target_effect_id
                    )
                })
                .map(move |binding| prepare_automation(clip, binding, params))
        })
        .collect()
}

pub(crate) fn prepare_automation(
    clip: &AutomationClip,
    binding: &AutomationBinding,
    params: &[ParamDecl],
) -> PreparedAutomation {
    let param = automation_param(binding);
    let indexes = params
        .iter()
        .enumerate()
        .map(|(index, declaration)| (&declaration.name, index))
        .collect::<IndexMap<_, _>>();
    // The compiler bounds parameter slots; sequence validation checks the target
    // and the rounded clock range before this conversion.
    let param_index = indexes[param] as u16;
    let start =
        donder_language::values::SampleTime::from_ticks(clip.start.as_micros_rounded() as u32);
    let duration = donder_language::values::SampleDuration::from_ticks(
        clip.duration.as_micros_rounded() as u32,
    );
    let mut curve = clip.curve.clone();
    curve
        .points
        .sort_by(|left, right| left.position.total_cmp(&right.position));
    PreparedAutomation {
        start,
        duration,
        curve: Arc::new(curve),
        mapping: binding.mapping.clone(),
        param_index,
    }
}

pub(crate) fn automation_param(binding: &AutomationBinding) -> &Identifier {
    match &binding.target {
        AutomationTarget::EffectParam { param, .. }
        | AutomationTarget::CompositionNodeParam { param, .. } => param,
    }
}

pub(crate) fn prepare_sample_program(
    programs: &mut IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    id: &EffectDefinitionId,
    program: &BytecodeProgram,
) -> usize {
    match programs.get_index_of(id) {
        Some(index) => index,
        None => {
            programs
                .insert_full(id.clone(), Arc::new(program.clone()))
                .0
        }
    }
}
