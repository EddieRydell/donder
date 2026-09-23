use donder_language::dsl::{
    BoundParams, GeneratorBinding, GeneratorContext, GeneratorInput, Identifier, ParamDecl,
    RunContext, SpecializedGenerator, TargetValue, Value, VmWorkspace, compile_effects,
};
use donder_language::values::{SampleDuration, SampleTime};
use std::sync::Arc;

fn specialize(source: &str, inputs: &[GeneratorInput]) -> SpecializedGenerator {
    let mut compiled = compile_effects(source).unwrap();
    compiled
        .remove(0)
        .generator
        .unwrap()
        .specialize(
            inputs,
            &GeneratorContext {
                start_time: SampleTime::from_ticks(2_000_000),
                duration: SampleDuration::from_ticks(1_000_000),
                target: Arc::new(TargetValue { groups: Vec::new() }),
            },
            64,
        )
        .unwrap()
}

fn value(binding: &GeneratorBinding, params: &[Value], calculations: &[Vec<Value>]) -> Value {
    match binding {
        GeneratorBinding::Constant(value) => value.clone(),
        GeneratorBinding::Parameter(index) => params[usize::from(*index)].clone(),
        GeneratorBinding::Calculation { index, output } => {
            calculations[*index as usize][usize::from(*output)].clone()
        }
    }
}

fn evaluate(generator: &SpecializedGenerator, params: &[Value], time: u32) -> Vec<Vec<Value>> {
    let mut outputs = Vec::new();
    let mut remaining_iterations = donder_runtime::dsl::MAX_VM_INSTRUCTIONS_PER_INVOCATION;
    for calculation in &generator.calculations {
        let declarations = calculation
            .inputs
            .iter()
            .enumerate()
            .map(|(index, (ty, _))| ParamDecl {
                name: Identifier::new(format!("input_{index}")).unwrap(),
                ty: ty.clone(),
                fixed: false,
                default: None,
            })
            .collect::<Vec<_>>();
        let arguments = declarations
            .iter()
            .zip(calculation.inputs.iter())
            .map(|(declaration, (_, binding))| {
                (declaration.name.clone(), value(binding, params, &outputs))
            })
            .collect::<Vec<_>>();
        let bound = BoundParams::bind_pairs(&declarations, &arguments).unwrap();
        let Value::Array(result) = calculation
            .program
            .evaluate_value(
                &bound,
                &RunContext {
                    progress: time as f32 / 1_000_000.0,
                    time: SampleDuration::from_ticks(time),
                    duration: SampleDuration::from_ticks(1_000_000),
                    pixel_index: 0,
                    pixel_count: 0,
                    pixel_fraction: 0.0,
                },
                &mut VmWorkspace::default(),
                &mut remaining_iterations,
            )
            .unwrap()
        else {
            panic!("expected calculation outputs")
        };
        outputs.push(result.to_vec());
    }
    generator
        .children
        .iter()
        .map(|child| {
            child
                .params
                .iter()
                .map(|(_, binding)| value(binding, params, &outputs))
                .collect()
        })
        .collect()
}

#[test]
fn fixed_loops_capture_values_and_live_loop_carried_calculations() {
    let source = "effect Parent { fixed param int count = 3; param float level = 1.0; void generate() { float accumulated = 0.0; for (int i = 0; i < count; i = i + 1) { accumulated = accumulated + level; timeline.emit Child { start: i * 0.25, duration: 1.0, target: target, value: accumulated + i }; } } }";
    let generator = specialize(
        source,
        &[GeneratorInput::Fixed(Value::Int(3)), GeneratorInput::Live],
    );
    assert_eq!(generator.children.len(), 3);
    assert_eq!(generator.children[2].start_time.as_ticks(), 2_500_000);
    assert_eq!(
        evaluate(&generator, &[Value::Int(3), Value::Float(0.5)], 0),
        vec![
            vec![Value::Float(0.5)],
            vec![Value::Float(2.0)],
            vec![Value::Float(3.5)]
        ]
    );
    assert_eq!(
        evaluate(&generator, &[Value::Int(3), Value::Float(2.0)], 0),
        vec![
            vec![Value::Float(2.0)],
            vec![Value::Float(5.0)],
            vec![Value::Float(8.0)]
        ]
    );
}

#[test]
fn unautomated_inputs_fold_to_the_static_child_path() {
    let generator = specialize(
        "effect Parent { param float level = 1.0; void generate() { float value = level * 0.5; timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: value }; } }",
        &[GeneratorInput::Fixed(Value::Float(0.5))],
    );
    assert!(generator.calculations.is_empty());
    assert_eq!(
        generator.children[0].params[0].1,
        GeneratorBinding::Constant(Value::Float(0.25))
    );
}

#[test]
fn live_only_branches_loops_and_arrays_remain_vm_programs() {
    let generator = specialize(
        "effect Parent { param int count = 1; void generate() { float total = 0.0; for (int i = 0; i < count; i = i + 1) { total = total + i; } if (count > 0) { total = total + 1.0 / count; } else { total = 0.0; } timeline.emit Child { start: 0.0, duration: 1.0, target: target, values: [total, seconds()] }; } }",
        &[GeneratorInput::Live],
    );
    assert_eq!(generator.children.len(), 1);
    assert_eq!(
        evaluate(&generator, &[Value::Int(0)], 250_000),
        vec![vec![Value::Array(
            vec![Value::Float(0.0), Value::Float(0.25)].into()
        )]]
    );
    assert_eq!(
        evaluate(&generator, &[Value::Int(2)], 750_000),
        vec![vec![Value::Array(
            vec![Value::Float(1.5), Value::Float(0.75)].into()
        )]]
    );
}

#[test]
fn fixed_local_capture_does_not_follow_later_assignment() {
    let generator = specialize(
        "effect Parent { param float level = 1.0; void generate() { float multiplier = 2.0; float value = level * multiplier; multiplier = 9.0; timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: value }; } }",
        &[GeneratorInput::Live],
    );
    assert_eq!(
        evaluate(&generator, &[Value::Float(3.0)], 0),
        vec![vec![Value::Float(6.0)]]
    );
}

#[test]
fn lexical_shadows_in_pure_control_do_not_mutate_outer_bindings() {
    let generator = specialize(
        "effect Parent { param int count = 1; void generate() { float value = 2.0; for (int value = 0; value < count; value = value + 1) { float value = 5.0; value = value + 1.0; } timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: value }; } }",
        &[GeneratorInput::Live],
    );
    assert_eq!(
        evaluate(&generator, &[Value::Int(2)], 0),
        vec![vec![Value::Float(2.0)]]
    );
}
