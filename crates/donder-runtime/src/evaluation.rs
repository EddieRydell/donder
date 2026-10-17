//! Signal graph evaluation. Every program runs over strips of up to [`STRIP`]
//! pixels; a single pixel is a one-pixel strip. Operators sample their inputs
//! over the same strip, so upstream programs run in strips too.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
mod frames;

use crate::dsl::bytecode::{Edges, SignalPixel};
use crate::dsl::{
    Bound, NoSignals, OUTSIDE, Pixels, RunContext, STRIP, ScanQuery, SourceWeights, States, Strip,
    StripSignals, StripWorkspace,
};
use crate::resources::{LiveView, ParamsView};
use crate::sections::SectionsView;
use crate::show::Show;
use crate::targets::{PreparedPixel, TargetCursor, TargetView};
use crate::workspace::{
    CachedEffectSample, Key, SamplingWorkspace, TemporalRecord, Timing, WINDOW, WINDOW_MARGIN,
    WindowKey,
};
use donder_runtime_types::image::{self, NONE};
use donder_runtime_types::{Color, SampleDuration, SampleTime};
use frames::live_effects;
pub(crate) use frames::{effect_params, effect_sampler, index_layer, sample_signal_graph};

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

    /// The layer's effects that may be active at `time`.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn live(&self, layer: &image::Layer, time: SampleTime) -> &'a [u32] {
        live_effects(layer, time, self.order, self.ends, |effect| {
            self.timing(effect).start
        })
    }
}

/// How a frame or run is written into its destination.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Blend {
    /// Replace what the destination holds.
    Write,
    /// Keep the channel-wise maximum, as signals fanning into a mix combine.
    Max,
}

impl Blend {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn apply(self, destination: &mut [Color], colors: &[Color]) {
        match self {
            Self::Write => destination.copy_from_slice(colors),
            Self::Max => {
                for (target, &color) in destination.iter_mut().zip(colors) {
                    compose_max(target, color);
                }
            }
        }
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
    /// The target indices in column `.2` of gathered cells.
    Cells(&'s TargetCursor<'a>, &'s [[u32; 2]], usize),
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
            Self::Cells(cursor, cells, column) => {
                let index = cells[k][column] as usize;
                RunPixel {
                    target_index: index,
                    pixel: cursor.pixel(index),
                }
            }
            #[cfg(feature = "host")]
            Self::Listed(pixels) => pixels[k],
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
}

/// Fill a strip from `pixel(pixels)`. A strip ends after [`STRIP`] pixels or,
/// for a program that reads them, where the pixel count or target bounds
/// change. One copy serves every traversal.
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
            let pixel = pixel.get(index).pixel;
            let offset = index - start;
            if offset == 0 {
                shape.pixel_count = pixel.pixel_count;
            } else if split && pixel.pixel_count != shape.pixel_count {
                break;
            }
            strip.index[offset].set(pixel.pixel_index as i32);
            strip.fraction[offset].set(pixel.fraction());
            shape.len += 1;
        }
        return shape;
    }
    for index in start..end.min(start + STRIP) {
        let RunPixel {
            target_index,
            pixel,
        } = pixel.get(index);
        let offset = index - start;
        let (mut min, mut max) = ([0.0; 2], [0.0; 2]);
        if spatial {
            let geometry = target.spatial_context(target_index, &pixel, show);
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
        strip.fraction[offset].set(pixel.fraction());
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

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(crate) fn uniform(&self) -> bool {
        !self.bound.program.flag(image::PROGRAM_USES_PIXEL_CONTEXT)
    }

    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn layout(&self) -> StripLayout<'a> {
        StripLayout {
            show: self.bound.show,
            target: self.target,
            spatial: self.spatial,
            sections: self.uses_sections.then_some(self.target.sections),
            split: self.bound.program.flag(image::PROGRAM_READS_TARGET),
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
                &mut NoSignals,
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

// Called once per frame, from flash, but its loops over the layer's effects
// stay here.
#[inline(never)]
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_layer_frame(
    show: Show<'_>,
    layer: &image::Layer,
    time: SampleTime,
    rendered: &mut [Color],
    blend: Blend,
    workspace: &mut SamplingWorkspace<'_>,
) {
    if blend == Blend::Write {
        rendered.fill(black());
    }
    if layer.enabled == 0 {
        return;
    }
    let SamplingWorkspace {
        effect_strip,
        gather,
        effect_samples,
        effect_states,
        timing,
        layer_order,
        layer_ends,
        live_tracks,
        live_ticks,
        ..
    } = workspace;
    let live = LiveView {
        tracks: live_tracks,
        ticks: live_ticks,
    };
    let timeline = Timeline::new(show, timing, layer_order, layer_ends);
    for &effect_index in timeline.live(layer, time) {
        let effect_index = effect_index as usize;
        let effect = &show.s.effects[effect_index];
        let timing = timeline.timing(effect_index);
        if !timing.is_active(time) {
            continue;
        }
        let params = effect_params(show, effect_index, time, effect_states, live);
        let sampler = effect_sampler(show, effect, timing, time, params);
        let target = sampler.target;
        let sample_count = target.sample_count();
        for sample in &mut effect_samples[..sample_count] {
            sample.pixel_count = 0;
        }
        if sampler.uniform() {
            let mut color = None;
            let cursor = target.cursor();
            sampler.sample_pixels(
                target.len().min(1),
                PixelSource::Target(&cursor),
                effect_strip,
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
        // One strip workspace serves every run of this effect at this time.
        // Duplicate fixture layouts share samples by pixel index and count.
        let cacheable = sample_count != 0 && !sampler.spatial && !sampler.uses_sections;
        let layout = sampler.layout();
        let mut strip = sampler.strip(effect_strip);
        // Strips gather pixels across fixtures: each pending pixel's target
        // index and destination.
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
            cacheable.then_some(&mut effect_samples[..]),
        );
    }
}

/// Run gathered pixels, each a target index and its destination in `rendered`,
/// in as few strips as the program's layout allows. With `samples`, each
/// result is kept for duplicate fixture layouts.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn run_gathered(
    layout: StripLayout<'_>,
    strip: &mut Strip<'_, '_>,
    cells: &[[u32; 2]],
    rendered: &mut [Color],
    mut samples: Option<&mut [CachedEffectSample]>,
) {
    // Gathered indices ascend, so both lookups follow the target's runs.
    let (target, cached) = (layout.target.cursor(), layout.target.cursor());
    let pixel = PixelSource::Cells(&target, cells, 0);
    let mut colors = [black(); STRIP];
    let mut start = 0;
    while start < cells.len() {
        let shape = fill_strip(strip.pixels(), layout, start..cells.len(), pixel);
        let count = shape.len;
        strip.run(
            shape.pixel_count,
            shape.min,
            shape.max,
            &mut NoSignals,
            &mut colors[..count],
        );
        for (k, color) in colors[..count].iter().enumerate() {
            compose_max(&mut rendered[cells[start + k][1] as usize], *color);
            if let Some(samples) = samples.as_deref_mut() {
                let pixel = cached.pixel(cells[start + k][0] as usize);
                samples[pixel.pixel_index] = CachedEffectSample {
                    pixel_count: pixel.pixel_count as u32,
                    color: *color,
                    pad: 0,
                };
            }
        }
        start += count;
    }
}

/// The workspaces nested sampling borrows: the shared sampling workspace, and
/// the automation states, strip workspaces and time-scan states of the
/// operators upstream. Inputs come before their consumers, so each of these
/// splits at the consumer.
pub(super) struct Lent<'l, 'w> {
    pub(super) sampling: &'l mut SamplingWorkspace<'w>,
    pub(super) states: States<'l>,
    pub(super) strips: &'l mut [Option<StripWorkspace<'w>>],
    pub(super) temporal: &'l mut [TemporalRecord],
    pub(super) running: &'l mut [[f32; 3]],
}

impl<'w> Lent<'_, 'w> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(super) fn reborrow(&mut self) -> Lent<'_, 'w> {
        Lent {
            sampling: self.sampling,
            states: self.states.reborrow(),
            strips: self.strips,
            temporal: self.temporal,
            running: self.running,
        }
    }
}

/// An operator's own time-scan records and the running values from the
/// first of them.
pub(super) struct Temporal<'l> {
    pub(super) records: &'l mut [TemporalRecord],
    pub(super) running: &'l mut [[f32; 3]],
}

/// Signal queries of an operator strip. Pixel `n` of the strip is plan-target pixel
/// `first + n`; whole-frame caches are used only at frame scope.
pub(super) struct GraphSignals<'g, 'w> {
    pub(super) show: Show<'g>,
    pub(super) plan: TargetView<'g>,
    pub(super) inputs: &'g [u32],
    /// The show's end, in ticks: inputs are black from then on.
    pub(super) end: u32,
    pub(super) first: usize,
    pub(super) count: usize,
    /// The operator's node, which keys its scans' frames.
    pub(super) node: usize,
    /// The operator's depth slot.
    pub(super) slot: usize,
    /// Whether the operator may use its whole-frame input caches.
    pub(super) frame_scope: bool,
    pub(super) temporal: Temporal<'g>,
    pub(super) lent: Lent<'g, 'w>,
}

impl GraphSignals<'_, '_> {
    /// A frame-cached input frame at `time`, computed on first use.
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    pub(super) fn cached_frame(&mut self, cache: usize, node: usize, time: SampleTime) -> &[Color] {
        let key = Key::of(node, time);
        let at = usize::from(self.lent.sampling.frame_base[self.slot]) + cache;
        if self.lent.sampling.frame_keys[at] != key {
            frames::fill_frame(self, at, key, node, time);
        }
        self.lent.sampling.frames[at]
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
        let key = Key::of(node, time);
        let window = &self.lent.sampling.windows[self.slot];
        if window.key.key == key && index.wrapping_sub(window.key.start) < window.key.len {
            return Some(window.colors[index - window.key.start]);
        }
        let shifted = index.saturating_sub(offset);
        let start = shifted.min(self.first).saturating_sub(WINDOW_MARGIN);
        let end = (shifted.max(self.first) + self.count + WINDOW_MARGIN).min(self.plan.len());
        if end - start > WINDOW {
            return None;
        }
        let mut stored = core::mem::take(&mut self.lent.sampling.windows[self.slot]);
        sample_signal_run(
            self.show,
            node,
            time,
            start,
            &mut stored.colors[..end - start],
            self.lent.reborrow(),
        );
        let color = stored.colors[index - start];
        stored.key = WindowKey {
            key,
            start,
            len: end - start,
        };
        self.lent.sampling.windows[self.slot] = stored;
        Some(color)
    }
}

impl StripSignals for GraphSignals<'_, '_> {
    #[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
    fn sample_strip(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        output: &mut [Color; STRIP],
    ) {
        let count = self.count;
        if time.as_ticks() >= self.end {
            output[..count].fill(black());
            return;
        }
        let node = self.inputs[input] as usize;
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            let first = self.first;
            let frame = self.cached_frame(cache, node, time);
            output[..count].copy_from_slice(&frame[first..first + count]);
            return;
        }
        sample_signal_run(
            self.show,
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
        let Some(index) = signal_pixel(self.plan, self.first + offset, pixel) else {
            return black();
        };
        if time.as_ticks() >= self.end {
            return black();
        }
        let node = self.inputs[input] as usize;
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            return self.cached_frame(cache, node, time)[index];
        }
        if let Some(color) = self.windowed(node, time, offset, index) {
            return color;
        }
        let mut color = [black()];
        sample_signal_run(
            self.show,
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
        let key = Key::of(self.node, query.time);
        let at = usize::from(self.lent.sampling.frame_base[self.slot]) + query.cache;
        if self.lent.sampling.frame_keys[at] != key {
            frames::fill_scan(self, at, key, query, weights);
        }
        let (first, count) = (self.first, self.count);
        let frame = &self.lent.sampling.frames[at];

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
        if inside.is_empty() || time.as_ticks() >= self.end {
            return;
        }
        let node = self.inputs[input] as usize;
        if let (true, Some(cache)) = (self.frame_scope, frame_cache) {
            let frame = self.cached_frame(cache, node, time);
            colors[inside].copy_from_slice(&frame[low..high]);
            return;
        }
        sample_signal_run(
            self.show,
            node,
            time,
            low,
            &mut colors[inside],
            self.lent.reborrow(),
        );
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

/// Evaluate operator `node` over plan-target pixels `first..first + output.len()`.
/// Frame scope (`frames`) lets the operator use its whole-frame input caches.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
#[allow(clippy::too_many_arguments)]
fn sample_operator(
    show: Show<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    blend: Blend,
    frame_scope: bool,
    lent: Lent<'_, '_>,
) {
    let record = &show.s.nodes[node];
    let program = show.program(record.program);
    let Lent {
        sampling,
        mut states,
        strips,
        temporal,
        running,
    } = lent;
    let (upstream, params) = match sampling.operator_state[node] {
        NONE => (states, ParamsView::image(show, record.params)),
        state => {
            let (upstream, current) = states.split(state as usize);
            (upstream, current.params_at(show, time, LiveView::default()))
        }
    };
    let slot = record.vm_slot as usize;
    let (upstream_strips, current) = strips.split_at_mut(slot);
    let Some(workspace) = current[0].as_mut() else {
        unreachable!("every operator depth slot has a strip workspace")
    };
    let (temporal, own_temporal, running, own_running) = match sampling.temporal_slot[node] {
        NONE => (temporal, &mut [][..], running, &mut [][..]),
        index => {
            let (temporal, own) = temporal.split_at_mut(index as usize);
            let (running, own_running) = running.split_at_mut(own[0].start as usize);
            (temporal, own, running, own_running)
        }
    };
    let duration = show.duration();
    let context = RunContext {
        progress: if program.flag(image::PROGRAM_USES_PROGRESS) && duration.as_ticks() != 0 {
            (time.as_ticks() as f32 / duration.as_ticks() as f32).clamp(0.0, 1.0)
        } else {
            0.0
        },
        time: SampleDuration::from_ticks(time.as_ticks()),
        duration,
        frame_rate: show.frame_rate() as f32,
        pixel_count: 0,
    };
    let plan = TargetView::of(show, &show.s.targets[show.meta().plan_target as usize]);
    let bound = Bound {
        show,
        program,
        params,
        sections: Some(plan.sections),
    };
    let mut strip = Strip::new(bound, &context, workspace);
    let mut signals = GraphSignals {
        show,
        plan,
        inputs: record.inputs.get(show.s.node_inputs),
        end: duration.as_ticks(),
        first,
        count: 0,
        node,
        slot,
        frame_scope,
        temporal: Temporal {
            records: own_temporal,
            running: own_running,
        },
        lent: Lent {
            sampling,
            states: upstream,
            strips: upstream_strips,
            temporal,
            running,
        },
    };
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
    };
    let end = first + output.len();
    let mut start = first;
    while start < end {
        let shape = fill_strip(strip.pixels(), layout, start..end, pixel);
        signals.first = start;
        signals.count = shape.len;
        let mut colors = [black(); STRIP];
        strip.run(
            shape.pixel_count,
            shape.min,
            shape.max,
            &mut signals,
            &mut colors[..shape.len],
        );
        let destination = &mut output[start - first..start - first + shape.len];
        blend.apply(destination, &colors[..shape.len]);
        start += shape.len;
    }
}

/// Evaluate `node` over plan-target pixels `first..first + output.len()`.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
pub(super) fn sample_signal_run(
    show: Show<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    lent: Lent<'_, '_>,
) {
    let record = &show.s.nodes[node];
    match record.kind {
        image::NODE_LAYER => {
            let layer = &show.s.layers[record.layer as usize];
            sample_layer_run(show, layer, time, first, output, lent.sampling);
        }
        image::NODE_OPERATOR => {
            sample_operator(show, node, time, first, output, Blend::Write, false, lent);
        }
        _ => frames::sample_mix_run(show, record, time, first, output, lent),
    }
}

/// A layer over plan-target pixels `first..first + output.len()`. Effects on
/// other targets sample the matching fixture cells.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_layer_run(
    show: Show<'_>,
    layer: &image::Layer,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    workspace: &mut SamplingWorkspace<'_>,
) {
    output.fill(black());
    if layer.enabled == 0 {
        return;
    }
    let SamplingWorkspace {
        effect_strip,
        gather,
        effect_states,
        timing,
        layer_order,
        layer_ends,
        live_tracks,
        live_ticks,
        ..
    } = workspace;
    let live = LiveView {
        tracks: live_tracks,
        ticks: live_ticks,
    };
    let timeline = Timeline::new(show, timing, layer_order, layer_ends);
    for &effect_index in timeline.live(layer, time) {
        let effect_index = effect_index as usize;
        let timing = timeline.timing(effect_index);
        if !timing.is_active(time) {
            continue;
        }
        let effect = &show.s.effects[effect_index];
        let params = effect_params(show, effect_index, time, effect_states, live);
        let sampler = effect_sampler(show, effect, timing, time, params);
        let target = sampler.target;
        // The plan target's index is the storage address, so the run's
        // cells that this effect covers are where its segments overlap it.
        let end = first + output.len();
        let segments = target.segment_count();
        let (mut low, mut high) = (0, segments);
        while low < high {
            let middle = (low + high) / 2;
            let segment = target.segment(middle);
            if base(show, segment.fixture, segment.first_cell) + segment.len <= first {
                low = middle + 1;
            } else {
                high = middle;
            }
        }
        let mut count = 0;
        for index in low..segments {
            let segment = target.segment(index);
            let base = base(show, segment.fixture, segment.first_cell);
            if base >= end {
                break;
            }
            for address in base.max(first)..(base + segment.len).min(end) {
                gather[count] = [
                    (address - first) as u32,
                    (segment.start + address - base) as u32,
                ];
                count += 1;
                if count == STRIP {
                    sample_cells(&sampler, gather, output, effect_strip);
                    count = 0;
                }
            }
        }
        sample_cells(&sampler, &gather[..count], output, effect_strip);
    }
}

/// Sample gathered cells, each an output offset and a target index, ascending.
#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
fn sample_cells(
    sampler: &EffectSampler<'_>,
    cells: &[[u32; 2]],
    output: &mut [Color],
    strip: &mut StripWorkspace<'_>,
) {
    let target = sampler.target.cursor();
    sampler.sample_pixels(
        cells.len(),
        PixelSource::Cells(&target, cells, 1),
        strip,
        |k, color| compose_max(&mut output[cells[k][0] as usize], color),
    );
}

#[cfg_attr(feature = "iram", unsafe(link_section = ".rwtext"))]
pub(super) fn compose_max(target: &mut Color, source: Color) {
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
