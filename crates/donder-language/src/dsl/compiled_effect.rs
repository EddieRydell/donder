//! Authored sample-effect declaration and its executable program.
use super::{BindingError, Identifier, ParamDecl, SampleInvocation, SampleProgram, Value};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    pub(super) name: Identifier,
    pub(super) params: Vec<ParamDecl>,
    pub(super) program: Arc<SampleProgram>,
}

impl CompiledEffect {
    pub fn sample_program(&self) -> &SampleProgram {
        &self.program
    }

    pub(crate) fn shared_sample_program(&self) -> &Arc<SampleProgram> {
        &self.program
    }

    pub fn name(&self) -> &Identifier {
        &self.name
    }

    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn bind<'p, P>(&self, params: P) -> Result<SampleInvocation, BindingError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        super::SampleDefinition::new(Arc::clone(&self.program))
            .bind(super::declarations::resolve_params(&self.params, params)?)
    }
}
