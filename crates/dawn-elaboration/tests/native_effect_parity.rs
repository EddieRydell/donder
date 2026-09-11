use dawn_elaboration::native_effect::{self, BoundNativeEffect};
use dawn_language::dsl::{
    GeneratorContext, Identifier, RunContext, TargetItemValue, TargetPixelValue, TargetValue,
    Value, compile_effects,
};
use dawn_language::effect::BuiltinEffect;
use dawn_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};
use indexmap::IndexMap;
use std::sync::Arc;

fn id(value: &str) -> Identifier {
    Identifier::new(value.to_string()).unwrap()
}

#[test]
fn native_parameter_layout_matches_the_compact_runtime_binding() {
    use dawn_language::effect::builtin_effect_definition;
    use dawn_language::operator::{BuiltinOperator, builtin_operator_definition};
    // This order is the native binding ABI. In particular Spin extends Chase;
    // adding its parameter must never shift Chase's slots.
    let chase = [
        "gradient",
        "gradient_mode",
        "pulse_overlap",
        "section_width_pixels",
        "chase_position",
        "reverse",
        "extend_to_start",
        "extend_to_end",
        "pulse_shape",
    ];
    let mut spin = chase.to_vec();
    spin.push("revolutions");
    for (effect, names) in [
        (BuiltinEffect::Pulse, &["gradient", "pulse_shape"][..]),
        (BuiltinEffect::Chase, &chase[..]),
        (BuiltinEffect::Spin, &spin[..]),
        (
            BuiltinEffect::MarkPulse,
            &[
                "beats",
                "base",
                "accent",
                "hue",
                "hue_mix",
                "offset_seconds",
                "decay_seconds",
                "section_width_pixels",
                "section_edge_fade_pixels",
                "sections_per_mark",
                "seed",
            ][..],
        ),
        (
            BuiltinEffect::MarkChase,
            &[
                "beats",
                "base",
                "gradient_mode",
                "gradients",
                "hue",
                "hue_mix",
                "offset_seconds",
                "chase_seconds",
                "pulse_overlap",
                "section_width_pixels",
                "chase_positions",
                "pulse_shape",
            ][..],
        ),
    ] {
        let actual: Vec<_> = builtin_effect_definition(effect)
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        assert_eq!(actual, names, "{effect:?} binding layout changed");
    }
    for operator in BuiltinOperator::ALL {
        assert_eq!(
            builtin_operator_definition(operator).inputs.len(),
            operator.input_count()
        );
    }
}
fn curve() -> Arc<Curve> {
    Arc::new(Curve {
        points: vec![
            CurvePoint {
                position: 0.0,
                value: 0.0,
            },
            CurvePoint {
                position: 0.2,
                value: 1.0,
            },
            CurvePoint {
                position: 1.0,
                value: 0.0,
            },
        ],
    })
}
fn gradient() -> Arc<Gradient> {
    Arc::new(Gradient {
        stops: vec![
            GradientStop {
                position: 0.0,
                color: Color {
                    red: 255,
                    green: 30,
                    blue: 10,
                },
            },
            GradientStop {
                position: 1.0,
                color: Color {
                    red: 10,
                    green: 80,
                    blue: 255,
                },
            },
        ],
    })
}
fn target() -> Arc<TargetValue> {
    Arc::new(TargetValue {
        groups: vec![Arc::new(TargetItemValue {
            pixels: Arc::from(
                (0..24)
                    .map(|pixel| TargetPixelValue {
                        fixture_index: 0,
                        fixture_pixel_index: pixel,
                        pixel_index: pixel,
                        pixel_count: 24,
                        pixel_fraction: pixel as f32 / 23.0,
                    })
                    .collect::<Vec<_>>(),
            ),
        })],
    })
}

#[test]
fn chase_and_spin_integer_sections_match_reference_sampling() {
    let effects = compile_effects(include_str!(
        "../../dawn-language/tests/fixtures/native_effect_reference.effect.dawn"
    ))
    .unwrap();
    for (name, builtin) in [
        ("Chase", BuiltinEffect::Chase),
        ("Spin", BuiltinEffect::Spin),
    ] {
        let reference = effects
            .iter()
            .find(|effect| effect.effect.name().as_str() == name)
            .unwrap();
        for width in [-1, 0, 1, 3, 7, 65_535] {
            for mode in ["through_effect", "across_items", "per_pulse"] {
                for reverse in [false, true] {
                    let params = IndexMap::from([
                        (id("gradient"), Value::Gradient(gradient())),
                        (id("gradient_mode"), Value::Enum(id(mode))),
                        (id("section_width_pixels"), Value::Int(width)),
                        (id("chase_position"), Value::Curve(curve())),
                        (id("pulse_shape"), Value::Curve(curve())),
                        (id("reverse"), Value::Bool(reverse)),
                        (id("extend_to_start"), Value::Bool(reverse)),
                        (id("extend_to_end"), Value::Bool(!reverse)),
                    ]);
                    let bound = reference.effect.bind_params(&params).unwrap();
                    let BoundNativeEffect::Sample { sample, .. } =
                        native_effect::bind(builtin, &params).unwrap()
                    else {
                        panic!("sample effect")
                    };
                    let mut vm = Default::default();
                    for count in [1, 2, 200, 65_535] {
                        for pixel in [0, 1, count / 2, count - 1] {
                            for progress in [0.0, 0.125, 0.5, 0.875, 1.0] {
                                let context = RunContext {
                                    progress,
                                    time: SampleDuration::from_ticks(
                                        (progress * 1_000_000.0) as u32,
                                    ),
                                    duration: SampleDuration::from_ticks(1_000_000),
                                    pixel_index: pixel,
                                    pixel_count: count,
                                    pixel_fraction: pixel as f32 / (count - 1).max(1) as f32,
                                };
                                assert_eq!(
                                    sample
                                        .sample(
                                            &context,
                                            SampleTime::from_ticks(context.time.as_ticks())
                                        )
                                        .unwrap(),
                                    reference
                                        .effect
                                        .sample_bound(&bound, &context, &mut vm)
                                        .unwrap(),
                                    "{name} width={width} mode={mode} reverse={reverse} count={count} pixel={pixel} progress={progress}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn mark_chase_matches_reference_schedule_and_samples() {
    let effects = compile_effects(include_str!(
        "../../dawn-language/tests/fixtures/native_effect_reference.effect.dawn"
    ))
    .unwrap();
    let generator = effects
        .iter()
        .find(|effect| effect.effect.name().as_str() == "MarkChase")
        .unwrap();
    let child = effects
        .iter()
        .find(|effect| effect.effect.name().as_str() == "MarkChaseChild")
        .unwrap();
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(1_000_000),
        duration: SampleDuration::from_ticks(4_000_000),
        target: target(),
    };
    for width in [0, 3, 5, 17] {
        for mode in ["through_effect", "across_items", "per_pulse"] {
            let params = IndexMap::from([
                (
                    id("beats"),
                    Value::Marks(Arc::new(Marks {
                        marks: vec![
                            SampleDuration::from_ticks(500_000),
                            SampleDuration::from_ticks(1_250_000),
                        ],
                    })),
                ),
                (
                    id("base"),
                    Value::Color(Color {
                        red: 2,
                        green: 3,
                        blue: 4,
                    }),
                ),
                (id("gradient_mode"), Value::Enum(id(mode))),
                (
                    id("gradients"),
                    Value::Array(Arc::from([Value::Gradient(gradient())])),
                ),
                (
                    id("chase_positions"),
                    Value::Array(Arc::from([Value::Curve(curve())])),
                ),
                (id("hue"), Value::Curve(curve())),
                (id("pulse_shape"), Value::Curve(curve())),
                (id("section_width_pixels"), Value::Int(width)),
                (id("offset_seconds"), Value::Float(0.125)),
            ]);
            let reference = generator
                .effect
                .generate_bound(
                    &generator.effect.bind_params(&params).unwrap(),
                    &context,
                    &mut Default::default(),
                )
                .unwrap();
            let generated = native_effect::bind(BuiltinEffect::MarkChase, &params)
                .unwrap()
                .generate(&context)
                .unwrap();
            assert_eq!(generated.len(), reference.len());
            for (native, reference) in generated.iter().zip(&reference) {
                assert_eq!(native.start_time, reference.start_time);
                assert_eq!(native.duration, reference.duration);
                assert_eq!(native.target, reference.target);
                let bound = child.effect.bind_params_pairs(&reference.params).unwrap();
                let mut vm = Default::default();
                for pixel in native.target.pixels.iter() {
                    for progress in [0.0, 0.125, 0.5, 0.875, 1.0] {
                        let sample = RunContext {
                            progress,
                            time: SampleDuration::from_ticks(
                                (progress * native.duration.as_ticks() as f32) as u32,
                            ),
                            duration: native.duration,
                            pixel_index: pixel.pixel_index,
                            pixel_count: pixel.pixel_count,
                            pixel_fraction: pixel.pixel_fraction,
                        };
                        let time = native.start_time.checked_add_duration(sample.time).unwrap();
                        assert_eq!(
                            native.sample.sample(&sample, time).unwrap(),
                            child.effect.sample_bound(&bound, &sample, &mut vm).unwrap(),
                            "width={width} mode={mode} pixel={} progress={progress}",
                            pixel.pixel_index
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn mark_pulse_matches_reference_schedule_and_samples() {
    for (width, fade) in [1, 3, 5, 17]
        .into_iter()
        .flat_map(|width| [0.0, 1.0, 3.0].map(|fade| (width, fade)))
    {
        let marks = Arc::new(Marks {
            marks: vec![
                SampleDuration::from_ticks(500_000),
                SampleDuration::from_ticks(1_250_000),
            ],
        });
        let params = IndexMap::from([
            (id("beats"), Value::Marks(marks)),
            (
                id("base"),
                Value::Color(Color {
                    red: 2,
                    green: 3,
                    blue: 4,
                }),
            ),
            (id("accent"), Value::Gradient(gradient())),
            (id("hue"), Value::Curve(curve())),
            (id("hue_mix"), Value::Float(0.35)),
            (id("offset_seconds"), Value::Float(0.1)),
            (id("decay_seconds"), Value::Float(0.3)),
            (id("section_width_pixels"), Value::Int(width)),
            (id("section_edge_fade_pixels"), Value::Float(fade)),
            (id("sections_per_mark"), Value::Int(3)),
            (id("seed"), Value::Float(29.0)),
        ]);
        let effects = compile_effects(include_str!(
            "../../dawn-language/tests/fixtures/native_effect_reference.effect.dawn"
        ))
        .unwrap();
        let generator = effects
            .iter()
            .find(|effect| effect.effect.name().as_str() == "MarkPulse")
            .unwrap();
        let child = effects
            .iter()
            .find(|effect| effect.effect.name().as_str() == "MarkPulseChild")
            .unwrap();
        let context = GeneratorContext {
            start_time: SampleTime::from_ticks(1_000_000),
            duration: SampleDuration::from_ticks(4_000_000),
            target: target(),
        };
        let reference = generator
            .effect
            .generate_bound(
                &generator.effect.bind_params(&params).unwrap(),
                &context,
                &mut Default::default(),
            )
            .unwrap();
        let BoundNativeEffect::MarkPulse(native) =
            native_effect::bind(BuiltinEffect::MarkPulse, &params).unwrap()
        else {
            panic!()
        };
        let generated = BoundNativeEffect::MarkPulse(native)
            .generate(&context)
            .unwrap();
        assert_eq!(generated.len(), reference.len());
        for (native, reference) in generated.iter().zip(&reference) {
            assert_eq!(native.start_time, reference.start_time);
            assert_eq!(native.duration, reference.duration);
            assert_eq!(native.target, reference.target);
            let bound = child.effect.bind_params_pairs(&reference.params).unwrap();
            for pixel in native.target.pixels.iter() {
                for progress in [0.0, 0.25, 0.75, 1.0] {
                    let context = RunContext {
                        progress,
                        time: SampleDuration::from_ticks(
                            (progress * native.duration.as_ticks() as f32).round() as u32,
                        ),
                        duration: native.duration,
                        pixel_index: pixel.pixel_index,
                        pixel_count: pixel.pixel_count,
                        pixel_fraction: pixel.pixel_fraction,
                    };
                    let sample_time = native
                        .start_time
                        .checked_add_duration(context.time)
                        .unwrap();
                    assert_eq!(
                        native.sample.sample(&context, sample_time).unwrap(),
                        child
                            .effect
                            .sample_bound(&bound, &context, &mut Default::default())
                            .unwrap()
                    );
                }
            }
        }
    }
}
