use super::arrays::ArrayParameter;
use super::targets::TargetRegister;
use super::{
    Arc, ArrayStorage, BoundParamValue, Color, Curve, Gradient, Identifier, Marks, PreparedCurve,
    RuntimeError, RuntimeValue, Type,
};
use super::{TargetItemValue, TargetItemsValue, TargetValue};
use alloc::vec::Vec;

/// Declaration order is metadata; dedicated value kinds live only in their typed bank.
/// A retained calculation reserves its complete layout before playback, including
/// inputs that will be supplied by another environment on each frame.
#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(super) struct ParameterValues {
    /// Calculated arrays live with their parameter handles. Frozen parameters
    /// have an empty arena and no calculated handles.
    #[rkyv(with = rkyv::with::Skip)]
    pub(super) arrays: ArrayStorage,
    pub(super) slots: Vec<ParameterAddress>,
    pub(super) target_items: Vec<TargetRegister<TargetItemValue>>,
    pub(super) target_lists: Vec<TargetRegister<TargetItemsValue>>,
    pub(super) targets: Vec<TargetRegister<TargetValue>>,
    pub(super) initialized: Vec<bool>,
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
    Target(usize),
    TargetItems(usize),
    TargetItem(usize),
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
                super::prepared_curve_crossing(&curve.crossings, value, fallback)
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
        if self.slots.len() != self.initialized.len() {
            return false;
        }
        let mut lengths = [0; 12];
        for address in &self.slots {
            let (bank, index) = match *address {
                ParameterAddress::Void => continue,
                ParameterAddress::Int(index) => (0, index),
                ParameterAddress::Float(index) => (1, index),
                ParameterAddress::Bool(index) => (2, index),
                ParameterAddress::Color(index) => (3, index),
                ParameterAddress::Array(index) => (4, index),
                ParameterAddress::Enum(index) => (11, index),
                ParameterAddress::Marks(index) => (5, index),
                ParameterAddress::Curve(index) => (6, index),
                ParameterAddress::Gradient(index) => (7, index),
                ParameterAddress::TargetItem(index) => (10, index),
                ParameterAddress::TargetItems(index) => (9, index),
                ParameterAddress::Target(index) => (8, index),
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
                self.targets.len(),
                self.target_lists.len(),
                self.target_items.len(),
                self.enums.len(),
            ]
    }

    pub(super) fn len(&self) -> usize {
        self.slots.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub(super) fn push(&mut self, ty: &Type, value: BoundParamValue) {
        // Void is an unresolved forwarded input, not its eventual storage type.
        let initialized = !matches!(value, BoundParamValue::Void);
        let address = match value {
            BoundParamValue::Array(values) => {
                self.array_values.push(ArrayParameter::Shared(values));
                ParameterAddress::Array(self.array_values.len() - 1)
            }
            BoundParamValue::CalculatedArray(index) => {
                self.array_values.push(ArrayParameter::Calculated(index));
                ParameterAddress::Array(self.array_values.len() - 1)
            }
            BoundParamValue::Enum(value) => {
                self.enums.push(value);
                ParameterAddress::Enum(self.enums.len() - 1)
            }
            BoundParamValue::TargetItem(value) => {
                self.target_items.push(TargetRegister::Shared(value));
                ParameterAddress::TargetItem(self.target_items.len() - 1)
            }
            BoundParamValue::TargetItems(value) => {
                self.target_lists.push(TargetRegister::Shared(value));
                ParameterAddress::TargetItems(self.target_lists.len() - 1)
            }
            BoundParamValue::Target(value) => {
                self.targets.push(TargetRegister::Shared(value));
                ParameterAddress::Target(self.targets.len() - 1)
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
            BoundParamValue::Void => match ty {
                Type::Enum(options) => {
                    self.enums.push(options[0].clone());
                    ParameterAddress::Enum(self.enums.len() - 1)
                }
                Type::TargetItem => {
                    self.target_items.push(TargetRegister::Empty);
                    ParameterAddress::TargetItem(self.target_items.len() - 1)
                }
                Type::TargetItems => {
                    self.target_lists.push(TargetRegister::Empty);
                    ParameterAddress::TargetItems(self.target_lists.len() - 1)
                }
                Type::Target => {
                    self.targets.push(TargetRegister::Empty);
                    ParameterAddress::Target(self.targets.len() - 1)
                }
                Type::Curve => {
                    self.curves.push(CurveRegister::Empty);
                    ParameterAddress::Curve(self.curves.len() - 1)
                }
                Type::Gradient => {
                    self.gradients.push(GradientRegister::Empty);
                    ParameterAddress::Gradient(self.gradients.len() - 1)
                }
                Type::Marks => {
                    self.marks.push(MarksRegister::Empty);
                    ParameterAddress::Marks(self.marks.len() - 1)
                }
                Type::Int => {
                    self.ints.push(0);
                    ParameterAddress::Int(self.ints.len() - 1)
                }
                Type::Float => {
                    self.floats.push(0.0);
                    ParameterAddress::Float(self.floats.len() - 1)
                }
                Type::Bool => {
                    self.bools.push(false);
                    ParameterAddress::Bool(self.bools.len() - 1)
                }
                Type::Color => {
                    self.colors.push(Color::BLACK);
                    ParameterAddress::Color(self.colors.len() - 1)
                }
                Type::Array(_) => {
                    self.array_values.push(ArrayParameter::Empty);
                    ParameterAddress::Array(self.array_values.len() - 1)
                }
                Type::Void | Type::Signal | Type::Timeline => ParameterAddress::Void,
            },
        };
        self.slots.push(address);
        self.initialized.push(initialized);
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = BoundParamValue> + '_ {
        self.slots.iter().enumerate().map(|(index, address)| {
            if self.initialized[index] {
                self.read(*address)
            } else {
                BoundParamValue::Void
            }
        })
    }

    fn read(&self, address: ParameterAddress) -> BoundParamValue {
        match address {
            ParameterAddress::Void => BoundParamValue::Void,
            ParameterAddress::TargetItem(index) => {
                BoundParamValue::TargetItem(self.target_items[index].owned())
            }
            ParameterAddress::TargetItems(index) => {
                BoundParamValue::TargetItems(self.target_lists[index].owned())
            }
            ParameterAddress::Target(index) => BoundParamValue::Target(self.targets[index].owned()),
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
        self.slots.get(index).map(|address| {
            if self.initialized[index] {
                self.read(*address)
            } else {
                BoundParamValue::Void
            }
        })
    }

    pub(super) fn runtime(&self, index: usize) -> Option<RuntimeValue> {
        let address = self.slots.get(index)?;
        Some(if !self.initialized[index] {
            RuntimeValue::Void
        } else {
            match *address {
                ParameterAddress::Void => RuntimeValue::Void,
                ParameterAddress::TargetItem(index) => {
                    RuntimeValue::TargetItem(self.target_items[index].owned())
                }
                ParameterAddress::TargetItems(index) => {
                    RuntimeValue::TargetItems(self.target_lists[index].owned())
                }
                ParameterAddress::Target(index) => {
                    RuntimeValue::Target(self.targets[index].owned())
                }
                ParameterAddress::Int(index) => RuntimeValue::Int(self.ints[index]),
                ParameterAddress::Float(index) => RuntimeValue::Float(self.floats[index]),
                ParameterAddress::Bool(index) => RuntimeValue::Bool(self.bools[index]),
                ParameterAddress::Color(index) => RuntimeValue::Color(self.colors[index]),
                ParameterAddress::Array(index) => self.array_values[index].register().runtime(),
                ParameterAddress::Enum(index) => RuntimeValue::Enum(self.enums[index].clone()),
                ParameterAddress::Marks(index) => RuntimeValue::Marks(self.marks[index].owned()),
                ParameterAddress::Curve(index) => self.curves[index].runtime(),
                ParameterAddress::Gradient(index) => {
                    RuntimeValue::Gradient(self.gradients[index].owned())
                }
            }
        })
    }

    pub(super) fn array_parameter(&self, index: usize) -> Option<&ArrayParameter> {
        match self.slots.get(index)? {
            ParameterAddress::Array(index) => self.array_values.get(*index),
            _ => None,
        }
    }

    pub(super) fn curve(&self, index: usize) -> Option<&CurveRegister> {
        match self.slots.get(index)? {
            ParameterAddress::Curve(slot) if self.initialized[index] => self.curves.get(*slot),
            _ => None,
        }
    }

    pub(super) fn curve_mut(&mut self, index: usize) -> Option<&mut CurveRegister> {
        match self.slots.get(index)? {
            ParameterAddress::Curve(slot) if self.initialized[index] => self.curves.get_mut(*slot),
            _ => None,
        }
    }

    pub(super) fn gradient(&self, index: usize) -> Option<&GradientRegister> {
        match self.slots.get(index)? {
            ParameterAddress::Gradient(slot) if self.initialized[index] => {
                self.gradients.get(*slot)
            }
            _ => None,
        }
    }

    pub(super) fn enum_value(&self, index: usize) -> Option<&Identifier> {
        match self.slots.get(index)? {
            ParameterAddress::Enum(slot) if self.initialized[index] => self.enums.get(*slot),
            _ => None,
        }
    }

    pub(super) fn enum_value_mut(&mut self, index: usize) -> Option<&mut Identifier> {
        match self.slots.get(index)? {
            ParameterAddress::Enum(slot) if self.initialized[index] => self.enums.get_mut(*slot),
            _ => None,
        }
    }

    pub(super) fn clear_slot(&mut self, index: usize) -> BoundParamValue {
        self.initialized[index] = false;
        match self.slots[index] {
            ParameterAddress::TargetItem(index) => {
                self.target_items[index] = TargetRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::TargetItems(index) => {
                self.target_lists[index] = TargetRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::Target(index) => {
                self.targets[index] = TargetRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::Curve(index) => {
                self.curves[index] = CurveRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::Gradient(index) => {
                self.gradients[index] = GradientRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::Marks(index) => {
                self.marks[index] = MarksRegister::Empty;
                BoundParamValue::Void
            }
            ParameterAddress::Array(index) => {
                match core::mem::take(&mut self.array_values[index]) {
                    ArrayParameter::Calculated(slot) => BoundParamValue::CalculatedArray(slot),
                    ArrayParameter::Empty | ArrayParameter::Shared(_) => BoundParamValue::Void,
                }
            }
            _ => BoundParamValue::Void,
        }
    }

    pub(super) fn write(
        &mut self,
        index: usize,
        value: BoundParamValue,
    ) -> Result<(), RuntimeError> {
        let address = self
            .slots
            .get(index)
            .ok_or_else(|| RuntimeError::new("invalid parameter binding destination"))?;
        match (*address, value) {
            (ParameterAddress::Enum(index), BoundParamValue::Enum(value)) => {
                self.enums[index] = value
            }
            (ParameterAddress::TargetItem(index), BoundParamValue::TargetItem(value)) => {
                self.target_items[index] = TargetRegister::Shared(value)
            }
            (ParameterAddress::TargetItems(index), BoundParamValue::TargetItems(value)) => {
                self.target_lists[index] = TargetRegister::Shared(value)
            }
            (ParameterAddress::Target(index), BoundParamValue::Target(value)) => {
                self.targets[index] = TargetRegister::Shared(value)
            }
            (ParameterAddress::Curve(index), BoundParamValue::Curve(value)) => {
                self.curves[index] = CurveRegister::Prepared(value)
            }
            (ParameterAddress::Curve(index), BoundParamValue::RawCurve(value)) => {
                self.curves[index] = CurveRegister::Raw(value)
            }
            (ParameterAddress::Gradient(index), BoundParamValue::Gradient(value)) => {
                self.gradients[index] = GradientRegister::Shared(value)
            }
            (ParameterAddress::Marks(index), BoundParamValue::Marks(value)) => {
                self.marks[index] = MarksRegister::Shared(value)
            }
            (ParameterAddress::Int(index), BoundParamValue::Int(value)) => self.ints[index] = value,
            (ParameterAddress::Float(index), BoundParamValue::Float(value)) => {
                self.floats[index] = value
            }
            (ParameterAddress::Bool(index), BoundParamValue::Bool(value)) => {
                self.bools[index] = value
            }
            (ParameterAddress::Color(index), BoundParamValue::Color(value)) => {
                self.colors[index] = value
            }
            (ParameterAddress::Array(index), BoundParamValue::Array(values)) => {
                self.array_values[index] = ArrayParameter::Shared(values)
            }
            (ParameterAddress::Array(index), BoundParamValue::CalculatedArray(slot)) => {
                self.array_values[index] = ArrayParameter::Calculated(slot)
            }
            (_, BoundParamValue::Void) => {
                self.initialized[index] = false;
                return Ok(());
            }
            _ => {
                return Err(RuntimeError::new(
                    "parameter value does not match its storage type",
                ));
            }
        }
        self.initialized[index] = true;
        Ok(())
    }
}
