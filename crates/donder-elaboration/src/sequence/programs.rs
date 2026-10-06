//! Lowering and interning. Instances of one definition usually lower to the
//! same program with different bound values; playback keeps one copy.
use donder_language::Shared;
use donder_language::dsl::{
    Instance, OperatorDefinition, OperatorInvocation, OperatorProgram, SampleDefinition,
    SampleInvocation, SampleProgram,
};

#[derive(Default)]
pub(super) struct Programs {
    samples: Vec<Shared<SampleProgram>>,
    operators: Vec<Shared<OperatorProgram>>,
}

fn intern<T: PartialEq + Clone>(programs: &mut Vec<Shared<T>>, program: &Shared<T>) -> Shared<T> {
    if let Some(existing) = programs.iter().find(|existing| ***existing == **program) {
        return Shared::clone(existing);
    }
    programs.push(Shared::clone(program));
    Shared::clone(program)
}

impl Programs {
    pub(super) fn sample(&mut self, instance: &Instance) -> SampleInvocation {
        let lowered = instance.sample();
        SampleDefinition::new(intern(&mut self.samples, lowered.program()))
            .bind(lowered.params().iter_values().collect())
            .and_then(|invocation| invocation.with_automation(lowered.automation().into()))
            .unwrap_or_else(|_| unreachable!("interning keeps the program's schema"))
    }

    pub(super) fn operator(&mut self, instance: &Instance) -> OperatorInvocation {
        let lowered = instance.operator();
        OperatorDefinition::new(intern(&mut self.operators, lowered.program()))
            .bind(lowered.params().iter_values().collect())
            .and_then(|invocation| invocation.with_automation(lowered.automation().into()))
            .unwrap_or_else(|_| unreachable!("interning keeps the program's schema"))
    }
}
