//! Mixed playback workload: four independently evaluated layers, followed by
//! hue adjustment, an afterglow, and output dimming.
use crate::playback::{build, definition, invoke, timing};
use donder_language::compiler::{Instance, ProgramConstants};
use donder_runtime::PreparedShow;
use donder_runtime_types::{SampleTime, Value};

pub fn layered_600() -> PreparedShow {
    let constants = ProgramConstants {
        pixel_count: Some(600),
        duration_seconds: Some(8.0),
    };
    let effects = crate::fixtures::cases().map(|(name, source, params)| {
        crate::playback::bind(
            &crate::fixtures::prepared_effect(name, source, params).instance(constants),
        )
    });
    let source = include_str!("../../../../examples/starter/operators/standard.donder");
    let operators = [
        ("HueShift", vec![("shift", Value::Float(0.13))]),
        ("Afterglow", vec![("seconds", Value::Float(0.075))]),
        ("Dim", vec![("amount", Value::Float(0.8))]),
    ]
    .map(|(name, values)| {
        invoke(&definition(source, name), &values).instance(ProgramConstants {
            pixel_count: None,
            ..constants
        })
    });
    // Each operator consumes only the previous one, so preparation fuses the chain.
    let mut fused: Vec<Instance> = Vec::new();
    for operator in operators {
        if let Some(source) = fused.last()
            && let Some(operator) = operator.fuse_input(0, source)
        {
            *fused.last_mut().unwrap() = operator;
        } else {
            fused.push(operator);
        }
    }
    let fused: Vec<_> = fused.iter().map(crate::playback::bind).collect();
    let session = timing(8_000_000);
    let duration = session.duration();
    build(600, session, |builder, target| {
        let layers = effects
            .iter()
            .map(|program| {
                let clip = builder.clip(None, program, SampleTime::from_ticks(0), duration, target);
                builder.layer(true, [clip])
            })
            .collect::<Vec<_>>();
        let mut signal = builder.mix(layers);
        for operator in &fused {
            signal = builder.operator(operator, |_| signal);
        }
        Some(signal)
    })
}
