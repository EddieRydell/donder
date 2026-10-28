//! Host-prepared mark fixtures using the same editable effects as projects.
use crate::playback::{definition, invoke};
use crate::workload::{Workload, sample_fixture};
use donder_runtime_types::Value;
use donder_runtime_types::{
    Color, Curve, CurvePoint, Gradient, GradientStop, Marks, SampleDuration, SampleTime,
};

pub fn mark_show(count: usize, pulse: bool) -> Workload {
    let effect = definition(
        include_str!("../../../examples/starter/effects/standard.donder"),
        if pulse { "MarkPulse" } else { "MarkChase" },
    );
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
                color: Color::from_bytes([193, 89, 47]),
            }],
        }
        .into(),
    );
    let mut overrides = vec![(
        "beats",
        Value::Marks(
            Marks::new((0..32).map(|index| SampleDuration::from_ticks(2_000_000 + index * 50_000)))
                .into(),
        ),
    )];
    if pulse {
        overrides.extend([
            ("accent", gradient),
            ("pulse_shape", pulse_shape),
            ("decay_seconds", Value::Float(1.2)),
        ]);
    } else {
        overrides.extend([
            ("gradients", Value::Array(vec![gradient].into())),
            ("chase_position", ramp),
            ("pulse_shape", pulse_shape),
            ("chase_seconds", Value::Float(1.2)),
        ]);
    }
    Workload::new(
        count,
        vec![vec![sample_fixture(
            &invoke(&effect, &overrides),
            count,
            SampleTime::from_ticks(0),
            SampleDuration::from_ticks(8_000_000),
        )]],
    )
}
