use donder_language::compiler::{CompiledOperator, Instance, ProgramConstants, compile_operators};
use donder_runtime_types::Color;
use donder_runtime_types::{Identifier, Value};
use donder_test_support::marks as mark_workload;
use donder_test_support::playback;
use donder_test_support::workload;

/// What preparation knows about an operator in the eight-second fixture sequence.
const OPERATOR_CONSTANTS: ProgramConstants = ProgramConstants {
    pixel_count: None,
    duration_seconds: Some(8.0),
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// Lowered programs as preparation emits them.
    Cached,
    /// The same programs with every whole-frame signal cache disabled.
    Uncached,
    /// The chain substituted into one program.
    Fused,
}

fn frame_caches(instance: &Instance) -> usize {
    workload::cached_samples(instance.operator().program().bytecode())
}

/// `operator` with the given values for whichever of these params it declares.
fn instance(operator: &CompiledOperator, values: &[(&str, Value)]) -> Instance {
    let values: Vec<_> = values
        .iter()
        .filter(|(name, _)| {
            operator
                .params()
                .iter()
                .any(|param| param.name.as_str() == *name)
        })
        .map(|(name, value)| (Identifier::new((*name).into()).unwrap(), value.clone()))
        .collect();
    operator
        .bind(values.iter().map(|(name, value)| (name, value)))
        .unwrap()
        .instance(OPERATOR_CONSTANTS)
}

#[test]
fn temporal_frame_caches_and_fusion_match_uncached_sampling_through_nested_operators() {
    let bases = [
        workload::chase_pulse_show(200, 4),
        mark_workload::mark_show(200, true),
        mark_workload::mark_show(200, false),
    ];
    let operators = compile_operators(include_str!(
        "../../../examples/starter/operators/standard.donder"
    ))
    .unwrap();
    let compiled = |name| {
        operators
            .iter()
            .find(|operator| operator.name().as_str() == name)
            .unwrap()
    };
    let invert = instance(compiled("Invert"), &[]);
    let multiply = instance(compiled("Multiply"), &[]);
    // Current-time samples are query-uniform, so they read whole-frame caches.
    assert!(frame_caches(&invert) > 0);
    assert!(frame_caches(&multiply) > 0);
    for (base, name) in bases
        .iter()
        .flat_map(|base| ["Delay", "Echo"].map(|name| (base, name)))
    {
        let temporal = instance(
            compiled(name),
            &[
                ("seconds", Value::Float(0.025)),
                ("repeats", Value::Int(3)),
                ("decay", Value::Float(0.6)),
            ],
        );
        if name == "Delay" {
            // A fixed offset from the query time is query-uniform too.
            assert!(frame_caches(&temporal) > 0);
        }
        let prepare = |variant| {
            base.clone()
                .prepare_with(|builder, layers, _signal| {
                    // The original fixture queries layer zero, including when the
                    // Chase/Pulse workload retains four layers.
                    let source = layers[0];
                    if variant == Variant::Fused {
                        let delayed = temporal.fuse_input(0, &invert).unwrap();
                        let multiplied = multiply.fuse_input(0, &delayed).unwrap();
                        let repeated = temporal.fuse_input(0, &multiplied).unwrap().operator();
                        // Multiply's second input, then Invert's input.
                        assert_eq!(repeated.program().input_count(), 2);
                        let repeated = builder.operator(&repeated, |_| source);
                        return builder.output([repeated]);
                    }
                    let [invert, temporal, multiply] =
                        [&invert, &temporal, &multiply].map(|instance| {
                            let lowered = instance.operator();
                            if variant == Variant::Uncached {
                                workload::edit_operator(&lowered, workload::without_frame_caches)
                            } else {
                                lowered
                            }
                        });
                    let inverted = builder.operator(&invert, |_| source);
                    let delayed = builder.operator(&temporal, |_| inverted);
                    let inputs = [delayed, source];
                    let multiplied = builder.operator(&multiply, |input| inputs[input]);
                    let repeated = builder.operator(&temporal, |_| multiplied);
                    builder.output([repeated])
                })
                .into_playback()
        };
        let mut cached = prepare(Variant::Cached);
        let mut uncached = prepare(Variant::Uncached);
        let mut fused = prepare(Variant::Fused);
        let mut lit = false;
        for frame in [0, 1, 4, 31, 12, 4, 0, 31] {
            let expected = uncached.evaluate(playback::time(frame));
            lit |= expected.colors().iter().any(|&color| color != Color::BLACK);
            assert_eq!(
                cached.evaluate(playback::time(frame)).colors(),
                expected.colors(),
                "{name} frame {frame}"
            );
            assert_eq!(
                fused.evaluate(playback::time(frame)).colors(),
                expected.colors(),
                "fused {name} frame {frame}"
            );
        }
        assert!(lit, "{name} rendered only black");
    }
}
