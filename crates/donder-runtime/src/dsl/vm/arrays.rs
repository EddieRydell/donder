//! Array references preserve their storage owner; element values are still tagged.
use super::{Arc, ArrayStorage, BoundParamValue, RuntimeError, RuntimeValue, Value};

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum ArrayParameter {
    #[default]
    Empty,
    Shared(Arc<[Value]>),
    Calculated(usize),
}

impl ArrayParameter {
    pub(super) fn bound(&self) -> BoundParamValue {
        match self {
            Self::Empty => BoundParamValue::Array(Arc::from([])),
            Self::Shared(values) => BoundParamValue::Array(Arc::clone(values)),
            Self::Calculated(index) => BoundParamValue::CalculatedArray(*index),
        }
    }

    pub(super) fn register(&self) -> ArrayRegister {
        match self {
            Self::Empty => ArrayRegister::Empty,
            Self::Shared(values) => ArrayRegister::Shared(Arc::clone(values)),
            Self::Calculated(index) => ArrayRegister::Parameter(*index),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum ArrayRegister {
    #[default]
    Empty,
    Shared(Arc<[Value]>),
    Local(usize),
    Parameter(usize),
}

impl ArrayRegister {
    pub(super) fn runtime(&self) -> RuntimeValue {
        match self {
            Self::Empty => RuntimeValue::Array(Arc::from([])),
            Self::Shared(values) => RuntimeValue::Array(Arc::clone(values)),
            Self::Local(index) => RuntimeValue::ArraySlot(*index),
            Self::Parameter(index) => RuntimeValue::ParameterArray(*index),
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        arrays: &'a ArrayStorage,
        parameters: &'a ArrayStorage,
    ) -> ArrayView<'a> {
        match self {
            Self::Empty => ArrayView::Shared(&[]),
            Self::Shared(values) => ArrayView::Shared(values),
            Self::Local(index) => ArrayView::Local(arrays.items(*index)),
            Self::Parameter(index) => ArrayView::Parameter(parameters.items(*index)),
        }
    }
}

pub(super) enum ArrayView<'a> {
    Shared(&'a [Value]),
    Local(&'a [RuntimeValue]),
    Parameter(&'a [RuntimeValue]),
}

impl<'a> ArrayView<'a> {
    pub(super) fn from_runtime(
        value: &'a RuntimeValue,
        arrays: &'a ArrayStorage,
        parameters: &'a ArrayStorage,
    ) -> Result<Self, RuntimeError> {
        match value {
            RuntimeValue::Array(values) => Ok(Self::Shared(values)),
            RuntimeValue::ArraySlot(index) => Ok(Self::Local(arrays.items(*index))),
            RuntimeValue::ParameterArray(index) => Ok(Self::Parameter(parameters.items(*index))),
            _ => Err(RuntimeError::new("expected array")),
        }
    }

    pub(super) fn len(&self) -> usize {
        match self {
            Self::Shared(values) => values.len(),
            Self::Local(values) | Self::Parameter(values) => values.len(),
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<RuntimeValue> {
        match self {
            Self::Shared(values) => values.get(index).map(RuntimeValue::from_value),
            Self::Local(values) => values.get(index).map(super::clone_runtime),
            Self::Parameter(values) => values.get(index).map(super::parameter_array_value),
        }
    }

    pub(super) fn iter(&self) -> impl ExactSizeIterator<Item = RuntimeValue> + '_ {
        (0..self.len()).map(|index| match self {
            Self::Shared(values) => RuntimeValue::from_value(&values[index]),
            Self::Local(values) => super::clone_runtime(&values[index]),
            Self::Parameter(values) => super::parameter_array_value(&values[index]),
        })
    }
}
