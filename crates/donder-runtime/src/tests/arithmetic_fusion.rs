use super::evaluation::{SampleEvaluation, context};
use super::std;
use crate::dsl::VmWorkspace;
use donder_language::dsl::bytecode::Instruction;
use donder_language::dsl::{SampleDefinition, Value, compile_effects};
use donder_language::execution::SpatialContext;
use std::prelude::rust_2024::*;

const SPATIAL: SpatialContext = SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [1.0; 2],
};

#[test]
fn arithmetic_pairs_preserve_special_values_and_shared_results() {
    let effect = compile_effects(
        "effect F { param float gain = 0.3; param float bias = 0.2;
        color sample() { float x = pixel_fraction();
            return rgb(value_or(x * gain + bias, 0.7), value_or(x * 0.3 + bias, 0.6),
                value_or(smoothstep(0.0, 1.0, x * gain), 0.5));
        } }",
    )
    .unwrap()
    .remove(0);
    for gain in [
        0.3,
        -2.0,
        0.0,
        -0.0,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NAN,
        f32::from_bits(1),
    ] {
        let original = SampleDefinition::new(effect.sample_program().clone())
            .bind(vec![Value::Float(gain), Value::Float(0.2)])
            .unwrap();
        let (program, params) = effect
            .sample_program()
            .prepare_bindings(original.params(), |_| true);
        assert!(
            program
                .bytecode()
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FloatMultiplyAdd { .. }))
        );
        assert!(
            program
                .bytecode()
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FloatMultiplyAddConst { .. }))
        );
        assert!(
            program
                .bytecode()
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::FloatMultiplySmoothstep { .. }))
        );
        let optimized = SampleDefinition::new(program)
            .bind(params.iter_values().collect())
            .unwrap();
        for pixel in 0..67 {
            let context = context(67, pixel, pixel % 4);
            assert_eq!(
                original.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
                optimized.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
                "gain={gain} pixel={pixel}"
            );
        }
    }
    let effect = compile_effects(
        "effect F { param float gain = 0.3; color sample() {
        float product = pixel_fraction() * gain;
        return rgb(product + 0.2, product, product * 0.5);
    } }",
    )
    .unwrap()
    .remove(0);
    let original = effect.bind([]).unwrap();
    let (program, params) = effect
        .sample_program()
        .prepare_bindings(original.params(), |_| true);
    let optimized = SampleDefinition::new(program)
        .bind(params.iter_values().collect())
        .unwrap();
    for pixel in 0..67 {
        let context = context(67, pixel, 0);
        assert_eq!(
            original.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
            optimized.evaluate(&context, &SPATIAL, &mut VmWorkspace::default())
        );
    }
}

#[test]
fn register_reuse_preserves_uniform_gradient_staging() {
    let effect = compile_effects(
        "effect F { param gradient colors; color sample() {
        color base = colors[progress()]; float x = pixel_fraction();
        float y = abs(x - 0.5); float z = max(0.0, 1.0 - y * 2.0);
        return base * z;
    } }",
    )
    .unwrap()
    .remove(0);
    let params = effect
        .sample_program()
        .bind(vec![Value::Gradient(
            donder_language::values::Gradient { stops: vec![] }.into(),
        )])
        .unwrap();
    let (program, _) = effect.sample_program().prepare_bindings(&params, |_| true);
    let bytecode = program.bytecode();
    assert!(
        bytecode.instructions[..bytecode.pixel_entry as usize]
            .iter()
            .any(|op| matches!(op, Instruction::GradientParamSample { .. }))
    );
    assert!(
        !bytecode.instructions[bytecode.pixel_entry as usize..]
            .iter()
            .any(|op| matches!(
                op,
                Instruction::GradientParamSample { .. }
                    | Instruction::GradientParamColorScaled { .. }
            ))
    );
    assert!(bytecode.layout.floats < effect.sample_program().bytecode().layout.floats);
}
