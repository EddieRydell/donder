//! Compiling, invoking and lowering definitions, and small prepared shows
//! over one fixture.
use donder_language::compiler::{
    CompiledDefinition, Instance, Invocation, ProgramConstants, compile_script,
};
use donder_runtime::{PreparedShow, ShowBuilder, SignalHandle, TargetHandle};
use donder_runtime_types::{
    BindingError, BoundProgram, Color, FixtureGeometry, OutputEncoding, RgbOrder, SampleTime,
    SequenceTiming, TargetGeometry, TargetScope, Value,
};
use std::num::NonZeroU32;

pub const IDENTITY_SOURCE: &str = "operator Identity { input source; sample { source } }";

pub fn timing(duration: u32) -> SequenceTiming {
    SequenceTiming::new(
        NonZeroU32::new(120).unwrap(),
        NonZeroU32::new((u64::from(duration) * 120).div_ceil(1_000_000) as u32).unwrap(),
        NonZeroU32::new(duration).unwrap(),
    )
}

/// A fixture of `count` pixels in a row, its first registered.
pub fn row(count: usize) -> FixtureGeometry {
    FixtureGeometry::admit((0..count).map(|pixel| [pixel as f32, 0.0]).collect()).unwrap()
}

/// The target of every pixel of the first registered fixture, `geometry`.
pub(crate) fn whole<'id>(
    builder: &mut ShowBuilder<'id>,
    geometry: &FixtureGeometry,
    scope: TargetScope,
) -> TargetHandle<'id> {
    builder.target(&TargetGeometry::new([(0, geometry)], scope))
}

/// A `count`-pixel fixture with a GRB port, built by `build`.
pub fn build(
    count: usize,
    timing: SequenceTiming,
    build: impl for<'id> FnOnce(&mut ShowBuilder<'id>, TargetHandle<'id>) -> Option<SignalHandle<'id>>,
) -> PreparedShow {
    PreparedShow::build(timing, |builder| {
        let geometry = row(count);
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Grb), None);
        build(builder, target)
    })
}

/// The only declaration of `source`.
pub fn compile(source: &str) -> CompiledDefinition {
    let mut definitions = compile_script(source).unwrap();
    assert_eq!(definitions.len(), 1);
    definitions.remove(0)
}

/// The declaration of `source` named `name`.
pub(crate) fn definition(source: &str, name: &str) -> CompiledDefinition {
    compile_script(source)
        .unwrap()
        .into_iter()
        .find(|definition| definition.name().as_str() == name)
        .unwrap()
}

/// `definition` with `values` by name and its declared defaults for the rest.
pub fn invoke(definition: &CompiledDefinition, values: &[(&str, Value)]) -> Invocation {
    try_invoke(definition, values).unwrap()
}

/// [`invoke`], or why the values are rejected.
pub fn try_invoke(
    definition: &CompiledDefinition,
    values: &[(&str, Value)],
) -> Result<Invocation, BindingError> {
    assert!(
        values.iter().all(|(name, _)| definition
            .params()
            .iter()
            .any(|param| param.name.as_str() == *name)),
        "values name parameters: {values:?}"
    );
    let values = definition
        .params()
        .iter()
        .map(|param| {
            values
                .iter()
                .find(|(name, _)| *name == param.name.as_str())
                .map_or_else(
                    || param.default.clone().unwrap(),
                    |(_, value)| value.clone(),
                )
        })
        .collect();
    definition.invoke(values, Box::new([]))
}

/// `instance` lowered and bound.
pub fn bind(instance: &Instance) -> BoundProgram {
    let lowered = instance.lower();
    BoundProgram::bind(lowered.program, lowered.values, lowered.automation).unwrap()
}

/// `invocation` lowered with nothing known about its placement, so the
/// clip duration and pixel count remain context reads.
pub fn lower(invocation: &Invocation) -> BoundProgram {
    bind(&invocation.instance(ProgramConstants::default()))
}

/// `definition` with its declared defaults, lowered with nothing known about
/// its placement.
pub fn program(definition: &CompiledDefinition) -> BoundProgram {
    lower(&invoke(definition, &[]))
}

/// `layers` layers of `program` over the whole eight-second sequence, mixed
/// and passed through `operators`.
pub fn show(
    count: usize,
    program: &BoundProgram,
    layers: usize,
    operators: &[BoundProgram],
) -> PreparedShow {
    let duration = timing(8_000_000).duration();
    build(count, timing(8_000_000), |builder, target| {
        let layers: Vec<_> = (0..layers)
            .map(|_| {
                let clip = builder.clip(None, program, SampleTime::from_ticks(0), duration, target);
                builder.layer(true, [clip])
            })
            .collect();
        let mut signal = builder.mix(layers);
        for operator in operators {
            signal = builder.operator(operator, |_| signal);
        }
        Some(signal)
    })
}

pub fn time(frame: usize) -> SampleTime {
    SampleTime::from_ticks(3_000_000 + frame as u32 * 8_333)
}

/// Whether two frames agree to well within one 8-bit level per channel: a
/// fused operator may order its float arithmetic differently from one
/// reading a frame.
pub fn assert_frames_close(left: &[Color], right: &[Color], what: &str) {
    assert_eq!(left.len(), right.len(), "{what}");
    for (index, (a, b)) in left.iter().zip(right).enumerate() {
        let close = |x: f32, y: f32| (x - y).abs() <= 1.0 / 1024.0 || x.to_bits() == y.to_bits();
        assert!(
            close(a.red, b.red) && close(a.green, b.green) && close(a.blue, b.blue),
            "{what}: pixel {index}: {a:?} != {b:?}"
        );
    }
}
