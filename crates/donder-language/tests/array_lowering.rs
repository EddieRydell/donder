use donder_language::dsl::{Color, RunContext, VmWorkspace, compile_effects};
use donder_language::values::SampleDuration;
use donder_runtime::dsl::bytecode::Instruction;
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
    .remove(0)
    .effect;
    let scalar = compile_effects(
        "effect Scalar { color sample() {
        return rgb(pixel_fraction(), progress(), 0.25);
    } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert_eq!(array.bytecode, scalar.bytecode);
    assert_eq!(array.bytecode.layout.refs, 0);
    assert_eq!(array.bytecode.layout.ints, 0);
    assert!(array.bytecode.value_operands.is_empty());
}

#[test]
fn removing_unused_arrays_does_not_remove_errors_in_their_items() {
    let effect = compile_effects(
        "effect Error { color sample() {
        array<int> unused = [pixel_index(), 1 % 0];
        return #000000;
    } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert_eq!(effect.bytecode.array_capacity, 0);
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let result = effect.sample_bound(&params, &context(0.25), &mut VmWorkspace::default());
    assert!(
        result
            .unwrap_err()
            .message
            .contains("integer arithmetic overflow or division by zero")
    );
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
    .remove(0)
    .effect;
    assert_eq!(effect.bytecode.array_capacity, 0);
    assert!(!effect.bytecode.instructions.iter().any(|op| matches!(
        op,
        Instruction::MakeArray { .. } | Instruction::Len { .. } | Instruction::Index { .. }
    )));
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let mut vm = VmWorkspace::default();
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
            effect
                .sample_bound(&params, &context(progress), &mut vm)
                .unwrap(),
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
        .remove(0)
        .effect;
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        let mut vm = VmWorkspace::default();
        for (progress, red) in [
            (0.25, expected[0]),
            (0.75, expected[1]),
            (0.25, expected[0]),
        ] {
            assert_eq!(
                effect
                    .sample_bound(&params, &context(progress), &mut vm)
                    .unwrap(),
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
fn dynamic_indices_need_no_array_storage_and_preserve_index_errors() {
    for index in ["pixel_index()", "-1", "2"] {
        let effect = compile_effects(&format!(
            "effect Dynamic {{ color sample() {{
            array<float> values = [progress(), 0.75];
            return rgb(values[{index}], 0.0, 0.0);
        }} }}"
        ))
        .unwrap()
        .remove(0)
        .effect;
        assert_eq!(effect.bytecode.array_capacity, 0);
        assert!(
            effect
                .bytecode
                .instructions
                .iter()
                .any(|op| matches!(op, Instruction::Select { .. }))
        );
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        let mut vm = VmWorkspace::default();
        let result = effect.sample_bound(&params, &context(0.25), &mut vm);
        if index == "pixel_index()" {
            assert_eq!(result.unwrap().red, 64);
        } else {
            assert!(result.is_err());
        }
    }
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
            .unwrap().remove(0).effect;
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        let mut vm = VmWorkspace::default();
        for progress in [0.0, 0.5, 1.0, 0.0] {
            for (pixel, red) in [64, (progress * 255.0_f32).round() as u8, 191].into_iter().enumerate() {
                let mut ctx = context(progress);
                ctx.pixel_index = pixel as i32;
                assert_eq!(effect.sample_bound(&params, &ctx, &mut vm).unwrap(), Color { red, green: 0, blue: 0 }, "{body}");
            }
        }
    }
}
