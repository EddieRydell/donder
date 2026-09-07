use dawn_language::dsl::{
    Color, GeneratedEffectSlot, GeneratorContext, Identifier, OperatorRunContext, RuntimeError,
    SignalSampler, TargetValue, Value, VmWorkspace, compile_effects, compile_operators,
};
use dawn_language::values::{SampleDuration, SampleTime};
use dawn_runtime::dsl::bytecode::{Instruction, SignalPixel};
use indexmap::IndexMap;

#[test]
fn context_only_types_cannot_be_nested_in_parameters() {
    for ty in ["Timeline", "Target", "TargetItems", "TargetItem"] {
        for nested in [format!("array<{ty}>"), format!("array<array<{ty}>>")] {
            let source = format!(
                "effect Invalid {{ param {nested} stored; color sample() {{ return rgb(0.0, 0.0, 0.0); }} }}"
            );
            assert!(
                compile_effects(&source).is_err(),
                "accepted context-only parameter {nested}"
            );
        }
    }
}
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
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
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
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
            Color {
                red,
                green: 0,
                blue: 0
            }
        );
    }
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
        let effect = compile_effects(&source).unwrap().remove(0).effect;
        assert_eq!(effect.bytecode.uses_pixel_context, expected, "{expression}");
    }
    let operator = compile_operators("operator Identity { input Signal source; color sample() { return source.at(seconds()); } }")
        .unwrap().remove(0);
    assert!(operator.bytecode.uses_pixel_context);
    let effect = compile_effects("effect Branch { color sample() { if (progress() > 0.5) { return rgb(pixel_index(), 0.0, 0.0); } return #000000; } }")
        .unwrap().remove(0).effect;
    assert!(effect.bytecode.uses_pixel_context);
}

#[test]
fn compiler_marks_only_pixel_uniform_signal_times_for_frame_caching() {
    let uniform = compile_operators(
        "operator Uniform { input Signal source; color sample() { return source.at(seconds() + 0.25); } }",
    )
    .unwrap()
    .remove(0);
    assert!(uniform.bytecode.instructions.iter().any(|instruction| {
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
            .bytecode
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
        .bytecode
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
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
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
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
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
    let effect = compile_effects(include_str!("fixtures/array-lifetimes.effect.dawn"))
        .unwrap()
        .remove(0)
        .effect;
    let small = compile_effects("effect Small { color sample() { return rgb(0.0, 0.0, 0.0); } }")
        .unwrap()
        .remove(0)
        .effect;
    let small_params = small.bind_params(&IndexMap::new()).unwrap();
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
        let failing = effect.bind_params(&values).unwrap();
        let error = effect
            .sample_bound(&failing, &context, &mut workspace)
            .unwrap_err();
        assert!(
            error
                .message
                .contains("integer arithmetic overflow or division by zero")
        );
        values.insert(Identifier::new("fail".into()).unwrap(), Value::Bool(false));
        let params = effect.bind_params(&values).unwrap();
        // Reuse after an error, then after a program with a different register layout.
        assert_eq!(
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
            expected
        );
        assert_eq!(
            small
                .sample_bound(&small_params, &context, &mut workspace)
                .unwrap(),
            Color {
                red: 0,
                green: 0,
                blue: 0
            }
        );
        assert_eq!(
            effect
                .sample_bound(&params, &context, &mut workspace)
                .unwrap(),
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
        )).unwrap().remove(0).effect;
        let params = effect.bind_params(&IndexMap::new()).unwrap();
        for _ in 0..3 {
            assert_eq!(
                effect
                    .sample_bound(&params, &context, &mut workspace)
                    .unwrap(),
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
fn generator_emitted_arrays_and_enums_outlive_vm_registers() {
    let effect = compile_effects(
        "effect EmitValues {
            param enum mode { alpha, beta } = beta;
            void generate() {
                for (int i = 0; i < 3; i = i + 1) {
                    timeline.emit Child {
                        start: 0.0, duration: 1.0, target: target,
                        mode: mode, values: [[i], [i + 1, i + 2]]
                    };
                    mode = alpha;
                }
            }
        }",
    )
    .unwrap()
    .remove(0)
    .effect;
    let params = effect.bind_params(&IndexMap::new()).unwrap();
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: SampleDuration::from_ticks(1_000_000),
        target: Arc::new(TargetValue { groups: Vec::new() }),
    };
    let mut workspace = VmWorkspace::default();
    let first = effect
        .generate_bound(&params, &context, &mut workspace)
        .unwrap();
    let second = effect
        .generate_bound(&params, &context, &mut workspace)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    for (index, child) in first.iter().enumerate() {
        let index = index as i32;
        assert_eq!(
            child.params,
            vec![
                (
                    Identifier::new("mode".into()).unwrap(),
                    Value::Enum(
                        Identifier::new(if index == 0 { "beta" } else { "alpha" }.into()).unwrap()
                    )
                ),
                (
                    Identifier::new("values".into()).unwrap(),
                    Value::Array(Arc::from([
                        Value::Array(Arc::from([Value::Int(index)])),
                        Value::Array(Arc::from([Value::Int(index + 1), Value::Int(index + 2)])),
                    ]))
                ),
            ]
        );
    }
}

#[test]
fn builtin_operator_names_are_reserved() {
    assert!(compile_operators("operator Delay { input Signal source; color sample() { return source.at(seconds()); } }").is_err());
    assert!(compile_operators("operator intensity_modulate { input Signal source; color sample() { return source.at(seconds()); } }").is_err());
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
    let params = operator.bind_params(&IndexMap::new()).unwrap();
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
    let color = operator
        .sample_bound(&params, &context, &mut sampler, &mut VmWorkspace::default())
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
fn generator_emit_events_carry_only_ordered_numeric_slots() {
    let effect = compile_effects(
        "effect EmitAll {
          void generate() {
            timeline.emit builtins.pulse { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.chase { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.spin { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.mark_pulse { start: 0.0, duration: 1.0, target: target };
            timeline.emit builtins.mark_chase { start: 0.0, duration: 1.0, target: target };
            timeline.emit LocalChild { start: 0.0, duration: 1.0, target: target };
          }
        }",
    )
    .expect("generator compiles")
    .into_iter()
    .next()
    .expect("one effect")
    .effect;
    let generated = effect
        .generate_bound(
            &effect.bind_params(&IndexMap::new()).unwrap(),
            &GeneratorContext {
                start_time: SampleTime::from_ticks(2_000_000),
                duration: SampleDuration::from_ticks(1_000_000),
                target: Arc::new(TargetValue { groups: Vec::new() }),
            },
            &mut VmWorkspace::default(),
        )
        .expect("generator runs");

    assert_eq!(
        generated
            .iter()
            .map(|effect| effect.definition)
            .collect::<Vec<_>>(),
        vec![
            GeneratedEffectSlot(0),
            GeneratedEffectSlot(1),
            GeneratedEffectSlot(2),
            GeneratedEffectSlot(3),
            GeneratedEffectSlot(4),
            GeneratedEffectSlot(5),
        ]
    );
    assert!(
        generated.iter().all(|effect| {
            effect.start_time == SampleTime::from_ticks(2_000_000)
                && effect.duration == SampleDuration::from_ticks(1_000_000)
        }),
        "emitted timing should be converted once to the portable clock"
    );
}

#[test]
fn emitted_reference_spans_do_not_change_semantics_or_bytecode_hashes() {
    use std::hash::Hasher;
    let source = "effect Parent { void generate() { timeline.emit Local { start: 0.0, duration: 1.0, target: target }; } }";
    let first = compile_effects(source).unwrap().remove(0);
    let second = compile_effects(&format!("\n// diagnostic-only displacement\n{source}"))
        .unwrap()
        .remove(0);
    assert_ne!(
        first.emitted_references[0].span,
        second.emitted_references[0].span
    );
    assert_eq!(first, second);
    let hash = |effect| {
        let mut state = std::collections::hash_map::DefaultHasher::new();
        dawn_language::dsl::hash_compiled_effect(effect, &mut state);
        state.finish()
    };
    assert_eq!(hash(&first.effect), hash(&second.effect));
    let document = format!("import fx from [\"child.effect.dawn\"];\n{source}");
    let first = dawn_language::dsl::compile_effect_document(&document).unwrap();
    let second = dawn_language::dsl::compile_effect_document(&format!("\n{document}")).unwrap();
    assert_ne!(first.imports[0].span, second.imports[0].span);
    assert_eq!(first, second);
}

#[test]
fn invalid_generated_effect_slot_is_rejected_by_the_vm() {
    let mut effect = compile_effects("effect Parent { void generate() { timeline.emit Local { start: 0.0, duration: 1.0, target: target }; } }").unwrap().remove(0).effect;
    effect.generated_effect_count = 0;
    let result = effect.generate_bound(
        &effect.bind_params(&IndexMap::new()).unwrap(),
        &GeneratorContext {
            start_time: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            target: Arc::new(TargetValue { groups: Vec::new() }),
        },
        &mut VmWorkspace::default(),
    );
    assert!(result.is_err());
}

#[test]
fn qualified_generator_emit_references_report_specific_diagnostics() {
    for reference in ["builtins.pulse.extra", "fx.Child.extra"] {
        let expected = "generated effect reference must contain exactly two segments";
        let source = format!(
            "effect Bad {{ void generate() {{ timeline.emit {reference} {{ start: 0.0, duration: 1.0, target: target }}; }} }}"
        );
        let diagnostics = compile_effects(&source).expect_err("invalid reference must fail");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains(expected)),
            "missing `{expected}` in {diagnostics:?}"
        );
    }
}

#[test]
fn effect_document_compilation_retains_explicit_imports_and_source_spans() {
    use dawn_language::dsl::compile_effect_document;
    let source = "import bursts from [\"effects/child.effect.dawn\"];\nimport library from local-effects.child-effects;\neffect Parent { void generate() { timeline.emit bursts.Child { start: 0.0, duration: 1.0, target: target }; } }";
    let compiled = compile_effect_document(source).unwrap();
    let dawn_language::imports::ImportSource::LocalDocuments { documents } =
        &compiled.imports[0].declaration.source
    else {
        panic!("document import")
    };
    assert_eq!(documents[0], "effects/child.effect.dawn");
    let span = compiled.imports[0].source_spans[0];
    assert_eq!(
        &source[span.start..span.end],
        "\"effects/child.effect.dawn\""
    );
    assert_eq!(
        compiled.imports[1].declaration.source,
        dawn_language::imports::ImportSource::DependencyExport {
            dependency: "local-effects".into(),
            export: "child-effects".into()
        }
    );
    assert_eq!(
        compiled.effects[0].emitted_references[0].reference,
        dawn_language::imports::SourceReference::Qualified {
            alias: dawn_language::imports::ImportAlias::new("bursts").unwrap(),
            name: Identifier::new("Child".into()).unwrap(),
        }
    );
    assert!(
        compile_effects(source).is_err(),
        "standalone callers cannot discard imports"
    );
    for invalid in [
        source.replace("import bursts", "import builtins"),
        format!("import bursts from [];{source}"),
        format!("{source}\nimport late from [\"other.effect.dawn\"];"),
        source.replace(
            "[\"effects/child.effect.dawn\"]",
            "\"effects/child.effect.dawn\"",
        ),
    ] {
        assert!(compile_effect_document(&invalid).is_err());
    }
    assert!(compile_operators("import effects from [\"child.effect.dawn\"]; operator Gain { input Signal source; color sample() { return source.at(seconds()); } }").is_err());
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
fn required_parameters_and_integer_remainder_fail_without_panicking() {
    let effect = compile_effects(
        "effect Required { param float amount; color sample() { int value = 1 % 0; return #000000; } }",
    )
    .expect("effect compiles")
    .into_iter()
    .next()
    .expect("one effect").effect;
    let missing = effect
        .bind_params(&IndexMap::new())
        .expect_err("required parameters must not synthesize a default");
    assert!(
        missing
            .message
            .contains("missing required parameter `amount`")
    );

    let params = IndexMap::from([(
        Identifier::new("amount".to_string()).unwrap(),
        dawn_language::dsl::Value::Float(1.0),
    )]);
    let bound = effect
        .bind_params(&params)
        .expect("required parameter binds");
    let error = effect
        .sample_bound(
            &bound,
            &dawn_language::dsl::RunContext {
                progress: 0.0,
                time: SampleDuration::from_ticks(0),
                duration: SampleDuration::from_ticks(1_000_000),
                pixel_index: 0,
                pixel_count: 1,
                pixel_fraction: 0.0,
            },
            &mut VmWorkspace::default(),
        )
        .expect_err("integer remainder by zero must become a runtime error");
    assert!(
        error
            .message
            .contains("integer arithmetic overflow or division by zero")
    );
}

struct ConstantSignal(Color);

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
    let params = operator.bind_params(&IndexMap::new()).unwrap();
    let context = OperatorRunContext {
        progress: 0.25,
        time: SampleDuration::from_ticks(250000),
        duration: SampleDuration::from_ticks(1000000),
        pixel_index: 3,
        pixel_count: 10,
        pixel_fraction: 0.3,
    };
    let mut samples = Samples::default();
    let result = operator
        .sample_bound(&params, &context, &mut samples, &mut VmWorkspace::default())
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
        let params = operator.bind_params(&IndexMap::new()).unwrap();
        let context = OperatorRunContext {
            progress: 1.0,
            time: SampleDuration::from_ticks(250000),
            duration: SampleDuration::from_ticks(1000000),
            pixel_index: 0,
            pixel_count: 1,
            pixel_fraction: 0.0,
        };
        let mut samples = Samples::default();
        let color = operator
            .sample_bound(&params, &context, &mut samples, &mut VmWorkspace::default())
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
