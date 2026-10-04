#[allow(dead_code)]
#[path = "support/mark_workload.rs"]
mod mark_workload;
#[allow(dead_code)]
#[path = "support/playback.rs"]
mod playback;
#[allow(dead_code)]
#[path = "support/workload.rs"]
mod workload;

use donder_language::dsl::BoundParams;
use donder_language::dsl::CompiledOperator;
use donder_language::dsl::Value;
use donder_language::dsl::bytecode::Instruction;

#[test]
fn temporal_frame_caches_match_uncached_sampling_through_nested_operators() {
    let bases = [
        workload::chase_pulse_show(200, 4),
        mark_workload::mark_show(200, true),
        mark_workload::mark_show(200, false),
    ];
    let operators = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap();
    let compiled = |name| {
        operators
            .iter()
            .find(|operator| operator.name().as_str() == name)
            .unwrap()
    };
    for (base, name) in bases
        .iter()
        .flat_map(|base| ["Delay", "Echo"].map(|name| (base, name)))
    {
        let declaration = compiled(name);
        let overrides = declaration
            .params()
            .iter()
            .filter_map(|param| {
                let value = match param.name.as_str() {
                    "seconds" => Value::Float(0.025),
                    "repeats" => Value::Int(3),
                    "decay" => Value::Float(0.6),
                    _ => return None,
                };
                Some((param.name.clone(), value))
            })
            .collect::<Vec<_>>();
        let temporal_params = donder_language::dsl::bind_params(
            declaration.params(),
            overrides.iter().map(|(name, value)| (name, value)),
        )
        .unwrap();
        assert!([compiled("Invert"), declaration, compiled("Multiply")].iter().any(
            |operator| operator.bytecode().instructions.iter().any(
                |instruction| matches!(instruction, Instruction::SignalSample { frame_cache, .. } if *frame_cache != u32::MAX)
            )
        ));
        let prepare = |cached| {
            let variant = |operator: &CompiledOperator| {
                if cached {
                    operator.program().clone()
                } else {
                    playback::map_operator_bytecode(operator, |program| {
                        for instruction in &mut program.instructions {
                            if let Instruction::SignalSample { frame_cache, .. } = instruction {
                                *frame_cache = u32::MAX;
                            }
                        }
                    })
                }
            };
            let invert = playback::operator_program(
                &variant(compiled("Invert")),
                &BoundParams::bind_values(&[], vec![]).unwrap(),
            );
            let temporal = playback::operator_program(&variant(declaration), &temporal_params);
            let multiply = playback::operator_program(
                &variant(compiled("Multiply")),
                &BoundParams::bind_values(&[], vec![]).unwrap(),
            );
            base.clone()
                .prepare_with(|builder, layers, _signal| {
                    // The original fixture queries layer zero, including when the
                    // Chase/Pulse workload retains four layers.
                    let source = layers[0];
                    let inverted = builder.operator(&invert, |_| source);
                    let delayed = builder.operator(&temporal, |_| inverted);
                    let inputs = [delayed, source];
                    let multiplied = builder.operator(&multiply, |input| inputs[input]);
                    let repeated = builder.operator(&temporal, |_| multiplied);
                    builder.output([repeated])
                })
                .into_playback()
        };
        let mut cached = prepare(true);
        let mut uncached = prepare(false);
        for frame in [0, 1, 4, 31, 12, 4, 0, 31] {
            let actual = cached.evaluate(workload::time(frame));
            let expected = uncached.evaluate(workload::time(frame));
            assert_eq!(actual.colors(), expected.colors(), "{name} frame {frame}");
        }
    }
}
