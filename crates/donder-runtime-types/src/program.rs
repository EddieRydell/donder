//! Programs as playback admits them, and programs bound to values.
//! An effect is a program with no signal inputs; an operator samples its
//! inputs.
use crate::Shared as Arc;
use crate::bytecode::BytecodeProgram;
use crate::prepared::PreparedAutomation;
use crate::{BoundParams, Value};
use alloc::{boxed::Box, vec::Vec};

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub bytecode: BytecodeProgram,
    /// The signals it samples; an effect's program samples none.
    pub inputs: usize,
}

/// A program bound to its parameter values and automation.
#[derive(Clone, Debug)]
pub struct BoundProgram {
    program: Arc<Program>,
    params: BoundParams,
    automation: Box<[PreparedAutomation]>,
}

impl BoundProgram {
    /// `program` with the values and automation lowering produced for it
    /// from an admitted invocation, which checked them against their
    /// declarations.
    pub fn new(
        program: impl Into<Arc<Program>>,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Self {
        let program = program.into();
        debug_assert_eq!(program.bytecode.params.len(), values.len());
        let params = BoundParams::from_values(program.bytecode.params.iter().zip(values));
        Self {
            program,
            params,
            automation,
        }
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
