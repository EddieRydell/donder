//! Executable program banks. Raw addresses are translated once at admission;
//! playback stores only the role-specific admitted instruction representations.
use crate::bindings::{ExecutableEnvironment, admit_environments};
use crate::dsl::AutomationPlan;
use crate::dsl::{OperatorProgram, SampleProgram};
use crate::signal::{
    PreparedEffect, PreparedEffectAutomation, PreparedEffectImplementation, PreparedOperator,
    PreparedSignalGraph, PreparedSignalKind, PreparedSignalNode,
};
use crate::wire::LoadError;
use alloc::{boxed::Box, vec, vec::Vec};

#[derive(Clone, Debug)]
pub(crate) struct AdmittedPrograms {
    samples: Box<[SampleProgram]>,
    operators: Box<[OperatorProgram]>,
}

impl AdmittedPrograms {
    /// Builder-owned addresses index these banks; callers already hold admitted code.
    pub(crate) fn new(samples: Box<[SampleProgram]>, operators: Box<[OperatorProgram]>) -> Self {
        Self { samples, operators }
    }

    pub(crate) fn sample(&self, index: usize) -> &SampleProgram {
        &self.samples[index]
    }

    pub(crate) fn operator(&self, index: usize) -> &OperatorProgram {
        &self.operators[index]
    }
}

pub(crate) type ExecutableGraph =
    PreparedSignalGraph<AdmittedPrograms, ExecutableEnvironment, AutomationPlan>;

/// The raw graph's references, timing, and bindings have already passed wire admission.
/// The additional DSL admission converts each used program to its executable role.
pub(super) fn admit_graph(mut graph: PreparedSignalGraph) -> Result<ExecutableGraph, LoadError> {
    let bad = LoadError::InvalidSequence;
    let mut samples = Vec::new();
    let mut operators = Vec::new();
    let mut sample_indices = vec![None; graph.programs.len()];
    let mut operator_indices: Vec<Vec<(usize, usize)>> = vec![Vec::new(); graph.programs.len()];
    for effect in &mut graph.effects {
        let index = effect.implementation.dsl_program();
        let bytecode = graph.programs.get(index).ok_or(bad)?;
        let mapped = match sample_indices[index] {
            Some(index) => index,
            None => {
                let sample = match &effect.implementation {
                    PreparedEffectImplementation::Dsl { bound_params, .. } => {
                        SampleProgram::admit_bound(bytecode.clone(), bound_params)
                    }
                    PreparedEffectImplementation::Bound { environment, .. } => {
                        let types = graph.parameter_environments.get(*environment).ok_or(bad)?;
                        SampleProgram::admit(bytecode.clone(), types.output_types().into())
                    }
                }
                .ok_or(bad)?;
                let mapped = samples.len();
                samples.push(sample);
                sample_indices[index] = Some(mapped);
                mapped
            }
        };
        match &mut effect.implementation {
            PreparedEffectImplementation::Dsl { program, .. }
            | PreparedEffectImplementation::Bound { program, .. } => *program = mapped,
        }
    }
    for node in &mut graph.plan.nodes {
        let PreparedSignalKind::Operator {
            operator, inputs, ..
        } = &mut node.kind
        else {
            continue;
        };
        let PreparedOperator::Dsl(index) = &mut operator.implementation;
        let bytecode = graph.programs.get(*index).ok_or(bad)?;
        let mapped = match operator_indices[*index]
            .iter()
            .find(|(count, _)| *count == inputs.len())
        {
            Some((_, mapped)) => *mapped,
            None => {
                let program =
                    OperatorProgram::admit_bound(bytecode.clone(), inputs.len(), &operator.params)
                        .ok_or(bad)?;
                let mapped = operators.len();
                operators.push(program);
                operator_indices[*index].push((inputs.len(), mapped));
                mapped
            }
        };
        *index = mapped;
    }
    let environments =
        admit_environments(core::mem::take(&mut graph.parameter_environments).into_vec())
            .map_err(|_| bad)?;
    let graph = graph.try_map_automation(admit_effect, admit_node)?;
    Ok(graph.map_storage(|_, _| {
        (
            AdmittedPrograms::new(samples.into(), operators.into()),
            environments,
        )
    }))
}

impl ExecutableGraph {
    /// Reconstruct archival addresses only while encoding or explicitly projecting raw data.
    pub(crate) fn to_raw(&self) -> PreparedSignalGraph {
        let mut graph = self.clone();
        let sample_count = graph.programs.samples.len();
        for node in &mut graph.plan.nodes {
            if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind {
                let PreparedOperator::Dsl(index) = &mut operator.implementation;
                *index += sample_count;
            }
        }
        let graph = match graph.try_map_automation(
            |effect| {
                map_effect(effect, |_, plan| {
                    Ok::<_, core::convert::Infallible>(plan.to_raw())
                })
            },
            |node| {
                map_node(node, |_, plan| {
                    Ok::<_, core::convert::Infallible>(plan.to_raw())
                })
            },
        ) {
            Ok(graph) => graph,
            Err(never) => match never {},
        };
        graph.map_storage(|programs, environments| {
            let programs = programs
                .samples
                .into_vec()
                .into_iter()
                .map(|program| program.into_parts().0)
                .chain(
                    programs
                        .operators
                        .into_vec()
                        .into_iter()
                        .map(|program| program.into_parts().0),
                )
                .collect();
            let environments = environments
                .iter()
                .map(ExecutableEnvironment::to_raw)
                .collect();
            (programs, environments)
        })
    }
}

fn admit_effect(effect: PreparedEffect) -> Result<PreparedEffect<AutomationPlan>, LoadError> {
    map_effect(effect, |implementation, bindings| {
        match implementation {
            PreparedEffectImplementation::Dsl { bound_params, .. } => {
                AutomationPlan::admit(bound_params, &bindings).ok_or(LoadError::InvalidSequence)
            }
            // Bound effects use their environment's admitted automation.
            PreparedEffectImplementation::Bound { .. } => Err(LoadError::InvalidSequence),
        }
    })
}

fn admit_node(node: PreparedSignalNode) -> Result<PreparedSignalNode<AutomationPlan>, LoadError> {
    map_node(node, |params, bindings| {
        AutomationPlan::admit(params, &bindings).ok_or(LoadError::InvalidSequence)
    })
}

fn map_effect<A, B, X>(
    effect: PreparedEffect<A>,
    map: impl FnOnce(&PreparedEffectImplementation, A) -> Result<B, X>,
) -> Result<PreparedEffect<B>, X> {
    let automation = match effect.automation {
        Some(automation) => Some(Box::new(PreparedEffectAutomation {
            workspace_slot: automation.workspace_slot,
            bindings: map(&effect.implementation, automation.bindings)?,
        })),
        None => None,
    };
    Ok(PreparedEffect {
        start_time: effect.start_time,
        duration: effect.duration,
        target: effect.target,
        implementation: effect.implementation,
        automation,
    })
}

fn map_node<A, B, X>(
    node: PreparedSignalNode<A>,
    map: impl FnOnce(&crate::dsl::BoundParams, A) -> Result<B, X>,
) -> Result<PreparedSignalNode<B>, X> {
    let kind = match node.kind {
        PreparedSignalKind::Layer { layer_index } => PreparedSignalKind::Layer { layer_index },
        PreparedSignalKind::Output { inputs } => PreparedSignalKind::Output { inputs },
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let automation = map(&operator.params, automation)?;
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                vm_slot,
            }
        }
    };
    Ok(PreparedSignalNode { kind })
}
