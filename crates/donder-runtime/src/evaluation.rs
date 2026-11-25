//! Frame evaluation. Every node of the graph runs once per frame, inputs
//! first, into its whole frame buffer: a layer composites its active clips,
//! an operator runs over the plan target reading its inputs' frames, and a mix
//! keeps the channel-wise maximum of its inputs. A layer with no active clip is
//! black; while every input is black, a mix is black and an operator the
//! compiler proves produces one color then is filled with it instead of
//! running. Every program runs over strips of up to [`STRIP`] pixels.
//!
//! With the `iram` feature, every function here is linked into instruction RAM
//! (see `docs/performance.md`); `pnpm firmware:build` checks the placement.
#![cfg_attr(feature = "iram", allow(unsafe_code))]
use crate::dsl::bytecode::{Edges, SignalPixel};
use crate::dsl::{
    Bound, Extent, NeighborWeights, OUTSIDE, Pixels, RunContext, STRIP, ScanQuery, Strip,
    StripSignals, StripWorkspace,
};
use crate::format::{self, NONE};
use crate::resources::{LiveView, ParamsView};
use crate::targets::{PreparedPixel, TargetCursor, TargetView};
use crate::view::{ProgramView, Show};
use crate::workspace::{CachedClipColor, ConsoleState, Timing, Workspace};
use core::cell::Cell;
use donder_runtime_iram::iram;
use donder_runtime_types::{Color, SampleDuration, SampleTime};

/// One clip at one time, evaluated over any selection of pixel coordinates.
/// Both sequence playback and sparse editor rasters use this evaluator.
pub(crate) struct ClipEvaluator<'a> {
    bound: Bound<'a>,
    context: RunContext,
    target: TargetView<'a>,
}

/// The clips' timing and each layer's start order: the show's, or a console's
/// as its commands moved them.
#[derive(Clone, Copy)]
pub(crate) struct Timeline<'a> {
    clips: &'a [format::Clip],
    order: &'a [u32],
    ends: &'a [u32],
    timing: &'a [Timing],
}

#[iram]
impl<'a> Timeline<'a> {
    fn new(show: Show<'a>, timing: &'a [Timing], order: &'a [u32], ends: &'a [u32]) -> Self {
        let (order, ends) = if timing.is_empty() {
            (show.s.layer_order, show.s.layer_ends)
        } else {
            (order, ends)
        };
        Self {
            clips: show.s.clips,
            order,
            ends,
            timing,
        }
    }

    #[inline(always)]
    fn timing(&self, clip: usize) -> Timing {
        match self.timing.get(clip) {
            Some(timing) => *timing,
            None => {
                let clip = &self.clips[clip];
                Timing {
                    start: clip.start,
                    duration: clip.duration,
                }
            }
        }
    }

    /// The layer's clips that may be active at `time`: those starting by
    /// then, from the first whose latest end so far is after it.
    fn live(&self, layer: &format::Layer, time: SampleTime) -> &'a [u32] {
        let order = layer.clips.get(self.order);
        let ends = layer.clips.get(self.ends);
        let ticks = time.as_ticks();
        let first = ends.partition_point(|&end| end <= ticks);
        let last = first
            + order[first..].partition_point(|&clip| self.timing(clip as usize).start <= ticks);
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
pub(crate) enum PixelSet<'s, 'a> {
    /// Target indices from zero.
    Target(&'s TargetCursor<'a>),
    /// Gathered cells: each a target index and its plan-target address.
    Cells(&'s TargetCursor<'a>, &'s [[u32; 2]]),
    /// Pixels listed in advance.
    #[cfg(feature = "host")]
    Listed(&'s [RunPixel]),
}

#[iram]
impl PixelSet<'_, '_> {
    #[inline(never)]
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
}

/// The shared shape of a strip.
struct StripShape {
    len: usize,
    pixel_count: usize,
    extent: Extent,
}

/// What a program reads of a target's pixels.
#[derive(Clone, Copy)]
struct StripLayout<'a> {
    show: Show<'a>,
    target: TargetView<'a>,
    spatial: bool,
    /// Whether the program reads pixels' depth or structure.
    structure: bool,
    /// Strips must not mix pixel counts or bounds.
    split: bool,
}

#[iram]
impl<'a> StripLayout<'a> {
    /// What `program` reads of `target`'s pixels.
    #[inline(always)]
    fn new(show: Show<'a>, program: ProgramView<'a>, target: TargetView<'a>) -> Self {
        Self {
            show,
            target,
            spatial: program.flag(format::PROGRAM_USES_SPATIAL),
            structure: program.flag(format::PROGRAM_USES_STRUCTURE),
            split: program.flag(format::PROGRAM_READS_TARGET),
        }
    }
}

/// A program's run context `time` into a span of `duration` that starts at
/// `origin`.
#[inline(always)]
#[iram]
fn run_context(
    show: Show<'_>,
    time: SampleDuration,
    duration: SampleDuration,
    origin: SampleTime,
) -> RunContext {
    RunContext {
        time,
        duration,
        frame_rate: show.frame_rate() as f32,
        pixel_count: 0,
        origin,
    }
}

/// Fill a strip from `pixel(pixels)`. A strip ends after [`STRIP`] pixels or,
/// for a program that reads them, where the pixel count or target bounds
/// change, or the addresses stop being consecutive. One copy serves every
/// traversal.
#[inline(never)]
#[iram]
fn fill_strip(
    strip: &Pixels<'_>,
    layout: StripLayout<'_>,
    pixels: core::ops::Range<usize>,
    pixel: PixelSet<'_, '_>,
) -> StripShape {
    let StripLayout {
        show,
        target,
        spatial,
        structure,
        split,
    } = layout;
    let (start, end) = (pixels.start, pixels.end);
    let mut shape = StripShape {
        len: 0,
        pixel_count: 0,
        extent: Extent::default(),
    };
    if !spatial && !structure {
        // Without geometry only the pixel count can split a run.
        for index in start..end.min(start + STRIP) {
            let pixel_at = pixel.get(index).pixel;
            let offset = index - start;
            if offset == 0 {
                shape.pixel_count = pixel_at.pixel_count;
            } else if split && pixel_at.pixel_count != shape.pixel_count {
                break;
            }
            strip.index[offset].set(pixel_at.pixel_index as i32);
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
        let mut extent = Extent::default();
        if spatial {
            let position;
            (position, extent) = target.spatial_context(target_index, &pixel_at, show);
            strip.x[offset].set(position[0]);
            strip.y[offset].set(position[1]);
            if let Some(rows) = &strip.structure {
                rows.z[offset].set(position[2]);
            }
        }
        if structure && let Some(rows) = &strip.structure {
            let place = target.structure(target_index);
            rows.member[offset].set(place.member as i32);
            rows.strand[offset].set(place.strand as i32);
            rows.along[offset].set(place.along as i32);
        }
        if offset == 0 {
            (shape.pixel_count, shape.extent) = (pixel_at.pixel_count, extent);
        } else if split && (pixel_at.pixel_count != shape.pixel_count || extent != shape.extent) {
            break;
        }
        strip.index[offset].set(pixel_at.pixel_index as i32);
        shape.len += 1;
    }
    shape
}

#[iram]
impl<'a> ClipEvaluator<'a> {
    fn strip<'s, 'w>(&'s self, workspace: &'s mut StripWorkspace<'w>) -> Strip<'s, 'w> {
        Strip::new(self.bound, &self.context, workspace)
    }

    /// Whether every pixel of the clip has one color.
    fn uniform(&self) -> bool {
        !self.bound.program.flag(format::PROGRAM_USES_PIXEL_CONTEXT)
    }

    fn layout(&self) -> StripLayout<'a> {
        StripLayout::new(self.bound.show, self.bound.program, self.target)
    }

    /// Evaluate `count` pixels of this clip's target, `pixel(0..count)`, and
    /// hand each color with its position to `write`. A uniform clip runs once.
    pub(crate) fn evaluate_pixels(
        &self,
        count: usize,
        pixel: PixelSet<'_, '_>,
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
                shape.extent,
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
#[iram]
fn base(show: Show<'_>, fixture: usize, cell: u32) -> usize {
    show.s.fixtures[fixture].offset as usize + cell as usize
}

/// The frames a node reads. The buffer it writes splits the frame buffers
/// into those before it and those after it; each input's entry, set once per
/// node, names its half and where its frame starts there, so a read indexes
/// rather than branches.
#[derive(Clone, Copy)]
struct InputFrames<'f> {
    halves: [&'f [Color]; 2],
    inputs: &'f [[u32; 2]],
    pixels: usize,
}

#[iram]
impl<'f> InputFrames<'f> {
    /// Pixel `index` of input `input`'s frame.
    #[inline(always)]
    fn pixel(&self, input: usize, index: usize) -> Color {
        let [half, start] = self.inputs[input];
        self.halves[half as usize & 1][start as usize + index]
    }

    /// Input `input`'s frame.
    #[inline(always)]
    fn frame(&self, input: usize) -> &'f [Color] {
        let [half, start] = self.inputs[input];
        let start = start as usize;
        &self.halves[half as usize & 1][start..start + self.pixels]
    }
}

/// Signal reads of a strip, from frames this frame already evaluated. Pixel
/// `n` of the strip is plan-target pixel `first + n`.
pub(crate) struct FrameSignals<'g> {
    plan: TargetView<'g>,
    frames: InputFrames<'g>,
    /// One frame per scan cache, one after another.
    scans: &'g mut [Color],
    /// Which of the running program's scans are computed this frame.
    scanned: &'g [Cell<bool>],
    first: usize,
    count: usize,
}

#[iram]
impl FrameSignals<'_> {
    /// The frame a program reads as `input`: an input node's frame of this
    /// frame.
    #[inline(always)]
    fn frame(&self, input: usize) -> &[Color] {
        self.frames.frame(input)
    }

    /// Before each node and clip: its program's scans are not computed yet.
    #[inline(always)]
    fn forget_scans(&self) {
        for scanned in self.scanned {
            scanned.set(false);
        }
    }
}

#[iram]
impl StripSignals for FrameSignals<'_> {
    fn read_strip(&mut self, input: usize, output: &mut [Color; STRIP]) {
        let (first, count) = (self.first, self.count);
        output[..count].copy_from_slice(&self.frame(input)[first..first + count]);
    }

    fn read_pixel(&mut self, input: usize, offset: usize, pixel: SignalPixel<i32>) -> Color {
        match signal_pixel(self.plan, self.first + offset, pixel) {
            Some(index) => self.frames.pixel(input, index),
            None => black(),
        }
    }

    fn scan(
        &mut self,
        query: &ScanQuery,
        weights: &mut dyn NeighborWeights,
        output: &mut [Color; STRIP],
    ) {
        let pixels = self.frames.pixels;
        let start = query.cache * pixels;
        if !self.scanned[query.cache].get() {
            let input = self.frames.frame(query.input);
            let scan = &mut self.scans[start..start + pixels];
            crate::scan::scan_frame(self.plan, query, weights, input, scan);
            self.scanned[query.cache].set(true);
        }
        let first = start + self.first;
        output[..self.count].copy_from_slice(&self.scans[first..first + self.count]);
    }

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

#[iram]
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
#[iram]
fn shifted(flat: usize, pixel: &PreparedPixel, shift: i32, edges: Edges) -> Option<usize> {
    let local = pixel.pixel_index as i32;
    let read = edges.local(local.wrapping_add(shift), pixel.pixel_count as i32)?;
    Some(flat - pixel.pixel_index + read as usize)
}

/// What every program of a frame runs with.
struct Programs<'r, 'w> {
    strip: &'r mut StripWorkspace<'w>,
    live: LiveView<'r>,
    time: SampleTime,
}

#[iram]
impl<'w> Programs<'_, 'w> {
    /// Parameter set `set`, reading this frame's live store.
    fn params<'s>(
        &'s mut self,
        show: Show<'s>,
        set: u32,
    ) -> (ParamsView<'s>, &'s mut StripWorkspace<'w>) {
        (ParamsView::show(show, set, self.live), self.strip)
    }
}

/// Every active clip of `layer`, max-composited into `rendered`. Returns
/// whether any clip was active.
// Called once per layer per frame, but its loops over the layer's clips
// stay here.
#[inline(never)]
#[iram]
#[allow(clippy::too_many_arguments)]
fn evaluate_layer(
    show: Show<'_>,
    layer: &format::Layer,
    timeline: Timeline<'_>,
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_>,
    programs: &mut Programs<'_, '_>,
    gather: &mut [[u32; 2]],
    clip_colors: &mut [CachedClipColor],
) -> bool {
    rendered.fill(black());
    if layer.enabled == 0 {
        return false;
    }
    let time = programs.time;
    let mut active = false;
    for &clip_index in timeline.live(layer, time) {
        let clip_index = clip_index as usize;
        let timing = timeline.timing(clip_index);
        if !timing.is_active(time) {
            continue;
        }
        active = true;
        let clip = &show.s.clips[clip_index];
        let (params, workspace) = programs.params(show, clip.params);
        let sampler = clip_evaluator(show, clip, timing, time, params);
        let target = sampler.target;
        signals.forget_scans();
        if sampler.uniform() {
            let mut color = None;
            let cursor = target.cursor();
            sampler.evaluate_pixels(
                target.len().min(1),
                PixelSet::Target(&cursor),
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
        // unless the clip reads its layer's frame by address.
        let sample_count = target.sample_count();
        let layout = sampler.layout();
        let cacheable = sample_count != 0 && !layout.spatial;
        if cacheable {
            for sample in &mut clip_colors[..sample_count] {
                sample.pixel_count = 0;
            }
        }
        let mut strip = sampler.strip(workspace);
        // Strips gather pixels across fixtures: each pending pixel's target
        // index and plan-target address.
        let mut count = 0;
        for segment in target.segments() {
            let base = base(show, segment.fixture, segment.first_cell);
            for offset in 0..segment.len {
                if cacheable {
                    let sample = &clip_colors[segment.first_index + offset];
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
                        cacheable.then_some(&mut clip_colors[..]),
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
            cacheable.then_some(&mut clip_colors[..]),
        );
    }
    active
}

/// Run gathered pixels, each a target index and its address in `rendered`,
/// in as few strips as the program's layout allows. With `samples`, each
/// result is kept for duplicate fixture layouts.
#[iram]
fn run_gathered(
    layout: StripLayout<'_>,
    strip: &mut Strip<'_, '_>,
    cells: &[[u32; 2]],
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_>,
    mut samples: Option<&mut [CachedClipColor]>,
) {
    // Gathered indices ascend, so both lookups follow the target's runs.
    let (target, cached) = (layout.target.cursor(), layout.target.cursor());
    let pixel = PixelSet::Cells(&target, cells);
    let mut colors = [black(); STRIP];
    let mut start = 0;
    while start < cells.len() {
        let shape = fill_strip(strip.pixels(), layout, start..cells.len(), pixel);
        let count = shape.len;
        signals.first = cells[start][1] as usize;
        signals.count = count;
        strip.run(
            shape.pixel_count,
            shape.extent,
            signals,
            &mut colors[..count],
        );
        for (k, color) in colors[..count].iter().enumerate() {
            compose_max(&mut rendered[cells[start + k][1] as usize], *color);
            if let Some(samples) = samples.as_deref_mut() {
                let pixel = cached.pixel(cells[start + k][0] as usize);
                samples[pixel.pixel_index] = CachedClipColor {
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
#[iram]
fn evaluate_operator(
    show: Show<'_>,
    node: &format::Node,
    plan: TargetView<'_>,
    rendered: &mut [Color],
    signals: &mut FrameSignals<'_>,
    programs: &mut Programs<'_, '_>,
) {
    let program = show.program(node.program);
    let context = run_context(
        show,
        SampleDuration::from_ticks(programs.time.as_ticks()),
        show.duration(),
        SampleTime::from_ticks(0),
    );
    let (params, workspace) = programs.params(show, node.params);
    let bound = Bound {
        show,
        program,
        params,
    };
    let mut strip = Strip::new(bound, &context, workspace);
    let pixels = plan.cursor();
    let pixel = PixelSet::Target(&pixels);
    let layout = StripLayout::new(show, program, plan);
    let end = rendered.len();
    let mut start = 0;
    while start < end {
        let shape = fill_strip(strip.pixels(), layout, start..end, pixel);
        signals.first = start;
        signals.count = shape.len;
        strip.run(
            shape.pixel_count,
            shape.extent,
            signals,
            &mut rendered[start..start + shape.len],
        );
        start += shape.len;
    }
}

#[inline(always)]
#[iram]
fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

#[iram]
const fn black() -> Color {
    Color::BLACK
}

/// The buffer holding the root's frame.
pub(crate) fn root_buffer(show: Show<'_>) -> usize {
    show.root().map_or(0, |node| node.buffer as usize)
}

/// Once per frame or per clip, so it stays in flash, outside the instruction
/// RAM that `pnpm firmware:build` checks the rest of evaluation against.
mod frames {
    #[allow(clippy::wildcard_imports)]
    use super::*;

    /// Point `entries` at the frames of `inputs`, the buffers a node writing
    /// buffer `written` reads: each one's half and where its frame starts.
    fn locate_inputs(
        entries: &mut [[u32; 2]],
        nodes: &[format::Node],
        inputs: &[u32],
        written: u32,
        pixels: usize,
    ) {
        for (entry, &input) in entries.iter_mut().zip(inputs) {
            let buffer = nodes[input as usize].buffer;
            let after = u32::from(buffer > written);
            let index = (buffer - after * (written + 1)) as usize;
            *entry = [after, (index * pixels) as u32];
        }
    }

    /// Sort a layer's clips by start and record, for each, the latest end of
    /// it and every clip before it. Clips before the first end after a time
    /// have all ended by then.
    pub(crate) fn index_layer(
        layer: &format::Layer,
        timing: &[Timing],
        order: &mut [u32],
        ends: &mut [u32],
    ) {
        let range = layer.clips.start as usize..(layer.clips.start + layer.clips.len) as usize;
        let order = &mut order[range.clone()];
        order.sort_unstable_by_key(|&clip| (timing[clip as usize].start, clip));
        let mut latest = 0;
        for (end, &clip) in ends[range].iter_mut().zip(order.iter()) {
            let timing = timing[clip as usize];
            latest = latest.max(timing.start.saturating_add(timing.duration));
            *end = latest;
        }
    }

    /// Evaluate every node at `time` into its frame buffer. Returns the buffer
    /// holding the root's frame.
    #[inline(never)]
    pub(crate) fn evaluate_frame(
        show: Show<'_>,
        workspace: &mut Workspace<'_>,
        time: SampleTime,
    ) -> usize {
        let result = root_buffer(show);
        let pixels = workspace.pixels;
        if show.root().is_none() || time.as_ticks() >= show.duration().as_ticks() {
            workspace.buffers[result * pixels..(result + 1) * pixels].fill(black());
            return result;
        }
        let Workspace {
            buffers,
            black: black_frames,
            scans,
            scanned,
            input_frames,
            strip,
            gather,
            clip_colors,
            console,
            ..
        } = workspace;
        let ConsoleState {
            timing,
            layer_order,
            layer_ends,
            live,
            live_tracks,
            live_ticks,
        } = console;
        let timeline = Timeline::new(show, timing, layer_order, layer_ends);
        let plan = TargetView::of(show, &show.s.targets[show.meta().plan_target as usize]);
        let mut programs = Programs {
            strip,
            live: LiveView {
                tracks: live_tracks,
                ticks: live_ticks,
                values: live,
            },
            time,
        };
        for node in show.s.nodes {
            let buffer = node.buffer as usize;
            let (before, rest) = buffers.split_at_mut(buffer * pixels);
            let (rendered, after) = rest.split_at_mut(pixels);
            let inputs = show.inputs(node);
            let inputs_black = inputs
                .iter()
                .all(|&input| black_frames[show.s.nodes[input as usize].buffer as usize].get());
            locate_inputs(input_frames, show.s.nodes, inputs, node.buffer, pixels);
            let is_black = {
                let mut signals = FrameSignals {
                    plan,
                    frames: InputFrames {
                        halves: [before, after],
                        inputs: &input_frames[..inputs.len()],
                        pixels,
                    },
                    scans,
                    scanned,
                    first: 0,
                    count: 0,
                };
                signals.forget_scans();
                match node.kind {
                    format::NODE_LAYER => !evaluate_layer(
                        show,
                        &show.s.layers[node.layer as usize],
                        timeline,
                        rendered,
                        &mut signals,
                        &mut programs,
                        gather,
                        clip_colors,
                    ),
                    format::NODE_OPERATOR if inputs_black && node.black_inputs != NONE => {
                        let color = show.s.colors[node.black_inputs as usize];
                        rendered.fill(color);
                        color == black()
                    }
                    format::NODE_OPERATOR => {
                        evaluate_operator(show, node, plan, rendered, &mut signals, &mut programs);
                        false
                    }
                    _ => {
                        rendered.fill(black());
                        if !inputs_black {
                            for input in 0..inputs.len() {
                                for (target, &color) in
                                    rendered.iter_mut().zip(signals.frame(input))
                                {
                                    compose_max(target, color);
                                }
                            }
                        }
                        inputs_black
                    }
                }
            };
            black_frames[buffer].set(is_black);
        }
        result
    }

    /// A clip's evaluator at `time`. Once per clip evaluation.
    #[inline(never)]
    pub(crate) fn clip_evaluator<'a>(
        show: Show<'a>,
        clip: &'a format::Clip,
        timing: Timing,
        time: SampleTime,
        params: ParamsView<'a>,
    ) -> ClipEvaluator<'a> {
        let program = show.program(clip.program);
        let target = TargetView::of(show, &show.s.targets[clip.target as usize]);
        let elapsed = time.as_ticks() - timing.start;
        ClipEvaluator {
            bound: Bound {
                show,
                program,
                params,
            },
            context: run_context(
                show,
                SampleDuration::from_ticks(elapsed),
                SampleDuration::from_ticks(timing.duration),
                SampleTime::from_ticks(timing.start),
            ),
            target,
        }
    }
}
pub(crate) use frames::{clip_evaluator, evaluate_frame, index_layer};
