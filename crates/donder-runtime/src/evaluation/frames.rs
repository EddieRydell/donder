//! Whole-frame evaluation: the output's frame, and each scan's frame. It runs
//! once per frame rather than per strip, so it stays in flash, outside
//! the instruction RAM that `pnpm firmware:build` checks the rest of
//! evaluation against.
use super::{
    Blend, EffectSampler, GraphSignals, Lent, black, compose_max, sample_layer_frame,
    sample_operator, sample_signal_run,
};
use crate::dsl::STRIP;
use crate::dsl::{Bound, RunContext, States};
use crate::dsl::{ScanQuery, SourceWeights};
use crate::resources::{LiveView, ParamsView};
use crate::scan::ScanColors;
use crate::show::Show;
use crate::targets::TargetView;
use crate::workspace::{Key, Timing, WindowKey, Workspace};
use donder_runtime_types::SampleDuration;
use donder_runtime_types::image::{self, NONE};
use donder_runtime_types::sampling::byte_channel;
use donder_runtime_types::{Color, SampleTime, sample_time_from_frame};

/// The layer's effects that may be active at `time`: those starting by
/// then, from the first whose latest end so far is after it.
#[inline(never)]
pub(crate) fn live_effects<'l>(
    layer: &image::Layer,
    time: SampleTime,
    order: &'l [u32],
    ends: &[u32],
    start: impl Fn(usize) -> u32,
) -> &'l [u32] {
    let order = layer.effects.get(order);
    let ends = layer.effects.get(ends);
    let ticks = time.as_ticks();
    let first = ends.partition_point(|&end| end <= ticks);
    let last = first + order[first..].partition_point(|&effect| start(effect as usize) <= ticks);
    &order[first..last]
}

/// Sort a layer's effects by start and record, for each, the latest end of
/// it and every effect before it. Effects before the first end after a time
/// have all ended by then.
pub(crate) fn index_layer(
    layer: &image::Layer,
    timing: &[Timing],
    order: &mut [u32],
    ends: &mut [u32],
) {
    let range = layer.effects.start as usize..(layer.effects.start + layer.effects.len) as usize;
    let order = &mut order[range.clone()];
    order.sort_unstable_by_key(|&effect| (timing[effect as usize].start, effect));
    let mut latest = 0;
    for (end, &effect) in ends[range].iter_mut().zip(order.iter()) {
        let timing = timing[effect as usize];
        latest = latest.max(timing.start.saturating_add(timing.duration));
        *end = latest;
    }
}

// Keep graph loops separate from patch/output loops; combining them worsens
// Xtensa code generation even though it removes one call per frame.
#[inline(never)]
pub(crate) fn sample_signal_graph(show: Show<'_>, time: SampleTime, workspace: &mut Workspace<'_>) {
    let Workspace {
        signal_buffer,
        operator_states,
        strips,
        temporal,
        running,
        sampling,
        ..
    } = workspace;
    let output = show.meta().output;
    if output == NONE || time.as_ticks() >= show.duration().as_ticks() {
        signal_buffer.fill(black());
        return;
    }
    for record in operator_states.records.iter_mut() {
        record.time = NONE;
    }
    sampling.frame_keys.fill(Key::NONE);
    for window in &mut sampling.windows {
        window.key = WindowKey::NONE;
    }
    sample_signal_frame(
        show,
        output as usize,
        time,
        signal_buffer,
        Blend::Write,
        Lent {
            sampling,
            states: operator_states.reborrow(),
            strips,
            temporal,
            running,
        },
    );
}

/// A whole plan-target frame of `node`.
#[inline(never)]
pub(super) fn sample_signal_frame(
    show: Show<'_>,
    node: usize,
    time: SampleTime,
    output: &mut [Color],
    blend: Blend,
    mut lent: Lent<'_, '_>,
) {
    let record = &show.s.nodes[node];
    match record.kind {
        image::NODE_LAYER => {
            let layer = &show.s.layers[record.layer as usize];
            sample_layer_frame(show, layer, time, output, blend, lent.sampling);
        }
        image::NODE_OPERATOR => {
            let output = &mut output[..show.pixel_count()];
            sample_operator(show, node, time, 0, output, blend, true, lent);
        }
        // A mix is the channel-wise maximum, so each input composes into
        // the output as it is drawn, without a frame of its own.
        _ => {
            if blend == Blend::Write {
                output.fill(black());
            }
            for &input in record.inputs.get(show.s.node_inputs) {
                sample_signal_frame(
                    show,
                    input as usize,
                    time,
                    output,
                    Blend::Max,
                    lent.reborrow(),
                );
            }
        }
    }
}

/// Every pixel of `query`'s scan, reading the input a strip at a time.
#[inline(never)]
pub(super) fn scan_frame(
    signals: &mut GraphSignals<'_, '_>,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
    output: &mut [Color],
) {
    let plan = signals.plan;
    let output = &mut output[..plan.len()];
    crate::scan::scan_frame(plan, query, weights, &mut Input(signals), output);
}

/// The operator's input, as a scan reads it.
struct Input<'s, 'g, 'w>(&'s mut GraphSignals<'g, 'w>);

impl ScanColors for Input<'_, '_, '_> {
    fn colors(&mut self, query: &ScanQuery, start: usize, colors: &mut [Color]) {
        let signals = &mut *self.0;
        let node = signals.inputs[query.input] as usize;
        if query.time.as_ticks() >= signals.end {
            colors.fill(black());
            return;
        }
        if let (true, Some(cache)) = (signals.frame_scope, query.frame_cache) {
            let frame = signals.cached_frame(cache, node, query.time);
            colors.copy_from_slice(&frame[start..start + colors.len()]);
            return;
        }
        sample_signal_run(
            signals.show,
            node,
            query.time,
            start,
            colors,
            signals.lent.reborrow(),
        );
    }
}

/// The longest a seek replays a time scan: four seconds of frames.
const PREROLL_SECONDS: u32 = 4;

/// Frames of history whose weight, after `decay` per frame, still reaches
/// half an 8-bit step; light older than that cannot change a written color.
fn preroll_frames(decay: f32, frame_rate: u32) -> u32 {
    let limit = PREROLL_SECONDS.saturating_mul(frame_rate);
    if decay <= 0.0 {
        return 0;
    }
    if decay >= 1.0 {
        return limit;
    }
    let frames = libm::ceilf(libm::logf(0.5 * (1.0 - decay) / 255.0) / libm::logf(decay));
    (frames as u32).min(limit)
}

/// The frame nearest `time`. Query times pass through float seconds, which
/// can land microseconds off a frame's start.
fn frame_at(time: SampleTime, frame_rate: u32) -> u32 {
    ((u64::from(time.as_ticks()) * u64::from(frame_rate) + 500_000) / 1_000_000) as u32
}

/// Every pixel of a time scan at `query`'s frame. The running values carry
/// from the frame before; after a seek they replay from black over the frames
/// that can still show, at most [`PREROLL_SECONDS`].
#[inline(never)]
pub(super) fn temporal_frame(
    signals: &mut GraphSignals<'_, '_>,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
    output: &mut [Color],
) {
    let frame_rate = signals.show.frame_rate();
    let decay = if query.decay.is_nan() {
        0.0
    } else {
        query.decay
    };
    let frame = frame_at(query.time, frame_rate);
    let pixels = signals.show.pixel_count();
    let temporal = core::mem::take(&mut signals.temporal.records);
    let running = core::mem::take(&mut signals.temporal.running);
    let base = temporal[0].start;
    let record = &mut temporal[query.cache];
    let offset = (record.start - base) as usize;
    let state = &mut running[offset..offset + pixels];
    let first = if record.frame != NONE && record.frame.checked_add(1) == Some(frame) {
        frame
    } else {
        state.fill([0.0; 3]);
        frame.saturating_sub(preroll_frames(decay, frame_rate))
    };
    let mut colors = [Color::BLACK; STRIP];
    let mut factors = [0.0f32; STRIP];
    for step in first..=frame {
        let Ok(time) = sample_time_from_frame(step, frame_rate) else {
            break;
        };
        let at = ScanQuery { time, ..*query };
        for start in (0..pixels).step_by(STRIP) {
            let size = (pixels - start).min(STRIP);
            Input(signals).colors(&at, start, &mut colors[..size]);
            weights.weights(&colors[..size], &mut factors[..size]);
            for (k, running) in state[start..start + size].iter_mut().enumerate() {
                let weight = if factors[k].is_nan() { 0.0 } else { factors[k] };
                let Color { red, green, blue } = colors[k];
                for (value, channel) in running.iter_mut().zip([red, green, blue]) {
                    *value = *value * decay + f32::from(channel) * weight;
                }
            }
        }
    }
    for (color, running) in output.iter_mut().zip(state.iter()) {
        *color = Color {
            red: byte_channel(running[0]),
            green: byte_channel(running[1]),
            blue: byte_channel(running[2]),
        };
    }
    record.frame = frame;
    signals.temporal.records = temporal;
    signals.temporal.running = running;
}

/// An effect's sampler at `time`. Once per effect sample, so it stays in flash.
#[inline(never)]
pub(crate) fn effect_sampler<'a>(
    show: Show<'a>,
    effect: &'a image::Effect,
    timing: Timing,
    time: SampleTime,
    params: ParamsView<'a>,
) -> EffectSampler<'a> {
    let program = show.program(effect.program);
    let target = TargetView::of(show, &show.s.targets[effect.target as usize]);
    let elapsed = time.as_ticks().saturating_sub(timing.start);
    EffectSampler {
        bound: Bound {
            show,
            program,
            params,
            sections: Some(target.sections),
        },
        context: RunContext {
            progress: if program.flag(image::PROGRAM_USES_PROGRESS) {
                (elapsed as f32 / timing.duration as f32).clamp(0.0, 1.0)
            } else {
                0.0
            },
            time: SampleDuration::from_ticks(elapsed),
            duration: SampleDuration::from_ticks(timing.duration),
            frame_rate: show.frame_rate() as f32,
            pixel_count: 0,
        },
        spatial: program.flag(image::PROGRAM_USES_SPATIAL),
        uses_sections: program.flag(image::PROGRAM_USES_SECTIONS),
        target,
    }
}

/// Effect `index`'s parameters at `time`: the image's, or its state's copy.
#[inline(never)]
pub(crate) fn effect_params<'s>(
    show: Show<'s>,
    index: usize,
    time: SampleTime,
    states: &'s mut States<'_>,
    live: LiveView<'s>,
) -> ParamsView<'s> {
    match show.s.effects[index].state {
        NONE => ParamsView::image(show, show.s.effects[index].params),
        state => states.get(state as usize).params_at(show, time, live),
    }
}

/// Compute frame cache `at`, keyed `key`: `node` at `time`. Once per frame.
#[inline(never)]
pub(super) fn fill_frame(
    signals: &mut GraphSignals<'_, '_>,
    at: usize,
    key: Key,
    node: usize,
    time: SampleTime,
) {
    let colors = core::mem::take(&mut signals.lent.sampling.frames[at]);
    sample_signal_frame(
        signals.show,
        node,
        time,
        colors,
        Blend::Write,
        signals.lent.reborrow(),
    );
    signals.lent.sampling.frame_keys[at] = key;
    signals.lent.sampling.frames[at] = colors;
}

/// Compute scan cache `at`, keyed `key`: every pixel of `query`'s scan.
#[inline(never)]
pub(super) fn fill_scan(
    signals: &mut GraphSignals<'_, '_>,
    at: usize,
    key: Key,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
) {
    let colors = core::mem::take(&mut signals.lent.sampling.frames[at]);
    if query.direction == crate::dsl::bytecode::Direction::Time {
        temporal_frame(signals, query, weights, colors);
    } else {
        scan_frame(signals, query, weights, colors);
    }
    signals.lent.sampling.frame_keys[at] = key;
    signals.lent.sampling.frames[at] = colors;
}

/// A mix over plan-target pixels `first..first + output.len()`: the
/// channel-wise maximum of its inputs. Only a strip-level read of a mix runs
/// it, so it stays in flash.
#[inline(never)]
pub(super) fn sample_mix_run(
    show: Show<'_>,
    record: &image::Node,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    mut lent: Lent<'_, '_>,
) {
    output.fill(black());
    let mut colors = [black(); STRIP];
    for chunk in (0..output.len()).step_by(STRIP) {
        let length = (output.len() - chunk).min(STRIP);
        for &input in record.inputs.get(show.s.node_inputs) {
            sample_signal_run(
                show,
                input as usize,
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
