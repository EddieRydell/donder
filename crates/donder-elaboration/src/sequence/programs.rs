//! Bind-time compilation and interning. Temporary compiler state is discarded
//! after preparation; playback retains only the resulting shared programs.
use donder_language::Shared;
use donder_language::dsl::{
    OperatorDefinition, OperatorInvocation, OperatorProgram, ProgramConstants, SampleDefinition,
    SampleInvocation, SampleProgram,
};

#[derive(Default)]
pub(super) struct Programs {
    samples: Vec<Shared<SampleProgram>>,
    operators: Vec<Shared<OperatorProgram>>,
}

fn intern<T: PartialEq>(programs: &mut Vec<Shared<T>>, program: T) -> Shared<T> {
    if let Some(existing) = programs.iter().find(|existing| ***existing == program) {
        return Shared::clone(existing);
    }
    let program = Shared::new(program);
    programs.push(Shared::clone(&program));
    program
}

impl Programs {
    pub(super) fn sample(
        &mut self,
        invocation: &SampleInvocation,
        constants: ProgramConstants,
    ) -> SampleInvocation {
        let program = invocation.program().specialize(
            invocation.params(),
            |param| {
                invocation
                    .automation()
                    .iter()
                    .any(|binding| usize::from(binding.param_index) == param)
            },
            constants,
        );
        let (program, params) = program.prepare_bindings(invocation.params(), |param| {
            invocation
                .automation()
                .iter()
                .any(|binding| usize::from(binding.param_index) == param)
        });
        SampleDefinition::new(intern(&mut self.samples, program))
            .bind(params.iter_values().collect())
            .unwrap_or_else(|_| unreachable!("specialization retains parameter schema"))
            .with_automation(invocation.automation().into())
            .unwrap_or_else(|_| unreachable!("specialization retains automation schema"))
    }

    pub(super) fn operator(
        &mut self,
        invocation: &OperatorInvocation,
        constants: ProgramConstants,
    ) -> OperatorInvocation {
        let program = invocation.program().specialize(
            invocation.params(),
            |param| {
                invocation
                    .automation()
                    .iter()
                    .any(|binding| usize::from(binding.param_index) == param)
            },
            constants,
        );
        let (program, params) = program.prepare_bindings(invocation.params(), |param| {
            invocation
                .automation()
                .iter()
                .any(|binding| usize::from(binding.param_index) == param)
        });
        OperatorDefinition::new(intern(&mut self.operators, program))
            .bind(params.iter_values().collect())
            .unwrap_or_else(|_| unreachable!("specialization retains parameter schema"))
            .with_automation(invocation.automation().into())
            .unwrap_or_else(|_| unreachable!("specialization retains automation schema"))
    }
}
