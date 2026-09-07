pub mod bytecode;
pub mod types;
mod vm;

use alloc::{boxed::Box, vec::Vec};

pub use bytecode::SignalPixel;

pub use types::{
    Identifier, TargetItemValue, TargetItemsValue, TargetPixelValue, TargetValue, Type, Value,
};
pub use vm::{
    BoundParams, DslBindCache, GeneratedEffect, GeneratorContext, OperatorRunContext, RunContext,
    RuntimeError, SignalSampler, VmWorkspace,
};
pub(crate) use vm::{PreparedCurveCrossings, prepared_curve_crossing};

#[derive(
    Clone, Copy, Debug, Eq, Hash, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize,
)]
pub struct GeneratedEffectSlot(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorInputDecl {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub name: Identifier,
    pub ty: Type,
    pub default: Option<Value>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectKind {
    Sample,
    Generator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    pub name: Identifier,
    pub params: Vec<ParamDecl>,
    pub kind: EffectKind,
    pub bytecode: bytecode::BytecodeProgram,
    // Generator-only authoring tables are not retained in prepared playback bytecode.
    pub emit_fields: Box<[(Identifier, bytecode::ValueSlot)]>,
    pub generated_effect_count: u32,
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

impl CompiledEffect {
    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub const fn kind(&self) -> EffectKind {
        self.kind
    }

    pub fn sample<'a, P>(
        &self,
        params: P,
        context: &RunContext,
    ) -> Result<crate::values::Color, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        let bound = self.bind_params(params)?;
        self.sample_bound(&bound, context, &mut VmWorkspace::default())
    }

    pub fn bind_params<'a, P>(&self, params: P) -> Result<BoundParams, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        BoundParams::bind(&self.params, params)
    }

    pub fn bind_params_cached<'a, P>(
        &self,
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<BoundParams, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'a Identifier, &'a Value)>,
    {
        BoundParams::bind_cached(&self.params, params, cache)
    }

    pub fn bind_params_pairs(
        &self,
        params: &[(Identifier, Value)],
    ) -> Result<BoundParams, RuntimeError> {
        BoundParams::bind_pairs(&self.params, params)
    }

    pub fn bind_params_pairs_cached(
        &self,
        params: &[(Identifier, Value)],
        cache: &mut DslBindCache,
    ) -> Result<BoundParams, RuntimeError> {
        BoundParams::bind_pairs_cached(&self.params, params, cache)
    }

    pub fn sample_bound(
        &self,
        params: &BoundParams,
        context: &RunContext,
        workspace: &mut VmWorkspace,
    ) -> Result<crate::values::Color, RuntimeError> {
        vm::run_sample_effect(self, params, context, workspace)
    }

    pub fn generate_bound(
        &self,
        params: &BoundParams,
        context: &GeneratorContext,
        workspace: &mut VmWorkspace,
    ) -> Result<Vec<GeneratedEffect>, RuntimeError> {
        vm::run_generator_effect(self, params, context, workspace)
    }
}
