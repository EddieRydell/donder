//! Host-prepared mark fixtures using the same editable effects as projects.
use donder_language::dsl::{GeneratorBinding, GeneratorContext, GeneratorInput, compile_effects};
use donder_runtime::dsl::{BoundParams, Identifier, TargetItemValue, TargetValue, Value};
use donder_runtime::sequence::PreparedSequence;
use donder_runtime::signal::*;
use donder_runtime::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};

#[allow(dead_code)] // Shared device-profile and host benchmark fixture.
pub fn mark_show(count: usize, pulse: bool) -> PreparedSequence {
    let definitions = compile_effects(include_str!(
        "../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap();
    let generator_name = if pulse { "MarkPulse" } else { "MarkChase" };
    let child_name = if pulse { "Pulse" } else { "Chase" };
    let generator = definitions
        .iter()
        .find(|definition| definition.effect.name.as_str() == generator_name)
        .unwrap();
    let child = definitions
        .iter()
        .find(|definition| definition.effect.name.as_str() == child_name)
        .unwrap();
    assert!(generator.emitted_references.iter().all(|emission| {
        emission.reference
            == donder_language::imports::SourceReference::Local(
                Identifier::new(child_name.into()).unwrap(),
            )
    }));
    let show = super::workload::show(
        count,
        child.effect.sample_program().unwrap().clone(),
        BoundParams::default(),
    );
    let mut signals = show.signals().clone();
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
    let params = generator.effect.bind_params_pairs(&overrides).unwrap();
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: signals.duration,
        target: TargetValue {
            groups: vec![
                TargetItemValue {
                    pixels: signals.target_pixels.to_vec().into(),
                }
                .into(),
            ],
        }
        .into(),
    };
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
        .specialize(&context)
        .unwrap();
    // These fixtures supply fixed inputs and use no live clock expressions.
    assert!(generated.calculations.is_empty());
    assert!(generated.children.len() >= 32);
    let mut pixels = signals.target_pixels.to_vec();
    let mut targets = signals.targets.to_vec();
    signals.effects = generated
        .children
        .into_iter()
        .map(|emission| {
            assert_eq!(emission.definition.0, 0);
            let target = if emission.target == context.target.groups[0] {
                0
            } else {
                let index = targets.len();
                let start = pixels.len();
                pixels.extend_from_slice(&emission.target.pixels);
                targets.push(PreparedTarget {
                    pixels: start..pixels.len(),
                    sample_count: 0,
                });
                index
            };
            let params = emission
                .params
                .into_iter()
                .map(|(name, binding)| {
                    let GeneratorBinding::Constant(value) = binding else {
                        panic!("fixed mark fixture produced a live child parameter");
                    };
                    (name, value)
                })
                .collect::<Vec<_>>();
            PreparedEffect {
                start_time: emission.start_time,
                duration: emission.duration,
                target,
                implementation: PreparedEffectImplementation::Dsl {
                    program: 0,
                    bound_params: child.effect.bind_params_pairs(&params).unwrap(),
                },
                automation: None,
            }
        })
        .collect();
    signals.targets = targets.into();
    signals.target_pixels = pixels.into();
    signals.effects_by_layer = vec![(0..signals.effects.len()).collect()].into();
    PreparedSequence::new(signals, show.patch().clone(), show.outputs().into())
}
