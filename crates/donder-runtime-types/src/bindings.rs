//! Immutable, schema-checked inputs for portable program construction.
//! Runtime parameter banks and prepared curve caches belong to the interpreter.
use crate::prepared::{AutomationSource, PreparedAutomation};
use crate::{Type, Value};
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

    pub fn from_values<'a>(values: impl IntoIterator<Item = (&'a Type, Value)>) -> Self {
        let (types, values): (Vec<_>, Vec<_>) = values
            .into_iter()
            .map(|(ty, value)| (ty.clone(), floats(ty, value)))
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

    pub fn accepts_automation(&self, bindings: &[PreparedAutomation]) -> bool {
        bindings.iter().all(|binding| {
            let source = match &binding.source {
                AutomationSource::Envelope {
                    duration, curve, ..
                } => duration.as_ticks() != 0 && curve.validate().is_ok(),
                AutomationSource::Live(_) => true,
            };
            source
                && binding.mapping.is_well_formed()
                && self
                    .types
                    .get(usize::from(binding.param_index))
                    .is_some_and(|ty| binding.mapping.accepts_type(ty))
        })
    }
}

/// `value` with each int that `ty` takes as a float, an array's items
/// among them, made a float: playback reads values by their declared type.
fn floats(ty: &Type, value: Value) -> Value {
    match (ty, value) {
        (Type::Float, Value::Int(value)) => Value::Float(value as f32),
        (Type::Array(item), Value::Array(items))
            if items.iter().any(|value| takes_int(item, value)) =>
        {
            Value::Array(
                items
                    .iter()
                    .map(|value| floats(item, value.clone()))
                    .collect(),
            )
        }
        (_, value) => value,
    }
}

/// Whether `value` holds an int where `ty` takes a float.
fn takes_int(ty: &Type, value: &Value) -> bool {
    match (ty, value) {
        (Type::Float, Value::Int(_)) => true,
        (Type::Array(item), Value::Array(items)) => {
            items.iter().any(|value| takes_int(item, value))
        }
        _ => false,
    }
}
