use super::generators::{GeneratorExpansion, GeneratorPrepareContext};
use super::preparation::prepare_sample_program;
use crate::sequence::targets::{
    generator_context_target, prepared_pixels_from_generated_target_cached, sorted_sample_target,
};
use crate::{PreparedEffect, PreparedEffectImplementation, RenderError};
use dawn_language::dsl::{
    BoundParams, GeneratorBinding, GeneratorContext, GeneratorInput, ParamDecl, Type, Value,
};
use dawn_language::effect::{EffectDefinition, EffectImplementation, EffectRef};
use dawn_runtime::bindings::{
    ParameterSource, PreparedParameterBinding, PreparedParameterCalculation,
    PreparedParameterEnvironment,
};
use dawn_runtime::signal::BoundEffectImplementation;

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
    let mut capacity = 0u32;
    let mut width = 0u32;
    for (index, input) in inputs.iter().enumerate() {
        values.push(match input {
            ParameterInput::Constant(value) => Some(value.clone()),
            ParameterInput::Source(source) => {
                let parent = &context.environments[source.environment as usize];
                capacity = capacity
                    .checked_add(parent.array_capacity)
                    .ok_or_else(|| error("parameter array capacity exceeded"))?;
                width = width.max(parent.array_width);
                bindings.push(PreparedParameterBinding {
                    parameter: u16::try_from(index).map_err(|_| error("too many parameters"))?,
                    source: *source,
                });
                None
            }
        });
    }
    if let Some(calculation) = &calculation
        && calculation
            .outputs
            .iter()
            .any(|ty| matches!(ty, Type::Array(_)))
    {
        capacity = capacity
            .checked_add(calculation.program.array_capacity)
            .ok_or_else(|| error("parameter array capacity exceeded"))?;
        width = width.max(calculation.program.array_width);
    }
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
    if expansion.depth >= 4 {
        return Err(error("generator depth limit exceeded"));
    }
    for (param, input) in definition.params.iter().zip(inputs) {
        if param.fixed && matches!(input, ParameterInput::Source(_)) {
            return Err(error(format!(
                "live binding reaches fixed parameter `{}`",
                param.name.as_str()
            )));
        }
    }
    if let EffectImplementation::Native(builtin) = definition.implementation {
        let params = constant_params(&definition.params, inputs, context)?;
        let bound = crate::native_effect::bind_prepared(builtin, params.clone())?;
        let live = inputs
            .iter()
            .any(|input| matches!(input, ParameterInput::Source(_)));
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
        for child in bound.generate_structure(&GeneratorContext {
            start_time: expansion.start_time,
            duration: expansion.duration,
            target: generator_context_target(context.target_cache, &expansion.target),
        })? {
            if *context.generated_child_count >= crate::MAX_GENERATED_EFFECTS {
                return Err(error("generated child limit exceeded"));
            }
            *context.generated_child_count += 1;
            let target = prepared_pixels_from_generated_target_cached(
                context.target_cache,
                context.fixtures,
                child.target,
            )?;
            let implementation = match environment {
                Some(environment) => PreparedEffectImplementation::Bound {
                    environment,
                    implementation: BoundEffectImplementation::Native(child.sample),
                },
                None => PreparedEffectImplementation::Native {
                    sample: child.sample.resolve(&params)?,
                    params: None,
                },
            };
            context.effects.push(PreparedEffect {
                start_time: child.start_time,
                duration: child.duration,
                target: context
                    .target_cache
                    .sample_target(sorted_sample_target(&target))?,
                implementation,
                automation: None,
            });
        }
        return Ok(());
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
        crate::MAX_GENERATED_EFFECTS.saturating_sub(*context.generated_child_count),
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
        if *context.generated_child_count >= crate::MAX_GENERATED_EFFECTS {
            return Err(error("generated child limit exceeded"));
        }
        *context.generated_child_count += 1;
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
    if definition.kind == dawn_language::dsl::EffectKind::Generator {
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
            let EffectRef::Custom(id) = reference else {
                unreachable!("DSL child is custom")
            };
            let program = prepare_sample_program(context.sample_programs, id, &compiled.bytecode)?;
            match environment {
                Some(environment) => PreparedEffectImplementation::Bound {
                    environment,
                    implementation: BoundEffectImplementation::Dsl(program),
                },
                None => PreparedEffectImplementation::Dsl {
                    program,
                    bound_params: constant_params(&definition.params, inputs, context)?,
                },
            }
        }
        EffectImplementation::Native(builtin) => match environment {
            Some(environment) => PreparedEffectImplementation::Bound {
                environment,
                implementation: BoundEffectImplementation::Native(
                    crate::native_effect::NativeParameterSample::Sample(*builtin),
                ),
            },
            None => {
                let params = constant_params(&definition.params, inputs, context)?;
                PreparedEffectImplementation::Native {
                    sample: crate::native_effect::prepare_sample(*builtin, &params)?,
                    params: None,
                }
            }
        },
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
