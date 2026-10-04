//! Array references preserve their storage owner; element values are still tagged.
use super::{Arc, BoundParamValue, RuntimeValue, Value};

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

    /// A loaded register value; an empty array is an empty register.
    pub(super) fn runtime(&self) -> RuntimeValue {
        match self {
            Self::Empty => RuntimeValue::Void,
            Self::Shared(values) => RuntimeValue::Array(Arc::clone(values)),
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
}
