//! Whole-frame evaluation: the frame's nodes, and each scan's frame. It runs
//! once per frame and node rather than per strip, so it stays in flash, outside
//! the instruction RAM that `pnpm firmware:build` checks the rest of
//! evaluation against.
use super::{
    GraphSignals, Lent, black, compose_max, frame_range, sample_layer_frame, sample_operator,
    sample_signal_run,
};
use crate::dsl::{ScanQuery, SourceWeights};
use crate::scan::ScanColors;
use crate::signal::{EvaluationWorkspace, PreparedSignalKind, SignalGraph};
use donder_runtime_types::{Color, SampleTime};

// Keep graph loops separate from patch/output loops; combining them worsens
// Xtensa code generation even though it removes one call per frame.
#[inline(never)]
pub(crate) fn sample_signal_graph<'a>(
    renderer: SignalGraph<'_>,
    sample_time: SampleTime,
    workspace: &'a mut EvaluationWorkspace,
) -> &'a [Color] {
    let graph = &renderer.plan;
    let EvaluationWorkspace {
        signal_buffers: buffers,
        operator_automation,
        operator_vm: operator_vms,
        sampling: workspace,
    } = workspace;
    for state in operator_automation.iter_mut() {
        state.sample_time = None;
    }
    for frames in &mut workspace.operator_frames {
        for frame in frames {
            frame.key = None;
        }
    }
    for window in &mut workspace.operator_windows {
        window.key = None;
    }
    for node_index in graph.frame_nodes.iter().copied() {
        let destination = frame_range(renderer, node_index);
        match &graph.nodes[node_index].kind {
            PreparedSignalKind::Output { inputs } => {
                buffers[destination.clone()].fill(black());
                for input in inputs {
                    let source = frame_range(renderer, *input);
                    for (target, source) in destination.clone().zip(source) {
                        let color = buffers[source];
                        compose_max(&mut buffers[target], color);
                    }
                }
            }
            _ => sample_signal_frame(
                renderer,
                node_index,
                sample_time,
                &mut buffers[destination],
                Lent {
                    sampling: &mut *workspace,
                    automation: &mut *operator_automation,
                    strips: &mut *operator_vms,
                },
            ),
        }
    }
    &buffers[frame_range(renderer, graph.output_index)]
}

/// A whole plan-target frame of `node`.
#[inline(never)]
pub(super) fn sample_signal_frame(
    renderer: SignalGraph<'_>,
    node_index: usize,
    sample_time: SampleTime,
    output: &mut [Color],
    mut lent: Lent<'_>,
) {
    match &renderer.plan.nodes[node_index].kind {
        PreparedSignalKind::Layer { layer_index } => {
            sample_layer_frame(renderer, *layer_index, sample_time, output, lent.sampling);
        }
        PreparedSignalKind::Operator { .. } => {
            let pixels = renderer.target(renderer.plan.target).len();
            let output = &mut output[..pixels];
            sample_operator(renderer, node_index, sample_time, 0, output, true, lent);
        }
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            let Some((&first, rest)) = inputs.split_first() else {
                return;
            };
            sample_signal_frame(renderer, first, sample_time, output, lent.reborrow());
            if rest.is_empty() {
                return;
            }
            let slot = lent.sampling.frame_scratch_used;
            lent.sampling.frame_scratch_used += 1;
            let mut frame = core::mem::take(&mut lent.sampling.frame_scratch[slot]);
            for &input in rest {
                sample_signal_frame(renderer, input, sample_time, &mut frame, lent.reborrow());
                for (a, b) in output.iter_mut().zip(frame.iter()) {
                    compose_max(a, *b);
                }
            }
            lent.sampling.frame_scratch[slot] = frame;
            lent.sampling.frame_scratch_used -= 1;
        }
    }
}
/// Every pixel of `query`'s scan, reading the input a strip at a time.
#[inline(never)]
pub(super) fn scan_frame(
    signals: &mut GraphSignals<'_>,
    query: &ScanQuery,
    weights: &mut dyn SourceWeights,
    output: &mut [Color],
) {
    let renderer = signals.renderer;
    let pixels = renderer.target(renderer.plan.target);
    let output = &mut output[..pixels.len()];
    crate::scan::scan_frame(pixels, query, weights, &mut Input(signals), output);
}

/// The operator's input, as a scan reads it.
struct Input<'s, 'a>(&'s mut GraphSignals<'a>);

impl ScanColors for Input<'_, '_> {
    fn colors(&mut self, query: &ScanQuery, start: usize, colors: &mut [Color]) {
        let signals = &mut *self.0;
        let node = signals.inputs[query.input];
        if query.time.as_ticks() >= signals.renderer.duration.as_ticks() {
            colors.fill(black());
            return;
        }
        if let (true, Some(cache)) = (signals.frame_scope, query.frame_cache) {
            let frame = signals.cached_frame(signals.slot, cache, node, query.time);
            colors.copy_from_slice(&frame[start..start + colors.len()]);
            return;
        }
        sample_signal_run(
            signals.renderer,
            node,
            query.time,
            start,
            colors,
            signals.lent.reborrow(),
        );
    }
}
