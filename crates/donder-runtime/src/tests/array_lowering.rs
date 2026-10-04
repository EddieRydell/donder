use super::evaluation::SampleEvaluation;
use super::std;
use std::prelude::rust_2024::*;
const SPATIAL: donder_language::execution::SpatialContext =
    donder_language::execution::SpatialContext {
        position: [0.0; 2],
        min: [0.0; 2],
        max: [0.0; 2],
    };

use crate::dsl::BatchWorkspace;
use crate::dsl::RunContext;
use donder_language::dsl::Color;
use donder_language::dsl::bytecode::Instruction;
use donder_language::dsl::compile_effects;
use donder_language::values::SampleDuration;
use indexmap::IndexMap;

fn context(progress: f32) -> RunContext {
    RunContext {
        progress,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    }
}

#[test]
fn fixed_array_syntax_compiles_to_the_same_program_as_scalar_syntax() {
    let array = compile_effects(
        "effect Array { color sample() {
        array<float> values = [pixel_fraction(), progress(), 0.25];
        array<float> saved = values;
        values = [0.0];
        return rgb(saved[0], saved[1], saved[2]);
    } }",
    )
    .unwrap()
    .remove(0);
    let scalar = compile_effects(
        "effect Scalar { color sample() {
        return rgb(pixel_fraction(), progress(), 0.25);
    } }",
    )
    .unwrap()
    .remove(0);
    assert_eq!(array.sample_program(), scalar.sample_program());
    assert_eq!(array.sample_program().bytecode().layout.arrays, 0);
    assert_eq!(array.sample_program().bytecode().layout.ints, 0);
    assert!(array.sample_program().bytecode().value_operands.is_empty());
}

#[test]
fn copying_and_selecting_array_items_keep_integer_to_float_conversion() {
    use donder_language::dsl::Identifier;
    use donder_language::dsl::Value;
    let source = "effect Array { param int index = 0; color sample() {
        array<float> values = [progress(), pixel_index() + 1];
        float assigned = progress();
        assigned = pixel_index() + 1;
        return rgb(values[1], values[index], assigned);
    } }";
    let scalar = "effect Scalar { param int index = 0; color sample() {
        float selected = progress();
        if (index == 1) { selected = pixel_index() + 1; }
        return rgb(pixel_index() + 1, selected, pixel_index() + 1);
    } }";
    let array = compile_effects(source).unwrap().remove(0);
    let scalar = compile_effects(scalar).unwrap().remove(0);
    assert_eq!(array.sample_program().bytecode().array_capacity, 0);
    assert!(
        array
            .sample_program()
            .bytecode()
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::Select { .. }))
    );
    assert!(
        array
            .sample_program()
            .bytecode()
            .instructions
            .iter()
            .any(|op| matches!(op, Instruction::IntToFloat { .. }))
    );
    let mut workspace = BatchWorkspace::default();
    for index in [0, 1] {
        let params = [(Identifier::new("index".into()).unwrap(), Value::Int(index))];
        let array_params = array
            .bind(params.iter().map(|(name, value)| (name, value)))
            .unwrap();
        let scalar_params = scalar
            .bind(params.iter().map(|(name, value)| (name, value)))
            .unwrap();
        for progress in [0.0, 0.25, 0.75, 1.0] {
            let context = context(progress);
            let expected = scalar_params.evaluate(&context, &SPATIAL, &mut workspace);
            assert_eq!(
                array_params.evaluate(&context, &SPATIAL, &mut workspace),
                expected
            );
        }
    }
}

#[test]
fn reference_sampling_keeps_integer_and_float_index_semantics() {
    use donder_language::dsl::Identifier;
    use donder_language::dsl::Value;
    use donder_language::values::{Curve, CurvePoint};

    let effect = compile_effects(
        "effect Indexed {
            param array<curve> shapes;
            param int integer;
            param float fraction;
            color sample() {
                curve shape = shapes[0];
                return rgb(shape[integer], shape[fraction], 0.0);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let curve = Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 1.0,
                value: 1.0,
            },
        ],
    };
    let shapes = Value::Array(vec![Value::Curve(curve.clone().into())].into());
    let mut workspace = BatchWorkspace::default();
    for integer in [i32::MIN, -1, 0, 1, i32::MAX] {
        for fraction in [
            f32::NEG_INFINITY,
            -1.0,
            0.0,
            0.25,
            1.0,
            f32::INFINITY,
            f32::NAN,
        ] {
            let params = effect
                .bind(
                    [
                        (Identifier::new("shapes".into()).unwrap(), shapes.clone()),
                        (
                            Identifier::new("integer".into()).unwrap(),
                            Value::Int(integer),
                        ),
                        (
                            Identifier::new("fraction".into()).unwrap(),
                            Value::Float(fraction),
                        ),
                    ]
                    .iter()
                    .map(|(name, value)| (name, value)),
                )
                .unwrap();
            let sampled = params.evaluate(&context(0.0), &SPATIAL, &mut workspace);
            let channel = |position| {
                (donder_language::sampling::sample_curve(&curve, position) * 255.0).round() as u8
            };
            assert_eq!(
                sampled,
                if fraction.is_nan() {
                    Color::BLACK
                } else {
                    Color {
                        red: channel(integer as f32),
                        green: channel(fraction),
                        blue: 0,
                    }
                }
            );
        }
    }
}

#[test]
fn unused_arrays_with_total_items_need_no_storage() {
    let effect = compile_effects(
        "effect Error { color sample() {
        array<int> unused = [pixel_index(), 1 % 0];
        return #000000;
    } }",
    )
    .unwrap()
    .remove(0);
    assert_eq!(effect.sample_program().bytecode().array_capacity, 0);
    let params = effect.bind(&IndexMap::new()).unwrap();
    let result = params.evaluate(&context(0.25), &SPATIAL, &mut BatchWorkspace::default());
    assert_eq!(result, Color::BLACK);
}

#[test]
fn fixed_indices_and_aliases_need_no_calculated_array_storage() {
    let effect = compile_effects(
        "effect Fixed { color sample() {
        array<float> values = [progress(), progress() + 0.25];
        array<float> saved = values;
        values = [0.0];
        return rgb(saved[0], saved[1], len(saved) * 0.25);
    } }",
    )
    .unwrap()
    .remove(0);
    assert_eq!(effect.sample_program().bytecode().array_capacity, 0);
    assert!(
        !effect
            .sample_program()
            .bytecode()
            .instructions
            .iter()
            .any(|op| matches!(
                op,
                Instruction::MakeArray { .. } | Instruction::Len { .. } | Instruction::Index { .. }
            ))
    );
    let params = effect.bind(&IndexMap::new()).unwrap();
    let mut vm = BatchWorkspace::default();
    for (progress, expected) in [
        (
            0.25,
            Color {
                red: 64,
                green: 128,
                blue: 128,
            },
        ),
        (
            0.0,
            Color {
                red: 0,
                green: 64,
                blue: 128,
            },
        ),
    ] {
        assert_eq!(
            params.evaluate(&context(progress), &SPATIAL, &mut vm),
            expected
        );
    }
}

#[test]
fn mutable_values_branches_and_backedges_preserve_array_snapshots() {
    for (body, expected) in [
        (
            "float value = progress(); array<float> saved = [value]; value = 0.9;
          return rgb(saved[0], 0.0, 0.0);",
            [64, 191],
        ),
        (
            "array<float> saved = [progress()];
          if (progress() > 0.5) { saved = [0.1]; }
          return rgb(saved[0], 0.0, 0.0);",
            [64, 26],
        ),
        (
            "array<float> saved = [progress()];
          if (progress() > 0.5) { saved = [0.1]; } else { saved = [0.9]; }
          return rgb(saved[0], 0.0, 0.0);",
            [230, 26],
        ),
        (
            "array<float> saved = [0.0];
          for (int i = 0; i < 3; i = i + 1) {
              array<float> current = [progress() + i * 0.1];
              if (i == 0) { saved = current; }
          }
          return rgb(saved[0], 0.0, 0.0);",
            [64, 191],
        ),
        (
            "array<float> saved = [progress()];
          if (progress() > 0.5) { array<float> saved = [0.9]; }
          return rgb(saved[0], 0.0, 0.0);",
            [64, 191],
        ),
    ] {
        let effect = compile_effects(&format!(
            "effect Snapshot {{ color sample() {{ {body} }} }}"
        ))
        .unwrap()
        .remove(0);
        let params = effect.bind(&IndexMap::new()).unwrap();
        let mut vm = BatchWorkspace::default();
        for (progress, red) in [
            (0.25, expected[0]),
            (0.75, expected[1]),
            (0.25, expected[0]),
        ] {
            assert_eq!(
                params.evaluate(&context(progress), &SPATIAL, &mut vm),
                Color {
                    red,
                    green: 0,
                    blue: 0
                },
                "{body}"
            );
        }
    }
}

#[test]
fn dynamic_indices_clamp_without_array_storage_and_empty_arrays_default() {
    for (index, expected_red) in [("pixel_index()", 64), ("-1", 64), ("2", 191)] {
        let effect = compile_effects(&format!(
            "effect Dynamic {{ color sample() {{
            array<float> values = [progress(), 0.75];
            return rgb(values[{index}], 0.0, 0.0);
        }} }}"
        ))
        .unwrap()
        .remove(0);
        assert_eq!(effect.sample_program().bytecode().array_capacity, 0);
        assert!(
            effect
                .sample_program()
                .bytecode()
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::Select { .. }))
        );
        let params = effect.bind(&IndexMap::new()).unwrap();
        let mut vm = BatchWorkspace::default();
        assert_eq!(
            params.evaluate(&context(0.25), &SPATIAL, &mut vm).red,
            expected_red
        );
    }

    let effect = compile_effects(
        "effect Empty { color sample() {
            array<float> values = [progress()];
            values = [];
            return rgb(values[pixel_index()], 0.0, 0.0);
        } }",
    )
    .unwrap()
    .remove(0);
    let params = effect.bind(&IndexMap::new()).unwrap();
    let mut vm = BatchWorkspace::default();
    assert_eq!(params.evaluate(&context(0.25), &SPATIAL, &mut vm).red, 0);

    let effect = compile_effects(
        "effect EmptyParameter {
            param array<float> values = [];
            color sample() { return rgb(values[pixel_index()], 0.0, 0.0); }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect.bind(&IndexMap::new()).unwrap();
    assert_eq!(params.evaluate(&context(0.25), &SPATIAL, &mut vm).red, 0);
}

#[test]
fn dynamic_selection_preserves_aliases_and_typed_values() {
    for body in [
        "array<float> values = [0.25, progress(), 0.75];
         array<float> saved = values; values = [0.0];
         return rgb(saved[pixel_index()], 0.0, 0.0);",
        "array<color> values = [rgb(0.25, 0.0, 0.0), rgb(progress(), 0.0, 0.0), rgb(0.75, 0.0, 0.0)];
         return values[pixel_index()];",
        "array<array<float>> values = [[0.25], [progress()], [0.75]];
         return rgb(values[pixel_index()][0], 0.0, 0.0);",
    ] {
        let effect = compile_effects(&format!("effect Select {{ color sample() {{ {body} }} }}"))
            .unwrap().remove(0);
        let params = effect.bind(&IndexMap::new()).unwrap();
        let mut vm = BatchWorkspace::default();
        for progress in [0.0, 0.5, 1.0, 0.0] {
            for (pixel, red) in [64, (progress * 255.0_f32).round() as u8, 191].into_iter().enumerate() {
                let mut ctx = context(progress);
                ctx.pixel_index = pixel as i32;
                assert_eq!(params.evaluate(&ctx, &SPATIAL, &mut vm), Color { red, green: 0, blue: 0 }, "{body}");
            }
        }
    }
}
