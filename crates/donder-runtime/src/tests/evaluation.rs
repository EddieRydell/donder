//! Private execution adapters shared by compiler/VM behavior tests. Programs
//! run as one-pixel strips of a one-program show. Automation is not applied,
//! so every parameter slot holds its bound value.
use super::playback;
use super::std;
use crate::arena::{Carve, Size};
use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{Bound, NoSignals, RunContext, STRIP, Strip, StripSignals, StripSlots};
use crate::dsl::{NeighborWeights, ScanQuery};
use crate::resources::ParamsView;
use crate::{PreparedShow, ShowBuilder, SignalHandle, TargetHandle};
use donder_language::compiler::{CompiledDefinition, Instance, ParamDecl, ParamRange};
use donder_runtime_types::{BoundProgram, Type, Value};
use donder_runtime_types::{Color, Curve, CurvePoint, Marks, SampleDuration, SampleTime};
use donder_runtime_types::{
    FixtureGeometry, OutputEncoding, PreparedAutomation, RgbOrder, SequenceTiming, SpatialContext,
    TargetGeometry, TargetScope,
};
use std::prelude::rust_2024::*;

/// A test signal source's failure, reported as the operator's result.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RuntimeError {
    pub(super) message: String,
}

/// Memory a test reuses across evaluations.
#[derive(Default)]
pub(super) struct StripWorkspace {
    buffer: Vec<u32>,
}

/// A test signal source. An error is reported as the operator's result.
pub(super) trait SignalSampler {
    fn sample_signal(
        &mut self,
        input: usize,
        pixel: SignalPixel<i32>,
    ) -> Result<Color, RuntimeError>;
}

/// The pixel of a one-pixel strip; the first error wins.
struct Adapter<'a> {
    sampler: &'a mut dyn SignalSampler,
    error: Option<RuntimeError>,
    /// The evaluated pixel's local index, which shifted queries are relative to.
    index: i32,
    /// Pixels in its fixture, where shifted queries read past its ends.
    count: i32,
}

impl Adapter<'_> {
    fn sample(&mut self, input: usize, pixel: SignalPixel<i32>) -> Color {
        match self.sampler.sample_signal(input, pixel) {
            Ok(color) => color,
            Err(error) => {
                self.error.get_or_insert(error);
                Color::BLACK
            }
        }
    }
}

impl StripSignals for Adapter<'_> {
    fn read_strip(&mut self, input: usize, output: &mut [Color; STRIP]) {
        output[0] = self.sample(input, SignalPixel::Current);
    }

    fn read_pixel(&mut self, input: usize, _: usize, pixel: SignalPixel<i32>) -> Color {
        let pixel = match pixel {
            SignalPixel::Shifted(shift, edges) => {
                let read = edges.local(self.index + shift, self.count);
                SignalPixel::Local(read.unwrap_or(-1))
            }
            pixel => pixel,
        };
        self.sample(input, pixel)
    }

    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn NeighborWeights,
        output: &mut [Color; STRIP],
    ) {
        let colors: Vec<Color> = (0..self.count)
            .map(|index| self.sample(query.input, SignalPixel::Local(index)))
            .collect();
        output[0] = scan_fixture(query, weights, &colors)[self.index as usize];
    }

    /// Positions past the fixture's ends sample black, so they may keep
    /// their would-be indices.
    fn sample_range(
        &mut self,
        input: usize,
        start: isize,
        colors: &mut [Color],
        locals: &mut [i32],
    ) {
        for (k, (color, local)) in colors.iter_mut().zip(locals).enumerate() {
            *local = self.index + start as i32 + k as i32;
            *color = self.sample(input, SignalPixel::Local(*local));
        }
    }
}

/// A scan of one fixture's `colors`, pixel by pixel as the language defines it.
pub(super) fn scan_fixture(
    query: &ScanQuery,
    weights: &mut dyn NeighborWeights,
    colors: &[Color],
) -> Vec<Color> {
    let mut factors = vec![0.0; colors.len()];
    weights.weights(colors, &mut factors);
    let decay = if query.decay.is_nan() {
        0.0
    } else {
        query.decay
    };
    let mut order: Vec<usize> = (0..colors.len()).collect();
    if query.direction == crate::dsl::bytecode::Direction::Backward {
        order.reverse();
    }
    let mut running = [0.0f32; 3];
    let mut scanned = vec![Color::BLACK; colors.len()];
    for index in order {
        let weight = if factors[index].is_nan() {
            0.0
        } else {
            factors[index]
        };
        let Color { red, green, blue } = colors[index];
        for (value, channel) in running.iter_mut().zip([red, green, blue]) {
            *value = *value * decay + channel * weight;
        }
        let [red, green, blue] = running;
        scanned[index] = Color { red, green, blue };
    }
    scanned
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

/// One invocation's program in a show of its own: what a test strip runs.
pub(super) struct Single {
    sequence: PreparedShow,
    program: u32,
    params: u32,
}

impl Single {
    /// An effect's program as a clip, or an operator's as a node.
    pub(super) fn new(program: &BoundProgram) -> Self {
        let sequence = PreparedShow::build(playback::timing(1_000_000), |builder| {
            let geometry = playback::row(1);
            builder.fixture(0, geometry.clone());
            let target = whole(builder, &geometry, TargetScope::WholeTarget);
            if program.program().inputs == 0 {
                let duration = builder_duration();
                builder.clip(None, program, SampleTime::from_ticks(0), duration, target);
                None
            } else {
                let layer = builder.layer(true, []);
                Some(builder.operator(program, |_| layer))
            }
        });
        let image = sequence.image();
        let (program, params) = match image.s.clips.first() {
            Some(effect) => (effect.program, effect.params),
            None => (image.s.nodes[1].program, image.s.nodes[1].params),
        };
        Self {
            sequence,
            program,
            params,
        }
    }

    /// Run `run` with a strip of the program in `context`.
    pub(super) fn with_strip<R>(
        &self,
        context: &RunContext,
        workspace: &mut StripWorkspace,
        run: impl FnOnce(&mut Strip<'_, '_>) -> R,
    ) -> R {
        let image = self.sequence.image();
        let program = image.program(self.program);
        let mut slots = StripSlots::default();
        slots.include(program);
        let mut size = Size::default();
        slots.size(&mut size);
        let words = size.bytes.div_ceil(4).max(workspace.buffer.len());
        workspace.buffer.resize(words, 0);
        let mut arena = Carve::new(bytemuck::cast_slice_mut(&mut workspace.buffer));
        let mut strip_workspace = slots.carve(&mut arena).unwrap();
        let bound = Bound {
            image,
            program,
            params: ParamsView::image(image, self.params),
            sections: None,
        };
        let mut strip = Strip::new(bound, context, &mut strip_workspace);
        run(&mut strip)
    }

    /// One pixel in `context`.
    pub(super) fn one(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        signals: &mut dyn StripSignals,
        workspace: &mut StripWorkspace,
    ) -> Color {
        self.with_strip(&context.run, workspace, |strip| {
            let pixels = strip.pixels();
            pixels.index[0].set(context.index);
            pixels.fraction[0].set(context.fraction);
            pixels.x[0].set(spatial.position[0]);
            pixels.y[0].set(spatial.position[1]);
            let mut color = [Color::BLACK];
            strip.run(
                context.pixel_count as usize,
                spatial.min,
                spatial.max,
                signals,
                &mut color,
            );
            color[0]
        })
    }
}

/// The one-second show of a [`Single`].
fn builder_duration() -> SampleDuration {
    playback::timing(1_000_000).duration()
}

pub(super) trait SampleEvaluation {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        workspace: &mut StripWorkspace,
    ) -> Color;
}

impl SampleEvaluation for BoundProgram {
    fn evaluate(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        workspace: &mut StripWorkspace,
    ) -> Color {
        Single::new(self).one(context, spatial, &mut NoSignals, workspace)
    }
}

pub(super) trait OperatorEvaluation {
    fn evaluate_signals(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut StripWorkspace,
    ) -> Result<Color, RuntimeError>;
}

impl OperatorEvaluation for BoundProgram {
    fn evaluate_signals(
        &self,
        context: &PixelContext,
        spatial: &SpatialContext,
        sampler: &mut dyn SignalSampler,
        workspace: &mut StripWorkspace,
    ) -> Result<Color, RuntimeError> {
        let mut signals = Adapter {
            sampler,
            error: None,
            index: context.index,
            count: context.run.pixel_count,
        };
        let color = Single::new(self).one(context, spatial, &mut signals, workspace);
        match signals.error {
            Some(error) => Err(error),
            None => Ok(color),
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
            frame_rate: 40.0,
            frames_elapsed: 1,
            pixel_count: count as i32,
        },
        index: pixel as i32,
        fraction: pixel as f32 / (count - 1).max(1) as f32,
    }
}

pub(super) use donder_test_support::playback::{compile, invoke as bind, try_invoke as try_bind};

/// A one-pixel context of a one-second sequence at `progress`, at time zero.
pub(super) fn one_pixel(progress: f32) -> PixelContext {
    PixelContext {
        run: RunContext {
            progress,
            time: SampleDuration::from_ticks(0),
            duration: SampleDuration::from_ticks(1_000_000),
            frame_rate: 40.0,
            frames_elapsed: 1,
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

// The test-support crate builds shows of its own copy of this crate, so
// tests inside it build theirs here.

/// The target of every pixel of the first registered fixture, `geometry`.
pub(super) fn whole<'id>(
    builder: &mut ShowBuilder<'id>,
    geometry: &FixtureGeometry,
    scope: TargetScope,
) -> TargetHandle<'id> {
    builder.target(&TargetGeometry::new([(0, geometry)], scope))
}

/// A `count`-pixel fixture with a GRB port, built by `build`.
pub(super) fn build(
    count: usize,
    timing: SequenceTiming,
    build: impl for<'id> FnOnce(&mut ShowBuilder<'id>, TargetHandle<'id>) -> Option<SignalHandle<'id>>,
) -> PreparedShow {
    PreparedShow::build(timing, |builder| {
        let geometry = playback::row(count);
        builder.fixture(0, geometry.clone());
        let target = whole(builder, &geometry, TargetScope::WholeTarget);
        let port = builder.port(0, 0);
        builder.route(port, target, OutputEncoding::Rgb(RgbOrder::Grb), None);
        build(builder, target)
    })
}

/// `layers` layers of `effect` over the whole eight-second sequence, mixed
/// and passed through `operators`.
pub(super) fn show(
    count: usize,
    effect: &BoundProgram,
    layers: usize,
    operators: &[BoundProgram],
) -> PreparedShow {
    let timing = playback::timing(8_000_000);
    let duration = timing.duration();
    build(count, timing, |builder, target| {
        let layers: Vec<_> = (0..layers)
            .map(|_| {
                let effect =
                    builder.clip(None, effect, SampleTime::from_ticks(0), duration, target);
                builder.layer(true, [effect])
            })
            .collect();
        let mut signal = builder.mix(layers);
        for operator in operators {
            signal = builder.operator(operator, |_| signal);
        }
        Some(signal)
    })
}

/// The only effect of `source` with its defaults, lowered with nothing known
/// about its placement.
pub(super) fn effect(source: &str) -> BoundProgram {
    playback::program(&compile(source))
}

/// The only operator of `source` with its defaults, lowered with nothing
/// known about its placement.
pub(super) fn operator(source: &str) -> BoundProgram {
    playback::program(&compile(source))
}

/// `definition` with the parameters named in `runtime` left to playback and
/// every other parameter fixed (`fixed` by name, defaults for the rest).
/// Preparation computes nothing that reads a runtime parameter; the VM does.
/// Also returns the runtime values in slot order, for [`lower_runtime`].
pub(super) fn runtime_instance(
    definition: &CompiledDefinition,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> (Instance, Vec<Value>) {
    let mut values = Vec::new();
    let mut automation = Vec::new();
    let mut slots = Vec::new();
    for (index, param) in definition.params().iter().enumerate() {
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
            .params()
            .iter()
            .any(|param| param.name.as_str() == *name)
    };
    assert!(
        runtime.iter().chain(fixed).all(|(name, _)| declared(name)),
        "values name parameters"
    );
    let invocation = definition.invoke(values, automation.into()).unwrap();
    (invocation.instance(Default::default()), slots)
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
    let mut values = values.values().to_vec();
    for (slot, value) in slots.into_iter().enumerate() {
        assert_eq!(usize::from(automation[slot].param_index), slot);
        values[slot] = value;
    }
    values
}

/// A lowered runtime instance whose runtime slots hold `slots`, which may lie
/// outside their declared ranges. The result has no automation, so playback
/// keeps those values too.
pub(super) fn lower_runtime(instance: &Instance, slots: Vec<Value>) -> BoundProgram {
    let lowered = playback::bind(instance);
    BoundProgram::bind(
        lowered.program().clone(),
        rebind(lowered.params(), lowered.automation(), slots),
        Box::new([]),
    )
    .unwrap()
}

/// `effect` evaluated by the VM wherever it reads a `runtime` parameter.
pub(super) fn runtime_effect(
    effect: &CompiledDefinition,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> BoundProgram {
    let (instance, slots) = runtime_instance(effect, fixed, runtime);
    lower_runtime(&instance, slots)
}

/// `operator` evaluated by the VM wherever it reads a `runtime` parameter.
pub(super) fn runtime_operator(
    operator: &CompiledDefinition,
    fixed: &[(&str, Value)],
    runtime: &[(&str, Value)],
) -> BoundProgram {
    let (instance, slots) = runtime_instance(operator, fixed, runtime);
    lower_runtime(&instance, slots)
}
