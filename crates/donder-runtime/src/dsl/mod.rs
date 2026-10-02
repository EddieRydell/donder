pub mod bytecode;
mod calculation;
pub mod types;
mod vm;

pub use calculation::{BoundCalculation, CalculationOutput, CalculationProgram};

use alloc::vec::Vec;

pub use bytecode::SignalPixel;

pub use types::{Identifier, TargetItemValue, TargetItemsValue, TargetValue, Type, Value};
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
    pub name: Identifier,
    pub inputs: Vec<OperatorInputDecl>,
    pub params: Vec<ParamDecl>,
    pub bytecode: bytecode::BytecodeProgram,
}

impl CompiledOperator {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.inputs
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn bind_params<'a, P>(&self, params: P) -> Result<BoundParams, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        BoundParams::bind(&self.params, params)
    }

    pub fn sample_bound(
        &self,
        params: &BoundParams,
        context: &OperatorRunContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut VmWorkspace,
    ) -> Result<crate::values::Color, RuntimeError> {
        vm::run_operator(self, params, context, sampler, workspace)
    }
}

pub use vm::SpatialContext;
