//! Bind fixed inputs and expand a compiled generator into children and retained
//! parameter calculations. Every expression was compiled with the declaration;
//! specialization never recompiles authored code.
use super::{
    CalculationProgram, EmittedReference, Identifier, ParamDecl, RunContext, RuntimeError,
    TargetItemValue, Type, Value, VmWorkspace,
};
use crate::values::{SampleDuration, SampleTime};
use std::sync::Arc;

mod bindings;
mod compilation;
use bindings::Bindings;
mod hashing;
use compilation::{Block, Calculation, Expression, FixedCalculation, Statement};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GeneratedEffectSlot(pub u32);

/// Host-only context for expanding a generator. Its calculations receive captured
/// targets and duration as ordinary typed inputs, not a second VM context.
#[derive(Clone, Debug)]
pub struct GeneratorContext {
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<super::TargetValue>,
}

impl GeneratorContext {
    /// Invalid or zero-length children are omitted during specialization.
    fn child_timing(
        &self,
        start_seconds: f32,
        duration_seconds: f32,
    ) -> Option<(SampleTime, SampleDuration)> {
        let start =
            crate::values::sample_time_with_seconds_offset(self.start_time, start_seconds).ok()?;
        let duration = crate::values::sample_duration_from_seconds_f32(duration_seconds).ok()?;
        (duration.as_ticks() != 0).then_some((start, duration))
    }
}

/// Resolved lexical storage. Only generator compilation constructs these slots;
/// branches and loop bodies refer directly to their declaration's storage.
#[derive(Clone, Copy, Debug, PartialEq, Hash)]
struct BindingSlot(usize);

/// Staging checked this use of a lexical slot as preparation-time data.
/// Only structural-expression compilation constructs these references.
#[derive(Clone, Copy, Debug, PartialEq, Hash)]
struct FixedBindingSlot(BindingSlot);

#[derive(Clone, Debug, PartialEq)]
pub struct GeneratorProgram {
    params: Vec<ParamDecl>,
    body: Block,
    slot_count: usize,
}

/// Immutable program/input pairing admitted before expansion. Borrowing the
/// input slice prevents callers from changing its values or fixed/live kinds
/// while the invocation is in use.
#[derive(Debug)]
pub struct BoundGenerator<'a> {
    program: &'a GeneratorProgram,
    inputs: &'a [GeneratorInput],
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorInput {
    Fixed(Value),
    Live,
}

#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorBinding {
    Constant(Value),
    Parameter(u16),
    Calculation { index: usize, output: u16 },
}

/// A VM program with declared output slots. Input/output names disappear before playback.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneratorCalculation {
    pub program: CalculationProgram,
    pub inputs: Box<[GeneratorBinding]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpecializedChild {
    pub definition: GeneratedEffectSlot,
    pub start_time: SampleTime,
    pub duration: SampleDuration,
    pub target: Arc<TargetItemValue>,
    pub params: Vec<(Identifier, GeneratorBinding)>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SpecializedGenerator {
    pub calculations: Vec<GeneratorCalculation>,
    pub children: Vec<SpecializedChild>,
}

impl GeneratorProgram {
    pub(super) fn new(
        params: Vec<ParamDecl>,
        body: super::checked::CheckedBlock,
        emissions: Vec<EmittedReference>,
        preparation_controls: &super::staging::PreparationControls,
    ) -> Result<Self, super::Diagnostic> {
        let (body, slot_count) =
            compilation::compile(&params, body, &emissions, preparation_controls)?;
        Ok(Self {
            params,
            body,
            slot_count,
        })
    }

    /// Check external arguments once, including unused parameters. The
    /// specialization executor only receives this immutable admitted pairing.
    pub fn bind<'a>(
        &'a self,
        inputs: &'a [GeneratorInput],
    ) -> Result<BoundGenerator<'a>, RuntimeError> {
        if inputs.len() != self.params.len() {
            return Err(error(
                "generator input count does not match its declaration",
            ));
        }
        for (param, input) in self.params.iter().zip(inputs) {
            match input {
                GeneratorInput::Fixed(value) if !param.ty.accepts_value(value) => {
                    return Err(error(format!(
                        "generator parameter `{}` does not match {:?}",
                        param.name.as_str(),
                        param.ty,
                    )));
                }
                GeneratorInput::Live if param.fixed => {
                    return Err(error(format!(
                        "fixed parameter `{}` cannot receive a live binding",
                        param.name.as_str(),
                    )));
                }
                _ => {}
            }
        }
        Ok(BoundGenerator {
            program: self,
            inputs,
        })
    }
}

impl BoundGenerator<'_> {
    pub fn specialize(
        &self,
        context: &GeneratorContext,
    ) -> Result<SpecializedGenerator, RuntimeError> {
        let mut bindings = Bindings::new(self.program.slot_count);
        for (index, input) in self.inputs.iter().enumerate() {
            let binding = match input {
                GeneratorInput::Fixed(value) => GeneratorBinding::Constant(value.clone()),
                GeneratorInput::Live => GeneratorBinding::Parameter(index as u16),
            };
            bindings.assign(BindingSlot(index), binding);
        }
        // Compilation reserves parameters, target, and duration in this order.
        bindings.assign(
            BindingSlot(self.program.params.len()),
            GeneratorBinding::Constant(Value::Target(Arc::clone(&context.target))),
        );
        bindings.assign(
            BindingSlot(self.program.params.len() + 1),
            GeneratorBinding::Constant(Value::Float(crate::values::sample_duration_seconds_f32(
                context.duration,
            ))),
        );
        let mut specializer = Specializer {
            context,
            result: SpecializedGenerator::default(),
            workspace: VmWorkspace::default(),
            bind_cache: super::DslBindCache::default(),
            bindings,
        };
        specializer.block(&self.program.body)?;
        Ok(specializer.result)
    }
}

struct Specializer<'a> {
    context: &'a GeneratorContext,
    result: SpecializedGenerator,
    workspace: VmWorkspace,
    bind_cache: super::DslBindCache,
    bindings: Bindings,
}

enum Calculated<O> {
    Fixed(O),
    Retained(usize),
}

impl Specializer<'_> {
    fn calculate<O: super::CalculationOutput>(
        &mut self,
        template: &Calculation<O>,
    ) -> Result<Calculated<O>, RuntimeError> {
        let inputs = template
            .inputs
            .iter()
            .map(|slot| self.bindings.read(*slot))
            .collect::<Vec<_>>();
        let constants = inputs
            .iter()
            .map(|binding| match binding {
                GeneratorBinding::Constant(value) => Some(value.clone()),
                _ => None,
            })
            .collect::<Option<Vec<_>>>();
        if let Some(constants) = constants.filter(|_| !template.program.uses_time()) {
            let invocation = template.program.bind(constants, &mut self.bind_cache)?;
            let values = invocation.evaluate(
                &RunContext {
                    progress: 0.0,
                    time: SampleDuration::from_ticks(0),
                    duration: self.context.duration,
                    pixel_index: 0,
                    pixel_count: 0,
                    pixel_fraction: 0.0,
                },
                &mut self.workspace,
            );
            return Ok(Calculated::Fixed(values));
        }
        let index = self.result.calculations.len();
        self.result.calculations.push(GeneratorCalculation {
            program: template.program.clone().into_values(),
            inputs: inputs.into(),
        });
        Ok(Calculated::Retained(index))
    }

    fn calculate_many(
        &mut self,
        template: &Calculation,
    ) -> Result<Vec<GeneratorBinding>, RuntimeError> {
        Ok(match self.calculate(template)? {
            Calculated::Fixed(values) => {
                values.into_iter().map(GeneratorBinding::Constant).collect()
            }
            Calculated::Retained(index) => template
                .program
                .output_types()
                .iter()
                .enumerate()
                .map(|(output, _)| GeneratorBinding::Calculation {
                    index,
                    output: output as u16,
                })
                .collect(),
        })
    }

    fn expression(&mut self, expression: &Expression) -> Result<GeneratorBinding, RuntimeError> {
        match expression {
            Expression::Constant(value) => Ok(GeneratorBinding::Constant(value.clone())),
            Expression::Read(slot) => Ok(self.bindings.read(*slot)),
            Expression::Calculate(template) => Ok(match self.calculate(template)? {
                Calculated::Fixed(value) => GeneratorBinding::Constant(value),
                Calculated::Retained(index) => GeneratorBinding::Calculation { index, output: 0 },
            }),
        }
    }

    fn fixed<O: super::CalculationOutput>(
        &mut self,
        template: &FixedCalculation<O>,
    ) -> Result<O, RuntimeError> {
        let inputs = template
            .inputs
            .iter()
            .map(|slot| self.bindings.fixed(*slot))
            .collect();
        let invocation = template.program.bind(inputs, &mut self.bind_cache)?;
        Ok(invocation.evaluate(
            &RunContext {
                progress: 0.0,
                time: SampleDuration::from_ticks(0),
                duration: self.context.duration,
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            },
            &mut self.workspace,
        ))
    }

    fn block(&mut self, block: &Block) -> Result<(), RuntimeError> {
        for statement in block {
            self.statement(statement)?;
        }
        Ok(())
    }

    fn statement(&mut self, statement: &Statement) -> Result<(), RuntimeError> {
        match statement {
            Statement::Assign { slot, value } => {
                let binding = self.expression(value)?;
                self.bindings.assign(*slot, binding);
            }
            Statement::Expression(value) => {
                self.expression(value)?;
            }
            Statement::Branch {
                condition,
                then_block,
                else_block,
            } => {
                if self.fixed(condition)? {
                    self.block(then_block)?;
                } else {
                    self.block(else_block)?;
                }
            }
            Statement::For {
                initializer,
                iterations,
                update,
                body,
            } => {
                self.statement(initializer)?;
                for _ in 0..*iterations {
                    self.block(body)?;
                    self.statement(update)?;
                }
            }
            Statement::Marks { index, marks, body } => {
                let marks = self.fixed(marks)?;
                for mark in 0..marks.marks.len() {
                    // Collection traversal uses native lengths. The exposed DSL
                    // int wraps just like the VM's loop-index increment.
                    self.bindings
                        .assign(*index, GeneratorBinding::Constant(Value::Int(mark as i32)));
                    self.block(body)?;
                }
            }
            Statement::Range {
                index,
                count,
                cap,
                body,
            } => {
                let count = self.fixed(count)?;
                for value in 0..count.max(0).min(*cap) {
                    self.bindings
                        .assign(*index, GeneratorBinding::Constant(Value::Int(value)));
                    self.block(body)?;
                }
            }
            Statement::Calculate {
                assigned,
                calculation,
            } => {
                let values = self.calculate_many(calculation)?;
                for (slot, binding) in assigned.iter().zip(values) {
                    self.bindings.assign(*slot, binding);
                }
            }
            Statement::Emit {
                slot,
                start,
                duration,
                target,
                params,
            } => {
                let start = self.fixed(start)?;
                let duration = self.fixed(duration)?;
                let target = self.fixed(target)?;
                let Some((start_time, duration)) = self.context.child_timing(start, duration)
                else {
                    return Ok(());
                };
                let params = params
                    .iter()
                    .map(|(name, expression)| Ok((name.clone(), self.expression(expression)?)))
                    .collect::<Result<_, RuntimeError>>()?;
                self.result.children.push(SpecializedChild {
                    definition: *slot,
                    start_time,
                    duration,
                    target,
                    params,
                });
            }
        }
        Ok(())
    }
}

fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}
