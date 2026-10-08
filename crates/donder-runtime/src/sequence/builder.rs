//! Invariant-preserving sequence construction. Checked authored inputs enter before
//! `build`; graph references and storage addresses are issued only by its owner.
use super::{ExecutableSequenceData, PreparedOutput, SequenceData, programs::AdmittedPrograms};
use crate::dsl::{AutomationPlan, BoundParams, DslBindCache};
use crate::patch::{PreparedPatch, PreparedPixelRoute};
use crate::signal::{
    PreparedClip, PreparedEffect, PreparedEffectAutomation, PreparedFixture, PreparedLayer,
    PreparedOperatorNode, PreparedPixel, PreparedSignalGraph, PreparedSignalKind,
    PreparedSignalNode, PreparedTarget,
};
use crate::values::{SampleDuration, SampleTime};
#[cfg(test)]
use alloc::vec;
use alloc::{boxed::Box, collections::BTreeSet, vec::Vec};
use core::marker::PhantomData;
use donder_language::Shared;
use donder_language::dsl::{OperatorInvocation, OperatorProgram, SampleInvocation, SampleProgram};
use donder_language::execution::{
    FixtureGeometry, OutputEncoding, SequenceTiming, SpatialContext, TargetGeometry, TargetScope,
};

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

#[derive(Clone, Copy)]
pub struct WindowHandle<'id> {
    start: SampleTime,
    duration: SampleDuration,
    brand: PhantomData<fn(&'id ()) -> &'id ()>,
}

struct Target {
    pixels: Vec<PreparedPixel>,
    spatial: Vec<SpatialContext>,
    source_pixels: Vec<(usize, u32)>,
    selection: Shared<[(usize, u32)]>,
    scope: TargetScope,
}

struct Output {
    controller_index: u32,
    port: u32,
    bytes: Vec<u8>,
}

pub struct SequenceBuilder<'id> {
    timing: SequenceTiming,
    fixtures: Vec<PreparedFixture>,
    geometry: Vec<FixtureGeometry>,
    storage: Vec<usize>,
    offsets: Vec<usize>,
    pixel_count: usize,
    targets: Vec<Target>,
    sample_programs: Vec<Shared<SampleProgram>>,
    operator_programs: Vec<Shared<OperatorProgram>>,
    bind_cache: DslBindCache,
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
            timing,
            fixtures: Vec::new(),
            geometry: Vec::new(),
            storage: Vec::new(),
            offsets: Vec::new(),
            pixel_count: 0,
            targets: Vec::new(),
            sample_programs: Vec::new(),
            operator_programs: Vec::new(),
            bind_cache: DslBindCache::default(),
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
        self.timing.windows().iter().map(|window| WindowHandle {
            start: window.start,
            duration: SampleDuration::from_ticks(window.duration.get()),
            brand: PhantomData,
        })
    }

    pub fn whole_sequence(&self) -> WindowHandle<'id> {
        WindowHandle {
            start: SampleTime::from_ticks(0),
            duration: self.timing.duration(),
            brand: PhantomData,
        }
    }

    /// Register original geometry and its already selected storage cells together.
    pub fn fixture(&mut self, source_id: u32, geometry: FixtureGeometry) -> FixtureHandle<'id> {
        let index = self.geometry.len();
        let pixel_count = geometry.cells().len();
        self.storage.push(self.fixtures.len());
        if geometry.retains_fixture() {
            self.offsets.push(self.pixel_count);
            self.pixel_count += pixel_count;
            self.fixtures.push(PreparedFixture {
                id: source_id,
                pixel_count,
            });
        }
        self.geometry.push(geometry);
        FixtureHandle::new(index)
    }

    /// Geometry and storage are derived from the same owner-issued handles.
    /// The language helper supplies original sampling coordinates and sections.
    pub fn target(
        &mut self,
        fixtures: impl IntoIterator<Item = FixtureHandle<'id>>,
        scope: TargetScope,
    ) -> TargetHandle<'id> {
        let domain = TargetGeometry::new(
            fixtures
                .into_iter()
                .map(|fixture| (fixture.index, &self.geometry[fixture.index])),
            scope,
        );
        let mut pixels = Vec::with_capacity(domain.pixels().len());
        let mut spatial = Vec::with_capacity(domain.pixels().len());
        let mut source_pixels = Vec::with_capacity(domain.pixels().len());
        for pixel in domain.pixels() {
            // The domain emits only registered retained cells. Empty fixtures
            // contribute original coordinates/sections but never storage addresses.
            let fixture_index = self.storage[pixel.fixture()];
            pixels.push(PreparedPixel {
                fixture_index,
                fixture_pixel_index: pixel.cell(),
                pixel_index: pixel.index(),
                pixel_count: pixel.count(),
                pixel_fraction: pixel.fraction(),
            });
            spatial.push(pixel.spatial());
            source_pixels.push((pixel.fixture(), pixel.source_cell()));
        }
        self.store_target(Target {
            pixels,
            spatial,
            source_pixels,
            selection: domain.selection().into(),
            scope,
        })
    }

    fn store_target(&mut self, target: Target) -> TargetHandle<'id> {
        if let Some(index) = self.targets.iter().position(|existing| {
            existing.pixels == target.pixels
                && existing.spatial == target.spatial
                && existing.source_pixels == target.source_pixels
                && existing.selection == target.selection
                && existing.scope == target.scope
        }) {
            return TargetHandle::new(index);
        }
        let index = self.targets.len();
        self.targets.push(target);
        TargetHandle::new(index)
    }

    pub fn target_pixel_count(&self, target: TargetHandle<'id>) -> usize {
        self.targets[target.index].pixels.len()
    }

    /// Intersect a range with the target's physical pixel order. Out-of-domain
    /// endpoints and reversed ranges select no extra pixels. Logical indices,
    /// counts, fractions, spatial scope and selection order survive.
    pub fn target_slice(
        &mut self,
        target: TargetHandle<'id>,
        range: core::ops::Range<usize>,
    ) -> TargetHandle<'id> {
        let original = &self.targets[target.index];
        let scope = original.scope;
        let (pixels, spatial): (Vec<_>, Vec<_>) = original
            .pixels
            .iter()
            .zip(&original.spatial)
            .enumerate()
            .filter(|(index, _)| range.contains(index))
            .map(|(_, (pixel, spatial))| (*pixel, *spatial))
            .unzip();
        let source_pixels: Vec<_> = original
            .source_pixels
            .iter()
            .enumerate()
            .filter(|(index, _)| range.contains(index))
            .map(|(_, source)| *source)
            .collect();
        let selected: BTreeSet<_> = source_pixels.iter().copied().collect();
        let selection = original
            .selection
            .iter()
            .filter(|source| selected.contains(source))
            .copied()
            .collect();
        self.store_target(Target {
            pixels,
            spatial,
            source_pixels,
            selection,
            scope,
        })
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
            .position(|program| Shared::ptr_eq(program, invocation.program()))
        {
            Some(index) => index,
            None => {
                let index = self.sample_programs.len();
                self.sample_programs
                    .push(Shared::clone(invocation.program()));
                index
            }
        };
        let params = BoundParams::from_validated(invocation.params(), &mut self.bind_cache);
        let automation_plan =
            AutomationPlan::from_accepted(&params, invocation.automation(), window.start);
        let index = self.effects.len();
        let automation = if invocation.automation().is_empty() {
            None
        } else {
            let workspace_slot = self.effect_automation_count;
            self.effect_automation_count += 1;
            Some(Box::new(PreparedEffectAutomation {
                workspace_slot,
                bindings: automation_plan,
            }))
        };
        self.effects.push(PreparedEffect {
            start_time: window.start,
            duration: window.duration,
            target: target.index,
            program,
            bound_params: params,
            automation,
        });
        EffectHandle::new(index)
    }

    /// Register authored clip identity independently from storage order.
    pub fn clip(&mut self, id: u32, effect: EffectHandle<'id>) {
        self.clips.push(PreparedClip {
            id,
            effect: effect.index,
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
            .position(|program| Shared::ptr_eq(program, invocation.program()))
        {
            Some(index) => index,
            None => {
                let index = self.operator_programs.len();
                self.operator_programs
                    .push(Shared::clone(invocation.program()));
                index
            }
        };
        let params = BoundParams::from_validated(invocation.params(), &mut self.bind_cache);
        let automation_plan = AutomationPlan::from_accepted(
            &params,
            invocation.automation(),
            SampleTime::from_ticks(0),
        );
        let inputs = (0..invocation.program().input_count())
            .map(|index| input(index).index)
            .collect();
        let index = self.nodes.len();
        let automation_slot = self.operator_automation_count;
        if !invocation.automation().is_empty() {
            self.operator_automation_count += 1;
        }
        self.nodes.push(PreparedSignalNode {
            kind: PreparedSignalKind::Operator {
                operator: PreparedOperatorNode {
                    automation_slot,
                    program,
                    params,
                },
                inputs,
                automation: automation_plan,
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
            (0..self.geometry.len())
                .map(FixtureHandle::new)
                .collect::<Vec<_>>(),
            TargetScope::PerFixture,
        );
        let mut needs_spatial = alloc::vec![false; self.targets.len()];
        let mut needs_sections = alloc::vec![false; self.targets.len()];
        for effect in &self.effects {
            let program = &self.sample_programs[effect.program];
            needs_spatial[effect.target] |= program.uses_spatial_context();
            needs_sections[effect.target] |= program.uses_sections();
        }
        for program in &self.operator_programs {
            needs_spatial[full.index] |= program.uses_spatial_context();
            needs_sections[full.index] |= program.uses_sections();
        }
        let mut fixture_spatial = alloc::vec![false; self.fixtures.len()];
        for (target, &spatial) in self.targets.iter().zip(&needs_spatial) {
            if spatial {
                for pixel in &target.pixels {
                    fixture_spatial[pixel.fixture_index] = true;
                }
            }
        }
        let positions = self
            .geometry
            .iter()
            .enumerate()
            .filter(|(_, geometry)| geometry.retains_fixture())
            .map(|(index, geometry)| {
                if fixture_spatial[self.storage[index]] {
                    geometry
                        .cells()
                        .iter()
                        .map(|&cell| geometry.positions()[cell as usize])
                        .collect()
                } else {
                    Box::new([]) as Box<[[f32; 2]]>
                }
            })
            .collect();
        let mut interner = crate::targets::TargetInterner::default();
        let targets = self
            .targets
            .into_iter()
            .enumerate()
            .map(|(index, target)| {
                let sections = if needs_sections[index] {
                    crate::sections::PreparedSections::new(
                        &target.selection,
                        &target.source_pixels,
                        target.scope == TargetScope::PerFixture,
                    )
                } else {
                    Default::default()
                };
                let max_count = target
                    .pixels
                    .iter()
                    .fold(0, |count, pixel| count.max(pixel.pixel_count));
                let sample_count = if target.pixels.len() > max_count {
                    max_count
                } else {
                    0
                };
                let spatial = if needs_spatial[index] {
                    PreparedTarget::prepare_spatial(&target.spatial)
                } else {
                    Box::new([])
                };
                PreparedTarget {
                    pixels: interner.prepare(target.pixels),
                    spatial,
                    sections,
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
        SequenceData {
            signals: PreparedSignalGraph {
                frame_rate: self.timing.frame_rate(),
                frame_count: self.timing.frame_count(),
                duration: self.timing.duration(),
                fixtures: self.fixtures.into(),
                fixture_pixel_offsets: self.offsets.into(),
                pixel_count: self.pixel_count,
                effects: self.effects.into(),
                clips: self.clips.into(),
                programs: AdmittedPrograms::new(
                    self.sample_programs
                        .into_iter()
                        .map(|program| program.as_ref().clone())
                        .collect(),
                    self.operator_programs
                        .into_iter()
                        .map(|program| program.as_ref().clone())
                        .collect(),
                ),
                targets,
                positions,
                effects_by_layer: self.effects_by_layer.into(),
                layers: self.layers.into(),
                plan: super::schedule::finish_plan(self.nodes, root.index, full.index),
            },
            patch: PreparedPatch {
                routes: self.routes.into(),
                lookups: self.lookups.into(),
            },
            outputs,
        }
    }
}
