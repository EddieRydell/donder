//! Authoring declarations and named parameter resolution.
//! Playback programs receive positional, typed values rather than source names.
use super::{
    BoundOperator, BoundParams, BytecodeProgram, DslBindCache, Identifier, RuntimeError, Type,
    Value,
};
use donder_runtime::OperatorProgram;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct OperatorInputDecl {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParamDecl {
    pub fixed: bool,
    pub name: Identifier,
    pub ty: Type,
    pub default: Option<Value>,
}

impl ParamDecl {
    pub fn supports_automation(&self) -> bool {
        !self.fixed
            && matches!(
                self.ty,
                Type::Float | Type::Int | Type::Bool | Type::Enum(_) | Type::Curve
            )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledOperator {
    name: Identifier,
    inputs: Vec<OperatorInputDecl>,
    params: Vec<ParamDecl>,
    program: Arc<OperatorProgram>,
}

impl CompiledOperator {
    pub(super) fn admit(
        name: Identifier,
        inputs: Vec<OperatorInputDecl>,
        params: Vec<ParamDecl>,
        bytecode: BytecodeProgram,
    ) -> Option<Self> {
        let program = OperatorProgram::admit(
            bytecode,
            inputs.len(),
            params.iter().map(|param| param.ty.clone()).collect(),
        )?;
        Some(Self {
            name,
            inputs,
            params,
            program: Arc::new(program),
        })
    }

    pub fn bytecode(
        &self,
    ) -> &BytecodeProgram<
        donder_runtime::ContextRead,
        donder_runtime::SignalAccess,
        donder_runtime::ColorSlot,
    > {
        self.program.bytecode()
    }

    pub fn program(&self) -> &OperatorProgram {
        &self.program
    }

    pub(crate) fn shared_program(&self) -> &Arc<OperatorProgram> {
        &self.program
    }
    pub fn name(&self) -> &Identifier {
        &self.name
    }
    pub fn inputs(&self) -> &[OperatorInputDecl] {
        &self.inputs
    }
    pub fn params(&self) -> &[ParamDecl] {
        &self.params
    }

    pub fn bind<'p, P>(
        &self,
        params: P,
        cache: &mut DslBindCache,
    ) -> Result<BoundOperator<'_>, RuntimeError>
    where
        P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
    {
        self.program
            .bind(resolve_params(&self.params, params)?, cache)
    }
}

/// Resolve authoring names/defaults and bind their validated positional values.
pub fn bind_params<'p, P>(
    declarations: &[ParamDecl],
    params: P,
    cache: &mut DslBindCache,
) -> Result<BoundParams, RuntimeError>
where
    P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
{
    let values = resolve_params(declarations, params)?;
    let types: Vec<_> = declarations.iter().map(|param| param.ty.clone()).collect();
    BoundParams::bind_values(&types, values, cache)
}

pub(super) fn resolve_params<'p, P>(
    declarations: &[ParamDecl],
    params: P,
) -> Result<Vec<Value>, RuntimeError>
where
    P: Clone + IntoIterator<Item = (&'p Identifier, &'p Value)>,
{
    if let Some(name) = params
        .clone()
        .into_iter()
        .map(|(name, _)| name)
        .find(|name| !declarations.iter().any(|param| param.name == **name))
    {
        return Err(RuntimeError {
            message: format!("unknown parameter `{}`", name.as_str()),
        });
    }
    declarations
        .iter()
        .map(|param| {
            let value = params
                .clone()
                .into_iter()
                .find(|(name, _)| **name == param.name)
                .map(|(_, value)| value.clone())
                .or_else(|| param.default.clone())
                .ok_or_else(|| RuntimeError {
                    message: format!("missing required parameter `{}`", param.name.as_str()),
                })?;
            if !param.ty.accepts_value(&value) {
                return Err(RuntimeError {
                    message: "parameter value does not match its declared type".into(),
                });
            }
            Ok(value)
        })
        .collect()
}
