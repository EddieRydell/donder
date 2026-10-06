//! Authored sample-effect declaration and its executable program.
use super::loop_bounds::LoopBound;
use super::{BindingError, Identifier, ParamDecl, SampleInvocation, SampleProgram, Value};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledEffect {
    pub(super) name: Identifier,
    pub(super) params: Vec<ParamDecl>,
    pub(super) loop_bounds: Vec<LoopBound>,
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

    /// Positional `values` respect every declared range and loop bound.
    pub fn check_values(&self, values: &[Value]) -> Result<(), BindingError> {
        super::declarations::check_values(&self.params, &self.loop_bounds, values)
    }

    pub fn bind<'p, P>(&self, params: P) -> Result<SampleInvocation, BindingError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        let values = super::declarations::resolve_params(&self.params, params)?;
        self.check_values(&values)?;
        super::SampleDefinition::new(Arc::clone(&self.program)).bind(values)
    }
}
