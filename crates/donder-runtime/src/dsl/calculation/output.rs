//! Result projections are chosen during compilation, never by downcasting an
//! evaluated Value. This trait is sealed by its private module.
use super::*;
use crate::dsl::bytecode::{BoolSlot, IntSlot, MarksSlot, NumberSlot, TargetSource};
use crate::dsl::vm::CalculationValues;
use crate::values::Marks;
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;

pub trait Projection: Clone + core::fmt::Debug + PartialEq {
    type Slots: Clone + core::fmt::Debug + PartialEq;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots>;
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]>;
    fn read(values: &CalculationValues<'_, '_>, slots: &Self::Slots) -> Self;
}

impl Projection for Vec<Value> {
    type Slots = Box<[ValueSlot]>;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        Some(results.into())
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        slots
    }
    fn read(values: &CalculationValues<'_, '_>, slots: &Self::Slots) -> Self {
        slots.iter().map(|slot| values.value(*slot)).collect()
    }
}

impl Projection for bool {
    type Slots = BoolSlot;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [ValueSlot::Bool(slot)] => Some(*slot),
            _ => None,
        }
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        Box::new([ValueSlot::Bool(slots)])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &BoolSlot) -> Self {
        values.boolean(*slot)
    }
}

impl Projection for i32 {
    type Slots = IntSlot;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [ValueSlot::Int(slot)] => Some(*slot),
            _ => None,
        }
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        Box::new([ValueSlot::Int(slots)])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &IntSlot) -> Self {
        values.integer(*slot)
    }
}

impl Projection for f32 {
    type Slots = NumberSlot;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [ValueSlot::Int(slot)] => Some(NumberSlot::Int(*slot)),
            [ValueSlot::Float(slot)] => Some(NumberSlot::Float(*slot)),
            _ => None,
        }
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        Box::new([slots.value_slot()])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &NumberSlot) -> Self {
        values.number(*slot)
    }
}

impl Projection for Arc<Marks> {
    type Slots = MarksSlot;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [ValueSlot::Marks(slot)] => Some(*slot),
            _ => None,
        }
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        Box::new([ValueSlot::Marks(slots)])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &MarksSlot) -> Self {
        values.marks(*slot)
    }
}

impl Projection for Arc<super::super::TargetItemValue> {
    type Slots = TargetSource;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [ValueSlot::Target(slot)] => Some(TargetSource::Target(*slot)),
            [ValueSlot::TargetItems(slot)] => Some(TargetSource::Items(*slot)),
            [ValueSlot::TargetItem(slot)] => Some(TargetSource::Item(*slot)),
            _ => None,
        }
    }
    fn into_slots(slots: Self::Slots) -> Box<[ValueSlot]> {
        Box::new([match slots {
            TargetSource::Target(slot) => ValueSlot::Target(slot),
            TargetSource::Items(slot) => ValueSlot::TargetItems(slot),
            TargetSource::Item(slot) => ValueSlot::TargetItem(slot),
        }])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &TargetSource) -> Self {
        values.target(*slot)
    }
}

impl Projection for Value {
    type Slots = ValueSlot;
    fn slots(results: &[ValueSlot]) -> Option<Self::Slots> {
        match results {
            [slot] => Some(*slot),
            _ => None,
        }
    }
    fn into_slots(slot: ValueSlot) -> Box<[ValueSlot]> {
        Box::new([slot])
    }
    fn read(values: &CalculationValues<'_, '_>, slot: &ValueSlot) -> Self {
        values.value(*slot)
    }
}
