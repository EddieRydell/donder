//! Frame evaluation. Every node of the graph runs once per frame, inputs
//! first, into its whole frame buffer: a layer composites its active clips,
//! an operator runs over the plan target reading its inputs' frames and its
//! own previous frame, and a mix keeps the channel-wise maximum of its
//! inputs. Every program runs over strips of up to [`STRIP`] pixels.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
use crate::dsl::bytecode::{Edges, STATE_INPUT, SignalPixel};
use crate::dsl::{
    Bound, Invocation, OUTSIDE, Override, Pixels, RunContext, STRIP, ScanQuery, Scratch,
    SourceWeights, Strip, StripSignals, StripWorkspace, invocation_params,
};
use crate::resources::{LiveView, ParamsView};
use crate::sections::SectionsView;
use crate::show::Show;
use crate::targets::{PreparedPixel, TargetCursor, TargetView};
use crate::workspace::{CachedEffectSample, RigState, Timing, Workspace};
use donder_runtime_types::image::{self, NONE};
use donder_runtime_types::{Color, SampleDuration, SampleTime};

/// One effect at one time, sampled over any selection of pixel coordinates.
/// Both sequence playback and sparse editor rasters use this evaluator.
pub(crate) struct EffectSampler<'a> {
    pub(crate) bound: Bound<'a>,
    context: RunContext,
    pub(crate) spatial: bool,
    pub(crate) uses_sections: bool,
    pub(crate) target: TargetView<'a>,
}

/// The effects' timing and each layer's start order: the image's, or a rig's
/// as its commands moved them.
#[derive(Clone, Copy)]
pub(crate) struct Timeline<'a> {
    effects: &'a [image::Effect],
    order: &'a [u32],
    ends: &'a [u32],
    timing: &'a [Timing],
}

impl<'a> Timeline<'a> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn new(
        show: Show<'a>,
        timing: &'a [Timing],
        order: &'a [u32],
        ends: &'a [u32],
    ) -> Self {
        let (order, ends) = if timing.is_empty() {
            (show.s.layer_order, show.s.layer_ends)
        } else {
            (order, ends)
        };
        Self {
            effects: show.s.effects,
            order,
            ends,
            timing,
        }
    }

    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn timing(&self, effect: usize) -> Timing {
        match self.timing.get(effect) {
            Some(timing) => *timing,
            None => {
                let effect = &self.effects[effect];
                Timing {
                    start: effect.start,
                    duration: effect.duration,
                }
            }
        }
    }

    /// The layer's effects that may be active at `time`: those starting by
    /// then, from the first whose latest end so far is after it.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn live(&self, layer: &image::Layer, time: SampleTime) -> &'a [u32] {
        let order = layer.effects.get(self.order);
        let ends = layer.effects.get(self.ends);
        let ticks = time.as_ticks();
        let first = ends.partition_point(|&end| end <= ticks);
        let last = first
            + order[first..].partition_point(|&effect| self.timing(effect as usize).start <= ticks);
        &order[first..last]
    }
}

/// A pixel of a run: its index in the program's target, and its context.
#[derive(Clone, Copy)]
pub(crate) struct RunPixel {
    pub(crate) target_index: usize,
    pub(crate) pixel: PreparedPixel,
}

/// Where a traversal's pixels come from. A named type rather than a closure,
/// so its lookup is placed with the rest of evaluation.
#[derive(Clone, Copy)]
pub(crate) enum PixelSource<'s, 'a> {
    /// Target indices from zero.
    Target(&'s TargetCursor<'a>),
    /// Gathered cells: each a target index and its plan-target address.
    Cells(&'s TargetCursor<'a>, &'s [[u32; 2]]),
    /// Pixels listed in advance.
    #[cfg(feature = "host")]
    Listed(&'s [RunPixel]),
}

impl PixelSource<'_, '_> {
    #[inline(never)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn get(&self, k: usize) -> RunPixel {
        match *self {
            Self::Target(cursor) => RunPixel {
                target_index: k,
                pixel: cursor.pixel(k),
            },
            Self::Cells(cursor, cells) => {
                let index = cells[k][0] as usize;
                RunPixel {
                    target_index: index,
                    pixel: cursor.pixel(index),
                }
            }
            #[cfg(feature = "host")]
            Self::Listed(pixels) => pixels[k],
        }
    }

    /// Whether pixel `k`'s address follows pixel `start`'s by `k - start`.
    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn adjacent(&self, start: usize, k: usize) -> bool {
        match *self {
            Self::Cells(_, cells) => cells[k][1] == cells[start][1] + (k - start) as u32,
            _ => true,
        }
    }
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
    show: Show<'a>,
    target: TargetView<'a>,
    spatial: bool,
    sections: Option<SectionsView<'a>>,
    /// Strips must not mix pixel counts or bounds.
    split: bool,
    /// Strips must cover consecutive plan-target addresses, as a program
    /// reading its layer's previous frame reads it by address.
    contiguous: bool,
}

/// Fill a strip from `pixel(pixels)`. A strip ends after [`STRIP`] pixels or,
/// for a program that reads them, where the pixel count or target bounds
/// change, or the addresses stop being consecutive. One copy serves every
/// traversal.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn fill_strip(
    strip: &Pixels<'_>,
    layout: StripLayout<'_>,
    pixels: core::ops::Range<usize>,
    pixel: PixelSource<'_, '_>,
) -> StripShape {
    let StripLayout {
        show,
        target,
        spatial,
        sections,
        split,
        contiguous,
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
            let pixel_at = pixel.get(index).pixel;
            let offset = index - start;
            if offset == 0 {
                shape.pixel_count = pixel_at.pixel_count;
            } else if (split && pixel_at.pixel_count != shape.pixel_count)
                || (contiguous && !pixel.adjacent(start, index))
            {
                break;
            }
            strip.index[offset].set(pixel_at.pixel_index as i32);
            strip.fraction[offset].set(pixel_at.fraction());
            shape.len += 1;
        }
        return shape;
    }
    for index in start..end.min(start + STRIP) {
        let RunPixel {
            target_index,
            pixel: pixel_at,
        } = pixel.get(index);
        let offset = index - start;
        let (mut min, mut max) = ([0.0; 2], [0.0; 2]);
        if spatial {
            let geometry = target.spatial_context(target_index, &pixel_at, show);
            strip.x[offset].set(geometry.position[0]);
            strip.y[offset].set(geometry.position[1]);
            (min, max) = (geometry.min, geometry.max);
        }
        if offset == 0 {
            (shape.pixel_count, shape.min, shape.max) = (pixel_at.pixel_count, min, max);
        } else if (split
            && (pixel_at.pixel_count != shape.pixel_count || (min, max) != (shape.min, shape.max)))
            || (contiguous && !pixel.adjacent(start, index))
        {
            break;
        }
        strip.index[offset].set(pixel_at.pixel_index as i32);
        strip.fraction[offset].set(pixel_at.fraction());
        if let Some(sections) = sections {
            strip.sections[offset].set(sections.pixel(target_index));
        }
        shape.len += 1;
    }
    shape
}

impl<'a> EffectSampler<'a> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn strip<'s, 'w>(&'s self, workspace: &'s mut StripWorkspace<'w>) -> Strip<'s, 'w> {
        Strip::new(self.bound, &self.context, workspace)
    }

    /// Whether every pixel of the effect has one color.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn uniform(&self) -> bool {
        !self.bound.program.flag(image::PROGRAM_USES_PIXEL_CONTEXT) && !self.reads_state()
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn reads_state(&self) -> bool {
        self.bound.program.flag(image::PROGRAM_READS_STATE)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn layout(&self) -> StripLayout<'a> {
        StripLayout {
            show: self.bound.show,
            target: self.target,
            spatial: self.spatial,
            sections: self.uses_sections.then_some(self.target.sections),
            split: self.bound.program.flag(image::PROGRAM_READS_TARGET),
            contiguous: self.reads_state(),
        }
    }

    /// Evaluate `count` pixels of this effect's target, `pixel(0..count)`, and
    /// hand each color with its position to `write`. A uniform effect runs once.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn sample_pixels(
        &self,
        count: usize,
        pixel: PixelSource<'_, '_>,
        workspace: &mut StripWorkspace<'_>,
        signals: &mut dyn StripSignals,
        mut write: impl FnMut(usize, Color),
    ) {
        if count == 0 {
            return;
        }
        let uniform = self.uniform();
        let end = if uniform { 1 } else { count };
        let layout = self.layout();
        let mut strip = self.strip(workspace);
        let mut colors = [black(); STRIP];
        let mut start = 0;
        while start < end {
            let shape = fill_strip(strip.pixels(), layout, start..end, pixel);
            strip.run(
                shape.pixel_count,
                shape.min,
                shape.max,
                signals,
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

/// A plan-target pixel's place: its fixture's offset plus its cell.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn base(show: Show<'_>, fixture: usize, cell: u32) -> usize {
    show.s.fixtures[fixture].offset as usize + cell as usize
}

/// The frame a program reads as `input`: its own previous frame, or an input
/// node's frame of this frame.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn input_frame<'f>(
    buffers: &'f [&mut [Color]],
    nodes: &[image::Node],
    inputs: &[u32],
    state: &'f [Color],
    input: usize,
) -> &'f [Color] {
    if input == usize::from(STATE_INPUT) {
        state
    } else {
        buffers[nodes[inputs[input] as usize].buffer as usize]
    }
}

/// Signal reads of a strip, from frames this frame already evaluated. Pixel
/// `n` of the strip is plan-target pixel `first + n`.
pub(crate) struct FrameSignals<'g, 'w> {
    plan: TargetView<'g>,
    nodes: &'g [image::Node],
    inputs: &'g [u32],
    buffers: &'g [&'w mut [Color]],
    /// The running node's previous frame, or empty.
    state: &'g [Color],
    scans: &'g mut [&'w mut [Color]],
    /// The running program's scans computed this frame, by bit.
    scanned: u32,
    first: usize,
    count: usize,
}

impl FrameSignals<'_, '_> {
    #[inline(always)]
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn frame(&self, input: usize) -> &[Color] {
        input_frame(self.buffers, self.nodes, self.inputs, self.state, input)
    }
}

impl StripSignals for FrameSignals<'_, '_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_strip(&mut self, input: usize, output: &mut [Color; STRIP]) {
        let (first, count) = (self.first, self.count);
        output[..count].copy_from_slice(&self.frame(input)[first..first + count]);
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_pixel(&mut self, input: usize, offset: usize, pixel: SignalPixel<i32>) -> Color {
        match signal_pixel(self.plan, self.first + offset, pixel) {
            Some(index) => self.frame(input)[index],
            None => black(),
        }
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn SourceWeights,
        output: &mut [Color; STRIP],
    ) {
        let bit = 1 << query.cache;
        if self.scanned & bit == 0 {
            let input = input_frame(
                self.buffers,
                self.nodes,
                self.inputs,
                self.state,
                query.input,
            );
            crate::scan::scan_frame(self.plan, query, weights, input, self.scans[query.cache]);
            self.scanned |= bit;
        }
        let (first, count) = (self.first, self.count);
        output[..count].copy_from_slice(&self.scans[query.cache][first..first + count]);
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_range(
        &mut self,
        input: usize,
        start: isize,
        colors: &mut [Color],
        locals: &mut [i32],
    ) {
        let begin = self.first as isize + start;
        let pixels = self.plan.len() as isize;
        let (low, high) = (
            begin.clamp(0, pixels),
            (begin + colors.len() as isize).clamp(0, pixels),
        );
        let inside = (low - begin) as usize..(high.max(low) - begin) as usize;
        colors.fill(black());
        locals.fill(OUTSIDE);
        let (low, high) = (low as usize, high.max(low) as usize);
        for (local, pixel) in locals[inside.clone()]
            .iter_mut()
            .zip(self.plan.iter_from(low))
        {
            *local = pixel.pixel_index as i32;
        }
        colors[inside].copy_from_slice(&self.frame(input)[low..high]);
    }
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn signal_pixel(plan: TargetView<'_>, flat: usize, pixel: SignalPixel<i32>) -> Option<usize> {
    match pixel {
        SignalPixel::Current => Some(flat),
        SignalPixel::Local(index) => {
            let index = usize::try_from(index).ok()?;
            let current = plan.pixel(flat);
            (index < current.pixel_count).then(|| flat - current.pixel_index + index)
        }
        SignalPixel::Shifted(shift, edges) => shifted(flat, &plan.pixel(flat), shift, edges),
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

/// What every program of a frame runs with.
struct Programs<'r, 'w> {
    strip: &'r mut StripWorkspace<'w>,
    scratch: &'r mut Scratch<'w>,
    overrides: &'r [Override],
    live: LiveView<'r>,
    time: SampleTime,
    frames_elapsed: u32,
}

impl<'w> Programs<'_, 'w> {
    /// An invocation's parameters at this frame.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn params<'s>(
        &'s mut self,
        show: Show<'s>,
        invocation: Invocation,
    ) -> (ParamsView<'s>, &'s mut StripWorkspace<'w>) {
        let overrides = overrides(show, invocation.plan, self.overrides);
        let params = invocation_params(
            show,
            self.scratch,
            invocation,
            self.time,
            overrides,
            self.live,
        );
        (params, self.strip)
    }
}

/// The controls of `plan`'s bindings, or none.
#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
pub(crate) fn overrides<'o>(show: Show<'_>, plan: u32, all: &'o [Override]) -> &'o [Override] {
    match show.plan(plan) {
        Some(plan) if !all.is_empty() => plan.bindings.get(all),
        _ => &[],
    }
}

/// Every active clip of `layer`, max-composited into `rendered`.
// Called once per layer per frame, but its loops over the layer's effects
// stay here.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
#[allow(clippy::too_many_arguments)]
fn sample_layer(
    show: Show<'_>,
    layer: &image::Layer,
    timeline: Timeline<'_>,
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_, '_>,
    programs: &mut Programs<'_, '_>,
    gather: &mut [[u32; 2]],
    effect_samples: &mut [CachedEffectSample],
) {
    rendered.fill(black());
    if layer.enabled == 0 {
        return;
    }
    let time = programs.time;
    for &effect_index in timeline.live(layer, time) {
        let effect_index = effect_index as usize;
        let timing = timeline.timing(effect_index);
        if !timing.is_active(time) {
            continue;
        }
        let effect = &show.s.effects[effect_index];
        let frames_elapsed = programs.frames_elapsed;
        let (params, workspace) = programs.params(
            show,
            Invocation {
                params: effect.params,
                plan: effect.automation,
                origin: timing.start,
                effect: effect_index as u32,
            },
        );
        let sampler = effect_sampler(show, effect, timing, time, frames_elapsed, params);
        let target = sampler.target;
        signals.scanned = 0;
        if sampler.uniform() {
            let mut color = None;
            let cursor = target.cursor();
            sampler.sample_pixels(
                target.len().min(1),
                PixelSource::Target(&cursor),
                workspace,
                signals,
                |_, sampled| color = Some(sampled),
            );
            if let Some(color) = color {
                for segment in target.segments() {
                    let base = base(show, segment.fixture, segment.first_cell);
                    for destination in &mut rendered[base..base + segment.len] {
                        compose_max(destination, color);
                    }
                }
            }
            continue;
        }
        // Duplicate fixture layouts share samples by pixel index and count,
        // unless the effect reads its layer's frame by address.
        let sample_count = target.sample_count();
        let cacheable = sample_count != 0
            && !sampler.spatial
            && !sampler.uses_sections
            && !sampler.reads_state();
        if cacheable {
            for sample in &mut effect_samples[..sample_count] {
                sample.pixel_count = 0;
            }
        }
        let layout = sampler.layout();
        let mut strip = sampler.strip(workspace);
        // Strips gather pixels across fixtures: each pending pixel's target
        // index and plan-target address.
        let mut count = 0;
        for segment in target.segments() {
            let base = base(show, segment.fixture, segment.first_cell);
            for offset in 0..segment.len {
                if cacheable {
                    let sample = &effect_samples[segment.first_index + offset];
                    if sample.pixel_count as usize == segment.count {
                        compose_max(&mut rendered[base + offset], sample.color);
                        continue;
                    }
                }
                gather[count] = [(segment.start + offset) as u32, (base + offset) as u32];
                count += 1;
                if count == STRIP {
                    run_gathered(
                        layout,
                        &mut strip,
                        gather,
                        rendered,
                        signals,
                        cacheable.then_some(&mut effect_samples[..]),
                    );
                    count = 0;
                }
            }
        }
        run_gathered(
            layout,
            &mut strip,
            &gather[..count],
            rendered,
            signals,
            cacheable.then_some(&mut effect_samples[..]),
        );
    }
}

/// Run gathered pixels, each a target index and its address in `rendered`,
/// in as few strips as the program's layout allows. With `samples`, each
/// result is kept for duplicate fixture layouts.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn run_gathered(
    layout: StripLayout<'_>,
    strip: &mut Strip<'_, '_>,
    cells: &[[u32; 2]],
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_, '_>,
    mut samples: Option<&mut [CachedEffectSample]>,
) {
    // Gathered indices ascend, so both lookups follow the target's runs.
    let (target, cached) = (layout.target.cursor(), layout.target.cursor());
    let pixel = PixelSource::Cells(&target, cells);
    let mut colors = [black(); STRIP];
    let mut start = 0;
    while start < cells.len() {
        let shape = fill_strip(strip.pixels(), layout, start..cells.len(), pixel);
        let count = shape.len;
        signals.first = cells[start][1] as usize;
        signals.count = count;
        strip.run(
            shape.pixel_count,
            shape.min,
            shape.max,
            signals,
            &mut colors[..count],
        );
        for (k, color) in colors[..count].iter().enumerate() {
            compose_max(&mut rendered[cells[start + k][1] as usize], *color);
            if let Some(samples) = samples.as_deref_mut() {
                let pixel = cached.pixel(cells[start + k][0] as usize);
                samples[pixel.pixel_index] = CachedEffectSample {
                    pixel_count: pixel.pixel_count as u32,
                    color: *color,
                };
            }
        }
        start += count;
    }
}

/// Operator `node` over every plan-target pixel into `rendered`.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_operator(
    show: Show<'_>,
    node: &image::Node,
    plan: TargetView<'_>,
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_, '_>,
    programs: &mut Programs<'_, '_>,
) {
    let program = show.program(node.program);
    let (time, frames_elapsed) = (programs.time, programs.frames_elapsed);
    let duration = show.duration();
    let context = RunContext {
        progress: time.as_ticks() as f32 / duration.as_ticks() as f32,
        time: SampleDuration::from_ticks(time.as_ticks()),
        duration,
        frame_rate: show.frame_rate() as f32,
        frames_elapsed,
        pixel_count: 0,
    };
    let (params, workspace) = programs.params(
        show,
        Invocation {
            params: node.params,
            plan: node.automation,
            origin: 0,
            effect: NONE,
        },
    );
    let bound = Bound {
        show,
        program,
        params,
        sections: Some(plan.sections),
    };
    let mut strip = Strip::new(bound, &context, workspace);
    let pixels = plan.cursor();
    let pixel = PixelSource::Target(&pixels);
    let layout = StripLayout {
        show,
        target: plan,
        spatial: program.flag(image::PROGRAM_USES_SPATIAL),
        sections: program
            .flag(image::PROGRAM_USES_SECTIONS)
            .then_some(plan.sections),
        split: program.flag(image::PROGRAM_READS_TARGET),
        contiguous: false,
    };
    let end = rendered.len();
    let mut start = 0;
    while start < end {
        let shape = fill_strip(strip.pixels(), layout, start..end, pixel);
        signals.first = start;
        signals.count = shape.len;
        strip.run(
            shape.pixel_count,
            shape.min,
            shape.max,
            signals,
            &mut rendered[start..start + shape.len],
        );
        start += shape.len;
    }
}

#[inline(always)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
pub(crate) fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
const fn black() -> Color {
    Color::BLACK
}

/// The buffer holding the output's frame.
pub(crate) fn output_buffer(show: Show<'_>) -> usize {
    show.output().map_or(0, |node| node.buffer as usize)
}

/// Once per frame or per clip, so it stays in flash, outside the instruction
/// RAM that `pnpm firmware:build` checks the rest of evaluation against.
mod frames {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    /// Sort a layer's effects by start and record, for each, the latest end of
    /// it and every effect before it. Effects before the first end after a time
    /// have all ended by then.
    pub(crate) fn index_layer(
        layer: &image::Layer,
        timing: &[Timing],
        order: &mut [u32],
        ends: &mut [u32],
    ) {
        let range =
            layer.effects.start as usize..(layer.effects.start + layer.effects.len) as usize;
        let order = &mut order[range.clone()];
        order.sort_unstable_by_key(|&effect| (timing[effect as usize].start, effect));
        let mut latest = 0;
        for (end, &effect) in ends[range].iter_mut().zip(order.iter()) {
            let timing = timing[effect as usize];
            latest = latest.max(timing.start.saturating_add(timing.duration));
            *end = latest;
        }
    }

    /// Evaluate every node at `time` into its frame buffer, `frames_elapsed`
    /// frames after the frame before, and keep each stateful node's frame as its
    /// previous. Returns the buffer holding the output's frame.
    #[inline(never)]
    pub(crate) fn evaluate_frame(
        show: Show<'_>,
        workspace: &mut Workspace<'_>,
        time: SampleTime,
        frames_elapsed: u32,
    ) -> usize {
        let result = output_buffer(show);
        if show.output().is_none() || time.as_ticks() >= show.duration().as_ticks() {
            workspace.buffers[result].fill(black());
            return result;
        }
        let Workspace {
            buffers,
            states,
            scans,
            strip,
            scratch,
            gather,
            effect_samples,
            rig,
            ..
        } = workspace;
        let RigState {
            timing,
            layer_order,
            layer_ends,
            overrides,
            live_tracks,
            live_ticks,
        } = rig;
        let timeline = Timeline::new(show, timing, layer_order, layer_ends);
        let plan = TargetView::of(show, &show.s.targets[show.meta().plan_target as usize]);
        let mut programs = Programs {
            strip,
            scratch,
            overrides,
            live: LiveView {
                tracks: live_tracks,
                ticks: live_ticks,
            },
            time,
            frames_elapsed,
        };
        for node in show.s.nodes {
            let buffer = node.buffer as usize;
            let rendered = core::mem::take(&mut buffers[buffer]);
            {
                let state: &[Color] = match node.state {
                    NONE => &[],
                    state => states[state as usize],
                };
                let mut signals = FrameSignals {
                    plan,
                    nodes: show.s.nodes,
                    inputs: show.inputs(node),
                    buffers,
                    state,
                    scans,
                    scanned: 0,
                    first: 0,
                    count: 0,
                };
                match node.kind {
                    image::NODE_LAYER => sample_layer(
                        show,
                        &show.s.layers[node.layer as usize],
                        timeline,
                        rendered,
                        &mut signals,
                        &mut programs,
                        gather,
                        effect_samples,
                    ),
                    image::NODE_OPERATOR => {
                        sample_operator(show, node, plan, rendered, &mut signals, &mut programs);
                    }
                    _ => {
                        rendered.fill(black());
                        for input in 0..signals.inputs.len() {
                            for (target, &color) in rendered.iter_mut().zip(signals.frame(input)) {
                                compose_max(target, color);
                            }
                        }
                    }
                }
            }
            if node.state != NONE {
                states[node.state as usize].copy_from_slice(rendered);
            }
            buffers[buffer] = rendered;
        }
        result
    }

    /// An effect's sampler at `time`. Once per effect sample.
    #[inline(never)]
    pub(crate) fn effect_sampler<'a>(
        show: Show<'a>,
        effect: &'a image::Effect,
        timing: Timing,
        time: SampleTime,
        frames_elapsed: u32,
        params: ParamsView<'a>,
    ) -> EffectSampler<'a> {
        let program = show.program(effect.program);
        let target = TargetView::of(show, &show.s.targets[effect.target as usize]);
        let elapsed = time.as_ticks() - timing.start;
        EffectSampler {
            bound: Bound {
                show,
                program,
                params,
                sections: Some(target.sections),
            },
            context: RunContext {
                progress: elapsed as f32 / timing.duration as f32,
                time: SampleDuration::from_ticks(elapsed),
                duration: SampleDuration::from_ticks(timing.duration),
                frame_rate: show.frame_rate() as f32,
                frames_elapsed,
                pixel_count: 0,
            },
            spatial: program.flag(image::PROGRAM_USES_SPATIAL),
            uses_sections: program.flag(image::PROGRAM_USES_SECTIONS),
            target,
        }
    }
}
pub(crate) use frames::{effect_sampler, evaluate_frame, index_layer};
