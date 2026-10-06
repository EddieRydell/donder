//! Small prepared shows over one fixture.
use donder_language::dsl::Invocation;
use donder_language::dsl::OperatorInvocation;
use donder_language::dsl::ProgramConstants;
use donder_language::dsl::SampleInvocation;
use donder_language::dsl::{CompiledEffect, CompiledOperator, compile_effects, compile_operators};
use donder_language::execution::FixtureGeometry;
use donder_language::execution::OutputEncoding;
use donder_language::execution::RgbOrder;
use donder_language::execution::SequenceTiming;
use donder_language::execution::TargetScope;
use donder_language::values::SampleTime;
use donder_runtime::PreparedSequence;
use donder_runtime::SequenceBuilder;
use donder_runtime::SequenceRoot;
use donder_runtime::TargetHandle;
use std::num::NonZeroU32;

pub const IDENTITY_SOURCE: &str = "operator Identity { input source; sample { source } }";

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

/// The only declaration of an effect source.
pub fn compile_effect(source: &str) -> CompiledEffect {
    let mut effects = compile_effects(source).unwrap();
    assert_eq!(effects.len(), 1);
    effects.remove(0)
}

/// The only declaration of an operator source.
pub fn compile_operator(source: &str) -> CompiledOperator {
    let mut operators = compile_operators(source).unwrap();
    assert_eq!(operators.len(), 1);
    operators.remove(0)
}

/// Lower an effect's invocation with nothing known about its placement, so
/// the clip duration and pixel count remain context reads.
pub fn lower_sample(invocation: &Invocation) -> SampleInvocation {
    invocation.instance(ProgramConstants::default()).sample()
}

pub fn lower_operator(invocation: &Invocation) -> OperatorInvocation {
    invocation.instance(ProgramConstants::default()).operator()
}

/// The effect with its declared defaults.
pub fn sample(effect: &CompiledEffect) -> SampleInvocation {
    lower_sample(&effect.bind(std::iter::empty()).unwrap())
}

/// The operator with its declared defaults.
pub fn operator(operator: &CompiledOperator) -> OperatorInvocation {
    lower_operator(&operator.bind(std::iter::empty()).unwrap())
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
