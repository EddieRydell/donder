const SPATIAL: donder_runtime::SpatialContext = donder_runtime::SpatialContext {
    position: [0.0; 2],
    min: [0.0; 2],
    max: [0.0; 2],
};

use donder_language::dsl::{
    Color, Identifier, OperatorRunContext, RuntimeError, SignalSampler, Value, VmWorkspace,
    compile_effects, compile_operators,
};
use donder_language::values::{Marks, SampleDuration, SampleTime};
use donder_runtime::{Instruction, SignalPixel};
use indexmap::IndexMap;

use std::sync::Arc;

#[test]
fn declaration_kinds_are_source_specific() {
    assert!(
        compile_effects(
            "operator Gain { input Signal source; color sample() { return source.at(seconds()); } }"
        )
        .is_err()
    );
    assert!(compile_operators("effect Solid { color sample() { return #ffffff; } }").is_err());
}

#[test]
fn assigned_parameters_are_invocation_local_across_branches_and_loops() {
    let effect = compile_effects(
        "effect Assigned {
        param float amount = 0.25;
        color sample() {
            if (progress() > 0.5) { amount = amount + 0.25; }
            for (int i = 0; i < 2; i = i + 1) { amount = amount + 0.1; }
            return rgb(amount, 0.0, 0.0);
        }
    }",
    )
    .unwrap()
    .remove(0);
    let params = effect
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let mut workspace = VmWorkspace::default();
    for (progress, red) in [(0.0, 115), (1.0, 179), (0.0, 115)] {
        let context = OperatorRunContext {
            progress,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        assert_eq!(
            params.evaluate(&context, &SPATIAL, &mut workspace),
            Color {
                red,
                green: 0,
                blue: 0
            }
        );
    }
}

#[test]
fn marks_iteration_captures_its_bound_and_rejects_index_assignment() {
    let effect = compile_effects(
        "effect Iterate {
            fixed param marks beats;
            color sample() {
                float total = 0.0;
                for (int mark in beats) { total = total + mark_at(beats, mark); }
                return rgb(total / 10.0, 0.0, 0.0);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(10_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut params = IndexMap::new();
    params.insert(
        Identifier::new("beats".to_string()).unwrap(),
        Value::Marks(Arc::new(Marks {
            marks: vec![
                SampleDuration::from_ticks(1_000_000),
                SampleDuration::from_ticks(2_000_000),
            ],
        })),
    );
    let bound = effect
        .bind(&params, &mut donder_runtime::DslBindCache::default())
        .unwrap();
    assert_eq!(
        bound
            .evaluate(&context, &SPATIAL, &mut VmWorkspace::default())
            .red,
        77
    );
    params.insert(
        Identifier::new("beats".to_string()).unwrap(),
        Value::Marks(Arc::new(Marks { marks: Vec::new() })),
    );
    let bound = effect
        .bind(&params, &mut donder_runtime::DslBindCache::default())
        .unwrap();
    assert_eq!(
        bound.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
        Color::BLACK
    );
    assert!(
        compile_effects(
            "effect Invalid { fixed param marks beats; color sample() {
            for (int mark in beats) { mark = mark + 1; }
            return #000000;
        } }"
        )
        .is_err()
    );
}

#[test]
fn marks_iteration_uses_collection_length_not_numeric_range_cap() {
    let effect = compile_effects(
        "effect CountMarks {
            fixed param marks beats;
            color sample() {
                int count = 0;
                for (int mark in beats) { count = count + 1; }
                return rgb(count / 10001.0, 0.0, 0.0);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect
        .bind(
            &IndexMap::from([(
                Identifier::new("beats".to_string()).unwrap(),
                Value::Marks(Arc::new(Marks {
                    marks: (0..10_001).map(SampleDuration::from_ticks).collect(),
                })),
            )]),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(10_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    assert_eq!(
        params
            .evaluate(&context, &SPATIAL, &mut VmWorkspace::default())
            .red,
        255
    );
}

#[test]
fn integer_comparisons_do_not_round_through_float() {
    let effect = compile_effects(
        "effect Exact {
            param int left = 16777217;
            param int right = 16777216;
            color sample() {
                if (left > 16777216 && left > right && right < left) {
                    return #ffffff;
                }
                return #000000;
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    assert_eq!(
        params.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
        Color {
            red: 255,
            green: 255,
            blue: 255
        }
    );
}

#[test]
fn c_style_loops_require_static_bounds_and_dynamic_ranges_are_capped() {
    compile_effects(
        "effect Fixed { color sample() {
            int total = 0;
            for (int i = 0; i < 3; i = i + 1) { total = total + i; }
            return rgb(total / 10.0, 0.0, 0.0);
        } }",
    )
    .unwrap()
    .remove(0);

    let dynamic = compile_effects(
        "effect Dynamic { param int count = 3; color sample() {
            int total = 0;
            for (int i = 0; i < count; i = i + 1) { total = total + i; }
            return rgb(total / 10.0, 0.0, 0.0);
        } }",
    )
    .unwrap_err();
    assert!(dynamic.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("compile-time-proven trip count")
    }));

    let capped = compile_effects(
        "effect Capped { param int count = 3; color sample() {
            int total = 0;
            for (int i in range(count, 4)) { total = total + i; }
            return rgb(total / 10.0, 0.0, 0.0);
        } }",
    )
    .unwrap()
    .remove(0);
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let sample = |count| {
        let mut values = IndexMap::new();
        values.insert(
            Identifier::new("count".to_string()).unwrap(),
            Value::Int(count),
        );
        let params = capped
            .bind(&values, &mut donder_runtime::DslBindCache::default())
            .unwrap();
        params
            .evaluate(&context, &SPATIAL, &mut VmWorkspace::default())
            .red
    };
    assert_eq!(sample(-3), 0);
    assert!(sample(3) < sample(4));
    assert_eq!(sample(4), sample(i32::MAX));

    let mutated = compile_effects(
        "effect Mutated { color sample() {
            int total = 0;
            for (int i = 0; i < 3; i = i + 1) { i = 0; total = total + 1; }
            return rgb(total, 0.0, 0.0);
        } }",
    )
    .unwrap_err();
    assert!(mutated.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("compile-time-proven trip count")
    }));

    for cap in ["0", "10001", "count"] {
        let source = format!(
            "effect Bad {{ param int count = 3; color sample() {{ for (int i in range(count, {cap})) {{ }} return rgb(0.0, 0.0, 0.0); }} }}"
        );
        assert!(
            compile_effects(&source)
                .unwrap_err()
                .iter()
                .any(|diagnostic| diagnostic.message.contains("range cap"))
        );
    }
}

#[test]
fn nested_counted_loops_reset_their_private_iteration_state() {
    let effect = compile_effects(
        "effect Nested { param int count = 2; color sample() {
            int total = 0;
            for (int outer = 0; outer < 2; outer = outer + 1) {
                for (int inner in range(count, 3)) { total = total + 1; }
            }
            if (total == 4) { return #ffffff; }
            return #000000;
        } }",
    )
    .unwrap()
    .remove(0);
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let params = effect
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    assert_eq!(
        params.evaluate(&context, &SPATIAL, &mut VmWorkspace::default()),
        Color {
            red: 255,
            green: 255,
            blue: 255
        }
    );
}

#[test]
fn compiler_tracks_pixel_dependency_including_branches_and_signal_samples() {
    for (expression, expected) in [
        ("rgb(progress(), sin(seconds()), 0.0)", false),
        ("rgb(pixel_fraction(), 0.0, 0.0)", true),
        ("rgb(pixel_count(), 0.0, 0.0)", true),
        ("rgb(section_position(4.0), 0.0, 0.0)", true),
    ] {
        let source = format!("effect Dependency {{ color sample() {{ return {expression}; }} }}");
        let effect = compile_effects(&source).unwrap().remove(0);
        assert_eq!(
            effect.sample_program().bytecode().uses_pixel_context,
            expected,
            "{expression}"
        );
    }
    let operator = compile_operators("operator Identity { input Signal source; color sample() { return source.at(seconds()); } }")
        .unwrap().remove(0);
    assert!(operator.bytecode().uses_pixel_context);
    let effect = compile_effects("effect Branch { color sample() { if (progress() > 0.5) { return rgb(pixel_index(), 0.0, 0.0); } return #000000; } }")
        .unwrap().remove(0);
    assert!(effect.sample_program().bytecode().uses_pixel_context);
}

#[test]
fn compiler_marks_only_pixel_uniform_signal_times_for_frame_caching() {
    let uniform = compile_operators(
        "operator Uniform { input Signal source; color sample() { return source.at(seconds() + 0.25); } }",
    )
    .unwrap()
    .remove(0);
    assert!(uniform.bytecode().instructions.iter().any(|instruction| {
        matches!(
            instruction,
            Instruction::SignalSample { frame_cache: 0, .. }
        )
    }));

    let pixel_dependent = compile_operators(
        "operator PerPixel { input Signal source; color sample() { return source.at(seconds() + pixel_fraction()); } }",
    )
    .unwrap()
    .remove(0);
    assert!(
        pixel_dependent
            .bytecode()
            .instructions
            .iter()
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::SignalSample {
                        frame_cache: u32::MAX,
                        ..
                    }
                )
            })
    );

    let independent_reads = compile_operators(
        "operator TwoReads { input Signal first; input Signal second; color sample() { return first.at(seconds()) + second.at(seconds() + 0.5); } }",
    )
    .unwrap()
    .remove(0);
    let caches = independent_reads
        .bytecode()
        .instructions
        .iter()
        .filter_map(|instruction| match instruction {
            Instruction::SignalSample { frame_cache, .. } => Some(*frame_cache),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(caches, vec![0, 1]);
}

#[test]
fn constant_and_calculated_arrays_preserve_nested_values_and_assignment() {
    let effect = compile_effects(
        "effect Arrays {
            color sample() {
                array<array<float>> table = [[0.1, 0.2], [0.3, 0.4, 0.5]];
                array<float> values = [progress(), table[1][2]];
                array<float> saved = values;
                values = [0.9];
                return rgb(saved[0], saved[1], values[0]);
            }
        }",
    )
    .unwrap()
    .remove(0);
    let params = effect
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(250_000),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut workspace = VmWorkspace::default();
    for _ in 0..3 {
        assert_eq!(
            params.evaluate(&context, &SPATIAL, &mut workspace),
            Color {
                red: 64,
                green: 128,
                blue: 230
            },
        );
    }
}

#[test]
fn operator_requires_a_signal_input() {
    let diagnostics = compile_operators("operator Empty { color sample() { return #000000; } }")
        .expect_err("operator without inputs must fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("at least one Signal input"))
    );
}

#[test]
fn array_aliases_survive_loops_nested_reassignment_and_workspace_reuse() {
    let effect = compile_effects(include_str!("fixtures/array-lifetimes.effect.donder"))
        .unwrap()
        .remove(0);
    let small = compile_effects("effect Small { color sample() { return rgb(0.0, 0.0, 0.0); } }")
        .unwrap()
        .remove(0);
    let small_params = small
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let mut workspace = VmWorkspace::default();
    for (progress, iterations, expected) in [
        (
            0.25,
            4,
            Color {
                red: 64,
                green: 89,
                blue: 230,
            },
        ),
        (
            0.5,
            256,
            Color {
                red: 128,
                green: 153,
                blue: 230,
            },
        ),
        (
            0.0,
            2,
            Color {
                red: 0,
                green: 26,
                blue: 230,
            },
        ),
    ] {
        let context = OperatorRunContext {
            progress,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        let mut values = IndexMap::from([
            (
                Identifier::new("iterations".into()).unwrap(),
                Value::Int(iterations),
            ),
            (Identifier::new("fail".into()).unwrap(), Value::Bool(true)),
        ]);
        let with_zero_remainder = effect
            .bind(&values, &mut donder_runtime::DslBindCache::default())
            .unwrap();
        assert_eq!(
            with_zero_remainder.evaluate(&context, &SPATIAL, &mut workspace),
            expected
        );
        values.insert(Identifier::new("fail".into()).unwrap(), Value::Bool(false));
        let params = effect
            .bind(&values, &mut donder_runtime::DslBindCache::default())
            .unwrap();
        // Reuse after a different branch, then after a program with a different register layout.
        assert_eq!(
            params.evaluate(&context, &SPATIAL, &mut workspace),
            expected
        );
        assert_eq!(
            small_params.evaluate(&context, &SPATIAL, &mut workspace),
            Color {
                red: 0,
                green: 0,
                blue: 0
            }
        );
        assert_eq!(
            params.evaluate(&context, &SPATIAL, &mut workspace),
            expected
        );
    }
}

#[test]
fn enum_identity_survives_subset_assignment_arrays_and_program_reuse() {
    let mut workspace = VmWorkspace::default();
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    for options in ["alpha, beta, gamma", "gamma, alpha, beta"] {
        let effect = compile_effects(&format!(
            "effect EnumIdentity {{
                param enum wide {{ {options} }} = alpha;
                param enum subset {{ gamma, beta }} = beta;
                color sample() {{
                    wide = subset;
                    if (wide != subset || wide != beta || wide == alpha) {{ return rgb(1.0, 0.0, 0.0); }}
                    subset = gamma;
                    if (wide != beta || subset != gamma) {{ return rgb(0.0, 1.0, 0.0); }}
                    wide = [wide, subset][1];
                    if (wide != gamma) {{ return rgb(0.0, 0.0, 1.0); }}
                    return rgb(0.25, 0.5, 0.75);
                }}
            }}"
        )).unwrap().remove(0);
        let params = effect
            .bind(
                &IndexMap::new(),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        for _ in 0..3 {
            assert_eq!(
                params.evaluate(&context, &SPATIAL, &mut workspace),
                Color {
                    red: 64,
                    green: 128,
                    blue: 191
                }
            );
        }
    }
}

#[test]
fn standard_operator_names_are_project_owned() {
    assert!(compile_operators("operator Delay { input Signal source; color sample() { return source.at(seconds()); } }").is_ok());
    assert!(compile_operators("operator intensity_modulate { input Signal source; color sample() { return source.at(seconds()); } }").is_ok());
}

#[test]
fn enum_params_require_an_option() {
    let diagnostics =
        compile_effects("effect Bad { param enum mode {}; color sample() { return #000000; } }")
            .expect_err("empty enum must fail");
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.message.contains("must declare an option"))
    );
}

#[test]
fn signal_is_only_valid_as_an_operator_input() {
    assert!(compile_operators("operator Bad { input Signal source; param Signal stored; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator Bad { input Signal source; param array<Signal> stored; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator Bad { input Signal source; color sample() { Signal local; return source.at(seconds()); } }").is_err());
}

#[test]
fn signal_sampling_and_color_operations_execute() {
    let operator = compile_operators(
        "operator Colors { input Signal source; color sample() { color sampled = source.at(seconds()); return max(invert(sampled) * 0.5 + #010203, sampled * #ffffff); } }",
    )
    .expect("operator compiles")
    .into_iter()
    .next()
    .expect("one operator");
    let params = operator
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(1_000_000),
        duration: SampleDuration::from_ticks(4_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let mut sampler = ConstantSignal(Color {
        red: 10,
        green: 20,
        blue: 30,
    });
    let color = params
        .evaluate(
            &context,
            &SPATIAL,
            &mut sampler,
            &mut VmWorkspace::default(),
        )
        .expect("operator samples");
    assert_eq!(
        color,
        Color {
            red: 124,
            green: 120,
            blue: 116
        }
    );
}

#[test]
fn source_numeric_overflow_and_integer_division_report_diagnostics() {
    let integer_overflow = compile_effects(
        "effect Bad { color sample() { int value = 999999999999999999999999999999; return #000000; } }",
    )
    .expect_err("out-of-range integer literals must fail");
    assert!(integer_overflow.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("integer literal is out of range")
    }));

    let integer_division =
        compile_effects("effect Bad { color sample() { int value = 4 / 2; return #000000; } }")
            .expect_err("integer division produces a float and cannot initialize an int");
    assert!(!integer_division.is_empty());
}

#[test]
fn required_parameters_bind_and_integer_remainder_by_zero_is_total() {
    let effect = compile_effects(
        "effect Required { param float amount; color sample() { int value = 1 % 0; return #000000; } }",
    )
    .expect("effect compiles")
    .into_iter()
    .next()
    .expect("one effect");
    let missing = effect
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .err()
        .expect("required parameters must not synthesize a default");
    assert!(
        missing
            .message
            .contains("missing required parameter `amount`")
    );

    let params = IndexMap::from([(
        Identifier::new("amount".to_string()).unwrap(),
        donder_language::dsl::Value::Float(1.0),
    )]);
    let bound = effect
        .bind(&params, &mut donder_runtime::DslBindCache::default())
        .expect("required parameter binds");
    let color = bound.evaluate(
        &donder_language::dsl::RunContext {
            progress: 0.0,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        },
        &SPATIAL,
        &mut VmWorkspace::default(),
    );
    assert_eq!(color, Color::BLACK);
}

#[test]
fn integer_arithmetic_wraps_and_remainder_is_total() {
    for (expression, left, right, expected) in [
        ("-a", i32::MIN, 0, "-2147483647 - 1"),
        ("a + b", i32::MAX, 1, "-2147483647 - 1"),
        ("a - b", i32::MIN, 1, "2147483647"),
        ("a * b", i32::MAX, 2, "-2"),
        ("a % b", 1, 0, "0"),
        ("a % b", i32::MIN, -1, "0"),
    ] {
        let source = format!(
            "effect Arithmetic {{
                param int a;
                param int b;
                color sample() {{
                    if (({expression}) == ({expected})) {{ return #ffffff; }}
                    return #000000;
                }}
            }}"
        );
        let effect = compile_effects(&source).unwrap().remove(0);
        let params = effect
            .bind(
                &IndexMap::from([
                    (Identifier::new("a".into()).unwrap(), Value::Int(left)),
                    (Identifier::new("b".into()).unwrap(), Value::Int(right)),
                ]),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        let color = params.evaluate(
            &donder_language::dsl::RunContext {
                progress: 0.0,
                time: SampleDuration::from_ticks(0),
                duration: SampleDuration::from_ticks(1_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &SPATIAL,
            &mut VmWorkspace::default(),
        );
        assert_eq!(
            color,
            Color {
                red: 255,
                green: 255,
                blue: 255
            },
            "{expression}"
        );
    }
}

struct ConstantSignal(Color);

#[test]
fn signal_sampling_outside_the_portable_clock_returns_black() {
    let operator = compile_operators(
        "operator Query {
            input Signal source;
            param float query_seconds;
            color sample() { return source.at(query_seconds); }
        }",
    )
    .unwrap()
    .remove(0);
    let context = OperatorRunContext {
        progress: 0.0,
        time: SampleDuration::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        pixel_index: 0,
        pixel_count: 1,
        pixel_fraction: 0.0,
    };
    let source_color = Color {
        red: 17,
        green: 34,
        blue: 51,
    };
    for (seconds, expected) in [
        (0.0, source_color),
        (-1.0, Color::BLACK),
        (f32::NAN, Color::BLACK),
        (f32::INFINITY, Color::BLACK),
        (f32::NEG_INFINITY, Color::BLACK),
        (f32::MAX, Color::BLACK),
    ] {
        let params = operator
            .bind(
                &IndexMap::from([(
                    Identifier::new("query_seconds".into()).unwrap(),
                    Value::Float(seconds),
                )]),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        let color = params
            .evaluate(
                &context,
                &SPATIAL,
                &mut ConstantSignal(source_color),
                &mut VmWorkspace::default(),
            )
            .unwrap();
        assert_eq!(color, expected, "{seconds}");
    }
}

#[test]
fn spatial_signal_queries_keep_coordinate_domains_and_mutations_distinct() {
    #[derive(Default)]
    struct Samples(Vec<SignalPixel<i32>>);
    impl SignalSampler for Samples {
        fn sample_signal(
            &mut self,
            _input: usize,
            _time: SampleTime,
            pixel: SignalPixel<i32>,
            _frame_cache: Option<usize>,
        ) -> Result<Color, RuntimeError> {
            self.0.push(pixel);
            let value = pixel.index().copied().unwrap_or_default() as u8;
            Ok(Color {
                red: value,
                green: value,
                blue: value,
            })
        }
    }
    let operator = compile_operators(
        "operator Spatial {
        input Signal source;
        color sample() {
            float t = seconds();
            int p = pixel_index();
            color a = source.at(t, p);
            color b = source.at_global(t, p);
            color c = source.at(t);
            color d = source.at(t, p);
            p = p + 1;
            return max(max(max(a, b), max(c, d)), source.at(t, p));
        }
    }",
    )
    .unwrap()
    .remove(0);
    let params = operator
        .bind(
            &IndexMap::new(),
            &mut donder_runtime::DslBindCache::default(),
        )
        .unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(250000),
        duration: SampleDuration::from_ticks(1000000),
        pixel_index: 3,
        pixel_count: 10,
        pixel_fraction: 0.3,
    };
    let mut samples = Samples::default();
    let result = params
        .evaluate(
            &context,
            &SPATIAL,
            &mut samples,
            &mut VmWorkspace::default(),
        )
        .unwrap();
    assert_eq!(
        samples.0,
        [
            SignalPixel::Local(3),
            SignalPixel::Global(3),
            SignalPixel::Current,
            SignalPixel::Local(4)
        ]
    );
    assert_eq!(result.red, 4);
    for query in [
        "source.at()",
        "source.at(0.0, 1.0)",
        "source.at(0.0, 1, 2)",
        "source.at_global(0.0)",
        "source.at_global(0.0, true)",
    ] {
        assert!(
            compile_operators(&format!(
                "operator Invalid {{ input Signal source; color sample() {{ return {query}; }} }}"
            ))
            .is_err(),
            "{query}"
        );
    }
}

#[test]
fn repeated_signal_reads_reuse_only_unchanged_values_in_one_block() {
    #[derive(Default)]
    struct Samples(Vec<(usize, u32)>);
    impl SignalSampler for Samples {
        fn sample_signal(
            &mut self,
            input: usize,
            time: SampleTime,
            _pixel: SignalPixel<i32>,
            _frame_cache: Option<usize>,
        ) -> Result<Color, RuntimeError> {
            self.0.push((input, time.as_ticks()));
            Ok(Color {
                red: (time.as_ticks() / 1000).min(255) as u8,
                green: input as u8,
                blue: 0,
            })
        }
    }
    for (body, calls, red, green) in [
        (
            "float t = seconds(); color a = source.at(t); color b = other.at(t); return max(max(a, b), source.at(t));",
            vec![(0, 250000), (1, 250000)],
            250,
            1,
        ),
        (
            "float t = seconds(); float p = t - 0.125; color a = source.at(t); color b = source.at(p); color c = source.at(t); color d = source.at(p); return max(max(a,b), max(c,d));",
            vec![(0, 250000), (0, 125000)],
            250,
            0,
        ),
        (
            "float t = seconds(); color a = source.at(t); t = t + 0.125; return max(a, source.at(t));",
            vec![(0, 250000), (0, 375000)],
            255,
            0,
        ),
        (
            "float t = seconds(); color a = source.at(t); if (progress() > 0.5) { a = source.at(t); } return max(a, source.at(t));",
            vec![(0, 250000); 3],
            250,
            0,
        ),
        (
            "float t = seconds(); color a = rgb(0.0, 0.0, 0.0); for (int i = 0; i < 2; i = i + 1) { a = max(source.at(t), source.at(t)); } return a;",
            vec![(0, 250000); 2],
            250,
            0,
        ),
    ] {
        let operator = compile_operators(&format!("operator Reads {{ input Signal source; input Signal other; color sample() {{ {body} }} }}")).unwrap().remove(0);
        let params = operator
            .bind(
                &IndexMap::new(),
                &mut donder_runtime::DslBindCache::default(),
            )
            .unwrap();
        let context = OperatorRunContext {
            progress: 1.0,
            time: SampleDuration::from_ticks(250000),
            duration: SampleDuration::from_ticks(1000000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        let mut samples = Samples::default();
        let color = params
            .evaluate(
                &context,
                &SPATIAL,
                &mut samples,
                &mut VmWorkspace::default(),
            )
            .unwrap();
        assert_eq!(samples.0, calls, "{body}");
        assert_eq!(
            color,
            Color {
                red,
                green,
                blue: 0
            },
            "{body}"
        );
    }
}

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
