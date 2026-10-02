use super::arrays::ArrayParameter;
use super::{
    Arc, BoundParamValue, Color, Curve, Gradient, Identifier, Marks, PreparedCurve, RuntimeValue,
    Type,
};
use alloc::vec::Vec;

/// Declaration order is metadata; dedicated value kinds live only in their typed bank.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) struct ParameterValues {
    pub(super) slots: Vec<ParameterAddress>,
    pub(super) types: Vec<Type>,
    pub(super) ints: Vec<i32>,
    pub(super) floats: Vec<f32>,
    pub(super) bools: Vec<bool>,
    pub(super) colors: Vec<Color>,
    pub(super) array_values: Vec<ArrayParameter>,
    pub(super) enums: Vec<Identifier>,
    pub(super) marks: Vec<MarksRegister>,
    pub(super) curves: Vec<CurveRegister>,
    pub(super) gradients: Vec<GradientRegister>,
}

#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum ParameterAddress {
    Void,
    Int(usize),
    Float(usize),
    Bool(usize),
    Color(usize),
    Array(usize),
    Enum(usize),
    Marks(usize),
    Curve(usize),
    Gradient(usize),
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum CurveRegister {
    #[default]
    Empty,
    Raw(Arc<Curve>),
    Prepared(Arc<PreparedCurve>),
}

impl CurveRegister {
    pub(super) fn raw(&self) -> &Curve {
        static EMPTY: Curve = Curve { points: Vec::new() };
        match self {
            Self::Empty => &EMPTY,
            Self::Raw(value) => value,
            Self::Prepared(value) => &value.raw,
        }
    }

    pub(super) fn owned(&self) -> Arc<Curve> {
        match self {
            Self::Empty => Arc::new(self.raw().clone()),
            Self::Raw(value) => Arc::clone(value),
            Self::Prepared(value) => value.raw(),
        }
    }

    pub(super) fn bound(&self) -> BoundParamValue {
        match self {
            Self::Prepared(value) => BoundParamValue::Curve(Arc::clone(value)),
            _ => BoundParamValue::RawCurve(self.owned()),
        }
    }

    pub(super) fn runtime(&self) -> RuntimeValue {
        match self {
            Self::Prepared(value) => RuntimeValue::PreparedCurve(Arc::clone(value)),
            _ => RuntimeValue::Curve(self.owned()),
        }
    }

    pub(super) fn sample(&self, position: f32) -> f32 {
        super::sample_curve(self.raw(), position)
    }

    pub(super) fn crossing(&self, value: f32, fallback: f32) -> f32 {
        match self {
            Self::Prepared(curve) => {
                super::prepared_curve_crossing(&curve.crossings, &curve.raw, value, fallback)
            }
            _ => super::curve_crossing_raw(self.raw(), value, fallback),
        }
    }
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum GradientRegister {
    #[default]
    Empty,
    Shared(Arc<Gradient>),
}

impl GradientRegister {
    pub(super) fn get(&self) -> &Gradient {
        static EMPTY: Gradient = Gradient { stops: Vec::new() };
        match self {
            Self::Empty => &EMPTY,
            Self::Shared(value) => value,
        }
    }

    pub(super) fn owned(&self) -> Arc<Gradient> {
        match self {
            Self::Empty => Arc::new(self.get().clone()),
            Self::Shared(value) => Arc::clone(value),
        }
    }
}

/// An empty collection needs no shared allocation. A loaded collection retains
/// its identity when copied between parameters, registers, and array elements.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) enum MarksRegister {
    #[default]
    Empty,
    Shared(Arc<Marks>),
}

impl MarksRegister {
    pub(super) fn get(&self) -> &Marks {
        static EMPTY: Marks = Marks { marks: Vec::new() };
        match self {
            Self::Empty => &EMPTY,
            Self::Shared(value) => value,
        }
    }

    pub(super) fn owned(&self) -> Arc<Marks> {
        match self {
            Self::Empty => Arc::new(self.get().clone()),
            Self::Shared(value) => Arc::clone(value),
        }
    }
}

impl<'a> FromIterator<(&'a Type, BoundParamValue)> for ParameterValues {
    fn from_iter<T: IntoIterator<Item = (&'a Type, BoundParamValue)>>(iter: T) -> Self {
        let mut values = Self::default();
        for (ty, value) in iter {
            values.push(ty, value);
        }
        values
    }
}

impl ParameterValues {
    pub(super) fn has_valid_layout(&self) -> bool {
        if self.slots.len() != self.types.len() {
            return false;
        }
        let mut lengths = [0; 9];
        for address in &self.slots {
            let (bank, index) = match *address {
                ParameterAddress::Void => continue,
                ParameterAddress::Int(index) => (0, index),
                ParameterAddress::Float(index) => (1, index),
                ParameterAddress::Bool(index) => (2, index),
                ParameterAddress::Color(index) => (3, index),
                ParameterAddress::Array(index) => (4, index),
                ParameterAddress::Enum(index) => (8, index),
                ParameterAddress::Marks(index) => (5, index),
                ParameterAddress::Curve(index) => (6, index),
                ParameterAddress::Gradient(index) => (7, index),
            };
            if index != lengths[bank] {
                return false;
            }
            lengths[bank] += 1;
        }
        lengths
            == [
                self.ints.len(),
                self.floats.len(),
                self.bools.len(),
                self.colors.len(),
                self.array_values.len(),
                self.marks.len(),
                self.curves.len(),
                self.gradients.len(),
                self.enums.len(),
            ]
    }

    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn push(&mut self, ty: &Type, value: BoundParamValue) {
        self.types.push(ty.clone());
        let address = match value {
            BoundParamValue::Array(values) => {
                self.array_values.push(ArrayParameter::Shared(values));
                ParameterAddress::Array(self.array_values.len() - 1)
            }
            BoundParamValue::Enum(value) => {
                self.enums.push(value);
                ParameterAddress::Enum(self.enums.len() - 1)
            }
            BoundParamValue::Curve(value) => {
                self.curves.push(CurveRegister::Prepared(value));
                ParameterAddress::Curve(self.curves.len() - 1)
            }
            BoundParamValue::RawCurve(value) => {
                self.curves.push(CurveRegister::Raw(value));
                ParameterAddress::Curve(self.curves.len() - 1)
            }
            BoundParamValue::Gradient(value) => {
                self.gradients.push(GradientRegister::Shared(value));
                ParameterAddress::Gradient(self.gradients.len() - 1)
            }
            BoundParamValue::Marks(value) => {
                self.marks.push(MarksRegister::Shared(value));
                ParameterAddress::Marks(self.marks.len() - 1)
            }
            BoundParamValue::Int(value) => {
                self.ints.push(value);
                ParameterAddress::Int(self.ints.len() - 1)
            }
            BoundParamValue::Float(value) => {
                self.floats.push(value);
                ParameterAddress::Float(self.floats.len() - 1)
            }
            BoundParamValue::Bool(value) => {
                self.bools.push(value);
                ParameterAddress::Bool(self.bools.len() - 1)
            }
            BoundParamValue::Color(value) => {
                self.colors.push(value);
                ParameterAddress::Color(self.colors.len() - 1)
            }
            BoundParamValue::Void => ParameterAddress::Void,
        };
        self.slots.push(address);
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = BoundParamValue> + '_ {
        self.slots.iter().map(|address| self.read(*address))
    }

    fn read(&self, address: ParameterAddress) -> BoundParamValue {
        match address {
            ParameterAddress::Void => BoundParamValue::Void,
            ParameterAddress::Int(index) => BoundParamValue::Int(self.ints[index]),
            ParameterAddress::Float(index) => BoundParamValue::Float(self.floats[index]),
            ParameterAddress::Bool(index) => BoundParamValue::Bool(self.bools[index]),
            ParameterAddress::Color(index) => BoundParamValue::Color(self.colors[index]),
            ParameterAddress::Array(index) => self.array_values[index].bound(),
            ParameterAddress::Enum(index) => BoundParamValue::Enum(self.enums[index].clone()),
            ParameterAddress::Marks(index) => BoundParamValue::Marks(self.marks[index].owned()),
            ParameterAddress::Curve(index) => self.curves[index].bound(),
            ParameterAddress::Gradient(index) => {
                BoundParamValue::Gradient(self.gradients[index].owned())
            }
        }
    }

    pub(super) fn get(&self, index: usize) -> Option<BoundParamValue> {
        self.slots.get(index).map(|address| self.read(*address))
    }

    #[cfg(test)]
    pub(super) fn gradient(&self, index: usize) -> Option<&GradientRegister> {
        match self.slots.get(index)? {
            ParameterAddress::Gradient(slot) => self.gradients.get(*slot),
            _ => None,
        }
    }
}
