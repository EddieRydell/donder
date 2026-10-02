//! Calculation admission and its immutable execution contract. Raw bytecode is
//! useful for compilation and serialization; it is not itself this contract.
use super::bytecode::{
    BytecodeProgram, CalculationRead, ContextRead, Instruction, ParameterKind, ProgramContext,
    ValueSlot,
};
use super::{BoundParams, DslBindCache, RunContext, RuntimeError, Type, Value, VmWorkspace};
use alloc::{boxed::Box, format, vec::Vec};
use core::convert::Infallible;
mod output;

/// Supported host result types. Projection is sealed and checked when the
/// compiler selects it; evaluation does not inspect dynamic result tags.
pub trait CalculationOutput: output::Projection {}
impl<T: output::Projection> CalculationOutput for T {}

/// A calculation whose instruction references, parameter reads, and return
/// output registers have been checked together. There is no mutable bytecode accessor.
///
/// ```compile_fail
/// fn mutate(program: &mut donder_runtime::dsl::CalculationProgram) {
///     program.bytecode.instructions = Box::new([]);
/// }
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct CalculationProgram<O: CalculationOutput = Vec<Value>> {
    bytecode: BytecodeProgram<CalculationRead, Infallible>,
    inputs: Box<[Type]>,
    outputs: Box<[Type]>,
    results: O::Slots,
    uses_time: bool,
}

/// One program paired with inputs admitted against its own declaration. Neither
/// the program nor its parameters can be replaced after binding.
///
/// ```compile_fail
/// fn replace_parameters(bound: &mut donder_runtime::dsl::BoundCalculation<'_>) {
///     bound.params = donder_runtime::dsl::BoundParams::default();
/// }
/// ```
#[derive(Debug)]
pub struct BoundCalculation<'a, O: CalculationOutput = Vec<Value>> {
    program: &'a CalculationProgram<O>,
    params: BoundParams,
}

impl CalculationProgram {
    /// Admit raw compiler output. Malformed bytecode is rejected here, before
    /// an instance's GUI values are supplied during preparation.
    pub fn new(
        bytecode: BytecodeProgram,
        inputs: Box<[Type]>,
        outputs: Box<[Type]>,
    ) -> Option<Self> {
        if !bytecode.has_valid_structure()
            || !bytecode.has_valid_context(ProgramContext::Calculation)
            || !bytecode
                .has_valid_parameter_reads(|index| inputs.get(index).map(ParameterKind::for_type))
            || !bytecode.has_valid_reference_parameter_reads(|index, expected| {
                inputs
                    .get(index)
                    .is_some_and(|actual| expected.accepts(actual))
            })
            || !bytecode.has_valid_calculation_outputs(&outputs)
        {
            return None;
        }
        let results = bytecode.calculation_outputs()?.into();
        let uses_time = bytecode
            .instructions
            .iter()
            .any(|instruction| match instruction {
                Instruction::ContextRead {
                    read: ContextRead::Seconds | ContextRead::Progress,
                    ..
                } => true,
                Instruction::Mark { op, .. } => op.reads_current_time(),
                _ => false,
            });
        let bytecode = bytecode
            .try_map_context(
                |read| CalculationRead::admit(read).ok_or(()),
                |()| Err::<Infallible, _>(()),
            )
            .ok()?;
        Some(Self {
            bytecode,
            inputs,
            outputs,
            results,
            uses_time,
        })
    }

    /// Select a typed projection after the complete bytecode has been admitted.
    pub fn into_output<O: CalculationOutput>(self) -> Option<CalculationProgram<O>> {
        let results = O::slots(&self.results)?;
        Some(CalculationProgram {
            bytecode: self.bytecode,
            inputs: self.inputs,
            outputs: self.outputs,
            results,
            uses_time: self.uses_time,
        })
    }
}

impl<O: CalculationOutput> CalculationProgram<O> {
    /// Retained calculations expose their original schema-ordered values.
    /// The projection carries their exact slots, so this does not revalidate
    /// bytecode or reconstruct result addresses from an execution result.
    pub fn into_values(self) -> CalculationProgram {
        CalculationProgram {
            bytecode: self.bytecode,
            inputs: self.inputs,
            outputs: self.outputs,
            results: O::into_slots(self.results),
            uses_time: self.uses_time,
        }
    }

    pub fn input_types(&self) -> &[Type] {
        &self.inputs
    }

    /// Inspect the admitted program without permitting mutation of its contract.
    pub fn bytecode(&self) -> &BytecodeProgram<CalculationRead, Infallible> {
        &self.bytecode
    }

    pub fn output_types(&self) -> &[Type] {
        &self.outputs
    }

    pub fn uses_time(&self) -> bool {
        self.uses_time
    }

    /// Admit an invocation before execution. Check the complete declaration,
    /// including unused inputs, enum membership, and nested array element types.
    /// Accepted integers are materialized as floats where the declaration allows it.
    pub fn bind(
        &self,
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<BoundCalculation<'_, O>, RuntimeError> {
        if values.len() != self.inputs.len() {
            return Err(RuntimeError {
                message: format!(
                    "calculation requires {} inputs, received {}",
                    self.inputs.len(),
                    values.len(),
                ),
            });
        }
        for (index, (ty, value)) in self.inputs.iter().zip(&values).enumerate() {
            if !ty.accepts_value(value) {
                return Err(RuntimeError {
                    message: format!("calculation input {index} does not match {ty:?}"),
                });
            }
        }
        Ok(BoundCalculation {
            program: self,
            params: BoundParams::from_values(self.inputs.iter().zip(values), cache),
        })
    }

    /// Consume the admitted program when placing it in the portable graph.
    /// The returned data is raw again; wire loading has its own admission step.
    pub fn into_parts(self) -> (BytecodeProgram, Box<[Type]>, Box<[Type]>) {
        let bytecode = match self.bytecode.try_map_context(
            |read| Ok::<_, Infallible>(ContextRead::from(read)),
            |never| match never {},
        ) {
            Ok(bytecode) => bytecode,
            Err(never) => match never {},
        };
        (bytecode, self.inputs, self.outputs)
    }
}

impl<O: CalculationOutput> BoundCalculation<'_, O> {
    /// Host calculation results are owned values. Playback retains the bytecode
    /// and writes its results into preallocated parameter environments instead.
    /// Admission excludes spatial/signal instructions, and binding fixes the
    /// input types; execution has no runtime-error alternative.
    pub fn evaluate(&self, context: &RunContext, workspace: &mut VmWorkspace) -> O {
        super::vm::evaluate_calculation::<O>(
            &self.program.bytecode,
            &self.program.results,
            &self.params,
            context,
            workspace,
        )
    }
}
