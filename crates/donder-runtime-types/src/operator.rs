//! Admitted operator programs, which also sample their input signals.
use crate::bytecode::{BytecodeProgram, ProgramContext};
use crate::{BindingError, BoundParams, Type, Value};
use alloc::vec::Vec;

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorProgram {
    bytecode: BytecodeProgram,
    inputs: usize,
    uses_spatial_context: bool,
    uses_sections: bool,
    reads_target: bool,
    uses_progress: bool,
}

impl OperatorProgram {
    /// A well-formed operator program over `inputs` signals and its declared
    /// parameters.
    pub fn admit(bytecode: BytecodeProgram, inputs: usize) -> Option<Self> {
        if !bytecode.is_well_formed(ProgramContext::Operator { inputs }) {
            return None;
        }
        Some(Self {
            uses_spatial_context: bytecode.uses_spatial_context(),
            uses_sections: bytecode.uses_sections(),
            reads_target: bytecode.reads_target(),
            uses_progress: bytecode.uses_progress(),
            bytecode,
            inputs,
        })
    }

    pub fn bytecode(&self) -> &BytecodeProgram {
        &self.bytecode
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }

    pub fn parameter_types(&self) -> &[Type] {
        &self.bytecode.params
    }

    pub fn bind(&self, values: Vec<Value>) -> Result<BoundParams, BindingError> {
        BoundParams::bind_values(&self.bytecode.params, values)
    }

    pub fn uses_spatial_context(&self) -> bool {
        self.uses_spatial_context
    }

    pub fn uses_sections(&self) -> bool {
        self.uses_sections
    }

    /// Strips must not mix target pixel counts or bounds.
    pub fn reads_target(&self) -> bool {
        self.reads_target
    }

    pub fn uses_progress(&self) -> bool {
        self.uses_progress
    }

    pub fn into_bytecode(self) -> BytecodeProgram {
        self.bytecode
    }
}
