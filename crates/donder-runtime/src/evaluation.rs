//! Signal graph evaluation. Every program runs in batches over runs of up to
//! [`BATCH_LANES`] pixels; a single pixel is a one-pixel run. Operators sample
//! their inputs over the same run, so upstream programs batch too.
use crate::dsl::AutomationPlan;
use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{
    BATCH_LANES, Batch, BatchMask, BatchSignals, BatchWorkspace, BoundParams, Lanes, RunContext,
};
use crate::signal::{
    CachedSignalFrame, EffectAutomationWorkspace, EvaluationWorkspace, PreparedEffect,
    PreparedOperatorNode, PreparedPixel, PreparedSignalKind, SignalGraph,
};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::boxed::Box;

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
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            },
            spatial: program.uses_spatial_context(),
            sections: &graph.targets[self.target].sections,
            uses_sections: program.uses_sections(),
        };
        run(&mut sampler)
    }

    pub(crate) fn automation_workspace(&self) -> Option<EffectAutomationWorkspace> {
        let automation = self.automation.as_ref()?;
        Some(EffectAutomationWorkspace {
            plan: automation.bindings.clone(),
            params: self.bound_params.clone(),
            sample_time: None,
        })
    }

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

/// The shared stage of a batch run.
struct RunShape {
    len: usize,
    pixel_count: usize,
    min: [f32; 2],
    max: [f32; 2],
}

/// Fill lanes from `pixel(start..end)`. A run ends after [`BATCH_LANES`]
/// pixels or, for a program that reads them, where the pixel count or target
/// bounds change.
#[allow(clippy::too_many_arguments)]
fn fill_lanes(
    lanes: &mut Lanes,
    renderer: SignalGraph<'_>,
    target: usize,
    spatial: bool,
    sections: Option<&crate::sections::PreparedSections>,
    split: bool,
    start: usize,
    end: usize,
    pixel: &impl Fn(usize) -> RunPixel,
) -> RunShape {
    let mut shape = RunShape {
        len: 0,
        pixel_count: 0,
        min: [0.0; 2],
        max: [0.0; 2],
    };
    if !spatial && sections.is_none() {
        // Without geometry only the pixel count can split a run.
        for index in start..end.min(start + BATCH_LANES) {
            let pixel = pixel(index).pixel;
            let lane = index - start;
            if lane == 0 {
                shape.pixel_count = pixel.pixel_count;
            } else if split && pixel.pixel_count != shape.pixel_count {
                break;
            }
            lanes.pixel_index[lane] = pixel.pixel_index as i32;
            lanes.pixel_fraction[lane] = pixel.pixel_fraction;
            shape.len += 1;
        }
        return shape;
    }
    for index in start..end.min(start + BATCH_LANES) {
        let RunPixel {
            target_index,
            pixel,
        } = pixel(index);
        let lane = index - start;
        let (mut min, mut max) = ([0.0; 2], [0.0; 2]);
        if spatial {
            let geometry = renderer.spatial_context(true, target, target_index, &pixel);
            lanes.x[lane] = geometry.position[0];
            lanes.y[lane] = geometry.position[1];
            (min, max) = (geometry.min, geometry.max);
        }
        if lane == 0 {
            (shape.pixel_count, shape.min, shape.max) = (pixel.pixel_count, min, max);
        } else if split
            && (pixel.pixel_count != shape.pixel_count || (min, max) != (shape.min, shape.max))
        {
            break;
        }
        lanes.pixel_index[lane] = pixel.pixel_index as i32;
        lanes.pixel_fraction[lane] = pixel.pixel_fraction;
        if let Some(sections) = sections {
            lanes.sections[lane] = sections.pixel(target_index);
        }
        shape.len += 1;
    }
    shape
}

impl EffectSampler<'_> {
    fn batch<'s>(&'s self, workspace: &'s mut BatchWorkspace) -> Batch<'s> {
        Batch::new(
            self.program.bytecode(),
            self.program.target_entry(),
            self.program.batch(),
            self.params,
            &self.context,
            Some(self.sections),
            workspace,
        )
    }

    fn uniform(&self) -> bool {
        !self.program.bytecode().uses_pixel_context
    }

    /// Evaluate `count` pixels of this effect's target, `pixel(0..count)`, and
    /// hand each color with its position to `write`. A uniform effect runs once.
    pub(crate) fn sample_pixels(
        &self,
        renderer: SignalGraph<'_>,
        target: usize,
        count: usize,
        pixel: impl Fn(usize) -> RunPixel,
        workspace: &mut BatchWorkspace,
        mut write: impl FnMut(usize, Color),
    ) {
        if count == 0 {
            return;
        }
        let uniform = self.uniform();
        let end = if uniform { 1 } else { count };
        let sections = self.uses_sections.then_some(self.sections);
        let mut batch = self.batch(workspace);
        let mut colors = [black(); BATCH_LANES];
        let mut start = 0;
        while start < end {
            let shape = fill_lanes(
                batch.lanes(),
                renderer,
                target,
                self.spatial,
                sections,
                self.program.batch().reads_target(),
                start,
                end,
                &pixel,
            );
            batch.run(
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

// Keep graph loops separate from patch/output loops; combining them worsens
// Xtensa code generation even though it removes one call per frame.
#[inline(never)]
pub(crate) fn sample_signal_graph<'a>(
    renderer: SignalGraph<'_>,
    sample_time: SampleTime,
    workspace: &'a mut EvaluationWorkspace,
) -> &'a [Color] {
    let graph = &renderer.plan;
    for state in &mut workspace.operator_automation {
        state.sample_time = None;
    }
    for frames in &mut workspace.operator_frames {
        for frame in frames {
            frame.key = None;
        }
    }
    // Frame buffers, upstream automation and depth-slot workspaces are lent
    // out separately during recursive sampling.
    let mut buffers = core::mem::take(&mut workspace.signal_buffers);
    let mut operator_automation = core::mem::take(&mut workspace.operator_automation);
    let mut operator_vms = core::mem::take(&mut workspace.operator_vm);
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
                workspace,
                &mut operator_automation,
                &mut operator_vms,
            ),
        }
    }
    let range = frame_range(renderer, graph.output_index);
    workspace.signal_buffers = buffers;
    workspace.operator_automation = operator_automation;
    workspace.operator_vm = operator_vms;
    &workspace.signal_buffers[range]
}

/// Graph admission orders dependencies before consumers and assigns automation
/// slots in that same order. While this operator borrows its parameters, nested
/// sampling may therefore borrow only the preceding automation states.
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

pub(crate) fn frame_range(renderer: SignalGraph<'_>, node_index: usize) -> core::ops::Range<usize> {
    let slot = renderer.plan.frame_slots[node_index];
    let start = slot * renderer.pixel_count;
    start..start + renderer.pixel_count
}

fn sample_layer_frame(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    sample_time: SampleTime,
    rendered: &mut [Color],
    workspace: &mut EvaluationWorkspace,
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
                    &mut workspace.effect_batch,
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
            // One batch covers every run of this effect at this time.
            // Duplicate fixture layouts share samples by pixel index and count.
            let cacheable = sample_count != 0 && !sampler.spatial && !sampler.uses_sections;
            let (spatial, sections) = (
                sampler.spatial,
                sampler.uses_sections.then_some(sampler.sections),
            );
            let mut batch = sampler.batch(&mut workspace.effect_batch);
            let mut colors = [black(); BATCH_LANES];
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
                    let cached_len = (length - offset).min(BATCH_LANES);
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
                    let shape = fill_lanes(
                        batch.lanes(),
                        renderer,
                        effect.target,
                        spatial,
                        sections,
                        true,
                        offset,
                        length,
                        &pixel,
                    );
                    let count = shape.len;
                    batch.run(
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

/// Signal queries of an operator run. Lane `n` is plan-target pixel
/// `first + n`; whole-frame caches are used only at frame scope.
struct GraphSignals<'a> {
    renderer: SignalGraph<'a>,
    inputs: &'a [usize],
    first: usize,
    count: usize,
    frames: Option<usize>,
    workspace: &'a mut EvaluationWorkspace,
    operator_automation: &'a mut [EffectAutomationWorkspace],
    operator_vms: &'a mut [BatchWorkspace],
}

impl GraphSignals<'_> {
    /// A frame-cached input frame at `time`, computed on first use.
    fn cached_frame(
        &mut self,
        slot: usize,
        cache: usize,
        node: usize,
        time: SampleTime,
    ) -> &[Color] {
        let key = Some((node, time));
        if self.workspace.operator_frames[slot][cache].key != key {
            let mut stored = core::mem::replace(
                &mut self.workspace.operator_frames[slot][cache],
                CachedSignalFrame {
                    key: None,
                    colors: Box::new([]),
                },
            );
            sample_signal_frame(
                self.renderer,
                node,
                time,
                &mut stored.colors,
                self.workspace,
                self.operator_automation,
                self.operator_vms,
            );
            stored.key = key;
            self.workspace.operator_frames[slot][cache] = stored;
        }
        &self.workspace.operator_frames[slot][cache].colors
    }
}

impl BatchSignals for GraphSignals<'_> {
    fn sample_run(
        &mut self,
        input: usize,
        time: SampleTime,
        frame_cache: Option<usize>,
        _: BatchMask,
        output: &mut [Color; BATCH_LANES],
    ) {
        let count = self.count;
        if time.as_ticks() >= self.renderer.duration.as_ticks() {
            output[..count].fill(black());
            return;
        }
        let node = self.inputs[input];
        if let (Some(slot), Some(cache)) = (self.frames, frame_cache) {
            let first = self.first;
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
            self.workspace,
            self.operator_automation,
            self.operator_vms,
        );
    }

    fn sample_pixel(
        &mut self,
        input: usize,
        time: SampleTime,
        lane: usize,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Color {
        let Some(index) = signal_pixel(self.renderer, self.first + lane, pixel) else {
            return black();
        };
        if time.as_ticks() >= self.renderer.duration.as_ticks() {
            return black();
        }
        let node = self.inputs[input];
        if let (Some(slot), Some(cache)) = (self.frames, frame_cache) {
            return self.cached_frame(slot, cache, node, time)[index];
        }
        let mut color = [black()];
        sample_signal_run(
            self.renderer,
            node,
            time,
            index,
            &mut color,
            self.workspace,
            self.operator_automation,
            self.operator_vms,
        );
        color[0]
    }
}

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
    }
}

/// Evaluate operator `node` over plan-target pixels `first..first + output.len()`.
/// Frame scope (`frames`) lets the operator use its whole-frame input caches.
#[allow(clippy::too_many_arguments)]
fn sample_operator(
    renderer: SignalGraph<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    frame_scope: bool,
    workspace: &mut EvaluationWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
    operator_vms: &mut [BatchWorkspace],
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
    let (params, upstream) = operator_params(operator, automation, time, operator_automation);
    let (upstream_vms, current) = operator_vms.split_at_mut(*vm_slot);
    let duration = renderer.duration;
    let context = RunContext {
        progress: if program.uses_progress() && duration.as_ticks() != 0 {
            (time.as_ticks() as f32 / duration.as_ticks() as f32).clamp(0.0, 1.0)
        } else {
            0.0
        },
        time: SampleDuration::from_ticks(time.as_ticks()),
        duration,
        pixel_index: 0,
        pixel_count: 0,
        pixel_fraction: 0.0,
    };
    let target = renderer.plan.target;
    let sections = &renderer.targets[target].sections;
    let mut batch = Batch::new(
        program.bytecode(),
        program.target_entry(),
        program.batch(),
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
        frames: frame_scope.then_some(*vm_slot),
        workspace,
        operator_automation: upstream,
        operator_vms: upstream_vms,
    };
    let pixels = renderer.target(target);
    let pixel = |index: usize| RunPixel {
        target_index: index,
        pixel: pixels.pixel(index),
    };
    let (spatial, sections) = (
        program.uses_spatial_context(),
        program.uses_sections().then_some(sections),
    );
    let end = first + output.len();
    let mut start = first;
    while start < end {
        let shape = fill_lanes(
            batch.lanes(),
            renderer,
            target,
            spatial,
            sections,
            program.batch().reads_target(),
            start,
            end,
            &pixel,
        );
        signals.first = start;
        signals.count = shape.len;
        let colors = &mut output[start - first..start - first + shape.len];
        batch.run(
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
#[allow(clippy::too_many_arguments)]
fn sample_signal_run(
    renderer: SignalGraph<'_>,
    node: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
    operator_vms: &mut [BatchWorkspace],
) {
    match &renderer.plan.nodes[node].kind {
        PreparedSignalKind::Layer { layer_index } => {
            sample_layer_run(renderer, *layer_index, time, first, output, workspace);
        }
        PreparedSignalKind::Operator { .. } => sample_operator(
            renderer,
            node,
            time,
            first,
            output,
            false,
            workspace,
            operator_automation,
            operator_vms,
        ),
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            let mut colors = [black(); BATCH_LANES];
            for chunk in (0..output.len()).step_by(BATCH_LANES) {
                let length = (output.len() - chunk).min(BATCH_LANES);
                for &input in inputs {
                    sample_signal_run(
                        renderer,
                        input,
                        time,
                        first + chunk,
                        &mut colors[..length],
                        workspace,
                        operator_automation,
                        operator_vms,
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
fn sample_layer_run(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    time: SampleTime,
    first: usize,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
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
                    &mut workspace.effect_batch,
                    |offset, color| compose_max(&mut output[offset], color),
                );
                return;
            }
            // Gather the run's cells that this effect covers into one batch.
            // Temporal queries revisit the same run, so the mapping is kept.
            for chunk in (0..output.len()).step_by(BATCH_LANES) {
                let length = (output.len() - chunk).min(BATCH_LANES);
                let key = Some((effect.target, first + chunk, length));
                let map = &mut workspace.gather;
                if map.key != key {
                    map.count = 0;
                    for offset in chunk..chunk + length {
                        let pixel = plan_target.pixel(first + offset);
                        if let Some(index) =
                            target.find(pixel.fixture_index, pixel.fixture_pixel_index)
                        {
                            map.cells[map.count] = (offset, index);
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
                    |lane| RunPixel {
                        target_index: cells[lane].1,
                        pixel: target.pixel(cells[lane].1),
                    },
                    &mut workspace.effect_batch,
                    |lane, color| compose_max(&mut output[cells[lane].0], color),
                );
            }
        });
    }
}

/// A whole plan-target frame of `node`.
#[allow(clippy::too_many_arguments)]
fn sample_signal_frame(
    renderer: SignalGraph<'_>,
    node_index: usize,
    sample_time: SampleTime,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
    operator_vms: &mut [BatchWorkspace],
) {
    match &renderer.plan.nodes[node_index].kind {
        PreparedSignalKind::Layer { layer_index } => {
            sample_layer_frame(renderer, *layer_index, sample_time, output, workspace);
        }
        PreparedSignalKind::Operator { .. } => {
            let pixels = renderer.target(renderer.plan.target).len();
            sample_operator(
                renderer,
                node_index,
                sample_time,
                0,
                &mut output[..pixels],
                true,
                workspace,
                operator_automation,
                operator_vms,
            );
        }
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            let Some((&first, rest)) = inputs.split_first() else {
                return;
            };
            sample_signal_frame(
                renderer,
                first,
                sample_time,
                output,
                workspace,
                operator_automation,
                operator_vms,
            );
            if rest.is_empty() {
                return;
            }
            let slot = workspace.frame_scratch_used;
            workspace.frame_scratch_used += 1;
            let mut frame = core::mem::take(&mut workspace.frame_scratch[slot]);
            for &input in rest {
                sample_signal_frame(
                    renderer,
                    input,
                    sample_time,
                    &mut frame,
                    workspace,
                    operator_automation,
                    operator_vms,
                );
                for (a, b) in output.iter_mut().zip(frame.iter()) {
                    compose_max(a, *b);
                }
            }
            workspace.frame_scratch[slot] = frame;
            workspace.frame_scratch_used -= 1;
        }
    }
}

fn compose_max(target: &mut Color, source: Color) {
    target.red = target.red.max(source.red);
    target.green = target.green.max(source.green);
    target.blue = target.blue.max(source.blue);
}

const fn black() -> Color {
    Color {
        red: 0,
        green: 0,
        blue: 0,
    }
}
