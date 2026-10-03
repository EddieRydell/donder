//! Validated portable construction inputs, independent of VM storage.
use super::{BindingError, BoundParams, OperatorProgram, SampleProgram, Value};
use crate::Shared as Arc;
use crate::execution::PreparedAutomation;
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Debug)]
pub struct SampleDefinition(Arc<SampleProgram>);

impl SampleDefinition {
    pub fn new(program: impl Into<Arc<SampleProgram>>) -> Self {
        Self(program.into())
    }

    pub fn bind(&self, values: Vec<Value>) -> Result<SampleInvocation, BindingError> {
        Ok(SampleInvocation {
            program: Arc::clone(&self.0),
            params: self.0.bind(values)?,
            automation: Box::new([]),
        })
    }
}

#[derive(Clone, Debug)]
pub struct SampleInvocation {
    program: Arc<SampleProgram>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl SampleInvocation {
    pub fn program(&self) -> &Arc<SampleProgram> {
        &self.program
    }
    pub fn params(&self) -> &BoundParams {
        &self.params
    }
    pub fn automation(&self) -> &[PreparedAutomation] {
        &self.automation
    }

    pub fn with_automation(
        mut self,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Self, BindingError> {
        if !self.params.accepts_automation(&automation) {
            return Err(BindingError {
                message: "sample automation does not match its declaration".into(),
            });
        }
        self.automation = automation;
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct OperatorDefinition(Arc<OperatorProgram>);

impl OperatorDefinition {
    pub fn new(program: impl Into<Arc<OperatorProgram>>) -> Self {
        Self(program.into())
    }

    pub fn bind(&self, values: Vec<Value>) -> Result<OperatorInvocation, BindingError> {
        Ok(OperatorInvocation {
            program: Arc::clone(&self.0),
            params: self.0.bind(values)?,
            automation: Box::new([]),
        })
    }
}

#[derive(Clone, Debug)]
pub struct OperatorInvocation {
    program: Arc<OperatorProgram>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl OperatorInvocation {
    pub fn program(&self) -> &Arc<OperatorProgram> {
        &self.program
    }
    pub fn params(&self) -> &BoundParams {
        &self.params
    }
    pub fn automation(&self) -> &[PreparedAutomation] {
        &self.automation
    }

    pub fn with_automation(
        mut self,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Self, BindingError> {
        if !self.params.accepts_automation(&automation) {
            return Err(BindingError {
                message: "operator automation does not match its declaration".into(),
            });
        }
        self.automation = automation;
        Ok(self)
    }
}
