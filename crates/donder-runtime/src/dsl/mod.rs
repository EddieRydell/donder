pub(crate) mod bytecode;
mod calculation;
pub(crate) mod generator;
mod operator;
mod sample;
pub(crate) mod types;
mod vm;

pub use calculation::{BoundCalculation, CalculationOutput, CalculationProgram};
pub use operator::{BoundOperator, OperatorProgram, SignalAccess};
pub use sample::{BoundSample, SampleProgram};

use alloc::vec::Vec;

pub use types::{Identifier, TargetItemValue, TargetItemsValue, TargetValue, Type, Value};
pub(crate) use vm::{AutomationPlan, ParameterLink, ParameterTransfer};
pub use vm::{
    BoundParams, DslBindCache, MAX_DSL_LOOP_ITERATIONS, OperatorRunContext, RunContext,
    RuntimeError, SignalSampler, VmWorkspace,
};

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorInputDecl {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub fixed: bool,
    pub name: Identifier,
    pub ty: Type,
    pub default: Option<Value>,
}

impl ParamDecl {
    pub fn supports_automation(&self) -> bool {
        !self.fixed
            && matches!(
                self.ty,
                Type::Float | Type::Int | Type::Bool | Type::Enum(_) | Type::Curve
            )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledOperator {
    name: Identifier,
    inputs: Vec<OperatorInputDecl>,
    params: Vec<ParamDecl>,
    program: OperatorProgram,
}

impl CompiledOperator {
    /// Admit the complete compiled declaration, so callers cannot later replace
    /// its bytecode independently of its parameter and signal schemas.
    pub fn admit(
        name: Identifier,
        inputs: Vec<OperatorInputDecl>,
        params: Vec<ParamDecl>,
        bytecode: bytecode::BytecodeProgram,
    ) -> Option<Self> {
        if params.len() > u16::MAX as usize
            || params.iter().any(|param| {
                param
                    .default
                    .as_ref()
                    .is_some_and(|value| !param.ty.accepts_value(value))
            })
        {
            return None;
        }
        let program = OperatorProgram::admit(
            bytecode,
            inputs.len(),
            params.iter().map(|param| param.ty.clone()).collect(),
        )?;
        Some(Self {
            name,
            inputs,
            params,
            program,
        })
    }

    pub fn bytecode(
        &self,
    ) -> &bytecode::BytecodeProgram<
        bytecode::ContextRead,
        SignalAccess,
        bytecode::ColorSlot,
        core::convert::Infallible,
    > {
        self.program.bytecode()
    }

    pub fn program(&self) -> &OperatorProgram {
        &self.program
    }

    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.inputs
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn bind<'p, P>(
        &self,
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<BoundOperator<'_>, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        Ok(BoundOperator {
            program: &self.program,
            params: sample::bind_named(
                self.program.parameter_types(),
                &self.params,
                params,
                cache,
            )?,
        })
    }
}

pub use vm::SpatialContext;
