use crate::RenderError;
use crate::native_effect::{self, BoundNativeEffect};
use crate::sequence::effects::generators::{GeneratorExpansion, GeneratorPrepareContext};
use crate::sequence::effects::parameters::{EffectParamTiming, prepare_params};
use crate::sequence::fixtures::PreparedFixture;
use crate::sequence::targets::{
    PreparedTargetCache, PreparedTargetPixel, generator_expansion_targets, prepare_target,
    prepare_target_pixels_cached, sorted_sample_target,
};
use crate::{
    PreparedAutomation, PreparedEffect, PreparedEffectAutomation, PreparedEffectImplementation,
};
use donder_language::dsl::{
    BoundParams, BytecodeProgram, DslBindCache, EffectKind, Identifier, ParamDecl,
};
use donder_language::effect::{EffectDefinitionId, EffectImplementation, EffectInstId, EffectRef};
use donder_language::layout::FixtureInstanceId;
use donder_language::model::DonderProject;
use donder_language::sequence::{AutomationBinding, AutomationClip, AutomationTarget, Sequence};
use indexmap::{IndexMap, IndexSet};
use std::sync::Arc;

pub(crate) struct PrepareEffectContext<'a> {
    pub(crate) project: &'a DonderProject,
    pub(crate) sequence: &'a Sequence,
    pub(crate) fixtures: &'a [PreparedFixture],
    pub(crate) fixture_ids: &'a IndexSet<FixtureInstanceId>,
    pub(crate) groups: &'a IndexMap<FixtureInstanceId, Vec<FixtureInstanceId>>,
    pub(crate) environments: &'a mut Vec<donder_runtime::bindings::PreparedParameterEnvironment>,
    pub(crate) effects: &'a mut Vec<PreparedEffect>,
    pub(crate) generated_child_count: &'a mut usize,
    pub(crate) bind_cache: &'a mut DslBindCache,
    pub(crate) sample_programs: &'a mut IndexMap<EffectDefinitionId, Arc<BytecodeProgram>>,
    pub(crate) target_cache: &'a mut PreparedTargetCache,
}

pub(crate) fn prepare_effect_inst(
    context: PrepareEffectContext<'_>,
    effect: &donder_language::effect::EffectInst,
) -> Result<Arc<[PreparedTargetPixel]>, RenderError> {
    if effect.duration.is_zero() {
        return Err(RenderError::InvalidTiming {
            reason: "effect duration must be positive".to_string(),
        });
    }
    let start_time =
        donder_language::values::sample_time_from_donder_time(&effect.start).map_err(|_| {
            RenderError::InvalidTiming {
                reason: "effect start exceeds the runtime clock range".to_string(),
            }
        })?;
    let duration = donder_language::values::sample_duration_from_donder_duration(&effect.duration)
        .map_err(|_| RenderError::InvalidTiming {
            reason: "effect duration exceeds the runtime clock range".to_string(),
        })?;
    let definition = context
        .project
        .definitions
        .effects
        .resolve(&effect.definition)
        .ok_or_else(|| RenderError::MissingEffect {
            effect_id: effect.definition.clone(),
        })?;
    let target_selection = prepare_target(&effect.target, context.fixture_ids, context.groups)?;
    let target = prepare_target_pixels_cached(
        context.target_cache,
        &target_selection,
        context.fixtures,
        &effect.scope,
    )?;
    let param_timing = EffectParamTiming {
        start: start_time,
        duration,
    };
    let automation = automation_for_effect(context.sequence, &effect.id, &definition.params)?;
    let params = prepare_params(
        context.project,
        context.sequence,
        &effect.param_overrides,
        param_timing,
    )?;
    match definition.kind {
        EffectKind::Sample => {
            let implementation = match &definition.implementation {
                EffectImplementation::Dsl(compiled) => {
                    let EffectRef::Custom(id) = &effect.definition else {
                        unreachable!("DSL effects are custom")
                    };
                    let program =
                        prepare_sample_program(context.sample_programs, id, &compiled.bytecode)?;
                    PreparedEffectImplementation::Dsl {
                        bound_params: compiled.bind_params_cached(&params, context.bind_cache)?,
                        program,
                    }
                }
                EffectImplementation::Native(builtin) => {
                    match native_effect::bind_cached(*builtin, &params, context.bind_cache)? {
                        BoundNativeEffect::Sample { sample, params } => {
                            PreparedEffectImplementation::Native {
                                sample,
                                params: (!automation.is_empty()).then_some((*builtin, params)),
                            }
                        }
                        _ => {
                            return Err(RenderError::GeneratorPrepare {
                                message: "native sample effect bound as generator".to_string(),
                            });
                        }
                    }
                }
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
                target: context.target_cache.sample_target(Arc::clone(&target))?,
                implementation,
                automation,
            });
        }
        EffectKind::Generator => {
            let params = BoundParams::bind_cached(&definition.params, &params, context.bind_cache)?;
            let mut inputs = (0..definition.params.len())
                .map(|index| {
                    params
                        .value(index)
                        .map(super::retained::ParameterInput::Constant)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if !automation.is_empty() {
                let environment = u32::try_from(context.environments.len()).map_err(|_| {
                    RenderError::GeneratorPrepare {
                        message: "too many parameter environments".to_string(),
                    }
                })?;
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
                fixtures: context.fixtures,
                effects: context.effects,
                generated_child_count: context.generated_child_count,
                bind_cache: context.bind_cache,
                sample_programs: context.sample_programs,
                target_cache: context.target_cache,
            };
            for expansion_target in generator_expansion_targets(&target, &effect.scope) {
                super::retained::expand(
                    &mut generator_context,
                    definition,
                    &inputs,
                    GeneratorExpansion {
                        start_time,
                        duration,
                        target: expansion_target,
                        depth: 0,
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
) -> Result<Vec<PreparedAutomation>, RenderError> {
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
) -> Result<PreparedAutomation, RenderError> {
    let param = automation_param(binding);
    let param_index = params
        .iter()
        .position(|declaration| declaration.name == *param)
        .ok_or_else(|| RenderError::BadGraph {
            message: format!("automation targets unknown parameter `{}`", param.as_str()),
        })?;
    let start =
        donder_language::values::sample_time_from_donder_time(&clip.start).map_err(|_| {
            RenderError::InvalidTiming {
                reason: "automation start exceeds the runtime clock range".to_string(),
            }
        })?;
    let duration = donder_language::values::sample_duration_from_donder_duration(&clip.duration)
        .map_err(|_| RenderError::InvalidTiming {
            reason: "automation duration exceeds the runtime clock range".to_string(),
        })?;
    let mut curve = clip.curve.clone();
    curve
        .points
        .sort_by(|left, right| left.position.total_cmp(&right.position));
    Ok(PreparedAutomation {
        start,
        duration,
        curve: Arc::new(curve),
        mapping: binding.mapping.clone(),
        param_index: u16::try_from(param_index).map_err(|_| RenderError::BadGraph {
            message: "effect or operator has too many parameters".to_string(),
        })?,
    })
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
) -> Result<u32, RenderError> {
    let index = match programs.get_index_of(id) {
        Some(index) => index,
        None => {
            programs
                .insert_full(id.clone(), Arc::new(program.clone()))
                .0
        }
    };
    u32::try_from(index).map_err(|_| RenderError::BadGraph {
        message: "prepared sequence has too many bytecode programs".to_string(),
    })
}
