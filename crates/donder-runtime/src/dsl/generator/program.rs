//! Raw generator instructions and whole-program admission. These input records
//! are mutable compiler data; only GeneratorProgram can execute admitted data.
use super::*;
use crate::values::Marks;
use alloc::vec;

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub struct BindingSlot(pub usize);
#[derive(Clone, Copy, Debug, PartialEq, Hash)]
pub struct FixedBindingSlot(pub BindingSlot);

#[derive(Clone, Debug, PartialEq)]
pub struct Calculation<O: super::super::CalculationOutput = Vec<Value>, I = BindingSlot> {
    pub program: CalculationProgram<O>,
    pub inputs: Box<[I]>,
}

pub type FixedCalculation<O> = Calculation<O, FixedBindingSlot>;

#[derive(Clone, Debug, PartialEq)]
pub enum Expression {
    Constant(Value),
    Read(BindingSlot),
    Calculate(Box<Calculation<Value>>),
}

pub type Block = Vec<Statement>;

#[derive(Clone, Debug, PartialEq)]
pub enum Statement {
    Assign {
        slot: BindingSlot,
        value: Expression,
    },
    Expression(Expression),
    Branch {
        condition: Box<FixedCalculation<bool>>,
        then_block: Block,
        else_block: Block,
    },
    For {
        initializer: Box<Statement>,
        iterations: usize,
        update: Box<Statement>,
        body: Block,
    },
    Marks {
        index: BindingSlot,
        marks: Box<FixedCalculation<Arc<Marks>>>,
        body: Block,
    },
    Range {
        index: BindingSlot,
        count: Box<FixedCalculation<i32>>,
        cap: i32,
        body: Block,
    },
    Emit {
        slot: GeneratedEffectSlot,
        start: Box<FixedCalculation<f32>>,
        duration: Box<FixedCalculation<f32>>,
        target: Box<FixedCalculation<Arc<TargetItemValue>>>,
        params: Vec<(Identifier, Expression)>,
    },
    Calculate {
        assigned: Vec<BindingSlot>,
        calculation: Box<Calculation>,
    },
}

type State = Vec<Option<bool>>;

pub(super) fn validate(
    params: &[ParamDecl],
    body: &Block,
    slots: &[Type],
    emissions: &[Box<[ParamDecl]>],
) -> Option<()> {
    if params.len() > u16::MAX as usize || slots.len() < params.len() + 2 {
        return None;
    }
    let mut state = vec![None; slots.len()];
    for (index, param) in params.iter().enumerate() {
        if slots[index] != param.ty || !well_formed(&param.ty) {
            return None;
        }
        state[index] = Some(param.fixed);
    }
    if slots[params.len()] != Type::Target
        || slots[params.len() + 1] != Type::Float
        || slots.iter().any(|ty| !well_formed(ty))
    {
        return None;
    }
    state[params.len()] = Some(true);
    state[params.len() + 1] = Some(true);
    Validator { slots, emissions }.block(body, &mut state)
}

fn well_formed(ty: &Type) -> bool {
    match ty {
        Type::Signal | Type::Timeline => false,
        Type::Enum(options) => !options.is_empty(),
        Type::Array(item) => well_formed(item),
        _ => true,
    }
}

struct Validator<'a> {
    slots: &'a [Type],
    emissions: &'a [Box<[ParamDecl]>],
}

impl Validator<'_> {
    fn input(&self, slot: BindingSlot, expected: &Type, state: &State) -> Option<bool> {
        expected.accepts(self.slots.get(slot.0)?).then_some(())?;
        *state.get(slot.0)?
    }

    fn calculation<O: super::super::CalculationOutput>(
        &self,
        value: &Calculation<O>,
        state: &State,
    ) -> Option<bool> {
        if value.inputs.len() != value.program.input_types().len()
            || value.program.output_types().len() > u16::MAX as usize
        {
            return None;
        }
        let mut fixed = !value.program.uses_time();
        for (slot, ty) in value.inputs.iter().zip(value.program.input_types()) {
            fixed &= self.input(*slot, ty, state)?;
        }
        Some(fixed)
    }

    fn fixed<O: super::super::CalculationOutput>(
        &self,
        value: &FixedCalculation<O>,
        state: &State,
    ) -> Option<()> {
        if value.program.uses_time() || value.inputs.len() != value.program.input_types().len() {
            return None;
        }
        for (slot, ty) in value.inputs.iter().zip(value.program.input_types()) {
            self.input(slot.0, ty, state)?.then_some(())?;
        }
        Some(())
    }

    fn expression(
        &self,
        value: &Expression,
        expected: Option<&Type>,
        state: &State,
    ) -> Option<bool> {
        match value {
            Expression::Constant(value) => expected
                .is_none_or(|ty| ty.accepts_value(value))
                .then_some(true),
            Expression::Read(slot) => {
                self.input(*slot, expected.unwrap_or(self.slots.get(slot.0)?), state)
            }
            Expression::Calculate(value) => {
                let [output] = value.program.output_types() else {
                    return None;
                };
                expected.is_none_or(|ty| ty.accepts(output)).then_some(())?;
                self.calculation(value, state)
            }
        }
    }

    fn block(&self, block: &Block, state: &mut State) -> Option<()> {
        for statement in block {
            self.statement(statement, state)?;
        }
        Some(())
    }

    fn statement(&self, statement: &Statement, state: &mut State) -> Option<()> {
        match statement {
            Statement::Assign { slot, value } => {
                let fixed = self.expression(value, Some(self.slots.get(slot.0)?), state)?;
                state[slot.0] = Some(fixed);
            }
            Statement::Expression(value) => {
                self.expression(value, None, state)?;
            }
            Statement::Calculate {
                assigned,
                calculation,
            } => {
                let fixed = self.calculation(calculation, state)?;
                if assigned.len() != calculation.program.output_types().len() {
                    return None;
                }
                for (slot, ty) in assigned.iter().zip(calculation.program.output_types()) {
                    self.slots.get(slot.0)?.accepts(ty).then_some(())?;
                    state[slot.0] = Some(fixed);
                }
            }
            Statement::Branch {
                condition,
                then_block,
                else_block,
            } => {
                self.fixed(condition, state)?;
                let mut other = state.clone();
                self.block(then_block, state)?;
                self.block(else_block, &mut other)?;
                join(state, &other);
            }
            Statement::For {
                initializer,
                iterations,
                update,
                body,
            } => {
                self.statement(initializer, state)?;
                self.loop_body(body, Some(update), state, *iterations != 1)?;
            }
            Statement::Marks { index, marks, body } => {
                self.fixed(marks, state)?;
                (self.slots.get(index.0)? == &Type::Int).then_some(())?;
                let entry = state.clone();
                state[index.0] = Some(true);
                self.loop_body(body, None, state, true)?;
                // An empty collection does not assign the index either.
                join(state, &entry);
            }
            Statement::Range {
                index,
                count,
                cap,
                body,
            } => {
                self.fixed(count, state)?;
                (*cap >= 0 && self.slots.get(index.0)? == &Type::Int).then_some(())?;
                let entry = state.clone();
                state[index.0] = Some(true);
                self.loop_body(body, None, state, true)?;
                join(state, &entry);
            }
            Statement::Emit {
                slot,
                start,
                duration,
                target,
                params,
            } => {
                let signature = self.emissions.get(slot.0 as usize)?;
                (signature.len() == params.len()).then_some(())?;
                self.fixed(start, state)?;
                self.fixed(duration, state)?;
                self.fixed(target, state)?;
                for ((name, expression), parameter) in params.iter().zip(signature.iter()) {
                    (name == &parameter.name
                        && parameter.default.is_none()
                        && well_formed(&parameter.ty))
                    .then_some(())?;
                    let fixed = self.expression(expression, Some(&parameter.ty), state)?;
                    (!parameter.fixed || fixed).then_some(())?;
                }
            }
        }
        Some(())
    }

    fn loop_body(
        &self,
        body: &Block,
        update: Option<&Statement>,
        state: &mut State,
        repeated: bool,
    ) -> Option<()> {
        loop {
            let entry = state.clone();
            self.block(body, state)?;
            if let Some(update) = update {
                self.statement(update, state)?;
            }
            if !repeated {
                return Some(());
            }
            join(state, &entry);
            if *state == entry {
                return Some(());
            }
        }
    }
}

fn join(state: &mut State, other: &State) {
    for (left, right) in state.iter_mut().zip(other) {
        *left = match (*left, *right) {
            (Some(left), Some(right)) => Some(left && right),
            _ => None,
        };
    }
}
