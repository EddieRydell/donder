//! Invariant-preserving host construction. Checked authored inputs enter before
//! `build`; graph references and storage addresses are issued only by its owner.
use super::{ExecutableSequenceData, PreparedOutput, SequenceData, programs::AdmittedPrograms};
use crate::bindings::ExecutableEnvironment;
use crate::dsl::{
    AutomationPlan, BoundParams, CompiledOperator, DslBindCache, RuntimeError, SampleProgram,
    SpatialContext, Value,
};
use crate::patch::{PixelEncoding, PreparedPatch, PreparedPixelRoute};
use crate::signal::{
    PreparedAutomation, PreparedClip, PreparedEffect, PreparedEffectAutomation,
    PreparedEffectImplementation, PreparedFixture, PreparedLayer, PreparedOperator,
    PreparedOperatorNode, PreparedPixel, PreparedSignalGraph, PreparedSignalKind,
    PreparedSignalNode, PreparedTarget,
};
use crate::values::{SampleDuration, SampleTime};
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Shared;
#[cfg(feature = "atomic")]
use alloc::sync::Arc as Shared;
use alloc::{boxed::Box, collections::BTreeSet, vec, vec::Vec};
use core::{marker::PhantomData, num::NonZeroU32};

mod generator;
pub use generator::{GeneratedEffect, GeneratorPlayback};
#[cfg(test)]
mod tests;

macro_rules! handle {
    ($($name:ident),+ $(,)?) => {$(
        #[derive(Clone, Copy)]
        pub struct $name<'id> {
            index: usize,
            brand: PhantomData<fn(&'id ()) -> &'id ()>,
        }
        impl<'id> $name<'id> {
            fn new(index: usize) -> Self { Self { index, brand: PhantomData } }
        }
    )+};
}
handle!(
    FixtureHandle,
    TargetHandle,
    EffectHandle,
    SignalHandle,
    SequenceRoot,
    OutputHandle,
    LookupHandle
);

#[derive(Clone, Copy, Debug)]
pub struct SequenceWindow {
    pub start: SampleTime,
    pub duration: NonZeroU32,
}

/// An accepted sequence clock and its authored/generated effect intervals.
/// Cache this at project admission so host construction cannot fail on timing.
#[derive(Clone, Debug)]
pub struct SequenceTiming {
    frame_rate: NonZeroU32,
    frame_count: u32,
    duration: NonZeroU32,
    windows: Box<[SequenceWindow]>,
}

impl SequenceTiming {
    pub fn frame_rate(&self) -> u32 {
        self.frame_rate.get()
    }
    pub fn frame_count(&self) -> u32 {
        self.frame_count
    }
    pub fn duration(&self) -> SampleDuration {
        SampleDuration::from_ticks(self.duration.get())
    }

    pub fn admit(
        frame_rate: NonZeroU32,
        frame_count: NonZeroU32,
        duration: NonZeroU32,
        windows: Box<[SequenceWindow]>,
    ) -> Option<Self> {
        for window in &windows {
            let end = window.start.as_ticks().checked_add(window.duration.get())?;
            if end > duration.get() {
                return None;
            }
        }
        Some(Self {
            frame_rate,
            frame_count: frame_count.get(),
            duration,
            windows,
        })
    }
}

#[derive(Clone, Copy)]
pub struct WindowHandle<'id> {
    start: SampleTime,
    duration: SampleDuration,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

/// Finite geometry with representable fixture-local storage addresses.
#[derive(Clone, Debug)]
pub struct FixtureGeometry {
    positions: Box<[[f32; 2]]>,
}

impl FixtureGeometry {
    pub fn admit(positions: Box<[[f32; 2]]>) -> Option<Self> {
        u32::try_from(positions.len()).ok()?;
        if positions.iter().flatten().any(|value| !value.is_finite()) {
            return None;
        }
        Some(Self { positions })
    }
}

/// Shared compiler-admitted code; binding cannot substitute a different program.
#[derive(Clone, Debug)]
pub struct SampleDefinition(Shared<SampleProgram>);

impl SampleDefinition {
    pub fn new(program: impl Into<Shared<SampleProgram>>) -> Self {
        Self(program.into())
    }

    pub fn bind(
        &self,
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<SampleInvocation, RuntimeError> {
        let slots: Vec<_> = values.into_iter().map(Some).collect();
        let params = BoundParams::bind_slots(self.0.input_types(), &slots, cache)?;
        let automation = AutomationPlan::admit(&params, &[]).ok_or_else(|| RuntimeError {
            message: "invalid sample automation layout".into(),
        })?;
        Ok(SampleInvocation {
            definition: self.clone(),
            params,
            automation,
            has_automation: false,
        })
    }
}

#[derive(Clone, Debug)]
pub struct SampleInvocation {
    definition: SampleDefinition,
    params: BoundParams,
    automation: AutomationPlan,
    has_automation: bool,
}

impl SampleInvocation {
    /// Admit authored automation together with the invocation it modifies.
    pub fn with_automation(
        mut self,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Self, RuntimeError> {
        if !self.params.has_valid_automation(&automation)
            || automation.iter().any(|binding| {
                self.definition
                    .0
                    .input_types()
                    .get(usize::from(binding.param_index))
                    .is_none_or(|ty| !binding.mapping.accepts_type(ty))
            })
        {
            return Err(RuntimeError {
                message: "sample automation does not match its declaration".into(),
            });
        }
        let plan =
            AutomationPlan::admit(&self.params, &automation).ok_or_else(|| RuntimeError {
                message: "sample automation does not match its declaration".into(),
            })?;
        self.has_automation = !automation.is_empty();
        self.automation = plan;
        Ok(self)
    }
}

#[derive(Clone, Debug)]
pub struct OperatorDefinition(Shared<CompiledOperator>);

impl OperatorDefinition {
    pub fn new(program: impl Into<Shared<CompiledOperator>>) -> Self {
        Self(program.into())
    }

    pub fn bind(
        &self,
        values: Vec<Value>,
        cache: &mut DslBindCache,
    ) -> Result<OperatorInvocation, RuntimeError> {
        let types: Vec<_> = self
            .0
            .params()
            .iter()
            .map(|param| param.ty.clone())
            .collect();
        let slots: Vec<_> = values.into_iter().map(Some).collect();
        let params = BoundParams::bind_slots(&types, &slots, cache)?;
        let automation = AutomationPlan::admit(&params, &[]).ok_or_else(|| RuntimeError {
            message: "invalid operator automation layout".into(),
        })?;
        Ok(OperatorInvocation {
            definition: self.clone(),
            params,
            automation,
            has_automation: false,
        })
    }
}

#[derive(Clone, Debug)]
pub struct OperatorInvocation {
    definition: OperatorDefinition,
    params: BoundParams,
    automation: AutomationPlan,
    has_automation: bool,
}

impl OperatorInvocation {
    pub fn with_automation(
        mut self,
        automation: Box<[PreparedAutomation]>,
    ) -> Result<Self, RuntimeError> {
        if !self.params.has_valid_automation(&automation)
            || automation.iter().any(|binding| {
                self.definition
                    .0
                    .params()
                    .get(usize::from(binding.param_index))
                    .is_none_or(|param| {
                        !param.supports_automation() || !binding.mapping.accepts_type(&param.ty)
                    })
            })
        {
            return Err(RuntimeError {
                message: "operator automation does not match its declaration".into(),
            });
        }
        let plan =
            AutomationPlan::admit(&self.params, &automation).ok_or_else(|| RuntimeError {
                message: "operator automation does not match its declaration".into(),
            })?;
        self.has_automation = !automation.is_empty();
        self.automation = plan;
        Ok(self)
    }
}

#[derive(Clone, Copy)]
pub enum TargetScope {
    PerFixture,
    WholeTarget,
}

#[derive(Clone, Copy)]
pub enum RgbOrder {
    Rgb,
    Rbg,
    Grb,
    Gbr,
    Brg,
    Bgr,
}

impl RgbOrder {
    fn admit(channels: [u8; 3]) -> Option<Self> {
        Some(match channels {
            [0, 1, 2] => Self::Rgb,
            [0, 2, 1] => Self::Rbg,
            [1, 0, 2] => Self::Grb,
            [1, 2, 0] => Self::Gbr,
            [2, 0, 1] => Self::Brg,
            [2, 1, 0] => Self::Bgr,
            _ => return None,
        })
    }

    fn channels(self) -> [u8; 3] {
        match self {
            Self::Rgb => [0, 1, 2],
            Self::Rbg => [0, 2, 1],
            Self::Grb => [1, 0, 2],
            Self::Gbr => [1, 2, 0],
            Self::Brg => [2, 0, 1],
            Self::Bgr => [2, 1, 0],
        }
    }
}

#[derive(Clone, Copy)]
pub enum WhitePosition {
    First,
    Second,
    Third,
    Fourth,
}

#[derive(Clone, Copy)]
pub enum OutputEncoding {
    Rgb(RgbOrder),
    Rgbw { rgb: RgbOrder, white: WhitePosition },
}

impl OutputEncoding {
    /// Admit the exact RGB/RGBW channel permutation from an authored route.
    pub fn admit(encoding: PixelEncoding) -> Option<Self> {
        Some(match encoding {
            PixelEncoding::Rgb { order } => Self::Rgb(RgbOrder::admit(order)?),
            PixelEncoding::Rgbw { order } => {
                let (channels, white) = match order {
                    [3, a, b, c] => ([a, b, c], WhitePosition::First),
                    [a, 3, b, c] => ([a, b, c], WhitePosition::Second),
                    [a, b, 3, c] => ([a, b, c], WhitePosition::Third),
                    [a, b, c, 3] => ([a, b, c], WhitePosition::Fourth),
                    _ => return None,
                };
                Self::Rgbw {
                    rgb: RgbOrder::admit(channels)?,
                    white,
                }
            }
        })
    }

    pub fn channel_count(self) -> usize {
        match self {
            Self::Rgb(_) => 3,
            Self::Rgbw { .. } => 4,
        }
    }

    fn encoding(self) -> PixelEncoding {
        match self {
            Self::Rgb(order) => PixelEncoding::Rgb {
                order: order.channels(),
            },
            Self::Rgbw { rgb, white } => {
                let [r, g, b] = rgb.channels();
                PixelEncoding::Rgbw {
                    order: match white {
                        WhitePosition::First => [3, r, g, b],
                        WhitePosition::Second => [r, 3, g, b],
                        WhitePosition::Third => [r, g, 3, b],
                        WhitePosition::Fourth => [r, g, b, 3],
                    },
                }
            }
        }
    }
}

struct Target {
    pixels: Vec<PreparedPixel>,
    spatial: Vec<SpatialContext>,
    selection: Shared<[PreparedPixel]>,
}

struct Output {
    controller_index: u32,
    port: u32,
    bytes: Vec<u8>,
}

pub struct SequenceBuilder<'id> {
    compact_outputs: bool,
    timing: SequenceTiming,
    fixtures: Vec<PreparedFixture>,
    geometry: Vec<FixtureGeometry>,
    offsets: Vec<usize>,
    pixel_count: usize,
    targets: Vec<Target>,
    sample_programs: Vec<SampleDefinition>,
    operator_programs: Vec<OperatorDefinition>,
    environments: Vec<ExecutableEnvironment>,
    effects: Vec<PreparedEffect<AutomationPlan>>,
    clips: Vec<PreparedClip>,
    effect_automation_count: usize,
    operator_automation_count: usize,
    layers: Vec<PreparedLayer>,
    effects_by_layer: Vec<Box<[usize]>>,
    nodes: Vec<PreparedSignalNode<AutomationPlan>>,
    outputs: Vec<Output>,
    routes: Vec<PreparedPixelRoute>,
    lookups: Vec<[u8; 256]>,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

impl<'id> SequenceBuilder<'id> {
    pub(super) fn new(timing: SequenceTiming) -> Self {
        Self {
            compact_outputs: false,
            timing,
            fixtures: Vec::new(),
            geometry: Vec::new(),
            offsets: Vec::new(),
            pixel_count: 0,
            targets: Vec::new(),
            sample_programs: Vec::new(),
            operator_programs: Vec::new(),
            environments: Vec::new(),
            effects: Vec::new(),
            clips: Vec::new(),
            effect_automation_count: 0,
            operator_automation_count: 0,
            layers: Vec::new(),
            effects_by_layer: Vec::new(),
            nodes: Vec::new(),
            outputs: Vec::new(),
            routes: Vec::new(),
            lookups: Vec::new(),
            brand: PhantomData,
        }
    }

    pub fn windows(&self) -> impl ExactSizeIterator<Item = WindowHandle<'id>> + '_ {
        self.timing.windows.iter().map(|window| WindowHandle {
            start: window.start,
            duration: SampleDuration::from_ticks(window.duration.get()),
            brand: PhantomData,
        })
    }

    /// Compact to the routes emitted for the selected ports when construction
    /// finishes. Full builds leave this off, including unpatched logical pixels.
    pub fn compact_to_outputs(&mut self) {
        self.compact_outputs = true;
    }

    pub fn whole_sequence(&self) -> WindowHandle<'id> {
        WindowHandle {
            start: SampleTime::from_ticks(0),
            duration: SampleDuration::from_ticks(self.timing.duration.get()),
            brand: PhantomData,
        }
    }

    pub fn fixture(&mut self, source_id: u32, geometry: FixtureGeometry) -> FixtureHandle<'id> {
        let index = self.fixtures.len();
        self.offsets.push(self.pixel_count);
        self.pixel_count += geometry.positions.len();
        self.fixtures.push(PreparedFixture {
            id: source_id,
            pixel_count: geometry.positions.len(),
        });
        self.geometry.push(geometry);
        FixtureHandle::new(index)
    }

    /// Fixture selection is a set; the first occurrence determines whole-target order.
    pub fn target(
        &mut self,
        fixtures: impl IntoIterator<Item = FixtureHandle<'id>>,
        scope: TargetScope,
    ) -> TargetHandle<'id> {
        let mut seen = BTreeSet::new();
        let fixtures: Vec<_> = fixtures
            .into_iter()
            .filter(|fixture| seen.insert(fixture.index))
            .collect();
        let total: usize = fixtures
            .iter()
            .map(|fixture| self.fixtures[fixture.index].pixel_count)
            .sum();
        let whole = bounds(
            fixtures
                .iter()
                .flat_map(|fixture| self.geometry[fixture.index].positions.iter().copied()),
        );
        let mut pixels = Vec::with_capacity(total);
        let mut logical_offset = 0;
        for fixture in fixtures {
            let positions = &self.geometry[fixture.index].positions;
            let extent = match scope {
                TargetScope::PerFixture => bounds(positions.iter().copied()),
                TargetScope::WholeTarget => whole,
            };
            if let Some((min, max)) = extent {
                for (cell, &position) in positions.iter().enumerate() {
                    let (index, count) = match scope {
                        TargetScope::PerFixture => (cell, positions.len()),
                        TargetScope::WholeTarget => (logical_offset + cell, total),
                    };
                    pixels.push((
                        PreparedPixel {
                            fixture_index: fixture.index,
                            fixture_pixel_index: cell as u32,
                            pixel_index: index,
                            pixel_count: count,
                            pixel_fraction: if count <= 1 {
                                0.0
                            } else {
                                index as f32 / (count - 1) as f32
                            },
                        },
                        SpatialContext { position, min, max },
                    ));
                }
            }
            logical_offset += positions.len();
        }
        let selection: Shared<[_]> = pixels.iter().map(|(pixel, _)| *pixel).collect();
        pixels.sort_by_key(|(pixel, _)| (pixel.fixture_index, pixel.fixture_pixel_index));
        let (pixels, spatial): (Vec<_>, Vec<_>) = pixels.into_iter().unzip();
        if let Some(index) = self.targets.iter().position(|target| {
            target.pixels == pixels && target.spatial == spatial && target.selection == selection
        }) {
            return TargetHandle::new(index);
        }
        let index = self.targets.len();
        self.targets.push(Target {
            pixels,
            spatial,
            selection,
        });
        TargetHandle::new(index)
    }

    pub fn target_pixel_count(&self, target: TargetHandle<'id>) -> usize {
        self.targets[target.index].pixels.len()
    }

    /// Intersect a range with the target's physical pixel order. Out-of-domain
    /// endpoints and reversed ranges select no extra pixels. Logical indices,
    /// counts, fractions, spatial scope and generator selection order survive.
    pub fn target_slice(
        &mut self,
        target: TargetHandle<'id>,
        range: core::ops::Range<usize>,
    ) -> TargetHandle<'id> {
        let original = &self.targets[target.index];
        let (pixels, spatial): (Vec<_>, Vec<_>) = original
            .pixels
            .iter()
            .zip(&original.spatial)
            .enumerate()
            .filter(|(index, _)| range.contains(index))
            .map(|(_, (pixel, spatial))| (*pixel, *spatial))
            .unzip();
        let selected: BTreeSet<_> = pixels
            .iter()
            .map(|pixel| (pixel.fixture_index, pixel.fixture_pixel_index))
            .collect();
        let selection: Shared<[_]> = original
            .selection
            .iter()
            .filter(|pixel| selected.contains(&(pixel.fixture_index, pixel.fixture_pixel_index)))
            .copied()
            .collect();
        if let Some(index) = self.targets.iter().position(|target| {
            target.pixels == pixels && target.spatial == spatial && target.selection == selection
        }) {
            return TargetHandle::new(index);
        }
        let index = self.targets.len();
        self.targets.push(Target {
            pixels,
            spatial,
            selection,
        });
        TargetHandle::new(index)
    }

    pub fn sample(
        &mut self,
        invocation: &SampleInvocation,
        window: WindowHandle<'id>,
        target: TargetHandle<'id>,
    ) -> EffectHandle<'id> {
        let program = match self
            .sample_programs
            .iter()
            .position(|program| Shared::ptr_eq(&program.0, &invocation.definition.0))
        {
            Some(index) => index,
            None => {
                let index = self.sample_programs.len();
                self.sample_programs.push(invocation.definition.clone());
                index
            }
        };
        let index = self.effects.len();
        let automation = if !invocation.has_automation {
            None
        } else {
            let workspace_slot = self.effect_automation_count;
            self.effect_automation_count += 1;
            Some(Box::new(PreparedEffectAutomation {
                workspace_slot,
                bindings: invocation.automation.clone(),
            }))
        };
        self.effects.push(PreparedEffect {
            start_time: window.start,
            duration: window.duration,
            target: target.index,
            implementation: PreparedEffectImplementation::Dsl {
                program,
                bound_params: invocation.params.clone(),
            },
            automation,
        });
        EffectHandle::new(index)
    }

    /// Register authored clip identity independently from generated effect order.
    pub fn clip(
        &mut self,
        id: u32,
        window: WindowHandle<'id>,
        target: TargetHandle<'id>,
        effects: impl IntoIterator<Item = EffectHandle<'id>>,
    ) {
        self.clips.push(PreparedClip {
            id,
            start_time: window.start,
            duration: window.duration,
            target: target.index,
            effects: effects.into_iter().map(|effect| effect.index).collect(),
        });
    }

    /// Request each declared input exactly once. References can only point to
    /// signals already created by this builder, so cycles cannot be expressed.
    pub fn operator(
        &mut self,
        invocation: &OperatorInvocation,
        mut input: impl FnMut(usize) -> SignalHandle<'id>,
    ) -> SignalHandle<'id> {
        let program = match self
            .operator_programs
            .iter()
            .position(|program| Shared::ptr_eq(&program.0, &invocation.definition.0))
        {
            Some(index) => index,
            None => {
                let index = self.operator_programs.len();
                self.operator_programs.push(invocation.definition.clone());
                index
            }
        };
        let inputs = (0..invocation.definition.0.inputs().len())
            .map(|index| input(index).index)
            .collect();
        let index = self.nodes.len();
        let automation_slot = self.operator_automation_count;
        if invocation.has_automation {
            self.operator_automation_count += 1;
        }
        self.nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot,
                    implementation: PreparedOperator::Dsl(program),
                    params: invocation.params.clone(),
                },
                inputs,
                automation: invocation.automation.clone(),
                vm_slot: 0,
            },
        });
        SignalHandle::new(index)
    }

    pub fn layer(
        &mut self,
        enabled: bool,
        effects: impl IntoIterator<Item = EffectHandle<'id>>,
    ) -> SignalHandle<'id> {
        let layer_index = self.layers.len();
        let mut effects: Vec<_> = effects.into_iter().map(|effect| effect.index).collect();
        effects.sort_by_key(|&index| self.effects[index].start_time);
        self.layers.push(PreparedLayer { enabled });
        self.effects_by_layer.push(effects.into());
        let index = self.nodes.len();
        self.nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Layer { layer_index },
        });
        SignalHandle::new(index)
    }

    pub fn mix(
        &mut self,
        inputs: impl IntoIterator<Item = SignalHandle<'id>>,
    ) -> SignalHandle<'id> {
        let index = self.nodes.len();
        self.nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Output {
                inputs: inputs.into_iter().map(|input| input.index).collect(),
            },
        });
        SignalHandle::new(index)
    }

    pub fn output(
        &mut self,
        inputs: impl IntoIterator<Item = SignalHandle<'id>>,
    ) -> SequenceRoot<'id> {
        SequenceRoot::new(self.mix(inputs).index)
    }

    pub fn port(&mut self, controller_index: u32, port: u32) -> OutputHandle<'id> {
        let index = self.outputs.len();
        self.outputs.push(Output {
            controller_index,
            port,
            bytes: Vec::new(),
        });
        OutputHandle::new(index)
    }

    pub fn lookup(&mut self, table: [u8; 256]) -> LookupHandle<'id> {
        let index = self.lookups.len();
        self.lookups.push(table);
        LookupHandle::new(index)
    }

    pub fn padding(&mut self, output: OutputHandle<'id>, bytes: usize) {
        self.outputs[output.index]
            .bytes
            .extend(core::iter::repeat_n(0, bytes));
    }

    /// Append the target's physical pixel order. Output width and all byte spans
    /// follow from storage actually reserved here, never caller-supplied offsets.
    pub fn route(
        &mut self,
        output: OutputHandle<'id>,
        target: TargetHandle<'id>,
        encoding: OutputEncoding,
        lookup: Option<LookupHandle<'id>>,
    ) {
        let encoding = encoding.encoding();
        let frame = output.index;
        let bytes = &mut self.outputs[frame].bytes;
        let channels = encoding.channel_order().len();
        let mut start_slot = bytes.len();
        let pixels = &self.targets[target.index].pixels;
        bytes.extend(core::iter::repeat_n(0, pixels.len() * channels));
        let mut indices = pixels
            .iter()
            .map(|pixel| self.offsets[pixel.fixture_index] + pixel.fixture_pixel_index as usize)
            .peekable();
        while let Some(start) = indices.next() {
            let mut end = start + 1;
            while indices.peek() == Some(&end) {
                indices.next();
                end += 1;
            }
            self.routes.push(PreparedPixelRoute {
                pixels: start..end,
                frame,
                start_slot,
                encoding,
                lookup: lookup.map(|lookup| lookup.index),
            });
            start_slot += (end - start) * channels;
        }
    }

    pub(super) fn finish(mut self, root: SequenceRoot<'id>) -> ExecutableSequenceData {
        let full = self.target(
            (0..self.fixtures.len())
                .map(FixtureHandle::new)
                .collect::<Vec<_>>(),
            TargetScope::PerFixture,
        );
        let needs_spatial = self
            .sample_programs
            .iter()
            .any(|program| program.0.uses_spatial_context())
            || self
                .operator_programs
                .iter()
                .any(|program| program.0.program().uses_spatial_context());
        let mut target_pixels = Vec::new();
        let mut spatial_contexts = Vec::new();
        let targets = self
            .targets
            .into_iter()
            .map(|target| {
                let start = target_pixels.len();
                let max_count = target
                    .pixels
                    .iter()
                    .fold(0, |count, pixel| count.max(pixel.pixel_count));
                let sample_count = if target.pixels.len() > max_count {
                    max_count
                } else {
                    0
                };
                target_pixels.extend(target.pixels);
                if needs_spatial {
                    spatial_contexts.extend(target.spatial);
                }
                PreparedTarget {
                    pixels: start..target_pixels.len(),
                    sample_count,
                }
            })
            .collect();
        let outputs = self
            .outputs
            .into_iter()
            .map(|output| PreparedOutput {
                controller_index: output.controller_index,
                port: output.port,
                width: output.bytes.len(),
            })
            .collect();
        let mut data = SequenceData {
            signals: PreparedSignalGraph {
                parameter_environments: self.environments.into(),
                frame_rate: self.timing.frame_rate.get(),
                frame_count: self.timing.frame_count,
                duration: SampleDuration::from_ticks(self.timing.duration.get()),
                fixtures: self.fixtures.into(),
                fixture_pixel_offsets: self.offsets.into(),
                pixel_count: self.pixel_count,
                effects: self.effects.into(),
                clips: self.clips.into(),
                programs: AdmittedPrograms::new(
                    self.sample_programs
                        .into_iter()
                        .map(|program| program.0.as_ref().clone())
                        .collect(),
                    self.operator_programs
                        .into_iter()
                        .map(|program| program.0.program().clone())
                        .collect(),
                ),
                targets,
                target_pixels: target_pixels.into(),
                spatial_contexts: spatial_contexts.into(),
                effects_by_layer: self.effects_by_layer.into(),
                layers: self.layers.into(),
                plan: super::compact::finish_plan(self.nodes, root.index, full.index),
            },
            patch: PreparedPatch {
                routes: self.routes.into(),
                lookups: self.lookups.into(),
            },
            outputs,
        };
        if self.compact_outputs {
            super::compact::compact(&mut data.signals, &mut data.patch);
        } else {
            super::compact::retain_environments(
                &mut data.signals.parameter_environments,
                &mut data.signals.effects,
            );
        }
        data
    }
}

fn bounds(points: impl IntoIterator<Item = [f32; 2]>) -> Option<([f32; 2], [f32; 2])> {
    let mut points = points.into_iter();
    let first = points.next()?;
    Some(points.fold((first, first), |(mut min, mut max), point| {
        for axis in 0..2 {
            min[axis] = min[axis].min(point[axis]);
            max[axis] = max[axis].max(point[axis]);
        }
        (min, max)
    }))
}
