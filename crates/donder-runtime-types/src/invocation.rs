//! Validated portable construction inputs, independent of VM storage.
use crate::Shared as Arc;
use crate::prepared::PreparedAutomation;
use crate::{BindingError, BoundParams, OperatorProgram, SampleProgram, Value};
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Debug)]
pub struct SampleInvocation {
    program: Arc<SampleProgram>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl SampleInvocation {
    /// Binds parameter values to a program, with no automation.
    pub fn bind(
        program: impl Into<Arc<SampleProgram>>,
        values: Vec<Value>,
    ) -> Result<Self, BindingError> {
        let program = program.into();
        let params = program.bind(values)?;
        Ok(Self {
            program,
            params,
            automation: Box::new([]),
        })
    }

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
pub struct OperatorInvocation {
    program: Arc<OperatorProgram>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl OperatorInvocation {
    /// Binds parameter values to a program, with no automation.
    pub fn bind(
        program: impl Into<Arc<OperatorProgram>>,
        values: Vec<Value>,
    ) -> Result<Self, BindingError> {
        let program = program.into();
        let params = program.bind(values)?;
        Ok(Self {
            program,
            params,
            automation: Box::new([]),
        })
    }

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
