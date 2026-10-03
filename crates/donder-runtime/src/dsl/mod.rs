pub(crate) use donder_language::dsl::bytecode;
mod operator;
mod sample;
pub(crate) use donder_language::dsl::types;
mod vm;

pub(crate) use donder_language::dsl::{OperatorProgram, SampleProgram, SignalAccess};
pub(crate) use donder_language::execution::SpatialContext;
pub(crate) use operator::OperatorProgramExt;
pub(crate) use sample::SampleProgramExt;
#[cfg(test)]
pub(crate) use types::{Type, Value};
pub(crate) use vm::AutomationPlan;
#[cfg(test)]
pub(crate) use vm::RuntimeError;
pub(crate) use vm::{
    BoundParams, COLOR_BLOCK_WIDTH, DslBindCache, LaneContext, RunContext, SignalSampler,
    VmWorkspace,
};
