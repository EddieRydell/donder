//! Host-prepared mark fixtures using the same editable effects as projects.
use super::workload::Workload;
use donder_language::dsl::{GeneratorBinding, GeneratorContext, GeneratorInput, compile_effects};
use donder_runtime::{BoundParams, DslBindCache, GeneratorPlayback, Identifier, Value};
use donder_runtime::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration};

#[allow(dead_code)] // Shared device-profile and host benchmark fixture.
pub fn mark_show(count: usize, pulse: bool) -> Workload {
    let definitions = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap();
    let generator_name = if pulse { "MarkPulse" } else { "MarkChase" };
    let child_name = if pulse { "Pulse" } else { "Chase" };
    let generator = definitions
        .iter()
        .find(|definition| definition.effect.name().as_str() == generator_name)
        .unwrap();
    assert!(generator.emitted_references.iter().all(|emission| {
        emission.reference
            == donder_language::imports::SourceReference::Local(
                Identifier::new(child_name.into()).unwrap(),
            )
    }));
    let ramp = Value::Curve(
        Curve {
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
        }
        .into(),
    );
    let pulse_shape = Value::Curve(
        Curve {
            points: vec![
                CurvePoint {
                    position: 0.0,
                    value: 1.0,
                },
                CurvePoint {
                    position: 1.0,
                    value: 0.0,
                },
            ],
        }
        .into(),
    );
    let gradient = Value::Gradient(
        Gradient {
            stops: vec![GradientStop {
                position: 0.0,
                color: Color {
                    red: 193,
                    green: 89,
                    blue: 47,
                },
            }],
        }
        .into(),
    );
    let mut overrides = vec![(
        Identifier::new("beats".into()).unwrap(),
        Value::Marks(
            Marks {
                marks: (0..32)
                    .map(|index| SampleDuration::from_ticks(2_000_000 + index * 50_000))
                    .collect(),
            }
            .into(),
        ),
    )];
    if pulse {
        overrides.extend([
            (Identifier::new("accent".into()).unwrap(), gradient),
            (Identifier::new("pulse_shape".into()).unwrap(), pulse_shape),
            (
                Identifier::new("decay_seconds".into()).unwrap(),
                Value::Float(1.2),
            ),
        ]);
    } else {
        overrides.extend([
            (
                Identifier::new("gradients".into()).unwrap(),
                Value::Array(vec![gradient].into()),
            ),
            (
                Identifier::new("chase_positions".into()).unwrap(),
                Value::Array(vec![ramp].into()),
            ),
            (Identifier::new("pulse_shape".into()).unwrap(), pulse_shape),
            (
                Identifier::new("chase_seconds".into()).unwrap(),
                Value::Float(1.2),
            ),
        ]);
    }
    let params = BoundParams::bind_pairs(generator.effect.params(), &overrides).unwrap();
    let generated = generator
        .effect
        .generator()
        .unwrap()
        .bind(
            &params
                .iter_values()
                .map(GeneratorInput::Fixed)
                .collect::<Vec<_>>(),
        )
        .unwrap()
        .specialize(&GeneratorContext {
            start_time: donder_runtime::SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            target: donder_runtime::TargetValue {
                groups: vec![
                    donder_runtime::TargetItemValue {
                        pixels: (0..count)
                            .map(|pixel| donder_runtime::PreparedPixel {
                                fixture_index: 0,
                                fixture_pixel_index: pixel as u32,
                                pixel_index: pixel,
                                pixel_count: count,
                                pixel_fraction: pixel as f32
                                    / count.saturating_sub(1).max(1) as f32,
                            })
                            .collect(),
                    }
                    .into(),
                ],
            }
            .into(),
        });
    // These fixtures supply fixed inputs and use no live clock expressions.
    assert!(generated.calculations.is_empty());
    assert!(generated.children.len() >= 32);
    for emission in generated.children {
        assert_eq!(emission.definition.0, 0);
        assert!(
            emission
                .params
                .iter()
                .all(|(_, binding)| matches!(binding, GeneratorBinding::Constant(_))),
            "fixed mark fixture produced a live child parameter"
        );
    }
    let linked = super::workload::link_generator(&definitions, generator_name);
    let playback = GeneratorPlayback::admit(
        linked,
        params.iter_values().collect(),
        Box::new([]),
        &mut DslBindCache::default(),
    )
    .unwrap();
    let show = Workload::generator(count, playback);
    // Fixed mark inputs must expand to at least one sample child per beat.
    assert!(show.clone().prepare().effect_count() >= 32);
    show
}
