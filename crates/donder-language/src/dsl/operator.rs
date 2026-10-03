//! Admitted operators require a signal provider. Its error type is carried into
//! the interpreter, so the playback provider can make execution infallible.
use super::bytecode::{BytecodeProgram, ColorSlot, ContextRead, ParameterKind, ProgramContext};
use super::{BindingError, BoundParams, Type, Value};
use alloc::{boxed::Box, vec::Vec};
use core::convert::Infallible;

/// A signal instruction admitted in an operator program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignalAccess(());

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BlockExecution {
    Scalar,
    Colors,
    Numeric,
    SingleQuery,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorProgram {
    bytecode: BytecodeProgram<ContextRead, SignalAccess, ColorSlot>,
    inputs: usize,
    parameters: Box<[Type]>,
    uses_spatial_context: bool,
    uses_sections: bool,
    target_entry: usize,
    uses_progress: bool,
    blocks: BlockExecution,
}

impl OperatorProgram {
    pub fn admit(
        bytecode: BytecodeProgram,
        inputs: usize,
        parameters: Box<[Type]>,
    ) -> Option<Self> {
        if !bytecode.has_valid_structure()
            || !bytecode.has_valid_context(ProgramContext::Operator { inputs })
            || !bytecode.has_valid_parameter_reads(|index| {
                parameters.get(index).map(ParameterKind::for_type)
            })
            || !bytecode.has_valid_reference_parameter_reads(|index, expected| {
                parameters
                    .get(index)
                    .is_some_and(|actual| expected.accepts(actual))
            })
        {
            return None;
        }
        Some(Self::from_trusted_bytecode(bytecode, inputs, parameters))
    }

    /// Restore a program emitted by a compatible Donder compiler. Operand
    /// addresses, parameter types, and control flow are the producer's contract.
    pub fn from_trusted_bytecode(
        bytecode: BytecodeProgram,
        inputs: usize,
        parameters: Box<[Type]>,
    ) -> Self {
        let uses_spatial_context = bytecode.uses_spatial_context();
        let target_entry = bytecode.target_entry();
        let uses_progress = bytecode.reads_progress();
        let blocks = if super::blocks::color_blocks(&bytecode) {
            BlockExecution::Colors
        } else if super::blocks::numeric_blocks(&bytecode) {
            BlockExecution::Numeric
        } else if super::blocks::single_query(&bytecode) {
            BlockExecution::SingleQuery
        } else {
            BlockExecution::Scalar
        };
        let uses_sections = bytecode.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                super::bytecode::Instruction::SectionQuery { .. }
            )
        });
        let bytecode =
            match bytecode.try_map_execution(Ok::<_, Infallible>, |()| Ok(SignalAccess(())), Ok) {
                Ok(bytecode) => bytecode,
                Err(never) => match never {},
            };
        Self {
            bytecode,
            inputs,
            parameters,
            uses_spatial_context,
            uses_sections,
            target_entry,
            uses_progress,
            blocks,
        }
    }

    pub fn bytecode(&self) -> &BytecodeProgram<ContextRead, SignalAccess, ColorSlot> {
        &self.bytecode
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }

    pub fn parameter_types(&self) -> &[Type] {
        &self.parameters
    }

    pub fn bind(&self, values: Vec<Value>) -> Result<BoundParams, BindingError> {
        BoundParams::bind_values(&self.parameters, values)
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

    /// Scalar registers/control are uniform; only color registers need lanes.
    pub fn supports_color_blocks(&self) -> bool {
        self.blocks == BlockExecution::Colors
    }

    pub fn supports_numeric_blocks(&self) -> bool {
        self.blocks == BlockExecution::Numeric
    }

    /// Bounded traversal may use color lanes or a single uniform source block
    /// followed by ordinary per-pixel scalar execution.
    pub fn supports_blocks(&self) -> bool {
        self.blocks != BlockExecution::Scalar
    }

    pub fn into_parts(self) -> (BytecodeProgram, usize, Box<[Type]>) {
        let bytecode = match self
            .bytecode
            .try_map_execution(Ok::<_, Infallible>, |_| Ok(()), Ok)
        {
            Ok(bytecode) => bytecode,
            Err(never) => match never {},
        };
        (bytecode, self.inputs, self.parameters)
    }
}
