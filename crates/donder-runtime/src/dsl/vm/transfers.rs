//! Resolve parameter/result bank addresses before playback. A transfer stores
//! the compatible operation, not two independently mutable type tags.
use super::*;

#[derive(Clone, Debug, PartialEq)]
enum Operation {
    Void,
    Int(usize, usize),
    Float(usize, usize),
    IntToFloat(usize, usize),
    Bool(usize, usize),
    Color(usize, usize),
    Enum(usize, usize),
    Array(usize, usize),
    Curve(usize, usize),
    Gradient(usize, usize),
    Marks(usize, usize),
    Target(usize, usize),
    TargetItems(usize, usize),
    TargetItem(usize, usize),
}

impl Operation {
    fn admit(source: ParameterAddress, destination: ParameterAddress) -> Option<Self> {
        use ParameterAddress as A;
        Some(match (source, destination) {
            (A::Void, A::Void) => Self::Void,
            (A::Int(a), A::Int(b)) => Self::Int(a, b),
            (A::Float(a), A::Float(b)) => Self::Float(a, b),
            (A::Int(a), A::Float(b)) => Self::IntToFloat(a, b),
            (A::Bool(a), A::Bool(b)) => Self::Bool(a, b),
            (A::Color(a), A::Color(b)) => Self::Color(a, b),
            (A::Enum(a), A::Enum(b)) => Self::Enum(a, b),
            (A::Array(a), A::Array(b)) => Self::Array(a, b),
            (A::Curve(a), A::Curve(b)) => Self::Curve(a, b),
            (A::Gradient(a), A::Gradient(b)) => Self::Gradient(a, b),
            (A::Marks(a), A::Marks(b)) => Self::Marks(a, b),
            (A::Target(a), A::Target(b)) => Self::Target(a, b),
            (A::TargetItems(a), A::TargetItems(b)) => Self::TargetItems(a, b),
            (A::TargetItem(a), A::TargetItem(b)) => Self::TargetItem(a, b),
            _ => return None,
        })
    }

    fn apply(&self, source: Banks<'_>, destination: &mut ParameterValues) {
        match *self {
            Self::Void => {}
            Self::Int(a, b) => destination.ints[b] = source.ints[a],
            Self::Float(a, b) => destination.floats[b] = source.floats[a],
            Self::IntToFloat(a, b) => destination.floats[b] = source.ints[a] as f32,
            Self::Bool(a, b) => destination.bools[b] = source.bools[a],
            Self::Color(a, b) => destination.colors[b] = source.colors[a],
            Self::Enum(a, b) => destination.enums[b].clone_from(&source.enums[a]),
            Self::Curve(a, b) => destination.curves[b].clone_from(&source.curves[a]),
            Self::Gradient(a, b) => destination.gradients[b].clone_from(&source.gradients[a]),
            Self::Marks(a, b) => destination.marks[b].clone_from(&source.marks[a]),
            Self::Target(a, b) => destination.targets[b].clone_from(&source.targets[a]),
            Self::TargetItems(a, b) => {
                destination.target_lists[b].clone_from(&source.target_lists[a])
            }
            Self::TargetItem(a, b) => {
                destination.target_items[b].clone_from(&source.target_items[a])
            }
            Self::Array(a, b) => {
                if let ArrayParameter::Calculated(old) =
                    core::mem::take(&mut destination.array_values[b])
                {
                    destination.arrays.release(RuntimeValue::ArraySlot(old));
                }
                destination.array_values[b] = match source.array_values.get(a) {
                    ArrayRegister::Empty => ArrayParameter::Empty,
                    ArrayRegister::Shared(values) => ArrayParameter::Shared(values),
                    array @ (ArrayRegister::Local(_) | ArrayRegister::Parameter(_)) => {
                        ArrayParameter::Calculated(destination.arrays.copy_admitted(
                            &array,
                            source.arrays,
                            source.parameters,
                        ))
                    }
                };
            }
        }
    }
}

enum Arrays<'a> {
    Parameters(&'a [ArrayParameter]),
    Registers(&'a [ArrayRegister]),
}

impl Arrays<'_> {
    fn get(&self, index: usize) -> ArrayRegister {
        match self {
            Self::Parameters(values) => values[index].register(),
            Self::Registers(values) => values[index].clone(),
        }
    }
}

struct Banks<'a> {
    ints: &'a [i32],
    floats: &'a [f32],
    bools: &'a [bool],
    colors: &'a [Color],
    enums: &'a [Identifier],
    curves: &'a [CurveRegister],
    gradients: &'a [GradientRegister],
    marks: &'a [MarksRegister],
    targets: &'a [TargetRegister<TargetValue>],
    target_lists: &'a [TargetRegister<TargetItemsValue>],
    target_items: &'a [TargetRegister<TargetItemValue>],
    array_values: Arrays<'a>,
    arrays: &'a ArrayStorage,
    parameters: &'a ArrayStorage,
}

#[derive(Clone, Debug)]
pub(crate) struct ParameterTransfer {
    operation: Operation,
    destination: usize,
}

/// A compatible source/destination schema edge from the admitted generator IR.
/// The builder resolves its slots only in layouts issued for those declarations.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParameterLink {
    operation: Operation,
}

impl ParameterLink {
    /// Generator admission checks all assignment/call edges, including the
    /// integer-to-float widening represented here. Only specialization of that
    /// admitted IR constructs these links; external inputs cannot supply them.
    pub(in crate::dsl) fn compiled(source: &Type, destination: &Type) -> Self {
        let operation = match destination {
            Type::Int => Operation::Int(0, 0),
            Type::Float if matches!(source, Type::Int) => Operation::IntToFloat(0, 0),
            Type::Float => Operation::Float(0, 0),
            Type::Bool => Operation::Bool(0, 0),
            Type::Color => Operation::Color(0, 0),
            Type::Enum(_) => Operation::Enum(0, 0),
            Type::Array(_) => Operation::Array(0, 0),
            Type::Curve => Operation::Curve(0, 0),
            Type::Gradient => Operation::Gradient(0, 0),
            Type::Marks => Operation::Marks(0, 0),
            Type::Target => Operation::Target(0, 0),
            Type::TargetItems => Operation::TargetItems(0, 0),
            Type::TargetItem => Operation::TargetItem(0, 0),
            Type::Void | Type::Signal | Type::Timeline => Operation::Void,
        };
        Self { operation }
    }

    pub(crate) fn resolve(
        &self,
        source: &BoundParams,
        source_slot: u16,
        destination: &BoundParams,
        destination_slot: u16,
    ) -> ParameterTransfer {
        fn bank(address: ParameterAddress) -> usize {
            match address {
                ParameterAddress::Void => 0,
                ParameterAddress::Int(index)
                | ParameterAddress::Float(index)
                | ParameterAddress::Bool(index)
                | ParameterAddress::Color(index)
                | ParameterAddress::Enum(index)
                | ParameterAddress::Array(index)
                | ParameterAddress::Curve(index)
                | ParameterAddress::Gradient(index)
                | ParameterAddress::Marks(index)
                | ParameterAddress::Target(index)
                | ParameterAddress::TargetItems(index)
                | ParameterAddress::TargetItem(index) => index,
            }
        }
        let from = bank(source.values.slots[usize::from(source_slot)]);
        let to = bank(destination.values.slots[usize::from(destination_slot)]);
        let operation = match self.operation {
            Operation::Void => Operation::Void,
            Operation::Int(..) => Operation::Int(from, to),
            Operation::Float(..) => Operation::Float(from, to),
            Operation::IntToFloat(..) => Operation::IntToFloat(from, to),
            Operation::Bool(..) => Operation::Bool(from, to),
            Operation::Color(..) => Operation::Color(from, to),
            Operation::Enum(..) => Operation::Enum(from, to),
            Operation::Array(..) => Operation::Array(from, to),
            Operation::Curve(..) => Operation::Curve(from, to),
            Operation::Gradient(..) => Operation::Gradient(from, to),
            Operation::Marks(..) => Operation::Marks(from, to),
            Operation::Target(..) => Operation::Target(from, to),
            Operation::TargetItems(..) => Operation::TargetItems(from, to),
            Operation::TargetItem(..) => Operation::TargetItem(from, to),
        };
        ParameterTransfer {
            operation,
            destination: usize::from(destination_slot),
        }
    }
}

impl ParameterTransfer {
    pub(crate) fn admit(
        source: &BoundParams,
        destination: &BoundParams,
        from: usize,
        to: usize,
    ) -> Option<Self> {
        Some(Self {
            operation: Operation::admit(
                *source.values.slots.get(from)?,
                *destination.values.slots.get(to)?,
            )?,
            destination: to,
        })
    }

    pub(crate) fn apply(&self, source: &BoundParams, destination: &mut BoundParams) {
        let source = &source.values;
        let empty = ArrayStorage::default();
        if matches!(self.operation, Operation::Array(..)) {
            destination.values.arrays.begin_copy(&empty, &source.arrays);
        }
        self.operation.apply(
            Banks {
                ints: &source.ints,
                floats: &source.floats,
                bools: &source.bools,
                colors: &source.colors,
                enums: &source.enums,
                curves: &source.curves,
                gradients: &source.gradients,
                marks: &source.marks,
                targets: &source.targets,
                target_lists: &source.target_lists,
                target_items: &source.target_items,
                array_values: Arrays::Parameters(&source.array_values),
                arrays: &empty,
                parameters: &source.arrays,
            },
            &mut destination.values,
        );
        destination.values.initialized[self.destination] =
            !matches!(self.operation, Operation::Void);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CalculationTransfer {
    operations: Box<[Operation]>,
}

impl CalculationTransfer {
    pub(crate) fn admit(results: &[ValueSlot], output: &BoundParams) -> Option<Self> {
        if results.len() != output.len() {
            return None;
        }
        let operations = results
            .iter()
            .zip(&output.values.slots)
            .map(|(source, destination)| {
                let address = match *source {
                    ValueSlot::Int(slot) => ParameterAddress::Int(slot.0 as usize),
                    ValueSlot::Float(slot) => ParameterAddress::Float(slot.0 as usize),
                    ValueSlot::Bool(slot) => ParameterAddress::Bool(slot.0 as usize),
                    ValueSlot::Color(slot) => ParameterAddress::Color(slot.0 as usize),
                    ValueSlot::Enum(slot) => ParameterAddress::Enum(slot.0 as usize),
                    ValueSlot::Array(slot) => ParameterAddress::Array(slot.0 as usize),
                    ValueSlot::Curve(slot) => ParameterAddress::Curve(slot.0 as usize),
                    ValueSlot::Gradient(slot) => ParameterAddress::Gradient(slot.0 as usize),
                    ValueSlot::Marks(slot) => ParameterAddress::Marks(slot.0 as usize),
                    ValueSlot::Target(slot) => ParameterAddress::Target(slot.0 as usize),
                    ValueSlot::TargetItems(slot) => ParameterAddress::TargetItems(slot.0 as usize),
                    ValueSlot::TargetItem(slot) => ParameterAddress::TargetItem(slot.0 as usize),
                    ValueSlot::Void => ParameterAddress::Void,
                };
                Operation::admit(address, *destination)
            })
            .collect::<Option<Box<[_]>>>()?;
        Some(Self { operations })
    }

    pub(super) fn apply(
        &self,
        workspace: &VmWorkspace,
        params: &BoundParams,
        destination: &mut BoundParams,
    ) {
        let source = &workspace.registers;
        destination
            .values
            .arrays
            .begin_copy(&workspace.arrays, &params.values.arrays);
        for (index, operation) in self.operations.iter().enumerate() {
            operation.apply(
                Banks {
                    ints: &source.ints,
                    floats: &source.floats,
                    bools: &source.bools,
                    colors: &source.colors,
                    enums: &source.enums,
                    curves: &source.curves,
                    gradients: &source.gradients,
                    marks: &source.marks,
                    targets: &source.targets,
                    target_lists: &source.target_lists,
                    target_items: &source.target_items,
                    array_values: Arrays::Registers(&source.array_values),
                    arrays: &workspace.arrays,
                    parameters: &params.values.arrays,
                },
                &mut destination.values,
            );
            destination.values.initialized[index] = !matches!(operation, Operation::Void);
        }
    }
}
