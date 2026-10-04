//! Host-prepared mark fixtures using the same editable effects as projects.
use super::workload::{SampleFixture, Workload};
use donder_language::dsl::{Identifier, Value, bind_params, compile_effects};
use donder_language::values::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};

#[allow(dead_code)]
pub(crate) fn mark_show(count: usize, pulse: bool) -> Workload {
    let definitions = compile_effects(include_str!(
        "../../../../examples/starter/effects/standard.effect.donder"
    ))
    .unwrap();
    let effect_name = if pulse { "MarkPulse" } else { "MarkChase" };
    let effect = definitions
        .iter()
        .find(|definition| definition.name().as_str() == effect_name)
        .unwrap();
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
            Marks::new((0..32).map(|index| SampleDuration::from_ticks(2_000_000 + index * 50_000)))
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
            (Identifier::new("chase_position".into()).unwrap(), ramp),
            (Identifier::new("pulse_shape".into()).unwrap(), pulse_shape),
            (
                Identifier::new("chase_seconds".into()).unwrap(),
                Value::Float(1.2),
            ),
        ]);
    }
    let params = bind_params(
        effect.params(),
        overrides.iter().map(|(name, value)| (name, value)),
    )
    .unwrap();
    Workload::samples(
        count,
        vec![vec![SampleFixture {
            program: effect.sample_program().clone(),
            params,
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(8_000_000),
            automation: vec![],
        }]],
    )
}
