use super::generators::{GeneratorExpansion, GeneratorPrepareContext};
use super::preparation::prepare_sample_program;
use crate::sequence::targets::{generator_context_target, sorted_sample_target};
use donder_language::dsl::{
    BoundParams, EffectProgram, GeneratorBinding, GeneratorContext, GeneratorInput,
    GeneratorProgram, ParamDecl, Type, Value,
};
use donder_language::effect::{EffectDefinition, EffectImplementation, EffectRef};
use donder_runtime::bindings::{
    ParameterSource, PreparedParameterBinding, PreparedParameterCalculation,
    PreparedParameterEnvironment,
};
use donder_runtime::dsl::RuntimeError;
use donder_runtime::signal::{PreparedEffect, PreparedEffectImplementation};

#[derive(Clone)]
pub(crate) enum ParameterInput {
    Constant(Value),
    Source(ParameterSource),
}

pub(crate) fn compact_environments(
    environments: &mut Box<[PreparedParameterEnvironment]>,
    effects: &mut [PreparedEffect],
) {
    let mut required = vec![false; environments.len()];
    for effect in effects.iter() {
        if let PreparedEffectImplementation::Bound { environment, .. } = effect.implementation {
            required[environment as usize] = true;
        }
    }
    for index in (0..required.len()).rev() {
        if required[index] {
            for binding in &environments[index].bindings {
                required[binding.source.environment as usize] = true;
            }
        }
    }
    let mut mapping = vec![0usize; required.len()];
    let mut retained = Vec::new();
    for (index, mut environment) in std::mem::take(environments)
        .into_vec()
        .into_iter()
        .enumerate()
    {
        if !required[index] {
            continue;
        }
        // Removing environments cannot exceed the original address space.
        mapping[index] = retained.len();
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
}

pub(crate) fn environment(
    context: &mut GeneratorPrepareContext<'_>,
    types: Box<[Type]>,
    inputs: &[ParameterInput],
    expansion: &GeneratorExpansion,
    calculation: Option<PreparedParameterCalculation>,
) -> usize {
    let mut bindings = Vec::new();
    let mut values = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        values.push(match input {
            ParameterInput::Constant(value) => value.clone(),
            ParameterInput::Source(source) => {
                bindings.push(PreparedParameterBinding {
                    parameter: index as u16,
                    source: *source,
                });
                Value::Void
            }
        });
    }
    let (capacity, width) = PreparedParameterEnvironment::required_array_storage(
        bindings
            .iter()
            .map(|binding| &context.environments[binding.source.environment]),
        calculation.as_ref(),
    );
    let index = context.environments.len();
    context.environments.push(PreparedParameterEnvironment {
        start_time: expansion.start_time,
        duration: expansion.duration,
        params: BoundParams::from_values(types.iter().zip(values), context.bind_cache),
        types,
        bindings: bindings.into_boxed_slice(),
        automation: Box::new([]),
        calculation,
        array_capacity: capacity,
        array_width: width,
    });
    index
}

fn resolve(
    binding: &GeneratorBinding,
    inputs: &[ParameterInput],
    calculations: &[usize],
) -> ParameterInput {
    match binding {
        GeneratorBinding::Constant(value) => ParameterInput::Constant(value.clone()),
        GeneratorBinding::Parameter(index) => inputs[usize::from(*index)].clone(),
        GeneratorBinding::Calculation { index, output } => {
            ParameterInput::Source(ParameterSource {
                environment: calculations[*index],
                parameter: *output,
            })
        }
    }
}

pub(crate) fn expand(
    context: &mut GeneratorPrepareContext<'_>,
    definition: &EffectDefinition,
    program: &GeneratorProgram,
    inputs: &[ParameterInput],
    expansion: GeneratorExpansion,
) -> Result<(), RuntimeError> {
    // Project acceptance links every numeric child slot, checks its parameters,
    // and rejects cycles. Expansion does not repeat those source checks.
    let specialized = program
        .bind(
            &inputs
                .iter()
                .map(|input| match input {
                    ParameterInput::Constant(value) => GeneratorInput::Fixed(value.clone()),
                    ParameterInput::Source(_) => GeneratorInput::Live,
                })
                .collect::<Vec<_>>(),
        )?
        .specialize(&GeneratorContext {
            start_time: expansion.start_time,
            duration: expansion.duration,
            target: generator_context_target(context.target_cache, &expansion.target),
        })?;
    let mut calculations = Vec::new();
    for calculation in specialized.calculations {
        let arguments = calculation
            .inputs
            .iter()
            .map(|binding| resolve(binding, inputs, &calculations))
            .collect::<Vec<_>>();
        let (program, types, outputs) = calculation.program.into_parts();
        calculations.push(environment(
            context,
            types,
            &arguments,
            &expansion,
            Some(PreparedParameterCalculation { program, outputs }),
        ));
    }
    for child in specialized.children {
        let reference = &definition.generated_effect_targets[child.definition.0 as usize];
        let EffectRef::Custom(id) = reference;
        let child_definition = &context.project.definitions.effects.definitions[id];
        // Start with declared defaults, then overlay the emission. Linking has
        // already established that every required parameter is supplied.
        let defaults = child_definition.params.iter().filter_map(|param| {
            param
                .default
                .as_ref()
                .map(|value| (&param.name, ParameterInput::Constant(value.clone())))
        });
        let supplied = child
            .params
            .iter()
            .map(|(name, binding)| (name, resolve(binding, inputs, &calculations)));
        let values: indexmap::IndexMap<_, _> = defaults.chain(supplied).collect();
        let arguments = child_definition
            .params
            .iter()
            .map(|param| values[&param.name].clone())
            .collect::<Vec<_>>();
        let target = std::sync::Arc::clone(&child.target.pixels);
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
) -> BoundParams {
    BoundParams::from_values(
        declarations.iter().zip(inputs).map(|(declaration, input)| {
            (
                &declaration.ty,
                match input {
                    ParameterInput::Constant(value) => value.clone(),
                    ParameterInput::Source(_) => Value::Void,
                },
            )
        }),
        context.bind_cache,
    )
}

fn prepare_child(
    context: &mut GeneratorPrepareContext<'_>,
    reference: &EffectRef,
    definition: &EffectDefinition,
    inputs: &[ParameterInput],
    expansion: GeneratorExpansion,
) -> Result<(), RuntimeError> {
    let live = inputs
        .iter()
        .any(|input| matches!(input, ParameterInput::Source(_)));
    let EffectImplementation::Dsl(compiled) = &definition.implementation;
    let bytecode = match &compiled.program {
        EffectProgram::Generator(program) => {
            return expand(context, definition, program, inputs, expansion);
        }
        EffectProgram::Sample(program) => program,
    };
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
        ))
    } else {
        None
    };
    let EffectRef::Custom(id) = reference;
    let program = prepare_sample_program(context.sample_programs, id, bytecode);
    let implementation = match environment {
        Some(environment) => PreparedEffectImplementation::Bound {
            environment,
            program,
        },
        None => PreparedEffectImplementation::Dsl {
            program,
            bound_params: constant_params(&definition.params, inputs, context),
        },
    };
    context.effects.push(PreparedEffect {
        start_time: expansion.start_time,
        duration: expansion.duration,
        target: context
            .target_cache
            .sample_target(sorted_sample_target(&expansion.target)),
        implementation,
        automation: None,
    });
    Ok(())
}
