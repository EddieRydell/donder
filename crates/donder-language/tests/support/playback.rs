use donder_language::dsl::{CompiledEffect, CompiledOperator};
use donder_runtime::{
    BoundParams, DslBindCache, FixtureGeometry, OperatorDefinition, OperatorInvocation,
    OutputEncoding, PreparedSequence, RgbOrder, SampleDefinition, SampleInvocation, SampleTime,
    SequenceBuilder, SequenceRoot, SequenceTiming, TargetHandle, TargetScope,
};
use std::num::NonZeroU32;

pub const IDENTITY_SOURCE: &str =
    "operator Identity { input Signal source; color sample() { return source.at(seconds()); } }";

pub fn timing(duration: u32) -> SequenceTiming {
    SequenceTiming::admit(
        NonZeroU32::new(120).unwrap(),
        NonZeroU32::new((u64::from(duration) * 120).div_ceil(1_000_000) as u32).unwrap(),
        NonZeroU32::new(duration).unwrap(),
        Box::new([]),
    )
    .unwrap()
}

pub fn build(
    count: usize,
    timing: SequenceTiming,
    build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>, TargetHandle<'id>) -> SequenceRoot<'id>,
) -> PreparedSequence {
    PreparedSequence::build(timing, |builder| {
        let fixture = builder.fixture(
            0,
            FixtureGeometry::admit((0..count).map(|pixel| [pixel as f32, 0.0]).collect()).unwrap(),
        );
        let target = builder.target([fixture], TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Grb), None);
        build(builder, target)
    })
}

pub fn sample(effect: &CompiledEffect, params: &BoundParams) -> SampleInvocation {
    SampleDefinition::new(effect.sample_program().clone())
        .bind(params.iter_values().collect(), &mut DslBindCache::default())
        .unwrap()
}

pub fn operator(operator: &CompiledOperator, params: &BoundParams) -> OperatorInvocation {
    operator_program(operator.program(), params)
}

pub fn operator_program(
    operator: &donder_runtime::OperatorProgram,
    params: &BoundParams,
) -> OperatorInvocation {
    OperatorDefinition::new(operator.clone())
        .bind(params.iter_values().collect(), &mut DslBindCache::default())
        .unwrap()
}

pub fn map_operator_bytecode(
    operator: &CompiledOperator,
    edit: impl FnOnce(&mut donder_runtime::BytecodeProgram),
) -> donder_runtime::OperatorProgram {
    let (mut bytecode, inputs, parameters) = operator.program().clone().into_parts();
    edit(&mut bytecode);
    donder_runtime::OperatorProgram::admit(bytecode, inputs, parameters).unwrap()
}

pub fn show(count: usize, invocation: &SampleInvocation, layers: usize) -> PreparedSequence {
    chain(count, invocation, layers, &[])
}

pub fn chain(
    count: usize,
    invocation: &SampleInvocation,
    layers: usize,
    operators: &[OperatorInvocation],
) -> PreparedSequence {
    build(count, timing(8_000_000), |builder, target| {
        let window = builder.whole_sequence();
        let layers: Vec<_> = (0..layers)
            .map(|_| {
                let effect = builder.sample(invocation, window, target);
                builder.layer(true, [effect])
            })
            .collect();
        let mut signal = builder.mix(layers);
        for operator in operators {
            signal = builder.operator(operator, |_| signal);
        }
        builder.output([signal])
    })
}

pub fn time(frame: usize) -> SampleTime {
    SampleTime::from_ticks(3_000_000 + frame as u32 * 8_333)
}
