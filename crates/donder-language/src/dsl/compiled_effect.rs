//! Authored effect compilation products. Generators are host programs, never playback bytecode.
use super::{
    BoundParams, BytecodeProgram, DslBindCache, GeneratorProgram, Identifier, ParamDecl,
    RunContext, RuntimeError, Value, VmWorkspace,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectKind {
    Sample,
    Generator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    pub name: Identifier,
    pub params: Vec<ParamDecl>,
    pub program: EffectProgram,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectProgram {
    Sample(BytecodeProgram),
    Generator(GeneratorProgram),
}

impl CompiledEffect {
    pub fn sample_program(&self) -> Option<&BytecodeProgram> {
        match &self.program {
            EffectProgram::Sample(program) => Some(program),
            EffectProgram::Generator(_) => None,
        }
    }

    pub fn generator(&self) -> Option<&GeneratorProgram> {
        match &self.program {
            EffectProgram::Sample(_) => None,
            EffectProgram::Generator(program) => Some(program),
        }
    }

    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub const fn kind(&self) -> EffectKind {
        match self.program {
            EffectProgram::Sample(_) => EffectKind::Sample,
            EffectProgram::Generator(_) => EffectKind::Generator,
        }
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
        match &self.program {
            EffectProgram::Sample(program) => program.sample_effect(params, context, workspace),
            EffectProgram::Generator(_) => Err(RuntimeError {
                message: "cannot sample generator effect".into(),
            }),
        }
    }
}
