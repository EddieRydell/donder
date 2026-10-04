//! Executable program banks. Trusted archival addresses are translated once;
//! playback stores the role-specific instruction representations.
use crate::archive::LoadError;
use crate::dsl::AutomationPlan;
use crate::dsl::{OperatorProgram, SampleProgram};
use crate::signal::{
    PreparedEffect, PreparedEffectAutomation, PreparedSignalGraph, PreparedSignalKind,
    PreparedSignalNode,
};
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

pub(crate) type ExecutableGraph = PreparedSignalGraph<AdmittedPrograms, AutomationPlan>;

/// Restore a compatible producer's graph without repeating compiler validation.
/// The only fallible step converts raw instructions to the effect representation.
pub(super) fn restore_graph(mut graph: PreparedSignalGraph) -> Result<ExecutableGraph, LoadError> {
    let mut samples = Vec::new();
    let mut operators = Vec::new();
    let mut sample_indices = vec![None; graph.programs.len()];
    let mut operator_indices: Vec<Vec<(usize, usize)>> = vec![Vec::new(); graph.programs.len()];
    for effect in &mut graph.effects {
        let index = effect.program;
        let bytecode = &graph.programs[index];
        let mapped = match sample_indices[index] {
            Some(index) => index,
            None => {
                let bound_params = &effect.bound_params;
                let sample = SampleProgram::from_trusted_bytecode(
                    bytecode.clone(),
                    bound_params.types().into(),
                )
                .ok_or(LoadError::Archive)?;
                let mapped = samples.len();
                samples.push(sample);
                sample_indices[index] = Some(mapped);
                mapped
            }
        };
        effect.program = mapped;
    }
    for node in &mut graph.plan.nodes {
        let PreparedSignalKind::Operator {
            operator, inputs, ..
        } = &mut node.kind
        else {
            continue;
        };
        let index = &mut operator.program;
        let bytecode = &graph.programs[*index];
        let mapped = match operator_indices[*index]
            .iter()
            .find(|(count, _)| *count == inputs.len())
        {
            Some((_, mapped)) => *mapped,
            None => {
                let program = OperatorProgram::from_trusted_bytecode(
                    bytecode.clone(),
                    inputs.len(),
                    operator.params.types().into(),
                );
                let mapped = operators.len();
                operators.push(program);
                operator_indices[*index].push((inputs.len(), mapped));
                mapped
            }
        };
        *index = mapped;
    }
    let graph = graph.map_automation(
        |effect| {
            map_effect(effect, |params, bindings| {
                AutomationPlan::from_accepted(params, &bindings)
            })
        },
        |node| {
            map_node(node, |params, bindings| {
                AutomationPlan::from_accepted(params, &bindings)
            })
        },
    );
    Ok(graph.map_storage(|_| AdmittedPrograms::new(samples.into(), operators.into())))
}

impl ExecutableGraph {
    /// Reconstruct archival addresses only while encoding or explicitly projecting raw data.
    pub(crate) fn to_raw(&self) -> PreparedSignalGraph {
        let mut graph = self.clone();
        let sample_count = graph.programs.samples.len();
        for node in &mut graph.plan.nodes {
            if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind {
                let index = &mut operator.program;
                *index += sample_count;
            }
        }
        let graph = graph.map_automation(
            |effect| map_effect(effect, |_, plan| plan.to_raw()),
            |node| map_node(node, |_, plan| plan.to_raw()),
        );
        graph.map_storage(|programs| {
            programs
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
                .collect()
        })
    }
}

fn map_effect<A, B>(
    effect: PreparedEffect<A>,
    map: impl FnOnce(&crate::dsl::BoundParams, A) -> B,
) -> PreparedEffect<B> {
    let automation = effect.automation.map(|automation| {
        Box::new(PreparedEffectAutomation {
            workspace_slot: automation.workspace_slot,
            bindings: map(&effect.bound_params, automation.bindings),
        })
    });
    PreparedEffect {
        start_time: effect.start_time,
        duration: effect.duration,
        target: effect.target,
        program: effect.program,
        bound_params: effect.bound_params,
        automation,
    }
}

fn map_node<A, B>(
    node: PreparedSignalNode<A>,
    map: impl FnOnce(&crate::dsl::BoundParams, A) -> B,
) -> PreparedSignalNode<B> {
    let kind = match node.kind {
        PreparedSignalKind::Layer { layer_index } => PreparedSignalKind::Layer { layer_index },
        PreparedSignalKind::Output { inputs } => PreparedSignalKind::Output { inputs },
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let automation = map(&operator.params, automation);
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                vm_slot,
            }
        }
    };
    PreparedSignalNode { kind }
}
