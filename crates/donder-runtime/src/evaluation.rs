//! Signal graph evaluation. Every program runs over strips of up to [`STRIP`]
//! pixels; a single pixel is a one-pixel strip. Operators sample their inputs
//! over the same strip, so upstream programs run in strips too.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
mod frames;

use crate::dsl::AutomationPlan;
use crate::dsl::bytecode::{Edges, SignalPixel};
use crate::dsl::{
    BoundParams, OUTSIDE, Pixels, RunContext, STRIP, ScanQuery, SourceWeights, Strip, StripSignals,
    StripWorkspace,
};
use crate::signal::{
    CachedSignalFrame, CachedSignalWindow, EffectAutomationWorkspace, PreparedEffect,
    PreparedOperatorNode, PreparedPixel, PreparedSignalKind, SamplingWorkspace, SignalGraph,
    WINDOW, WINDOW_MARGIN,
};
use alloc::boxed::Box;
use donder_runtime_types::{Color, SampleDuration, SampleTime};
pub(crate) use frames::sample_signal_graph;

/// One effect at one time, sampled over any selection of pixel coordinates.
/// Both sequence playback and sparse editor rasters use this evaluator.
pub(crate) struct EffectSampler<'a> {
    program: &'a crate::dsl::SampleProgram,
    params: &'a BoundParams,
    context: RunContext,
    spatial: bool,
    sections: &'a crate::sections::PreparedSections,
    uses_sections: bool,
}

impl PreparedEffect<AutomationPlan> {
    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn with_sampler<R>(
        &self,
        graph: SignalGraph<'_>,
        sample_time: SampleTime,
        params: &BoundParams,
        run: impl FnOnce(&mut EffectSampler<'_>) -> R,
    ) -> R {
        let program = graph.sample_program(self.program);
        let mut sampler = EffectSampler {
            program,
            params,
            context: RunContext {
                progress: if program.uses_progress() {
                    self.progress(sample_time)
                } else {
                    0.0
                },
                time: self.local_time(sample_time),
                duration: self.duration,
                pixel_count: 0,
            },
            spatial: program.uses_spatial_context(),
            sections: &graph.targets[self.target].sections,
            uses_sections: program.uses_sections(),
        };
        run(&mut sampler)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn automation_workspace(&self) -> Option<EffectAutomationWorkspace> {
        let automation = self.automation.as_ref()?;
        Some(EffectAutomationWorkspace {
            plan: automation.bindings.clone(),
            params: automation.bindings.detach(&self.bound_params),
            sample_time: None,
        })
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn resolve_params<'a>(
        &'a self,
        sample_time: SampleTime,
        automation: impl FnOnce(usize) -> &'a mut EffectAutomationWorkspace,
    ) -> &'a BoundParams {
        match &self.automation {
            None => &self.bound_params,
            Some(binding) => automation(binding.workspace_slot).params_at(sample_time),
        }
    }
}

/// A pixel of a run: its index in the program's target, and its context.
#[derive(Clone, Copy)]
pub(crate) struct RunPixel {
    pub(crate) target_index: usize,
    pub(crate) pixel: PreparedPixel,
}

/// The shared shape of a strip.
struct StripShape {
    len: usize,
    pixel_count: usize,
    min: [f32; 2],
    max: [f32; 2],
}

/// What a program reads of a target's pixels.
#[derive(Clone, Copy)]
struct StripLayout<'a> {
    target: usize,
    spatial: bool,
    sections: Option<&'a crate::sections::PreparedSections>,
    /// Strips must not mix pixel counts or bounds.
    split: bool,
}

/// Fill a strip from `pixel(pixels)`. A strip ends after [`STRIP`] pixels or,
/// for a program that reads them, where the pixel count or target bounds
/// change.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn fill_strip(
    strip: &mut Pixels,
    renderer: SignalGraph<'_>,
    layout: StripLayout<'_>,
    pixels: core::ops::Range<usize>,
    pixel: &impl Fn(usize) -> RunPixel,
) -> StripShape {
    let StripLayout {
        target,
        spatial,
        sections,
        split,
    } = layout;
    let (start, end) = (pixels.start, pixels.end);
    let mut shape = StripShape {
        len: 0,
        pixel_count: 0,
        min: [0.0; 2],
        max: [0.0; 2],
    };
    if !spatial && sections.is_none() {
        // Without geometry only the pixel count can split a run.
        for index in start..end.min(start + STRIP) {
            let pixel = pixel(index).pixel;
            let offset = index - start;
            if offset == 0 {
                shape.pixel_count = pixel.pixel_count;
            } else if split && pixel.pixel_count != shape.pixel_count {
                break;
            }
            strip.index[offset].set(pixel.pixel_index as i32);
            strip.fraction[offset].set(pixel.pixel_fraction);
            shape.len += 1;
        }
        return shape;
    }
    for index in start..end.min(start + STRIP) {
        let RunPixel {
            target_index,
            pixel,
        } = pixel(index);
        let offset = index - start;
        let (mut min, mut max) = ([0.0; 2], [0.0; 2]);
        if spatial {
            let geometry = renderer.spatial_context(true, target, target_index, &pixel);
            strip.x[offset].set(geometry.position[0]);
            strip.y[offset].set(geometry.position[1]);
            (min, max) = (geometry.min, geometry.max);
        }
        if offset == 0 {
            (shape.pixel_count, shape.min, shape.max) = (pixel.pixel_count, min, max);
        } else if split
            && (pixel.pixel_count != shape.pixel_count || (min, max) != (shape.min, shape.max))
        {
            break;
        }
        strip.index[offset].set(pixel.pixel_index as i32);
        strip.fraction[offset].set(pixel.pixel_fraction);
        if let Some(sections) = sections {
            strip.sections[offset] = sections.pixel(target_index);
        }
        shape.len += 1;
    }
    shape
}

impl EffectSampler<'_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn strip<'s>(&'s self, workspace: &'s mut StripWorkspace) -> Strip<'s> {
        Strip::new(
            self.program.bytecode(),
            self.params,
            &self.context,
            Some(self.sections),
            workspace,
        )
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn uniform(&self) -> bool {
        !self.program.bytecode().uses_pixel_context()
    }

    /// Evaluate `count` pixels of this effect's target, `pixel(0..count)`, and
    /// hand each color with its position to `write`. A uniform effect runs once.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn sample_pixels(
        &self,
        renderer: SignalGraph<'_>,
        target: usize,
        count: usize,
        pixel: impl Fn(usize) -> RunPixel,
        workspace: &mut StripWorkspace,
        mut write: impl FnMut(usize, Color),
    ) {
        if count == 0 {
            return;
        }
        let uniform = self.uniform();
        let end = if uniform { 1 } else { count };
        let sections = self.uses_sections.then_some(self.sections);
        let mut strip = self.strip(workspace);
        let mut colors = [black(); STRIP];
        let mut start = 0;
        while start < end {
            let layout = StripLayout {
                target,
                spatial: self.spatial,
                sections,
                split: self.program.reads_target(),
            };
            let shape = fill_strip(strip.pixels(), renderer, layout, start..end, &pixel);
            strip.run(
                shape.pixel_count,
                shape.min,
                shape.max,
                &mut crate::dsl::NoSignals,
                &mut colors[..shape.len],
            );
            for (offset, color) in colors[..shape.len].iter().enumerate() {
                write(start + offset, *color);
            }
            start += shape.len;
        }
        if uniform {
            for index in 1..count {
                write(index, colors[0]);
            }
        }
    }
}

/// Graph admission orders dependencies before consumers and assigns automation
/// slots in that same order. While this operator borrows its parameters, nested
/// sampling may therefore borrow only the preceding automation states.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn operator_params<'a>(
    operator: &'a PreparedOperatorNode,
    automation: &AutomationPlan,
    time: SampleTime,
    states: &'a mut [EffectAutomationWorkspace],
) -> (&'a BoundParams, &'a mut [EffectAutomationWorkspace]) {
    if automation.is_empty() {
        (&operator.params, states)
    } else {
        let (upstream, current) = states.split_at_mut(operator.automation_slot);
        (current[0].params_at(time), upstream)
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
pub(crate) fn frame_range(renderer: SignalGraph<'_>, node_index: usize) -> core::ops::Range<usize> {
    let slot = renderer.plan.frame_slots[node_index];
    let start = slot * renderer.pixel_count;
    start..start + renderer.pixel_count
}

// Called once per frame, from flash, but its loops over the layer's effects
// stay here.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_layer_frame(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    sample_time: SampleTime,
    rendered: &mut [Color],
    workspace: &mut SamplingWorkspace,
) {
    rendered.fill(black());
    if !renderer.layers[layer_index].enabled {
        return;
    }
    for effect_index in &renderer.effects_by_layer[layer_index] {
        let effect = &renderer.effects[*effect_index];
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let sample_count = renderer.targets[effect.target].sample_count;
        for sample in &mut workspace.effect_samples[..sample_count] {
            sample.pixel_count = 0;
        }
        let params =
            effect.resolve_params(sample_time, |slot| &mut workspace.effect_automation[slot]);
        effect.with_sampler(renderer, sample_time, params, |sampler| {
            let target = renderer.target(effect.target);
            if sampler.uniform() {
                let mut color = None;
                sampler.sample_pixels(
                    renderer,
                    effect.target,
                    target.len().min(1),
                    |index| RunPixel {
                        target_index: index,
                        pixel: target.pixel(index),
                    },
                    &mut workspace.effect_strip,
                    |_, sampled| color = Some(sampled),
                );
                if let Some(color) = color {
                    for segment in target.segments() {
                        let base = renderer.fixture_pixel_offsets[segment.fixture]
                            + segment.first_cell as usize;
                        for destination in &mut rendered[base..base + segment.fractions.len()] {
                            compose_max(destination, color);
                        }
                    }
                }
                return;
            }
            // One strip workspace serves every run of this effect at this time.
            // Duplicate fixture layouts share samples by pixel index and count.
            let cacheable = sample_count != 0 && !sampler.spatial && !sampler.uses_sections;
            let (spatial, sections) = (
                sampler.spatial,
                sampler.uses_sections.then_some(sampler.sections),
            );
            let mut strip = sampler.strip(&mut workspace.effect_strip);
            let mut colors = [black(); STRIP];
            for segment in target.segments() {
                let base =
                    renderer.fixture_pixel_offsets[segment.fixture] + segment.first_cell as usize;
                let length = segment.fractions.len();
                let destinations = &mut rendered[base..base + length];
                let pixel = |offset: usize| RunPixel {
                    target_index: segment.start + offset,
                    pixel: segment.pixel(offset),
                };
                let mut offset = 0;
                while offset < length {
                    let samples = &mut workspace.effect_samples;
                    let first = segment.first_index + offset;
                    let cached_len = (length - offset).min(STRIP);
                    if cacheable
                        && samples[first..first + cached_len]
                            .iter()
                            .all(|sample| sample.pixel_count == segment.count)
                    {
                        for (destination, sample) in destinations[offset..offset + cached_len]
                            .iter_mut()
                            .zip(&samples[first..first + cached_len])
                        {
                            compose_max(destination, sample.color);
                        }
                        offset += cached_len;
                        continue;
                    }
                    let layout = StripLayout {
                        target: effect.target,
                        spatial,
                        sections,
                        split: true,
                    };
                    let shape =
                        fill_strip(strip.pixels(), renderer, layout, offset..length, &pixel);
                    let count = shape.len;
                    strip.run(
                        shape.pixel_count,
                        shape.min,
                        shape.max,
                        &mut crate::dsl::NoSignals,
                        &mut colors[..count],
                    );
                    for (destination, color) in
                        destinations[offset..offset + count].iter_mut().zip(&colors)
                    {
                        compose_max(destination, *color);
                    }
                    if cacheable {
                        for (sample, color) in samples[first..first + count].iter_mut().zip(&colors)
                        {
                            sample.pixel_count = segment.count;
                            sample.color = *color;
                        }
                    }
                    offset += count;
                }
            }
        });
    }
}

/// The workspaces nested sampling borrows: the shared sampling workspace, and
/// the automation states and strip workspaces of the operators upstream.
struct Lent<'a> {
    sampling: &'a mut SamplingWorkspace,
    automation: &'a mut [EffectAutomationWorkspace],
    strips: &'a mut [StripWorkspace],
}

impl Lent<'_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn reborrow(&mut self) -> Lent<'_> {
        Lent {
            sampling: self.sampling,
            automation: self.automation,
            strips: self.strips,
        }
    }
}

/// Signal queries of an operator strip. Pixel `n` of the strip is plan-target pixel
/// `first + n`; whole-frame caches are used only at frame scope.
struct GraphSignals<'a> {
    renderer: SignalGraph<'a>,
    inputs: &'a [usize],
    first: usize,
    count: usize,
    /// The operator's node, which keys its scans' frames.
    node: usize,
    /// The operator's depth slot.
    slot: usize,
    /// Whether the operator may use its whole-frame input caches.
    frame_scope: bool,
    lent: Lent<'a>,
}

impl GraphSignals<'_> {
    /// A frame-cached input frame at `time`, computed on first use.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn cached_frame(
        &mut self,
        slot: usize,
        cache: usize,
        node: usize,
        time: SampleTime,
    ) -> &[Color] {
        let key = Some((node, time));
        if self.lent.sampling.operator_frames[slot][cache].key != key {
            let mut stored = core::mem::replace(
                &mut self.lent.sampling.operator_frames[slot][cache],
                CachedSignalFrame {
                    key: None,
                    colors: Box::new([]),
                },
            );
            frames::sample_signal_frame(
                self.renderer,
                node,
                time,
                &mut stored.colors,
                self.lent.reborrow(),
            );
            stored.key = key;
            self.lent.sampling.operator_frames[slot][cache] = stored;
        }
        &self.lent.sampling.operator_frames[slot][cache].colors
    }

    /// `node` at `time` and plan-target pixel `index`, read by strip pixel
    /// `offset`. One run covers the strip and the strip shifted to `index`,
    /// plus margins, so the strip's other reads at nearby offsets reuse it;
    /// `None` when those are too far apart for one window.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn windowed(
        &mut self,
        node: usize,
        time: SampleTime,
        offset: usize,
        index: usize,
    ) -> Option<Color> {
        let key = Some((node, time));
        let window = &self.lent.sampling.operator_windows[self.slot];
        if window.key == key && index.wrapping_sub(window.start) < window.len {
            return Some(window.colors[index - window.start]);
        }
        let shifted = index.saturating_sub(offset);
        let start = shifted.min(self.first).saturating_sub(WINDOW_MARGIN);
        let end =
            (shifted.max(self.first) + self.count + WINDOW_MARGIN).min(self.renderer.pixel_count);
        if end - start > WINDOW {
            return None;
        }
        let mut stored = core::mem::replace(
            &mut self.lent.sampling.operator_windows[self.slot],
            CachedSignalWindow {
                key: None,
                start: 0,
                len: 0,
                colors: Box::new([]),
            },
        );
        sample_signal_run(
            self.renderer,
            node,
            time,
            start,
            &mut stored.colors[..end - start],
            self.lent.reborrow(),
        );
        let color = stored.colors[index - start];
        stored.key = key;
        stored.start = start;
        stored.len = end - start;
        self.lent.sampling.operator_windows[self.slot] = stored;
        Some(color)
    }
}

impl StripSignals for GraphSignals<'_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_strip(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        output: &mut [Color; STRIP],
    ) {
        let count = self.count;
        if time.as_ticks() >= self.renderer.duration.as_ticks() {
            output[..count].fill(black());
            return;
        }
        let node = self.inputs[input];
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            let (slot, first) = (self.slot, self.first);
            let frame = self.cached_frame(slot, cache, node, time);
            output[..count].copy_from_slice(&frame[first..first + count]);
            return;
        }
        sample_signal_run(
            self.renderer,
            node,
            time,
            self.first,
            &mut output[..count],
            self.lent.reborrow(),
        );
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        offset: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        let Some(index) = signal_pixel(self.renderer, self.first + offset, pixel) else {
            return black();
        };
        if time.as_ticks() >= self.renderer.duration.as_ticks() {
            return black();
        }
        let node = self.inputs[input];
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            return self.cached_frame(self.slot, cache, node, time)[index];
        }
        if let Some(color) = self.windowed(node, time, offset, index) {
            return color;
        }
        let mut color = [black()];
        sample_signal_run(
            self.renderer,
            node,
            time,
            index,
            &mut color,
            self.lent.reborrow(),
        );
        color[0]
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn SourceWeights,
        output: &mut [Color; STRIP],
    ) {
        let key = Some((self.node, query.time));
        let (slot, cache) = (self.slot, query.cache);
        if self.lent.sampling.operator_frames[slot][cache].key != key {
            let mut stored = core::mem::replace(
                &mut self.lent.sampling.operator_frames[slot][cache],
                CachedSignalFrame {
                    key: None,
                    colors: Box::new([]),
                },
            );
            frames::scan_frame(self, query, weights, &mut stored.colors);
            stored.key = key;
            self.lent.sampling.operator_frames[slot][cache] = stored;
        }
        let (first, count) = (self.first, self.count);
        let frame = &self.lent.sampling.operator_frames[slot][cache].colors;
        output[..count].copy_from_slice(&frame[first..first + count]);
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_range(
        &mut self,
        input: usize,
        time: SampleTime,
        start: isize,
        frame_cache: Option<usize>,
        colors: &mut [Color],
        locals: &mut [i32],
    ) {
        let renderer = self.renderer;
        let begin = self.first as isize + start;
        let pixels = renderer.target(renderer.plan.target).len() as isize;
        let (low, high) = (
            begin.clamp(0, pixels),
            (begin + colors.len() as isize).clamp(0, pixels),
        );
        let inside = (low - begin) as usize..(high.max(low) - begin) as usize;
        colors.fill(black());
        locals.fill(OUTSIDE);
        let (low, high) = (low as usize, high.max(low) as usize);
        let target = renderer.target(renderer.plan.target);
        for (local, pixel) in locals[inside.clone()].iter_mut().zip(target.iter_from(low)) {
            *local = pixel.pixel_index as i32;
        }
        if inside.is_empty() || time.as_ticks() >= renderer.duration.as_ticks() {
            return;
        }
        let node = self.inputs[input];
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            let frame = self.cached_frame(self.slot, cache, node, time);
            colors[inside].copy_from_slice(&frame[low..high]);
            return;
        }
        sample_signal_run(
            renderer,
            node,
            time,
            low,
            &mut colors[inside],
            self.lent.reborrow(),
        );
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn signal_pixel(
    renderer: SignalGraph<'_>,
    flat_pixel_index: usize,
    pixel: SignalPixel<i32>,
) -> Option<usize> {
    match pixel {
        SignalPixel::Current => Some(flat_pixel_index),
        SignalPixel::Global(index) => usize::try_from(index)
            .ok()
            .filter(|&index| index < renderer.pixel_count),
        SignalPixel::Local(index) => {
            let index = usize::try_from(index).ok()?;
            let current = renderer
                .target(renderer.plan.target)
                .pixel(flat_pixel_index);
            (index < current.pixel_count).then(|| flat_pixel_index - current.pixel_index + index)
        }
        SignalPixel::Shifted(shift, edges) => {
            let current = renderer
                .target(renderer.plan.target)
                .pixel(flat_pixel_index);
            shifted(flat_pixel_index, &current, shift, edges)
        }
    }
}

/// Plan-target pixel `shift` pixels from `flat` along its fixture, read
/// past the fixture's ends as `edges` say.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn shifted(flat: usize, pixel: &PreparedPixel, shift: i32, edges: Edges) -> Option<usize> {
    let local = pixel.pixel_index as i32;
    let read = edges.local(local.wrapping_add(shift), pixel.pixel_count as i32)?;
    Some(flat - pixel.pixel_index + read as usize)
}

/// Evaluate operator `node` over plan-target pixels `first..first + output.len()`.
/// Frame scope (`frames`) lets the operator use its whole-frame input caches.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_operator(
    renderer: SignalGraph<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    frame_scope: bool,
    lent: Lent<'_>,
) {
    let PreparedSignalKind::Operator {
        operator,
        inputs,
        automation,
        vm_slot,
    } = &renderer.plan.nodes[node].kind
    else {
        unreachable!("operator node");
    };
    let program = renderer.operator_program(operator.program);
    let Lent {
        sampling,
        automation: states,
        strips,
    } = lent;
    let (params, upstream) = operator_params(operator, automation, time, states);
    let (upstream_strips, current) = strips.split_at_mut(*vm_slot);
    let duration = renderer.duration;
    let context = RunContext {
        progress: if program.uses_progress() && duration.as_ticks() != 0 {
            (time.as_ticks() as f32 / duration.as_ticks() as f32).clamp(0.0, 1.0)
        } else {
            0.0
        },
        time: SampleDuration::from_ticks(time.as_ticks()),
        duration,
        pixel_count: 0,
    };
    let target = renderer.plan.target;
    let sections = &renderer.targets[target].sections;
    let mut strip = Strip::new(
        program.bytecode(),
        params,
        &context,
        Some(sections),
        &mut current[0],
    );
    let mut signals = GraphSignals {
        renderer,
        inputs,
        first,
        count: 0,
        node,
        slot: *vm_slot,
        frame_scope,
        lent: Lent {
            sampling,
            automation: upstream,
            strips: upstream_strips,
        },
    };
    let pixels = renderer.target(target);
    let pixel = |index: usize| RunPixel {
        target_index: index,
        pixel: pixels.pixel(index),
    };
    let layout = StripLayout {
        target,
        spatial: program.uses_spatial_context(),
        sections: program.uses_sections().then_some(sections),
        split: program.reads_target(),
    };
    let end = first + output.len();
    let mut start = first;
    while start < end {
        let shape = fill_strip(strip.pixels(), renderer, layout, start..end, &pixel);
        signals.first = start;
        signals.count = shape.len;
        let colors = &mut output[start - first..start - first + shape.len];
        strip.run(
            shape.pixel_count,
            shape.min,
            shape.max,
            &mut signals,
            colors,
        );
        start += shape.len;
    }
}

/// Evaluate `node` over plan-target pixels `first..first + output.len()`.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_signal_run(
    renderer: SignalGraph<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    mut lent: Lent<'_>,
) {
    match &renderer.plan.nodes[node].kind {
        PreparedSignalKind::Layer { layer_index } => {
            sample_layer_run(renderer, *layer_index, time, first, output, lent.sampling);
        }
        PreparedSignalKind::Operator { .. } => {
            sample_operator(renderer, node, time, first, output, false, lent);
        }
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            let mut colors = [black(); STRIP];
            for chunk in (0..output.len()).step_by(STRIP) {
                let length = (output.len() - chunk).min(STRIP);
                for &input in inputs {
                    sample_signal_run(
                        renderer,
                        input,
                        time,
                        first + chunk,
                        &mut colors[..length],
                        lent.reborrow(),
                    );
                    for (target, color) in output[chunk..chunk + length].iter_mut().zip(&colors) {
                        compose_max(target, *color);
                    }
                }
            }
        }
    }
}

/// A layer over plan-target pixels `first..first + output.len()`. Effects on
/// other targets sample the matching fixture cells.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_layer_run(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    workspace: &mut SamplingWorkspace,
) {
    output.fill(black());
    if !renderer.layers[layer_index].enabled {
        return;
    }
    let plan_target = renderer.target(renderer.plan.target);
    for &effect_index in &renderer.effects_by_layer[layer_index] {
        let effect = &renderer.effects[effect_index];
        if effect.start_time > time {
            break;
        }
        if !effect.is_active(time) {
            continue;
        }
        let params = effect.resolve_params(time, |slot| &mut workspace.effect_automation[slot]);
        let target = renderer.target(effect.target);
        effect.with_sampler(renderer, time, params, |sampler| {
            if effect.target == renderer.plan.target {
                // Prepared contexts are interned, so the run is the same pixels.
                sampler.sample_pixels(
                    renderer,
                    effect.target,
                    output.len(),
                    |offset| RunPixel {
                        target_index: first + offset,
                        pixel: target.pixel(first + offset),
                    },
                    &mut workspace.effect_strip,
                    |offset, color| compose_max(&mut output[offset], color),
                );
                return;
            }
            // Gather the run's cells that this effect covers into one strip.
            // Temporal queries revisit the same run, so the mapping is kept.
            for chunk in (0..output.len()).step_by(STRIP) {
                let length = (output.len() - chunk).min(STRIP);
                let key = Some((effect.target, first + chunk, length));
                let map = &mut workspace.gather;
                if map.key != key {
                    map.count = 0;
                    for offset in chunk..chunk + length {
                        let pixel = plan_target.pixel(first + offset);
                        if let Some(index) =
                            target.find(pixel.fixture_index, pixel.fixture_pixel_index)
                        {
                            map.cells[map.count] = (offset - chunk, index);
                            map.count += 1;
                        }
                    }
                    map.key = key;
                }
                let (cells, count) = (map.cells, map.count);
                sampler.sample_pixels(
                    renderer,
                    effect.target,
                    count,
                    |offset| RunPixel {
                        target_index: cells[offset].1,
                        pixel: target.pixel(cells[offset].1),
                    },
                    &mut workspace.effect_strip,
                    |offset, color| compose_max(&mut output[chunk + cells[offset].0], color),
                );
            }
        });
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
const fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
