use super::generators::{GeneratorExpansion, GeneratorPrepareContext};
use super::preparation::prepare_sample_program;
use crate::sequence::targets::{
    generator_context_target, prepared_pixels_from_generated_target_cached, sorted_sample_target,
};
use crate::{PreparedEffect, PreparedEffectImplementation, RenderError};
use donder_language::dsl::{
    BoundParams, GeneratorBinding, GeneratorContext, GeneratorInput, ParamDecl, Type, Value,
};
use donder_language::effect::{EffectDefinition, EffectImplementation, EffectRef};
use donder_runtime::bindings::{
    ParameterSource, PreparedParameterBinding, PreparedParameterCalculation,
    PreparedParameterEnvironment,
};

#[derive(Clone)]
pub(crate) enum ParameterInput {
    Constant(Value),
    Source(ParameterSource),
}

pub(crate) fn compact_environments(
    environments: &mut Box<[PreparedParameterEnvironment]>,
    effects: &mut [PreparedEffect],
) -> Result<(), RenderError> {
    let mut required = vec![false; environments.len()];
    for effect in effects.iter() {
        if let PreparedEffectImplementation::Bound { environment, .. } = effect.implementation {
            *required
                .get_mut(environment as usize)
                .ok_or_else(|| error("invalid parameter environment"))? = true;
        }
    }
    for index in (0..required.len()).rev() {
        if required[index] {
            for binding in &environments[index].bindings {
                required[binding.source.environment as usize] = true;
            }
        }
    }
    let mut mapping = vec![0u32; required.len()];
    let mut retained = Vec::new();
    for (index, mut environment) in std::mem::take(environments)
        .into_vec()
        .into_iter()
        .enumerate()
    {
        if !required[index] {
            continue;
        }
        mapping[index] =
            u32::try_from(retained.len()).map_err(|_| error("too many parameter environments"))?;
        for binding in &mut environment.bindings {
            binding.source.environment = mapping[binding.source.environment as usize];
        }
        retained.push(environment);
    }
    for effect in effects {
        if let PreparedEffectImplementation::Bound { environment, .. } = &mut effect.implementation
        {
            *environment = mapping[*environment as usize];
        }
    }
    *environments = retained.into_boxed_slice();
    Ok(())
}

fn error(message: impl Into<String>) -> RenderError {
    RenderError::GeneratorPrepare {
        message: message.into(),
    }
}

pub(crate) fn environment(
    context: &mut GeneratorPrepareContext<'_>,
    types: Box<[Type]>,
    inputs: &[ParameterInput],
    expansion: &GeneratorExpansion,
    calculation: Option<PreparedParameterCalculation>,
) -> Result<u32, RenderError> {
    let mut bindings = Vec::new();
    let mut values = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        values.push(match input {
            ParameterInput::Constant(value) => Some(value.clone()),
            ParameterInput::Source(source) => {
                bindings.push(PreparedParameterBinding {
                    parameter: u16::try_from(index).map_err(|_| error("too many parameters"))?,
                    source: *source,
                });
                None
            }
        });
    }
    let (capacity, width) = PreparedParameterEnvironment::required_array_storage(
        context.environments,
        &bindings,
        calculation.as_ref(),
    )
    .ok_or_else(|| error("parameter array storage bound could not be calculated"))?;
    let index = u32::try_from(context.environments.len())
        .map_err(|_| error("too many parameter environments"))?;
    context.environments.push(PreparedParameterEnvironment {
        start_time: expansion.start_time,
        duration: expansion.duration,
        params: BoundParams::bind_slots(&types, &values, context.bind_cache)?,
        types,
        bindings: bindings.into_boxed_slice(),
        automation: Box::new([]),
        calculation,
        array_capacity: capacity,
        array_width: width,
    });
    Ok(index)
}

fn resolve(
    binding: &GeneratorBinding,
    inputs: &[ParameterInput],
    calculations: &[u32],
) -> ParameterInput {
    match binding {
        GeneratorBinding::Constant(value) => ParameterInput::Constant(value.clone()),
        GeneratorBinding::Parameter(index) => inputs[usize::from(*index)].clone(),
        GeneratorBinding::Calculation { index, output } => {
            ParameterInput::Source(ParameterSource {
                environment: calculations[*index as usize],
                parameter: *output,
            })
        }
    }
}

pub(crate) fn expand(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &EffectDefinition,
    inputs: &[ParameterInput],
    expansion: GeneratorExpansion,
) -> Result<(), RenderError> {
    if expansion.depth >= context.project.definitions.effects.definitions.len() {
        return Err(error("generated effect references form a cycle"));
    }
    for (param, input) in definition.params.iter().zip(inputs) {
        if param.fixed && matches!(input, ParameterInput::Source(_)) {
            return Err(error(format!(
                "live binding reaches fixed parameter `{}`",
                param.name.as_str()
            )));
        }
    }
    let program = definition
        .generator
        .as_ref()
        .ok_or_else(|| error("missing checked generator program"))?;
    let specialized = program.specialize(
        &inputs
            .iter()
            .map(|input| match input {
                ParameterInput::Constant(value) => GeneratorInput::Fixed(value.clone()),
                ParameterInput::Source(_) => GeneratorInput::Live,
            })
            .collect::<Vec<_>>(),
        &GeneratorContext {
            start_time: expansion.start_time,
            duration: expansion.duration,
            target: generator_context_target(context.target_cache, &expansion.target),
        },
    )?;
    let mut calculations = Vec::new();
    for calculation in specialized.calculations {
        let types = calculation
            .inputs
            .iter()
            .map(|(ty, _)| ty.clone())
            .collect();
        let arguments = calculation
            .inputs
            .iter()
            .map(|(_, binding)| resolve(binding, inputs, &calculations))
            .collect::<Vec<_>>();
        calculations.push(environment(
            context,
            types,
            &arguments,
            &expansion,
            Some(PreparedParameterCalculation {
                program: calculation.program,
                outputs: calculation.output_types,
            }),
        )?);
    }
    for child in specialized.children {
        let reference = definition
            .generated_effect_targets
            .get(child.definition.0 as usize)
            .ok_or_else(|| error("invalid generated effect slot"))?;
        let child_definition = context
            .project
            .definitions
            .effects
            .resolve(reference)
            .ok_or_else(|| error("missing linked child definition"))?;
        let arguments = child_definition
            .params
            .iter()
            .map(|param| {
                if let Some((_, binding)) =
                    child.params.iter().find(|(name, _)| *name == param.name)
                {
                    Ok(resolve(binding, inputs, &calculations))
                } else {
                    param
                        .default
                        .clone()
                        .map(ParameterInput::Constant)
                        .ok_or_else(|| {
                            error(format!("missing generated param `{}`", param.name.as_str()))
                        })
                }
            })
            .collect::<Result<Vec<_>, RenderError>>()?;
        let target = prepared_pixels_from_generated_target_cached(
            context.target_cache,
            context.fixtures,
            child.target,
        )?;
        if child
            .start_time
            .checked_add_duration(child.duration)
            .is_none_or(|end| end.as_ticks() > context.sequence_duration.as_ticks())
        {
            continue;
        }
        let child_expansion = GeneratorExpansion {
            start_time: child.start_time,
            duration: child.duration,
            target,
            depth: expansion.depth + 1,
        };
        prepare_child(
            context,
            reference,
            child_definition,
            &arguments,
            child_expansion,
        )?;
    }
    Ok(())
}

fn constant_params(
    declarations: &[ParamDecl],
    inputs: &[ParameterInput],
    context: &mut GeneratorPrepareContext<'_>,
) -> Result<BoundParams, RenderError> {
    let types = declarations
        .iter()
        .map(|param| param.ty.clone())
        .collect::<Vec<_>>();
    let values = inputs
        .iter()
        .map(|input| match input {
            ParameterInput::Constant(value) => Some(value.clone()),
            ParameterInput::Source(_) => None,
        })
        .collect::<Vec<_>>();
    Ok(BoundParams::bind_slots(
        &types,
        &values,
        context.bind_cache,
    )?)
}

fn prepare_child(
    context: &mut GeneratorPrepareContext<'_>,
    reference: &EffectRef,
    definition: &EffectDefinition,
    inputs: &[ParameterInput],
    expansion: GeneratorExpansion,
) -> Result<(), RenderError> {
    let live = inputs
        .iter()
        .any(|input| matches!(input, ParameterInput::Source(_)));
    if definition.kind == donder_language::dsl::EffectKind::Generator {
        return expand(context, definition, inputs, expansion);
    }
    let environment = if live {
        Some(environment(
            context,
            definition
                .params
                .iter()
                .map(|param| param.ty.clone())
                .collect(),
            inputs,
            &expansion,
            None,
        )?)
    } else {
        None
    };
    let implementation = match &definition.implementation {
        EffectImplementation::Dsl(compiled) => {
            let EffectRef::Custom(id) = reference;
            let program = prepare_sample_program(context.sample_programs, id, &compiled.bytecode)?;
            match environment {
                Some(environment) => PreparedEffectImplementation::Bound {
                    environment,
                    program,
                },
                None => PreparedEffectImplementation::Dsl {
                    program,
                    bound_params: constant_params(&definition.params, inputs, context)?,
                },
            }
        }
    };
    context.effects.push(PreparedEffect {
        start_time: expansion.start_time,
        duration: expansion.duration,
        target: context
            .target_cache
            .sample_target(sorted_sample_target(&expansion.target))?,
        implementation,
        automation: None,
    });
    Ok(())
}
