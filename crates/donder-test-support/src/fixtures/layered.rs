//! Mixed playback workload: four independently evaluated layers, followed by
//! hue adjustment, a two-repeat temporal echo, and output dimming.
use donder_language::dsl::{ProgramConstants, Value, compile_operators};
use donder_runtime::PreparedSequence;

pub fn layered_600() -> PreparedSequence {
    let constants = ProgramConstants {
        pixel_count: Some(600),
        duration_seconds: Some(8.0),
    };
    let effects = crate::fixtures::cases().map(|(name, source, params)| {
        crate::fixtures::prepared_effect(name, source, params)
            .instance(constants)
            .sample()
    });
    let definitions = compile_operators(include_str!(
        "../../../../examples/starter/operators/standard.donder"
    ))
    .unwrap();
    let operators = [
        ("HueShift", vec![("shift", Value::Float(0.13))]),
        (
            "Echo",
            vec![
                ("seconds", Value::Float(0.075)),
                ("repeats", Value::Int(2)),
                ("decay", Value::Float(0.5)),
            ],
        ),
        ("Dim", vec![("amount", Value::Float(0.8))]),
    ]
    .map(|(name, values)| {
        let definition = definitions
            .iter()
            .find(|op| op.name().as_str() == name)
            .unwrap();
        let values: Vec<_> = values
            .into_iter()
            .map(|(key, value)| {
                (
                    donder_language::dsl::Identifier::new(key.into()).unwrap(),
                    value,
                )
            })
            .collect();
        definition
            .bind(values.iter().map(|(key, value)| (key, value)))
            .unwrap()
            .instance(ProgramConstants {
                pixel_count: None,
                ..constants
            })
    });
    // Each operator consumes only the previous one, so preparation fuses the chain.
    let mut fused: Vec<donder_language::dsl::Instance> = Vec::new();
    for operator in operators {
        if let Some(source) = fused.last()
            && let Some(operator) = operator.fuse_input(0, source)
        {
            *fused.last_mut().unwrap() = operator;
        } else {
            fused.push(operator);
        }
    }
    let fused: Vec<_> = fused.iter().map(|operator| operator.operator()).collect();
    crate::playback::build(
        600,
        crate::playback::timing(8_000_000),
        |builder, target| {
            let window = builder.whole_sequence();
            let layers = effects
                .iter()
                .map(|effect| {
                    let effect = builder.sample(effect, window, target);
                    builder.layer(true, [effect])
                })
                .collect::<Vec<_>>();
            let mut signal = builder.mix(layers);
            for operator in &fused {
                signal = builder.operator(operator, |_| signal);
            }
            builder.output([signal])
        },
    )
}
