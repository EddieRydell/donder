use donder_runtime::dsl::{BoundParams, bytecode::BytecodeProgram};
use donder_runtime::sequence::PreparedSequence;
use donder_runtime::signal::*;
use donder_runtime::values::{SampleDuration, SampleTime};
extern crate alloc;
use alloc::vec;

#[allow(dead_code)] // Shared native-generator fixture; construction is outside sampling.
pub fn mark_show(
    count: usize,
    pulse: bool,
    edge_fade: f32,
    program: BytecodeProgram,
) -> PreparedSequence {
    use donder_runtime::dsl::{
        GeneratorContext, Identifier, ParamDecl, TargetItemValue, TargetPixelValue, TargetValue,
        Type, Value,
    };
    use donder_runtime::values::{Color, Curve, CurvePoint, Gradient, GradientStop, Marks};
    let mut show = super::workload::show(count, program, BoundParams::default());
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
    let mut values = vec![
        (
            Type::Marks,
            Value::Marks(
                Marks {
                    marks: (0..32)
                        .map(|i| SampleDuration::from_ticks(2_000_000 + i * 50_000))
                        .collect(),
                }
                .into(),
            ),
        ),
        (
            Type::Color,
            Value::Color(Color {
                red: 0,
                green: 0,
                blue: 0,
            }),
        ),
    ];
    if pulse {
        values.extend([
            (Type::Gradient, gradient),
            (Type::Curve, ramp),
            (Type::Float, Value::Float(0.35)),
            (Type::Float, Value::Float(0.0)),
            (Type::Float, Value::Float(1.2)),
            (Type::Int, Value::Int(5)),
            (Type::Float, Value::Float(edge_fade)),
            (Type::Int, Value::Int(3)),
            (Type::Float, Value::Float(0.0)),
        ]);
    } else {
        let mode = Identifier::new("per_pulse".into()).unwrap();
        values.extend([
            (Type::Enum(vec![mode.clone()]), Value::Enum(mode)),
            (
                Type::Array(alloc::boxed::Box::new(Type::Gradient)),
                Value::Array(vec![gradient].into()),
            ),
            (Type::Curve, ramp.clone()),
            (Type::Float, Value::Float(0.35)),
            (Type::Float, Value::Float(0.0)),
            (Type::Float, Value::Float(1.2)),
            (Type::Float, Value::Float(8.0)),
            (Type::Int, Value::Int(5)),
            (
                Type::Array(alloc::boxed::Box::new(Type::Curve)),
                Value::Array(vec![ramp.clone()].into()),
            ),
            (Type::Curve, ramp),
        ]);
    }
    let declarations = values
        .into_iter()
        .enumerate()
        .map(|(index, (ty, value))| ParamDecl {
            fixed: false,
            name: Identifier::new(alloc::format!("p{index}")).unwrap(),
            ty,
            default: Some(value),
        })
        .collect::<vec::Vec<_>>();
    let params = BoundParams::bind_pairs(&declarations, &[]).unwrap();
    let generator = donder_elaboration::native_effect::bind_prepared(
        if pulse {
            donder_runtime::BuiltinEffect::MarkPulse
        } else {
            donder_runtime::BuiltinEffect::MarkChase
        },
        params,
    )
    .unwrap();
    let context = GeneratorContext {
        start_time: SampleTime::from_ticks(0),
        duration: show.signals.duration,
        target: TargetValue {
            groups: vec![
                TargetItemValue {
                    pixels: show
                        .signals
                        .target_pixels
                        .iter()
                        .map(|p| TargetPixelValue {
                            fixture_index: p.fixture_index as i32,
                            fixture_pixel_index: p.fixture_pixel_index as i32,
                            pixel_index: p.pixel_index as i32,
                            pixel_count: p.pixel_count as i32,
                            pixel_fraction: p.pixel_fraction,
                        })
                        .collect::<vec::Vec<_>>()
                        .into(),
                }
                .into(),
            ],
        }
        .into(),
    };
    let generated = generator.generate(&context).unwrap();
    assert!(generated.len() >= 32);
    let mut pixels = show.signals.target_pixels.to_vec();
    let mut targets = show.signals.targets.to_vec();
    show.signals.effects = generated
        .into_iter()
        .map(|child| {
            let target = if child.target == context.target.groups[0] {
                0
            } else {
                let target = targets.len() as u32;
                let start = pixels.len() as u32;
                pixels.extend(child.target.pixels.iter().map(|p| PreparedPixel {
                    fixture_index: p.fixture_index as u16,
                    fixture_pixel_index: p.fixture_pixel_index as u16,
                    pixel_index: p.pixel_index as u32,
                    pixel_count: p.pixel_count as u32,
                    pixel_fraction: p.pixel_fraction,
                }));
                targets.push(PreparedTarget {
                    pixels: start..pixels.len() as u32,
                    sample_count: 0,
                });
                target
            };
            PreparedEffect {
                start_time: child.start_time,
                duration: child.duration,
                target,
                implementation: PreparedEffectImplementation::Native {
                    sample: child.sample,
                    params: None,
                },
                automation: None,
            }
        })
        .collect();
    show.signals.targets = targets.into();
    show.signals.target_pixels = pixels.into();
    show.signals.programs = vec![].into();
    show.signals.effects_by_layer = vec![(0..show.signals.effects.len()).collect()].into();
    show
}
