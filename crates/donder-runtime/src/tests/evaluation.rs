//! Private execution adapters shared by compiler/VM behavior tests. Programs
//! run as one-pixel strips. Automation is not applied, so every parameter slot
//! holds its bound value.
use super::playback;
use super::std;
use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{
    BoundParams, DslBindCache, RunContext, RuntimeError, STRIP, SpatialContext, Strip,
    StripSignals, StripWorkspace,
};
use crate::{PreparedSequence, SequenceBuilder, SequenceRoot, TargetHandle};
use donder_language::compiler::{
    CompiledEffect, CompiledOperator, Instance, Invocation, ParamDecl, ParamRange, ProgramConstants,
};
use donder_runtime_types::{Color, Curve, CurvePoint, Marks, SampleDuration, SampleTime};
use donder_runtime_types::{
    FixtureGeometry, OutputEncoding, PreparedAutomation, RgbOrder, TargetScope,
};
use donder_runtime_types::{Identifier, OperatorInvocation, SampleInvocation, Type, Value};
use std::prelude::rust_2024::*;

/// A test signal source. An error is reported as the operator's result.
pub(super) trait SignalSampler {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError>;
}

/// The pixel of a one-pixel strip; the first error wins.
struct Adapter<'a> {
    sampler: &'a mut dyn SignalSampler,
    error: Option<RuntimeError>,
}

impl Adapter<'_> {
    fn sample(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        match self.sampler.sample_signal(input, time, pixel, frame_cache) {
            Ok(color) => color,
            Err(error) => {
                self.error.get_or_insert(error);
                Color::BLACK
            }
        }
    }
}

impl StripSignals for Adapter<'_> {
    fn sample_strip(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        output: &mut [Color; STRIP],
    ) {
        output[0] = self.sample(input, time, SignalPixel::Current, frame_cache);
    }

    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        _: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        self.sample(input, time, pixel, frame_cache)
    }
}

/// A run context and the one pixel a test evaluates.
#[derive(Clone, Copy, Debug)]
pub(super) struct PixelContext {
    pub(super) run: RunContext,
    pub(super) index: i32,
    pub(super) fraction: f32,
}

impl core::ops::Deref for PixelContext {
    type Target = RunContext;

    fn deref(&self) -> &RunContext {
        &self.run
    }
}

pub(super) trait SampleEvaluation {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        workspace: &mut StripWorkspace,
    ) -> Color;
}

impl SampleEvaluation for SampleInvocation {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        workspace: &mut StripWorkspace,
    ) -> Color {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        let pixel = (context.index, context.fraction);
        crate::dsl::sample_once(
            self.program(),
            &params,
            &context.run,
            pixel,
            spatial,
            workspace,
        )
    }
}

pub(super) trait OperatorEvaluation {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut StripWorkspace,
    ) -> Result<Color, RuntimeError>;
}

impl OperatorEvaluation for OperatorInvocation {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut StripWorkspace,
    ) -> Result<Color, RuntimeError> {
        let params = BoundParams::from_validated(self.params(), &mut DslBindCache::default());
        let program = self.program();
        workspace.reserve(program.bytecode());
        let mut strip = Strip::new(program.bytecode(), &params, &context.run, None, workspace);
        let pixels = strip.pixels();
        pixels.index[0].set(context.index);
        pixels.fraction[0].set(context.fraction);
        pixels.x[0].set(spatial.position[0]);
        pixels.y[0].set(spatial.position[1]);
        let mut signals = Adapter {
            sampler,
            error: None,
        };
        let mut color = [Color::BLACK];
        strip.run(
            context.pixel_count as usize,
            spatial.min,
            spatial.max,
            &mut signals,
            &mut color,
        );
        match signals.error {
            Some(error) => Err(error),
            None => Ok(color[0]),
        }
    }
}

pub(super) fn context(count: usize, pixel: usize, frame: usize) -> PixelContext {
    let time = 3_000_000 + frame as u32 * 8_333;
    PixelContext {
        run: RunContext {
            progress: time as f32 / 8_000_000.0,
            time: SampleDuration::from_ticks(time),
            duration: SampleDuration::from_ticks(8_000_000),
            pixel_count: count as i32,
        },
        index: pixel as i32,
        fraction: pixel as f32 / (count - 1).max(1) as f32,
    }
}

pub(super) use donder_test_support::playback::{compile_effect, compile_operator};

/// A one-pixel context of a one-second sequence at `progress`, at time zero.
pub(super) fn one_pixel(progress: f32) -> PixelContext {
    PixelContext {
        run: RunContext {
            progress,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            pixel_count: 1,
        },
        index: 0,
        fraction: 0.0,
    }
}

pub(super) fn curve(points: &[(f32, f32)]) -> Value {
    Value::Curve(
        Curve {
            points: points
                .iter()
                .map(|&(position, value)| CurvePoint { position, value })
                .collect(),
        }
        .into(),
    )
}

pub(super) fn marks(ticks: &[u32]) -> Value {
    Value::Marks(Marks::new(ticks.iter().copied().map(SampleDuration::from_ticks)).into())
}

/// A `count`-pixel fixture with a GRB port, built by `build`.
pub(super) fn build(
    count: usize,
    duration: u32,
    build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>, TargetHandle<'id>) -> SequenceRoot<'id>,
) -> PreparedSequence {
    PreparedSequence::build(playback::timing(duration), |builder| {
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

/// `layers` layers of `invocation` over the whole eight-second sequence,
/// mixed and passed through `operators`.
pub(super) fn chain(
    count: usize,
    invocation: &SampleInvocation,
    layers: usize,
    operators: &[OperatorInvocation],
) -> PreparedSequence {
    build(count, 8_000_000, |builder, target| {
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

/// The only effect of `source` with its defaults, lowered with nothing known
/// about its placement.
pub(super) fn effect(source: &str) -> SampleInvocation {
    playback::sample(&compile_effect(source))
}

/// The only operator of `source` with its defaults, lowered with nothing
/// known about its placement.
pub(super) fn operator(source: &str) -> OperatorInvocation {
    playback::operator(&compile_operator(source))
}

/// Effect and operator definitions alike.
pub(super) trait Definition {
    fn declarations(&self) -> &[ParamDecl];
    fn invoke_with(
        &self,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Invocation, donder_runtime_types::BindingError>;
}

impl Definition for CompiledEffect {
    fn declarations(&self) -> &[ParamDecl] {
        self.params()
    }
    fn invoke_with(
        &self,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Invocation, donder_runtime_types::BindingError> {
        self.invoke(values, automation)
    }
}

impl Definition for CompiledOperator {
    fn declarations(&self) -> &[ParamDecl] {
        self.params()
    }
    fn invoke_with(
        &self,
        values: Vec<Value>,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Invocation, donder_runtime_types::BindingError> {
        self.invoke(values, automation)
    }
}

/// `definition` with `values` by name and declared defaults for the rest.
pub(super) fn bind(definition: &impl Definition, values: &[(&str, Value)]) -> Invocation {
    try_bind(definition, values).unwrap()
}

pub(super) fn try_bind(
    definition: &impl Definition,
    values: &[(&str, Value)],
) -> Result<Invocation, donder_runtime_types::BindingError> {
    let named: Vec<_> = values
        .iter()
        .map(|(name, value)| (Identifier::new((*name).into()).unwrap(), value.clone()))
        .collect();
    let values = donder_language::compiler::bind_params(
        definition.declarations(),
        named.iter().map(|(name, value)| (name, value)),
    )?;
    definition.invoke_with(values.iter_values().collect(), Box::new([]))
}

/// `definition` with the parameters named in `runtime` left to playback and
/// every other parameter fixed (`fixed` by name, defaults for the rest).
/// Preparation computes nothing that reads a runtime parameter; the VM does.
/// Also returns the runtime values in slot order, for [`lower_runtime_effect`]
/// and [`lower_runtime_operator`].
pub(super) fn runtime_instance(
    definition: &impl Definition,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> (Instance, Vec<Value>) {
    let mut values = Vec::new();
    let mut automation = Vec::new();
    let mut slots = Vec::new();
    for (index, param) in definition.declarations().iter().enumerate() {
        let name = param.name.as_str();
        if let Some((_, value)) = runtime.iter().find(|(runtime, _)| *runtime == name) {
            automation.push(PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: SampleDuration::from_ticks(1),
                curve: Curve { points: Vec::new() }.into(),
                mapping: param.automation_mapping().unwrap(),
                quantity: donder_runtime_types::AutomatedQuantity::Value,
                param_index: index as u16,
            });
            values.push(placeholder(param));
            slots.push(value.clone());
        } else if let Some((_, value)) = fixed.iter().find(|(fixed, _)| *fixed == name) {
            values.push(value.clone());
        } else {
            values.push(param.default.clone().unwrap());
        }
    }
    let declared = |name: &&str| {
        definition
            .declarations()
            .iter()
            .any(|param| param.name.as_str() == *name)
    };
    assert!(
        runtime.iter().chain(fixed).all(|(name, _)| declared(name)),
        "values name parameters"
    );
    let invocation = definition.invoke_with(values, automation.into()).unwrap();
    (invocation.instance(ProgramConstants::default()), slots)
}

/// A value of `param` inside its declared range.
fn placeholder(param: &ParamDecl) -> Value {
    if let Some(default) = &param.default {
        return default.clone();
    }
    match (&param.ty, param.range) {
        (Type::Int, Some(ParamRange::Int { min, .. })) => Value::Int(min),
        (Type::Float, Some(ParamRange::Float { min, .. })) => Value::Float(min),
        (ty, _) => ty.default_value(),
    }
}

/// Lowered programs place automated parameters first, in declaration order;
/// integral slots follow them and keep their placeholder values.
fn rebind(
    values: &donder_runtime_types::BoundParams,
    automation: &[PreparedAutomation],
    slots: Vec<Value>,
) -> Vec<Value> {
    let parameters = automation
        .iter()
        .filter(|binding| binding.quantity == donder_runtime_types::AutomatedQuantity::Value)
        .count();
    assert_eq!(parameters, slots.len());
    let mut values: Vec<Value> = values.iter_values().collect();
    for (slot, value) in slots.into_iter().enumerate() {
        assert_eq!(usize::from(automation[slot].param_index), slot);
        values[slot] = value;
    }
    values
}

/// A lowered runtime instance whose runtime slots hold `slots`, which may lie
/// outside their declared ranges. The result has no automation, so playback
/// keeps those values too.
pub(super) fn lower_runtime_effect(instance: &Instance, slots: Vec<Value>) -> SampleInvocation {
    let lowered = instance.sample();
    SampleInvocation::bind(
        lowered.program().clone(),
        rebind(lowered.params(), lowered.automation(), slots),
    )
    .unwrap()
}

/// The operator counterpart of [`lower_runtime_effect`].
pub(super) fn lower_runtime_operator(instance: &Instance, slots: Vec<Value>) -> OperatorInvocation {
    let lowered = instance.operator();
    OperatorInvocation::bind(
        lowered.program().clone(),
        rebind(lowered.params(), lowered.automation(), slots),
    )
    .unwrap()
}

/// `effect` evaluated by the VM wherever it reads a `runtime` parameter.
pub(super) fn runtime_effect(
    effect: &CompiledEffect,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> SampleInvocation {
    let (instance, slots) = runtime_instance(effect, fixed, runtime);
    lower_runtime_effect(&instance, slots)
}

/// `operator` evaluated by the VM wherever it reads a `runtime` parameter.
pub(super) fn runtime_operator(
    operator: &CompiledOperator,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> OperatorInvocation {
    let (instance, slots) = runtime_instance(operator, fixed, runtime);
    lower_runtime_operator(&instance, slots)
}
