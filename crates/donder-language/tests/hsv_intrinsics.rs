use donder_language::dsl::{
    Color, Identifier, OperatorRunContext, RuntimeError, SignalSampler, Value, VmWorkspace,
    compile_effects, compile_operators,
};
use donder_language::values::{SampleDuration, SampleTime};
use donder_runtime::dsl::bytecode::{ColorComponent, Instruction, SignalPixel};
use indexmap::IndexMap;

fn color([red, green, blue]: [u8; 3]) -> Color {
    Color { red, green, blue }
}

fn context() -> OperatorRunContext {
    OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    }
}

struct ConstantSignal(Color);
impl SignalSampler for ConstantSignal {
    fn sample_signal(
        &mut self,
        _input: usize,
        _sample_time: SampleTime,
        _pixel: SignalPixel<i32>,
        _frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        Ok(self.0)
    }
}

#[test]
fn hsv_intrinsics_require_exactly_one_color() {
    for name in ["hue", "saturation", "intensity"] {
        for args in ["", "0.5", "true", "#ffffff, #000000"] {
            let source = format!(
                "effect Invalid {{ color sample() {{ return rgb({name}({args}), 0.0, 0.0); }} }}"
            );
            assert!(compile_effects(&source).is_err(), "{source}");
        }
    }
}

#[test]
fn hsv_components_execute_and_hoist_uniform_color_reads() {
    let effect = compile_effects("effect Components { param color c; color sample() { return rgb(hue(c), saturation(c), intensity(c)); } }")
        .unwrap().remove(0).effect;
    for expected in [
        ColorComponent::Hue,
        ColorComponent::Saturation,
        ColorComponent::Intensity,
    ] {
        assert!(
            effect.sample_program().unwrap().instructions
                [..effect.sample_program().unwrap().pixel_entry as usize]
                .iter()
                .any(|op| {
                    matches!(op, Instruction::ColorComponent { op, .. } if *op == expected)
                })
        );
    }
    let mut workspace = VmWorkspace::default();
    for (input, output) in [
        ([255, 0, 0], [0, 255, 255]),
        ([255, 255, 0], [43, 255, 255]),
        ([0, 255, 0], [85, 255, 255]),
        ([0, 255, 255], [128, 255, 255]),
        ([0, 0, 255], [170, 255, 255]),
        ([255, 0, 255], [213, 255, 255]),
        ([128, 64, 96], [234, 128, 128]),
        ([128, 128, 128], [0, 0, 128]),
        ([0, 0, 0], [0, 0, 0]),
    ] {
        let params = effect
            .bind_params(&IndexMap::from([(
                Identifier::new("c".into()).unwrap(),
                Value::Color(color(input)),
            )]))
            .unwrap();
        assert_eq!(
            effect
                .sample_bound(&params, &context(), &mut workspace)
                .unwrap(),
            color(output)
        );
    }
}

#[test]
fn standard_hue_shift_preserves_value_and_saturation_and_wraps() {
    let operator = compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|op| op.name.as_str() == "HueShift")
    .unwrap();
    let mut workspace = VmWorkspace::default();
    for (input, shift, output) in [
        ([255, 0, 0], 1.0 / 6.0, [255, 255, 0]),
        ([255, 0, 0], -1.0 / 6.0, [255, 0, 255]),
        ([0, 255, 0], 1.0 / 3.0, [0, 0, 255]),
        ([0, 0, 255], 1.0 / 3.0, [255, 0, 0]),
        ([128, 64, 64], 0.5, [64, 128, 128]),
        ([1, 2, 3], 0.5, [3, 2, 1]),
        ([128, 128, 128], -0.25, [128, 128, 128]),
        ([0, 0, 0], 0.25, [0, 0, 0]),
        ([255, 255, 255], 0.25, [255, 255, 255]),
        ([17, 93, 201], 0.0, [17, 93, 201]),
        ([17, 93, 201], 1.0, [17, 93, 201]),
        ([17, 93, 201], -1.0, [17, 93, 201]),
    ] {
        let params = operator
            .bind_params(&IndexMap::from([(
                Identifier::new("shift".into()).unwrap(),
                Value::Float(shift),
            )]))
            .unwrap();
        let actual = operator
            .sample_bound(
                &params,
                &context(),
                &mut ConstantSignal(color(input)),
                &mut workspace,
            )
            .unwrap();
        assert_eq!(actual, color(output), "input={input:?}, shift={shift}");
    }
}
