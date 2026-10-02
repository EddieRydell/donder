//! Sample-effect admission. Rendering receives the exact context and return
//! capabilities admitted here, rather than discovering missing ones per pixel.
use super::bytecode::{BytecodeProgram, ColorSlot, ContextRead, ParameterKind, ProgramContext};
use super::{
    BoundParams, DslBindCache, Identifier, ParamDecl, RunContext, RuntimeError, SpatialContext,
    Type, Value, VmWorkspace,
};
use crate::values::Color;
use alloc::{boxed::Box, vec::Vec};
use core::convert::Infallible;

#[derive(Clone, Debug, PartialEq)]
pub struct SampleProgram {
    bytecode: BytecodeProgram<ContextRead, Infallible, ColorSlot, Infallible>,
    inputs: Box<[Type]>,
    uses_spatial_context: bool,
    uses_sections: bool,
}

pub struct BoundSample<'a> {
    program: &'a SampleProgram,
    params: BoundParams,
}

impl SampleProgram {
    pub(crate) fn admit_bound(bytecode: BytecodeProgram, params: &BoundParams) -> Option<Self> {
        if !params.is_frozen()
            || params
                .types()
                .iter()
                .enumerate()
                .any(|(index, ty)| !params.parameter_accepts_type(index, ty))
        {
            return None;
        }
        Self::admit(bytecode, params.types().into())
    }

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
        let uses_spatial_context = bytecode.uses_spatial_context();
        let uses_sections = bytecode.instructions.iter().any(|instruction| {
            matches!(
                instruction,
                super::bytecode::Instruction::SectionQuery { .. }
            )
        });
        let bytecode = bytecode
            .try_map_execution(
                Ok,
                |()| Err::<Infallible, _>(()),
                Ok,
                |_| Err::<Infallible, _>(()),
            )
            .ok()?;
        Some(Self {
            bytecode,
            inputs,
            uses_spatial_context,
            uses_sections,
        })
    }

    pub fn bytecode(&self) -> &BytecodeProgram<ContextRead, Infallible, ColorSlot, Infallible> {
        &self.bytecode
    }

    pub fn input_types(&self) -> &[Type] {
        &self.inputs
    }

    pub fn uses_spatial_context(&self) -> bool {
        self.uses_spatial_context
    }

    pub(crate) fn uses_sections(&self) -> bool {
        self.uses_sections
    }

    pub fn bind(
        &self,
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<BoundSample<'_>, RuntimeError> {
        if values.len() != self.inputs.len()
            || self
                .inputs
                .iter()
                .zip(&values)
                .any(|(ty, value)| !ty.accepts_value(value))
        {
            return Err(RuntimeError {
                message: "sample inputs do not match its declaration".into(),
            });
        }
        Ok(BoundSample {
            program: self,
            params: BoundParams::from_values(self.inputs.iter().zip(values), cache),
        })
    }

    /// Resolve authored names and defaults against this program's exact input
    /// schema. The returned invocation owns its parameters; execution cannot
    /// substitute a separately constructed parameter bank.
    pub fn bind_named<'p, P>(
        &self,
        declarations: &[ParamDecl],
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<BoundSample<'_>, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        Ok(BoundSample {
            program: self,
            params: bind_named(&self.inputs, declarations, params, cache)?,
        })
    }

    pub fn into_parts(self) -> (BytecodeProgram, Box<[Type]>) {
        let bytecode = match self.bytecode.try_map_execution(
            Ok::<_, Infallible>,
            |never| match never {},
            Ok,
            |never| match never {},
        ) {
            Ok(bytecode) => bytecode,
            Err(never) => match never {},
        };
        (bytecode, self.inputs)
    }

    pub(crate) fn sample(
        &self,
        params: &BoundParams,
        context: &RunContext,
        spatial: &SpatialContext,
        sections: crate::sections::SectionContext<'_>,
        workspace: &mut VmWorkspace,
        reuse_uniform: bool,
    ) -> Color {
        super::vm::evaluate_sample(
            &self.bytecode,
            params,
            context,
            spatial,
            sections,
            workspace,
            if reuse_uniform {
                self.bytecode.pixel_entry as usize
            } else {
                0
            },
        )
    }
}

pub(super) fn bind_named<'p, P>(
    inputs: &[Type],
    declarations: &[ParamDecl],
    params: P,
    cache: &mut DslBindCache,
) -> Result<BoundParams, RuntimeError>
where
    P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
{
    if inputs.len() != declarations.len()
        || inputs
            .iter()
            .zip(declarations)
            .any(|(ty, declaration)| ty != &declaration.ty)
    {
        return Err(RuntimeError {
            message: "parameter declaration does not match the admitted program".into(),
        });
    }
    BoundParams::bind_cached(declarations, params, cache)
}

impl BoundSample<'_> {
    /// Evaluate one standalone virtual fixture described by the context's pixel
    /// index/count. Prepared sequence playback supplies its full fixture topology.
    pub fn evaluate(
        &self,
        context: &RunContext,
        spatial: &SpatialContext,
        workspace: &mut VmWorkspace,
    ) -> Color {
        self.program.sample(
            &self.params,
            context,
            spatial,
            crate::sections::SectionContext::Single {
                index: context.pixel_index,
                count: context.pixel_count,
            },
            workspace,
            false,
        )
    }
}
