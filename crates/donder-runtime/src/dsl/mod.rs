pub(crate) mod bytecode;
mod operator;
mod sample;
pub(crate) mod types;
mod vm;

pub use operator::{BoundOperator, OperatorProgram, SignalAccess};
pub use sample::{BoundSample, SampleProgram};

pub use types::{Identifier, Type, Value};
pub(crate) use vm::AutomationPlan;
pub use vm::{
    BoundParams, DslBindCache, MAX_DSL_LOOP_ITERATIONS, OperatorRunContext, RunContext,
    RuntimeError, SignalSampler, VmWorkspace,
};

pub use vm::SpatialContext;
