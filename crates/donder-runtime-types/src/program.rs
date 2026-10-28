//! Programs as playback admits them, and programs bound to values.
//! An effect is a program with no signal inputs; an operator samples its
//! inputs.
use crate::Shared as Arc;
use crate::bytecode::BytecodeProgram;
use crate::prepared::PreparedAutomation;
use crate::{BindingError, BoundParams, Value};
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub bytecode: BytecodeProgram,
    /// The signals it samples; an effect's program samples none.
    pub inputs: usize,
}

/// A program bound to checked parameter values and automation.
#[derive(Clone, Debug)]
pub struct BoundProgram {
    program: Arc<Program>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl BoundProgram {
    pub fn bind(
        program: impl Into<Arc<Program>>,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Self, BindingError> {
        let program = program.into();
        let params = BoundParams::bind_values(&program.bytecode.params, values)?;
        if !params.accepts_automation(&automation) {
            return Err(BindingError {
                message: "automation does not match its declaration".into(),
            });
        }
        Ok(Self {
            program,
            params,
            automation,
        })
    }

    pub fn program(&self) -> &Arc<Program> {
        &self.program
    }

    pub fn params(&self) -> &BoundParams {
        &self.params
    }

    pub fn automation(&self) -> &[PreparedAutomation] {
        &self.automation
    }
}
