//! Programs as playback admits them, and programs bound to values.
//! An effect is a program with no signal inputs; an operator samples its
//! inputs.
use crate::Shared as Arc;
use crate::bytecode::BytecodeProgram;
use crate::{BoundParams, Value};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub bytecode: BytecodeProgram,
    /// The signals it samples; an effect's program samples none.
    pub inputs: usize,
}

/// A program bound to its parameter values.
#[derive(Clone, Debug)]
pub struct BoundProgram {
    program: Arc<Program>,
    params: BoundParams,
}

impl BoundProgram {
    /// `program` with the values lowering produced for it from an admitted
    /// invocation, which checked them against their declarations.
    pub fn new(program: impl Into<Arc<Program>>, values: Vec<Value>) -> Self {
        let program = program.into();
        debug_assert_eq!(program.bytecode.params.len(), values.len());
        let params = BoundParams::from_values(program.bytecode.params.iter().zip(values));
        Self { program, params }
    }

    pub fn program(&self) -> &Arc<Program> {
        &self.program
    }

    pub fn params(&self) -> &BoundParams {
        &self.params
    }
}
