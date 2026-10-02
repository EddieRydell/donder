use donder_language::dsl::{compile_effects, compile_operators};
use donder_runtime::{
    Color, DslBindCache, FixtureGeometry, Instruction, OperatorDefinition, OperatorProgram,
    PreparedSequence, SampleDefinition, SampleTime, SequenceTiming, TargetScope,
};
use std::num::NonZeroU32;

fn sequence(query: Option<(&str, bool)>) -> PreparedSequence {
    let effect = compile_effects(
        "effect Positions { color sample() {
            return rgb(pixel_index() / 8.0, seconds() / 8.0, 0.0);
        } }",
    )
    .unwrap()
    .remove(0);
    let sample = SampleDefinition::new(effect.sample_program().clone())
        .bind(vec![], &mut DslBindCache::default())
        .unwrap();
    let operator = query.map(|(query, cached)| {
        let compiled = compile_operators(&format!(
            "operator Spatial {{ input Signal source; color sample() {{ return {query}; }} }}"
        ))
        .unwrap()
        .remove(0);
        let program = if cached {
            compiled.program().clone()
        } else {
            let mut bytecode = compiled.program().clone().into_parts().0;
            for instruction in &mut bytecode.instructions {
                if let Instruction::SignalSample { frame_cache, .. } = instruction {
                    *frame_cache = u32::MAX;
                }
            }
            OperatorProgram::admit(
                bytecode,
                compiled.inputs().len(),
                compiled
                    .params()
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
            )
            .unwrap()
        };
        OperatorDefinition::new(program)
            .bind(vec![], &mut DslBindCache::default())
            .unwrap()
    });
    let timing = SequenceTiming::admit(
        NonZeroU32::new(120).unwrap(),
        NonZeroU32::new(960).unwrap(),
        NonZeroU32::new(8_000_000).unwrap(),
        Box::new([]),
    )
    .unwrap();
    PreparedSequence::build(timing, |builder| {
        let fixtures = [0, 1].map(|id| {
            builder.fixture(
                id,
                FixtureGeometry::admit(vec![[0.0; 2]; 4].into()).unwrap(),
            )
        });
        let target = builder.target(fixtures, TargetScope::WholeTarget);
        let effect = builder.sample(&sample, builder.whole_sequence(), target);
        let layer = builder.layer(true, [effect]);
        let output = operator
            .as_ref()
            .map_or(layer, |operator| builder.operator(operator, |_| layer));
        builder.output([output])
    })
}

#[test]
fn spatial_queries_match_explicit_source_pixels_with_and_without_frame_caches() {
    for (query, indices) in [
        (
            "source.at(seconds(), pixel_count() - 1 - pixel_index())",
            vec![
                Some(3),
                Some(2),
                Some(1),
                Some(0),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        (
            "source.at(seconds(), pixel_index() + 1)",
            vec![
                Some(1),
                Some(2),
                Some(3),
                None,
                Some(5),
                Some(6),
                Some(7),
                None,
            ],
        ),
        (
            "source.at_global(seconds(), 7 - pixel_index())",
            vec![
                Some(7),
                Some(6),
                Some(5),
                Some(4),
                Some(7),
                Some(6),
                Some(5),
                Some(4),
            ],
        ),
        ("source.at(seconds(), -1)", vec![None; 8]),
        ("source.at_global(seconds(), 8)", vec![None; 8]),
        (
            "max(source.at_global(seconds(), 1), source.at_global(seconds(), 6))",
            vec![Some(6); 8],
        ),
    ] {
        for cached in [true, false] {
            let mut workspace = sequence(Some((query, cached))).into_playback();
            let mut source_workspace = sequence(None).into_playback();
            for ticks in [0, 500000, 2000000, 100000, 0] {
                let time = SampleTime::from_ticks(ticks);
                let source = source_workspace.evaluate(time).colors();
                let expected = indices
                    .iter()
                    .map(|index| {
                        index.map(|index| source[index]).unwrap_or(Color {
                            red: 0,
                            green: 0,
                            blue: 0,
                        })
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    workspace.evaluate(time).colors(),
                    expected,
                    "{query}, cached={cached}, time={ticks}"
                );
            }
        }
    }
}
