//! Admitted operators require a signal provider. Its error type is carried into
//! the interpreter, so the playback provider can make execution infallible.
use super::bytecode::{BytecodeProgram, ColorSlot, ContextRead, ParameterKind, ProgramContext};
use super::{
    BoundParams, DslBindCache, RunContext, RuntimeError, SignalSampler, SpatialContext, Type,
    Value, VmWorkspace,
};
use crate::values::Color;
use alloc::{boxed::Box, vec::Vec};
use core::convert::Infallible;

/// A signal instruction admitted in an operator program.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SignalAccess(());

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorProgram {
    bytecode: BytecodeProgram<ContextRead, SignalAccess, ColorSlot, Infallible>,
    inputs: usize,
    parameters: Box<[Type]>,
    uses_spatial_context: bool,
}

/// One admitted operator paired with its immutable, schema-checked parameters.
pub struct BoundOperator<'a> {
    pub(super) program: &'a OperatorProgram,
    pub(super) params: BoundParams,
}

impl OperatorProgram {
    pub(crate) fn admit_bound(
        bytecode: BytecodeProgram,
        inputs: usize,
        params: &BoundParams,
    ) -> Option<Self> {
        if !params.is_frozen()
            || params
                .types()
                .iter()
                .enumerate()
                .any(|(index, ty)| !params.parameter_accepts_type(index, ty))
        {
            return None;
        }
        Self::admit(bytecode, inputs, params.types().into())
    }

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
        let uses_spatial_context = bytecode.uses_spatial_context();
        let bytecode = bytecode
            .try_map_execution(
                Ok,
                |()| Ok(SignalAccess(())),
                Ok,
                |_| Err::<Infallible, _>(()),
            )
            .ok()?;
        Some(Self {
            bytecode,
            inputs,
            parameters,
            uses_spatial_context,
        })
    }

    pub fn bytecode(&self) -> &BytecodeProgram<ContextRead, SignalAccess, ColorSlot, Infallible> {
        &self.bytecode
    }

    pub fn input_count(&self) -> usize {
        self.inputs
    }

    pub fn parameter_types(&self) -> &[Type] {
        &self.parameters
    }

    pub fn bind(
        &self,
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<BoundOperator<'_>, RuntimeError> {
        if values.len() != self.parameters.len()
            || self
                .parameters
                .iter()
                .zip(&values)
                .any(|(ty, value)| !ty.accepts_value(value))
        {
            return Err(RuntimeError {
                message: "operator inputs do not match its declaration".into(),
            });
        }
        Ok(BoundOperator {
            program: self,
            params: BoundParams::from_values(self.parameters.iter().zip(values), cache),
        })
    }

    pub fn uses_spatial_context(&self) -> bool {
        self.uses_spatial_context
    }

    pub fn into_parts(self) -> (BytecodeProgram, usize, Box<[Type]>) {
        let bytecode = match self.bytecode.try_map_execution(
            Ok::<_, Infallible>,
            |_| Ok(()),
            Ok,
            |never| match never {},
        ) {
            Ok(bytecode) => bytecode,
            Err(never) => match never {},
        };
        (bytecode, self.inputs, self.parameters)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn sample<E>(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler<E>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Result<Color, E> {
        super::vm::evaluate_operator(
            &self.bytecode,
            params,
            context,
            spatial,
            sampler,
            workspace,
            if reuse_uniform {
                self.bytecode.pixel_entry as usize
            } else {
                0
            },
        )
    }
}

impl BoundOperator<'_> {
    pub fn evaluate<E>(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler<E>,
        workspace: &mut VmWorkspace,
    ) -> Result<Color, E> {
        self.program
            .sample(&self.params, context, spatial, sampler, workspace, false)
    }
}
