//! Array references preserve their storage owner; element values are still tagged.
use super::{Arc, ArrayStorage, BoundParamValue, RuntimeValue, Value};

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum ArrayParameter {
    #[default]
    Empty,
    Shared(Arc<[Value]>),
}

impl ArrayParameter {
    pub(super) fn bound(&self) -> BoundParamValue {
        match self {
            Self::Empty => BoundParamValue::Array(Arc::from([])),
            Self::Shared(values) => BoundParamValue::Array(Arc::clone(values)),
        }
    }

    pub(super) fn register(&self) -> ArrayRegister {
        match self {
            Self::Empty => ArrayRegister::Empty,
            Self::Shared(values) => ArrayRegister::Shared(Arc::clone(values)),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) enum ArrayRegister {
    #[default]
    Empty,
    Shared(Arc<[Value]>),
    Local(usize),
}

impl ArrayRegister {
    pub(super) fn runtime(&self) -> RuntimeValue {
        match self {
            Self::Empty => RuntimeValue::Array(Arc::from([])),
            Self::Shared(values) => RuntimeValue::Array(Arc::clone(values)),
            Self::Local(index) => RuntimeValue::ArraySlot(*index),
        }
    }

    pub(super) fn view<'a>(&'a self, arrays: &'a ArrayStorage) -> ArrayView<'a> {
        match self {
            Self::Empty => ArrayView::Shared(&[]),
            Self::Shared(values) => ArrayView::Shared(values),
            Self::Local(index) => ArrayView::Local(arrays.items(*index)),
        }
    }
}

pub(super) enum ArrayView<'a> {
    Shared(&'a [Value]),
    Local(&'a [RuntimeValue]),
}

impl ArrayView<'_> {
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Shared(values) => values.len(),
            Self::Local(values) => values.len(),
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<RuntimeValue> {
        match self {
            Self::Shared(values) => values.get(index).map(RuntimeValue::from_value),
            Self::Local(values) => values.get(index).map(super::clone_runtime),
        }
    }
}
