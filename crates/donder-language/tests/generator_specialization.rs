const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use donder_language::dsl::{
    DslBindCache, GeneratorBinding, GeneratorContext, GeneratorInput, Identifier, RunContext,
    SpecializedGenerator, TargetValue, Value, VmWorkspace, compile_effects,
};
use donder_language::values::Marks;
use donder_language::values::{SampleDuration, SampleTime};
use std::sync::Arc;

#[allow(dead_code)]
#[path = "support/playback.rs"]
mod playback;

fn specialize(source: &str, inputs: &[GeneratorInput]) -> SpecializedGenerator {
    let mut compiled = compile_effects(source).unwrap();
    compiled
        .remove(0)
        .effect
        .generator()
        .unwrap()
        .clone()
        .bind(inputs)
        .unwrap()
        .specialize(&GeneratorContext {
            start_time: SampleTime::from_ticks(2_000_000),
            duration: SampleDuration::from_ticks(1_000_000),
            target: Arc::new(TargetValue { groups: Vec::new() }),
        })
}

fn value(binding: &GeneratorBinding, params: &[Value], calculations: &[Vec<Value>]) -> Value {
    match binding {
        GeneratorBinding::Constant(value) => value.clone(),
        GeneratorBinding::Parameter(index) => params[usize::from(*index)].clone(),
        GeneratorBinding::Calculation { index, output } => {
            calculations[*index][usize::from(*output)].clone()
        }
    }
}

fn evaluate(generator: &SpecializedGenerator, params: &[Value], time: u32) -> Vec<Vec<Value>> {
    let mut outputs = Vec::new();
    for calculation in &generator.calculations {
        let arguments = calculation
            .inputs
            .iter()
            .map(|binding| value(binding, params, &outputs))
            .collect();
        let result = calculation
            .program
            .bind(arguments, &mut DslBindCache::default())
            .unwrap()
            .evaluate(
                &RunContext {
                    progress: time as f32 / 1_000_000.0,
                    time: SampleDuration::from_ticks(time),
                    duration: SampleDuration::from_ticks(1_000_000),
                    pixel_index: 0,
                    pixel_count: 0,
                    pixel_fraction: 0.0,
                },
                &mut VmWorkspace::default(),
            );
        outputs.push(result);
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
fn fixed_control_keeps_timing_separate_from_live_rendering_values() {
    let source = "effect Parent {
        fixed param bool choose = true;
        param float level = 0.5;
        void generate() {
            float start = 0.0;
            float brightness = level;
            if (choose) { start = 0.25; brightness = level * 2.0; }
            else { start = 0.5; brightness = level * 3.0; }
            timeline.emit Child { start: start, duration: 0.25, target: target, value: brightness };
        }
    }";
    for (choose, start, scale) in [(true, 2_250_000, 2.0), (false, 2_500_000, 3.0)] {
        let generator = specialize(
            source,
            &[
                GeneratorInput::Fixed(Value::Bool(choose)),
                GeneratorInput::Live,
            ],
        );
        assert_eq!(generator.children.len(), 1);
        assert_eq!(
            generator.children[0].start_time,
            SampleTime::from_ticks(start)
        );
        for level in [0.0, 0.25, 1.0] {
            assert_eq!(
                evaluate(&generator, &[Value::Bool(choose), Value::Float(level)], 0),
                vec![vec![Value::Float(level * scale)]]
            );
        }
    }
}

#[test]
fn fixed_loop_outputs_remain_available_to_child_timing() {
    for loop_body in [
        "for (int i = 0; i < 2; i = i + 1) { start = start + 0.125; brightness = brightness + level; }",
        "for (int i in range(2, 2)) { start = start + 0.125; brightness = brightness + level; }",
        "for (int i in beats) { start = start + 0.125; brightness = brightness + level; }",
    ] {
        let source = format!("effect Parent {{
            fixed param marks beats;
            param float level = 0.5;
            void generate() {{
                float start = 0.0;
                float brightness = level;
                {loop_body}
                timeline.emit Child {{ start: start, duration: 0.25, target: target, value: brightness }};
            }}
        }}");
        let marks = Value::Marks(Arc::new(Marks {
            marks: vec![
                SampleDuration::from_ticks(0),
                SampleDuration::from_ticks(500_000),
            ],
        }));
        let generator = specialize(
            &source,
            &[GeneratorInput::Fixed(marks.clone()), GeneratorInput::Live],
        );
        assert_eq!(generator.children.len(), 1, "{loop_body}");
        assert_eq!(
            generator.children[0].start_time,
            SampleTime::from_ticks(2_250_000),
            "{loop_body}"
        );
        for level in [0.0, 0.25, 1.0] {
            assert_eq!(
                evaluate(&generator, &[marks.clone(), Value::Float(level)], 0),
                vec![vec![Value::Float(level * 3.0)]],
                "{loop_body}"
            );
        }
    }
}

#[test]
fn implicit_generator_mark_time_is_independent_of_specialization() {
    let source = "effect Parent { param marks marks; void generate() {
        timeline.emit Child { start: 0.0, duration: 1.0, target: target,
            previous: mark_prev(marks), previous_index: mark_prev_index(marks),
            next_index: mark_next_index(marks), elapsed: mark_elapsed(marks), phase: mark_phase(marks)
        };
    } }";
    let marks = Value::Marks(Arc::new(Marks {
        marks: vec![
            SampleDuration::from_ticks(0),
            SampleDuration::from_ticks(1_000_000),
        ],
    }));
    let expected = vec![vec![
        Value::Float(0.0),
        Value::Int(0),
        Value::Int(1),
        Value::Float(0.0),
        Value::Float(0.0),
    ]];
    let fixed = specialize(source, &[GeneratorInput::Fixed(marks.clone())]);
    let live = specialize(source, &[GeneratorInput::Live]);
    assert!(fixed.calculations.is_empty());
    assert_eq!(live.calculations.len(), 5);
    assert!(
        live.calculations
            .iter()
            .all(|calculation| !calculation.program.uses_time())
    );
    for time in [0, 500_000, 1_000_000, 0] {
        assert_eq!(
            evaluate(&fixed, core::slice::from_ref(&marks), time),
            expected
        );
        assert_eq!(
            evaluate(&live, core::slice::from_ref(&marks), time),
            expected
        );
    }

    // An explicit clock remains live, even when every parameter is fixed.
    let explicit = specialize(
        &source.replace("(marks)", "(marks, seconds())"),
        &[GeneratorInput::Fixed(marks.clone())],
    );
    assert_eq!(explicit.calculations.len(), 5);
    assert!(
        explicit
            .calculations
            .iter()
            .all(|calculation| calculation.program.uses_time())
    );
    let mut at_half_second = expected.clone();
    at_half_second[0][3] = Value::Float(0.5);
    at_half_second[0][4] = Value::Float(0.5);
    assert_eq!(
        evaluate(&explicit, core::slice::from_ref(&marks), 500_000),
        at_half_second
    );

    // Raw calculation admission must also recognize implicit clock reads;
    // not every accepted program originated in this generator compiler.
    use donder_runtime::{CalculationProgram, Instruction, MarkOp};
    let context = RunContext {
        progress: 0.5,
        time: SampleDuration::from_ticks(500_000),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 0,
        pixel_fraction: 0.0,
    };
    let mut implicit_values = Vec::new();
    for calculation in &live.calculations {
        let (mut code, inputs, outputs) = calculation.program.clone().into_parts();
        for instruction in &mut code.instructions {
            if let Instruction::Mark { op, .. } = instruction {
                match op {
                    MarkOp::Prev { seconds, .. }
                    | MarkOp::PrevIndex { seconds, .. }
                    | MarkOp::NextIndex { seconds, .. }
                    | MarkOp::Elapsed { seconds, .. }
                    | MarkOp::Phase { seconds, .. } => *seconds = None,
                    _ => panic!("expected a time-based mark query"),
                }
            }
        }
        let program = CalculationProgram::new(code, inputs, outputs).unwrap();
        assert!(program.uses_time());
        implicit_values.extend(
            program
                .bind(vec![marks.clone()], &mut DslBindCache::default())
                .unwrap()
                .evaluate(&context, &mut VmWorkspace::default()),
        );
    }
    assert_eq!(vec![implicit_values], at_half_second);
}

#[test]
fn repeated_calculation_outputs_share_the_result_array() {
    use donder_runtime::{CalculationProgram, Instruction, PoolSpan};

    let generator = specialize(
        "effect Parent { param float seed; void generate() {
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: [seed + seconds()] };
        } }", &[GeneratorInput::Live],
    );
    let (mut program, inputs, original_outputs) =
        generator.calculations[0].program.clone().into_parts();
    let slot = program.calculation_outputs().unwrap()[0];
    let outputs = vec![original_outputs[0].clone(); 64].into_boxed_slice();
    let mut operands = program.value_operands.into_vec();
    let start = operands.len() as u32;
    operands.extend(core::iter::repeat_n(slot, outputs.len()));
    program.value_operands = operands.into();
    *program.instructions.last_mut().unwrap() = Instruction::ReturnValues(PoolSpan {
        start,
        len: outputs.len() as u32,
    });
    let program = CalculationProgram::new(program, inputs, outputs).unwrap();
    let invocation = program
        .bind(vec![Value::Float(0.25)], &mut DslBindCache::default())
        .unwrap();
    let mut workspace = VmWorkspace::for_program(program.bytecode());
    for time in [0, 500_000, 0] {
        let context = RunContext {
            progress: time as f32 / 1_000_000.0,
            time: SampleDuration::from_ticks(time),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 0,
            pixel_fraction: 0.0,
        };
        let result = invocation.evaluate(&context, &mut workspace);
        let expected = Value::Array(vec![Value::Float(0.25 + time as f32 / 1_000_000.0)].into());
        assert!(result.iter().all(|value| value == &expected));
    }
}

#[test]
fn retained_array_copies_preserve_sharing_across_nested_calculations() {
    let source = "effect Parent { param float seed; void generate() {
            array<float> item = [seed + seconds()];
            array<array<float>> repeated = [item, item, item, item, item, item, item, item];
            array<array<array<float>>> wrapped = [repeated, repeated, repeated, repeated];
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: wrapped };
        } }";
    let generator = specialize(source, &[GeneratorInput::Live]);
    assert_eq!(generator.calculations.len(), 3);
    for (index, calculation) in generator.calculations.iter().enumerate() {
        let source = if index == 0 {
            GeneratorBinding::Parameter(0)
        } else {
            GeneratorBinding::Calculation {
                index: index - 1,
                output: 0,
            }
        };
        assert_eq!(calculation.inputs.as_ref(), &[source]);
    }
    let sample = compile_effects("effect Child { param array<array<array<float>>> value; color sample() { return rgb(value[0][0][0], value[1][3][0], value[3][7][0]); } }")
        .unwrap().remove(0).effect;
    let parent = compile_effects(source).unwrap().remove(0).effect;
    // Constant-valued automation keeps the seed live through all three retained
    // calculation banks, just as an authored automated generator parameter does.
    let invocation = playback::generator(
        &parent,
        &sample,
        vec![Value::Float(0.25)],
        vec![donder_runtime::PreparedAutomation {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            curve: donder_runtime::Curve {
                points: vec![donder_runtime::CurvePoint {
                    position: 0.0,
                    value: 0.25,
                }],
            }
            .into(),
            mapping: donder_runtime::AutomationMapping::Float { min: 0.0, max: 1.0 },
            param_index: 0,
        }]
        .into(),
    );
    let mut playback = playback::generated(2, &invocation).into_playback();
    for time in [0, 500_000, 200_000, 0] {
        let channel = ((0.25 + time as f32 / 1_000_000.0) * 255.0).round() as u8;
        let output = playback.evaluate(SampleTime::from_ticks(time));
        assert_eq!(
            output.colors(),
            &[donder_runtime::Color {
                red: channel,
                green: channel,
                blue: channel,
            }; 2]
        );
    }
}

#[test]
fn random_seed_lowering_matches_fixed_and_live_evaluation() {
    use donder_runtime::deterministic_random;

    let source = "effect Parent { param float seed; void generate() {
        timeline.emit Child {
            start: 0.0, duration: 1.0, target: target,
            empty: rand(), single: srand(seed), multiple: rand(seed, 0.25, seed, -0.0, 5)
        };
    } }";
    let live = specialize(source, &[GeneratorInput::Live]);
    let sample = compile_effects(
        "effect Random { param float seed; color sample() {
            return rgb(rand(), srand(seed), rand(seed, 0.25, seed, -0.0, 5));
        } }",
    )
    .unwrap()
    .remove(0)
    .effect;
    assert!(
        sample
            .sample_program()
            .unwrap()
            .bytecode()
            .value_operands
            .is_empty()
    );
    let context = RunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut workspace = VmWorkspace::default();
    for seed in [
        f32::NEG_INFINITY,
        -f32::MAX,
        -7.25,
        -0.0,
        0.0,
        f32::MIN_POSITIVE,
        0.75,
        16_777_216.0,
        f32::MAX,
        f32::INFINITY,
        f32::NAN,
        f32::from_bits(0x7f80_0001),
        f32::from_bits(0xffc0_1234),
    ] {
        let expected = [
            deterministic_random([].into_iter()),
            deterministic_random([seed].into_iter()),
            deterministic_random([seed, 0.25, seed, -0.0, 5.0].into_iter()),
        ];
        let expected_values = vec![expected.into_iter().map(Value::Float).collect::<Vec<_>>()];
        let params = [Value::Float(seed)];
        let fixed = specialize(source, &[GeneratorInput::Fixed(params[0].clone())]);
        assert_eq!(
            evaluate(&fixed, &params, 0),
            expected_values,
            "fixed seed {seed:?}"
        );
        assert_eq!(
            evaluate(&live, &params, 0),
            expected_values,
            "live seed {seed:?}"
        );

        let bound = sample
            .bind(
                [(Identifier::new("seed".into()).unwrap(), params[0].clone())]
                    .iter()
                    .map(|(name, value)| (name, value)),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        let [red, green, blue] = expected.map(|value| (value * 255.0).round() as u8);
        assert_eq!(
            bound.evaluate(&context, &SPATIAL, &mut workspace),
            donder_language::dsl::Color { red, green, blue }
        );
    }
}

#[test]
fn fixed_loops_capture_values_and_live_loop_carried_calculations() {
    let source = "effect Parent { fixed param int count = 3; param float level = 1.0; void generate() { float accumulated = 0.0; for (int i in range(count, 10000)) { accumulated = accumulated + level; timeline.emit Child { start: i * 0.25, duration: 1.0, target: target, value: accumulated + i }; } } }";
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
fn fixed_range_can_emit_more_than_the_old_global_child_limit() {
    let generator = specialize(
        "effect Parent { fixed param int count = 5000; void generate() { for (int i in range(count, 10000)) { timeline.emit Child { start: 0.0, duration: 1.0, target: target }; } } }",
        &[GeneratorInput::Fixed(Value::Int(5_000))],
    );
    assert_eq!(generator.children.len(), 5_000);
}

#[test]
fn marks_collection_emission_has_no_shared_iteration_budget() {
    let generator = specialize(
        "effect Parent { fixed param marks beats; void generate() { for (int mark in beats) { timeline.emit Child { start: 0.0, duration: 1.0, target: target }; } } }",
        &[GeneratorInput::Fixed(Value::Marks(Arc::new(Marks {
            marks: vec![SampleDuration::from_ticks(0); 10_001],
        })))],
    );
    assert_eq!(generator.children.len(), 10_001);
}

#[test]
fn invalid_fixed_emission_timing_omits_only_that_child() {
    let source = "effect Parent {
        fixed param float early = 0.0;
        fixed param float empty = 0.0;
        void generate() {
            timeline.emit Child { start: early, duration: 1.0, target: target };
            timeline.emit Child { start: 0.0, duration: empty, target: target };
            timeline.emit Child { start: 0.5, duration: 1.0, target: target };
        }
    }";
    let compiled = compile_effects(source).unwrap().remove(0);
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(2_000_000),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(TargetValue { groups: Vec::new() }),
    };
    let specialized = compiled
        .effect
        .generator()
        .unwrap()
        .clone()
        .bind(&[
            GeneratorInput::Fixed(Value::Float(-3.0)),
            GeneratorInput::Fixed(Value::Float(0.0)),
        ])
        .unwrap()
        .specialize(&context);
    assert_eq!(specialized.children.len(), 1);
    assert_eq!(specialized.children[0].start_time.as_ticks(), 2_500_000);
}

#[test]
fn specialization_obeys_portable_child_clock_rules() {
    let compiled = compile_effects(
        "effect Timing {
            fixed param float offset = 0.0;
            fixed param float length = 1.0;
            void generate() {
                timeline.emit Child { target: target, duration: length, start: offset };
            }
        }",
    )
    .unwrap()
    .remove(0);
    let generator = compiled.effect.generator().unwrap().clone();
    let cases = [
        f32::NAN,
        f32::NEG_INFINITY,
        -10.0,
        -0.000_001,
        0.0,
        0.000_000_1,
        0.000_001,
        0.25,
        1.0,
        u32::MAX as f32 / 1_000_000.0,
        f32::INFINITY,
    ];
    for start in [0, 2_000_000, u32::MAX] {
        let context = GeneratorContext {
            start_time: SampleTime::from_ticks(start),
            duration: SampleDuration::from_ticks(1_000_000),
            target: Arc::new(TargetValue { groups: Vec::new() }),
        };
        for offset in cases {
            for length in cases {
                let specialized = generator
                    .bind(&[
                        GeneratorInput::Fixed(Value::Float(offset)),
                        GeneratorInput::Fixed(Value::Float(length)),
                    ])
                    .unwrap()
                    .specialize(&context);
                let expected = donder_language::values::sample_time_with_seconds_offset(
                    context.start_time,
                    offset,
                )
                .ok()
                .zip(donder_language::values::sample_duration_from_seconds_f32(length).ok())
                .filter(|(_, duration)| duration.as_ticks() != 0)
                .into_iter()
                .collect::<Vec<_>>();
                let retained = specialized
                    .children
                    .iter()
                    .map(|child| (child.start_time, child.duration))
                    .collect::<Vec<_>>();
                assert_eq!(
                    expected, retained,
                    "parent={start}, offset={offset}, length={length}"
                );
            }
        }
    }
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
        "effect Parent { param int count = 1; void generate() { float total = 0.0; for (int i in range(count, 10000)) { total = total + i; } if (count > 0) { total = total + 1.0 / count; } else { total = 0.0; } timeline.emit Child { start: 0.0, duration: 1.0, target: target, values: [total, seconds()] }; } }",
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
fn calculation_admission_checks_code_and_signature_together() {
    use donder_runtime::{CalculationProgram, Instruction, Type};
    let generator = specialize(
        "effect Parent { param float level = 1.0; void generate() {
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: level + seconds() };
        } }",
        &[GeneratorInput::Live],
    );
    let (bytecode, inputs, outputs) = generator.calculations[0].program.clone().into_parts();
    assert!(CalculationProgram::new(bytecode.clone(), inputs.clone(), outputs.clone()).is_some());

    let mut invalid_outputs = bytecode.clone();
    let Some(Instruction::ReturnValues(span)) = invalid_outputs.instructions.last_mut() else {
        panic!("calculation must return declared output slots");
    };
    span.start = u32::MAX;
    assert!(CalculationProgram::new(invalid_outputs, inputs.clone(), outputs.clone()).is_none());

    let mut early_return = bytecode.clone();
    let mut instructions = early_return.instructions.into_vec();
    instructions.insert(0, instructions.last().unwrap().clone());
    early_return.instructions = instructions.into();
    assert!(CalculationProgram::new(early_return, inputs.clone(), outputs.clone()).is_none());

    let mut invalid_code = bytecode.clone();
    let parameter_read = invalid_code
        .instructions
        .iter_mut()
        .find_map(|instruction| match instruction {
            Instruction::LoadFloatParam { param, .. } => Some(param),
            _ => None,
        })
        .unwrap();
    *parameter_read = usize::MAX;
    assert!(CalculationProgram::new(invalid_code, inputs.clone(), outputs.clone()).is_none());

    let mut wrong_inputs = inputs.clone();
    wrong_inputs[0] = Type::Bool;
    assert!(CalculationProgram::new(bytecode.clone(), wrong_inputs, outputs).is_none());
    assert!(
        CalculationProgram::new(bytecode.clone(), inputs.clone(), Box::new([Type::Bool])).is_none()
    );
    assert!(CalculationProgram::new(bytecode, inputs, Box::new([])).is_none());
}

#[test]
fn calculation_binding_checks_its_entire_signature_before_execution() {
    use donder_runtime::{CalculationProgram, Type};
    let generator = specialize(
        "effect Parent { param float level = 1.0; void generate() {
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: level + seconds() };
        } }",
        &[GeneratorInput::Live],
    );
    let (bytecode, inputs, outputs) = generator.calculations[0].program.clone().into_parts();
    let mut inputs = inputs.into_vec();
    let on = Identifier::new("on".into()).unwrap();
    inputs.push(Type::array(Type::array(Type::Enum(vec![on.clone()]))));
    // Even an input not read by this bytecode belongs to its declared contract.
    let program = CalculationProgram::new(bytecode, inputs.into(), outputs).unwrap();
    let nested = |value| Value::Array(Arc::from([Value::Array(Arc::from([value]))]));
    let mut cache = DslBindCache::default();
    let valid = vec![Value::Int(7), nested(Value::Enum(on))];
    let bound = program.bind(valid.clone(), &mut cache).unwrap();
    let context = RunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 0,
        pixel_fraction: 0.0,
    };
    let mut workspace = VmWorkspace::default();
    assert_eq!(
        bound.evaluate(&context, &mut workspace),
        vec![Value::Float(7.0)]
    );
    for invalid in [
        vec![],
        vec![valid[0].clone()],
        vec![valid[0].clone(), valid[1].clone(), Value::Int(1)],
        vec![Value::Bool(true), valid[1].clone()],
        vec![valid[0].clone(), nested(Value::Bool(true))],
        vec![
            valid[0].clone(),
            nested(Value::Enum(Identifier::new("invalid".into()).unwrap())),
        ],
    ] {
        assert!(program.bind(invalid, &mut cache).is_err());
    }
    assert!(
        program
            .bind(
                vec![Value::Float(2.0), Value::Array(Arc::from([]))],
                &mut cache
            )
            .is_ok()
    );
    // Failed admissions cannot alter an already-bound invocation.
    assert_eq!(
        bound.evaluate(&context, &mut workspace),
        vec![Value::Float(7.0)]
    );
}

#[test]
fn calculation_outputs_keep_their_types_without_a_tuple_array() {
    use donder_runtime::{Instruction, Type};
    let generator = specialize(
        "effect Parent { param float level = 1.0; void generate() {
            float x = level;
            int n = 1;
            bool flag = false;
            if (level > 0.0) { x = x + 2.0; n = 3; flag = true; }
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: x, index: n, selected: flag };
        } }",
        &[GeneratorInput::Live],
    );
    assert_eq!(generator.calculations.len(), 1);
    let (program, _, outputs) = generator.calculations[0].program.clone().into_parts();
    assert_eq!(outputs.as_ref(), &[Type::Float, Type::Int, Type::Bool]);
    assert_eq!(program.array_capacity, 0);
    assert_eq!(program.array_width, 0);
    assert!(program.array_types.is_empty());
    assert!(
        matches!(program.instructions.last(), Some(Instruction::ReturnValues(span)) if span.len == 3)
    );
    assert_eq!(
        evaluate(&generator, &[Value::Float(1.0)], 0),
        vec![vec![Value::Float(3.0), Value::Int(3), Value::Bool(true)]]
    );
    assert_eq!(
        evaluate(&generator, &[Value::Float(-1.0)], 0),
        vec![vec![Value::Float(-1.0), Value::Int(1), Value::Bool(false)]]
    );
}

#[test]
fn typed_builtin_operand_addresses_are_checked_at_admission() {
    use donder_runtime::{
        CalculationProgram, Instruction, IntSlot, MarkOp, NumberSlot, TargetItemsOp,
    };
    let generator = specialize(
        "effect Parent { fixed param marks beats; param float query = 0.0;
            void generate() {
                timeline.emit Child { start: 0.0, duration: 1.0, target: target,
                    value: mark_at(beats, query, 0.0) + count(sections(target, 3)) };
            }
        }",
        &[
            GeneratorInput::Fixed(Value::Marks(Arc::new(Marks { marks: vec![] }))),
            GeneratorInput::Live,
        ],
    );
    let (bytecode, inputs, outputs) = generator.calculations[0].program.clone().into_parts();
    assert!(CalculationProgram::new(bytecode.clone(), inputs.clone(), outputs.clone()).is_some());
    for mark_operand in [true, false] {
        let mut invalid = bytecode.clone();
        let operand = invalid
            .instructions
            .iter_mut()
            .find_map(|instruction| match instruction {
                Instruction::Mark {
                    op: MarkOp::At { index, .. },
                    ..
                } if mark_operand => Some(index),
                Instruction::TargetItems {
                    op: TargetItemsOp::Sections { width, .. },
                    ..
                } if !mark_operand => Some(width),
                _ => None,
            })
            .unwrap();
        *operand = NumberSlot::Int(IntSlot(u32::MAX));
        assert!(CalculationProgram::new(invalid, inputs.clone(), outputs.clone()).is_none());
    }
}

#[test]
fn lexical_shadows_in_pure_control_do_not_mutate_outer_bindings() {
    let generator = specialize(
        "effect Parent { param int count = 1; void generate() { float value = 2.0; for (int value in range(count, 10000)) { float value = 5.0; value = value + 1.0; } timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: value }; } }",
        &[GeneratorInput::Live],
    );
    assert_eq!(
        evaluate(&generator, &[Value::Int(2)], 0),
        vec![vec![Value::Float(2.0)]]
    );
}

#[test]
fn emission_scopes_resolve_shadows_once_and_preserve_outer_assignments() {
    let source = "effect Scoped {
        fixed param bool choose = true;
        param float level = 1.0;
        void generate() {
            float total = level;
            int i = 40;
            for (int i = 0; i < 3; i = i + 1) {
                if (choose) {
                    float total = total + 10.0;
                    timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: total };
                } else {
                    float total = total + 20.0;
                    timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: total };
                }
                total = total + level;
                for (int i in range(2, 2)) {
                    float total = total + i;
                    timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: total };
                }
            }
            timeline.emit Child { start: 0.0, duration: 1.0, target: target, value: total + i };
        }
    }";
    let compiled = compile_effects(source).unwrap().remove(0);
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(2_000_000),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(TargetValue { groups: Vec::new() }),
    };
    for choose in [false, true] {
        for level in [0.5, 3.0] {
            let arguments = [Value::Bool(choose), Value::Float(level)];
            let branch = if choose { 10.0 } else { 20.0 };
            let expected = [
                level + branch,
                2.0 * level,
                2.0 * level + 1.0,
                2.0 * level + branch,
                3.0 * level,
                3.0 * level + 1.0,
                3.0 * level + branch,
                4.0 * level,
                4.0 * level + 1.0,
                4.0 * level + 40.0,
            ]
            .into_iter()
            .map(|value| vec![Value::Float(value)])
            .collect::<Vec<_>>();
            for input in [
                GeneratorInput::Live,
                GeneratorInput::Fixed(Value::Float(level)),
            ] {
                let specialized = compiled
                    .effect
                    .generator()
                    .unwrap()
                    .clone()
                    .bind(&[GeneratorInput::Fixed(Value::Bool(choose)), input])
                    .unwrap()
                    .specialize(&context);
                assert_eq!(evaluate(&specialized, &arguments, 0), expected);
            }
        }
    }
}

#[test]
fn generated_selection_preserves_native_pixel_addresses_and_sample_context() {
    // One record is enough to exercise large addresses: no huge fixture or
    // allocation is needed, and selecting a record must not narrow its fields.
    let pixel = donder_runtime::PreparedPixel {
        fixture_index: i32::MAX as usize + 1,
        fixture_pixel_index: u32::MAX,
        pixel_index: i32::MAX as usize + 2,
        pixel_count: u32::MAX as usize,
        pixel_fraction: 0.625,
    };
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(TargetValue {
            groups: vec![Arc::new(donder_language::dsl::TargetItemValue {
                pixels: Arc::from([pixel]),
            })],
        }),
    };
    for selection in [
        "target",
        "pick(pixels(target), 0.0)",
        "pick(fixtures(target), 0.0)",
    ] {
        let compiled = compile_effects(&format!(
            "effect Select {{ void generate() {{
                timeline.emit Child {{ start: 0.0, duration: 1.0, target: {selection} }};
            }} }}"
        ))
        .unwrap()
        .remove(0);
        let specialized = compiled
            .effect
            .generator()
            .unwrap()
            .clone()
            .bind(&[])
            .unwrap()
            .specialize(&context);
        assert_eq!(specialized.children.len(), 1);
        assert_eq!(specialized.children[0].target.pixels.as_ref(), &[pixel]);
    }
}
