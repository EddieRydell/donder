//! Executable program banks. Archived programs are admitted once, as effect
//! or operator programs, and their addresses translated to those banks.
use crate::archive::LoadError;
use crate::dsl::AutomationPlan;
use crate::dsl::{OperatorProgram, SampleProgram};
use crate::signal::{
    PreparedEffect, PreparedEffectAutomation, PreparedOperatorNode, PreparedSignalGraph,
    PreparedSignalKind, PreparedSignalNode,
};
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::SampleTime;

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

/// Admit an archived graph's programs. A program that is not well formed for
/// its role and inputs, or bound values not in its parameters' layout, reject
/// the archive.
pub(super) fn restore_graph(mut graph: PreparedSignalGraph) -> Result<ExecutableGraph, LoadError> {
    let mut samples = Vec::new();
    let mut operators = Vec::new();
    let mut sample_indices = vec![None; graph.programs.len()];
    let mut operator_indices: Vec<Vec<(usize, usize)>> = vec![Vec::new(); graph.programs.len()];
    for effect in &mut graph.effects {
        let index = effect.program;
        if !effect.bound_params.fits(&graph.programs[index]) {
            return Err(LoadError::Archive);
        }
        let mapped = match sample_indices[index] {
            Some(index) => index,
            None => {
                let sample = SampleProgram::admit(graph.programs[index].clone())
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
        if !operator.params.fits(&graph.programs[*index]) {
            return Err(LoadError::Archive);
        }
        let mapped = match operator_indices[*index]
            .iter()
            .find(|(count, _)| *count == inputs.len())
        {
            Some((_, mapped)) => *mapped,
            None => {
                let program = OperatorProgram::admit(graph.programs[*index].clone(), inputs.len())
                    .ok_or(LoadError::Archive)?;
                let mapped = operators.len();
                operators.push(program);
                operator_indices[*index].push((inputs.len(), mapped));
                mapped
            }
        };
        *index = mapped;
    }
    let programs = AdmittedPrograms::new(samples.into(), operators.into());
    let graph = graph.map_automation(
        |effect| {
            let origin = effect.start_time;
            map_effect(effect, |program, bindings| {
                AutomationPlan::from_accepted(
                    programs.sample(program).bytecode(),
                    &bindings,
                    origin,
                )
            })
        },
        |node| {
            map_node(node, |operator, bindings| {
                AutomationPlan::from_accepted(
                    programs.operator(operator.program).bytecode(),
                    &bindings,
                    SampleTime::from_ticks(0),
                )
            })
        },
    );
    Ok(graph.map_storage(|_| programs))
}

impl ExecutableGraph {
    /// Reconstruct archival addresses only while encoding or explicitly projecting raw data.
    pub(crate) fn to_raw(&self) -> PreparedSignalGraph {
        let programs = &self.programs;
        let mut graph = self.clone().map_automation(
            |effect| {
                map_effect(effect, |program, plan| {
                    plan.to_raw(programs.sample(program).bytecode())
                })
            },
            |node| {
                map_node(node, |operator, plan| {
                    plan.to_raw(programs.operator(operator.program).bytecode())
                })
            },
        );
        let sample_count = programs.samples.len();
        for node in &mut graph.plan.nodes {
            if let PreparedSignalKind::Operator { operator, .. } = &mut node.kind {
                operator.program += sample_count;
            }
        }
        graph.map_storage(|programs| {
            programs
                .samples
                .into_vec()
                .into_iter()
                .map(SampleProgram::into_bytecode)
                .chain(
                    programs
                        .operators
                        .into_vec()
                        .into_iter()
                        .map(OperatorProgram::into_bytecode),
                )
                .collect()
        })
    }
}

/// `map` receives the effect's program index.
fn map_effect<A, B>(
    effect: PreparedEffect<A>,
    map: impl FnOnce(usize, A) -> B,
) -> PreparedEffect<B> {
    let automation = effect.automation.map(|automation| {
        Box::new(PreparedEffectAutomation {
            workspace_slot: automation.workspace_slot,
            bindings: map(effect.program, automation.bindings),
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
    map: impl FnOnce(&PreparedOperatorNode, A) -> B,
) -> PreparedSignalNode<B> {
    let kind = match node.kind {
        PreparedSignalKind::Layer { layer_index } => PreparedSignalKind::Layer { layer_index },
        PreparedSignalKind::Output { inputs } => PreparedSignalKind::Output { inputs },
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => PreparedSignalKind::Operator {
            automation: map(&operator, automation),
            operator,
            inputs,
            vm_slot,
        },
    };
    PreparedSignalNode { kind }
}
