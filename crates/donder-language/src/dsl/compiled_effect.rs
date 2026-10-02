//! Authored effect compilation products. Generators are host programs, never playback bytecode.
use super::{
    BoundSample, DslBindCache, GeneratorProgram, Identifier, ParamDecl, RuntimeError,
    SampleProgram, Value,
};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectKind {
    Sample,
    Generator,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    pub(super) name: Identifier,
    pub(super) params: Vec<ParamDecl>,
    pub(super) program: EffectProgram,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EffectProgram {
    Sample(Arc<SampleProgram>),
    Generator(Arc<GeneratorProgram>),
}

impl CompiledEffect {
    pub fn program(&self) -> &EffectProgram {
        &self.program
    }

    pub fn sample_program(&self) -> Option<&SampleProgram> {
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

    pub fn bind<'p, P>(
        &self,
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<BoundSample<'_>, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        match &self.program {
            EffectProgram::Sample(program) => program.bind_named(&self.params, params, cache),
            EffectProgram::Generator(_) => Err(RuntimeError {
                message: "cannot sample generator effect".into(),
            }),
        }
    }
}
