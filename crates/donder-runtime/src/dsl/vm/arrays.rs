//! Array references preserve their storage owner; element values are still tagged.
use super::{Arc, BoundParamValue, Value};

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

    pub(super) fn values(&self) -> &[Value] {
        match self {
            Self::Empty => &[],
            Self::Shared(values) => values,
        }
    }
}
