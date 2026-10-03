//! Immutable, schema-checked inputs for portable program construction.
//! Runtime register banks and prepared curve caches belong to the interpreter.
use super::{Type, Value};
use crate::execution::PreparedAutomation;
use alloc::{boxed::Box, string::String, vec::Vec};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BindingError {
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct BoundParams {
    types: Box<[Type]>,
    values: Box<[Value]>,
}

impl BoundParams {
    pub fn bind_values(types: &[Type], values: Vec<Value>) -> Result<Self, BindingError> {
        if types.len() != values.len()
            || types
                .iter()
                .zip(&values)
                .any(|(ty, value)| !ty.accepts_value(value))
        {
            return Err(BindingError {
                message: "parameter values do not match the admitted program".into(),
            });
        }
        Ok(Self::from_values(types.iter().zip(values)))
    }

    pub(super) fn from_values<'a>(values: impl IntoIterator<Item = (&'a Type, Value)>) -> Self {
        let (types, values): (Vec<_>, Vec<_>) = values
            .into_iter()
            .map(|(ty, value)| {
                let value = match (ty, value) {
                    (Type::Float, Value::Int(value)) => Value::Float(value as f32),
                    (_, value) => value,
                };
                (ty.clone(), value)
            })
            .unzip();
        Self {
            types: types.into(),
            values: values.into(),
        }
    }

    pub fn types(&self) -> &[Type] {
        &self.types
    }
    pub fn values(&self) -> &[Value] {
        &self.values
    }
    pub fn iter_values(&self) -> impl Iterator<Item = Value> + '_ {
        self.values.iter().cloned()
    }

    pub(super) fn accepts_automation(&self, bindings: &[PreparedAutomation]) -> bool {
        bindings.iter().all(|binding| {
            binding.duration.as_ticks() != 0
                && binding.curve.validate().is_ok()
                && binding.mapping.is_well_formed()
                && self
                    .types
                    .get(usize::from(binding.param_index))
                    .is_some_and(|ty| binding.mapping.accepts_type(ty))
        })
    }
}
