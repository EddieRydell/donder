//! Partial evaluation for one prepared invocation. Only values guaranteed by
//! preparation enter this pass; automation parameters remain runtime inputs.
use super::bytecode::{
    BytecodeProgram, ContextRead as ContextField, EnumSlotType, Instruction, NumberSlot,
};
use super::{BoundParams, OperatorProgram, SampleProgram, Value};

/// Context values shared by every sample of a prepared invocation. An absent
/// value remains a context read (for example, differently sized fixtures).
#[derive(Clone, Copy, Debug, Default)]
pub struct ProgramConstants {
    pub pixel_count: Option<i32>,
    pub duration_seconds: Option<f32>,
}

impl SampleProgram {
    /// Evaluate fixed-input arithmetic while keeping its results in bindings.
    /// Automated parameters must remain dynamic. The returned schema may grow.
    /// Use the returned bindings: unused fixed slots may now hold folded results.
    pub fn prepare_bindings(
        &self,
        params: &BoundParams,
        dynamic: impl Fn(usize) -> bool,
    ) -> (Self, BoundParams) {
        let (program, types) = self.clone().into_parts();
        let (program, types, params) = prepare_bindings(program, types, params, dynamic);
        let program = Self::admit(program, types)
            .unwrap_or_else(|| unreachable!("binding preparation preserves effect invariants"));
        (program, params)
    }

    /// Specialize control flow for these bindings. Ordinary uniform data remains
    /// parameterized so differently configured clips can share the program.
    /// Callers must mark every parameter that can change as dynamic.
    pub fn specialize(
        &self,
        params: &BoundParams,
        dynamic: impl Fn(usize) -> bool,
        context: ProgramConstants,
    ) -> Self {
        assert_eq!(self.input_types(), params.types());
        let (program, inputs) = self.clone().into_parts();
        Self::admit(specialize(program, params, dynamic, context), inputs)
            .unwrap_or_else(|| unreachable!("specialization preserves admitted effect invariants"))
    }
}

impl OperatorProgram {
    /// Evaluate fixed-input arithmetic without embedding binding values in code.
    /// Use the returned bindings, including any reused fixed slots or new inputs.
    pub fn prepare_bindings(
        &self,
        params: &BoundParams,
        dynamic: impl Fn(usize) -> bool,
    ) -> (Self, BoundParams) {
        let (program, inputs, types) = self.clone().into_parts();
        let (program, types, params) = prepare_bindings(program, types, params, dynamic);
        let program = Self::admit(program, inputs, types)
            .unwrap_or_else(|| unreachable!("binding preparation preserves operator invariants"));
        (program, params)
    }

    /// Compile for the retained bindings and explicitly dynamic parameters.
    pub fn specialize(
        &self,
        params: &BoundParams,
        dynamic: impl Fn(usize) -> bool,
        context: ProgramConstants,
    ) -> Self {
        assert_eq!(self.parameter_types(), params.types());
        let (program, inputs, parameters) = self.clone().into_parts();
        Self::admit(
            specialize(program, params, dynamic, context),
            inputs,
            parameters,
        )
        .unwrap_or_else(|| unreachable!("specialization preserves admitted operator invariants"))
    }
}

fn prepare_bindings(
    mut program: BytecodeProgram,
    types: Box<[super::Type]>,
    params: &BoundParams,
    dynamic: impl Fn(usize) -> bool,
) -> (BytecodeProgram, Box<[super::Type]>, BoundParams) {
    assert_eq!(types.as_ref(), params.types());
    let mut types = types.into_vec();
    let mut values = params.iter_values().collect();
    let mut code = program.instructions.into_vec();
    let entry = super::optimize::prepare_bindings(
        &mut code,
        &mut program.value_operands,
        program.pixel_entry as usize,
        &mut program.layout,
        &mut types,
        &mut values,
        dynamic,
    );
    program.instructions = code.into();
    let params = BoundParams::bind_values(&types, values)
        .unwrap_or_else(|_| unreachable!("prepared values retain their primitive types"));
    (
        finish(program, Finish::PreparedPrefix(entry)),
        types.into(),
        params,
    )
}

fn specialize(
    mut program: BytecodeProgram,
    params: &BoundParams,
    dynamic: impl Fn(usize) -> bool,
    context: ProgramConstants,
) -> BytecodeProgram {
    use Instruction::*;
    let (control_params, control_context) = super::optimize::control_inputs(
        &program.instructions,
        &mut program.value_operands,
        &dynamic,
        context,
    );
    let fixed = |index: usize| {
        (control_params.contains(&index) && !dynamic(index)).then(|| &params.values()[index])
    };
    for op in &mut program.instructions {
        let replacement = match *op {
            LoadIntParam { dst, param, .. } => match fixed(param) {
                Some(Value::Int(value)) => Some(LoadIntConst { dst, value: *value }),
                _ => None,
            },
            LoadFloatParam { dst, param, .. } => match fixed(param) {
                Some(Value::Float(value)) => Some(LoadFloatConst {
                    dst,
                    bits: value.to_bits(),
                }),
                _ => None,
            },
            LoadBoolParam { dst, param, .. } => match fixed(param) {
                Some(Value::Bool(value)) => Some(LoadBoolConst { dst, value: *value }),
                _ => None,
            },
            LoadColorParam { dst, param, .. } => match fixed(param) {
                Some(Value::Color(value)) => Some(LoadColorConst { dst, value: *value }),
                _ => None,
            },
            EnumParamEqualConst {
                dst,
                param,
                constant,
                negate,
                ..
            } => match fixed(param) {
                Some(Value::Enum(value)) => Some(LoadBoolConst {
                    dst,
                    value: (*value == program.enums[constant]) != negate,
                }),
                _ => None,
            },
            ContextRead {
                dst: NumberSlot::Int(dst),
                read: ContextField::PixelCount,
            } => context
                .pixel_count
                .filter(|_| control_context.contains(&ContextField::PixelCount))
                .map(|value| LoadIntConst { dst, value }),
            ContextRead {
                dst: NumberSlot::Float(dst),
                read: ContextField::Duration,
            } => context
                .duration_seconds
                .filter(|_| control_context.contains(&ContextField::Duration))
                .map(|value| LoadFloatConst {
                    dst,
                    bits: value.to_bits(),
                }),
            _ => None,
        };
        if let Some(replacement) = replacement {
            *op = replacement;
        }
    }
    finish(program, Finish::Optimize)
}

enum Finish {
    Optimize,
    PreparedPrefix(u32),
}

pub(super) fn optimize_program(program: BytecodeProgram) -> BytecodeProgram {
    finish(program, Finish::Optimize)
}

fn finish(mut program: BytecodeProgram, finish: Finish) -> BytecodeProgram {
    let mut code = program.instructions.into_vec();
    let mut constants = program.array_constants.into_vec();
    let mut operands = program.value_operands.into_vec();
    let mut arrays = program.array_types.into_vec();
    let mut enums = program
        .enum_types
        .iter()
        .map(|ty| ty.ty().clone())
        .collect();
    program.pixel_entry = match finish {
        Finish::Optimize => {
            super::optimize::cleanup(
                &mut code,
                &mut constants,
                &mut operands,
                &mut program.layout,
                &mut arrays,
                &mut enums,
            );
            super::optimize::prepare_pixels(&mut code, &mut operands, &mut program.layout)
        }
        Finish::PreparedPrefix(entry) => {
            super::optimize::compact_storage(
                &mut code,
                &mut constants,
                &mut operands,
                &mut program.layout,
                &mut arrays,
                &mut enums,
            );
            entry
        }
    };
    if matches!(finish, Finish::PreparedPrefix(_)) {
        // Keep logical temporaries distinct through specialization and staging.
        // Early reuse hides single-assignment values from those analyses.
        super::optimize::fuse(&mut code, &mut operands, program.pixel_entry);
        super::optimize::reuse(
            &mut code,
            &mut operands,
            &mut program.layout,
            program.pixel_entry,
        );
    }
    program.loop_count = super::optimize::compact_loops(&mut code);
    program.instructions = code.into();
    program.array_constants = constants.into();
    program.value_operands = operands.into();
    program.array_types = arrays.into();
    program.enum_types = enums
        .into_iter()
        .map(|ty| {
            EnumSlotType::new(ty).unwrap_or_else(|| unreachable!("admitted enum register type"))
        })
        .collect();
    program.uses_pixel_context = program.reads_pixel_context();
    (program.array_capacity, program.array_width) = program
        .required_array_storage()
        .unwrap_or_else(|| unreachable!("specialization only removes array operations"));
    program
}
