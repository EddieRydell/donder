//! Sample-effect admission excludes signal queries. Samples and operators share
//! an instruction representation so playback can share its native interpreter.
use super::bytecode::{BytecodeProgram, ColorSlot, ContextRead, ParameterKind, ProgramContext};
use super::{BindingError, BoundParams, SignalAccess, Type, Value};
use alloc::{boxed::Box, vec::Vec};
use core::convert::Infallible;

#[derive(Clone, Debug, PartialEq)]
pub struct SampleProgram {
    bytecode: BytecodeProgram<ContextRead, SignalAccess, ColorSlot>,
    inputs: Box<[Type]>,
    uses_spatial_context: bool,
    uses_sections: bool,
    target_entry: usize,
    uses_progress: bool,
    batch: super::BatchPlan,
}

impl SampleProgram {
    pub fn admit(bytecode: BytecodeProgram, inputs: Box<[Type]>) -> Option<Self> {
        if !bytecode.has_valid_structure()
            || !bytecode.has_valid_context(ProgramContext::Effect)
            || !bytecode
                .has_valid_parameter_reads(|index| inputs.get(index).map(ParameterKind::for_type))
            || !bytecode.has_valid_reference_parameter_reads(|index, expected| {
                inputs
                    .get(index)
                    .is_some_and(|actual| expected.accepts(actual))
            })
        {
            return None;
        }
        Self::from_trusted_bytecode(bytecode, inputs)
    }

    /// Restore a program emitted by a compatible Donder compiler. This trusts
    /// operand addresses, parameter types, and control flow without rechecking
    /// them. Signal instructions are still rejected at this boundary.
    pub fn from_trusted_bytecode(bytecode: BytecodeProgram, inputs: Box<[Type]>) -> Option<Self> {
        let uses_spatial_context = bytecode.uses_spatial_context();
        let target_entry = bytecode.target_entry();
        let uses_progress = bytecode.reads_progress();
        let uses_sections = bytecode.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                super::bytecode::Instruction::SectionQuery { .. }
            )
        });
        let batch = super::blocks::batch_plan(&bytecode);
        let bytecode = bytecode
            .try_map_execution(Ok, |()| Err::<SignalAccess, _>(()), Ok)
            .ok()?;
        Some(Self {
            bytecode,
            inputs,
            uses_spatial_context,
            uses_sections,
            target_entry,
            uses_progress,
            batch,
        })
    }

    pub fn bytecode(&self) -> &BytecodeProgram<ContextRead, SignalAccess, ColorSlot> {
        &self.bytecode
    }

    pub fn input_types(&self) -> &[Type] {
        &self.inputs
    }

    pub fn uses_spatial_context(&self) -> bool {
        self.uses_spatial_context
    }

    pub fn uses_sections(&self) -> bool {
        self.uses_sections
    }

    pub fn target_entry(&self) -> usize {
        self.target_entry
    }

    pub fn uses_progress(&self) -> bool {
        self.uses_progress
    }

    pub fn batch(&self) -> &super::BatchPlan {
        &self.batch
    }

    pub fn bind(&self, values: Vec<Value>) -> Result<BoundParams, BindingError> {
        BoundParams::bind_values(&self.inputs, values)
    }

    pub fn into_parts(self) -> (BytecodeProgram, Box<[Type]>) {
        let bytecode = match self
            .bytecode
            .try_map_execution(Ok::<_, Infallible>, |_| Ok(()), Ok)
        {
            Ok(bytecode) => bytecode,
            Err(never) => match never {},
        };
        (bytecode, self.inputs)
    }
}
