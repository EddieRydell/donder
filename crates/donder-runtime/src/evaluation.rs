use crate::dsl::AutomationPlan;
use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{BoundParams, OperatorRunContext, RunContext, SignalSampler, VmWorkspace};
use crate::signal::{
    CachedSignal, CachedSignalFrame, CachedVmSample, EffectAutomationWorkspace,
    EvaluationWorkspace, PreparedEffect, PreparedOperatorNode, PreparedSignalKind, SignalGraph,
};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::boxed::Box;
use core::convert::Infallible;

/// A frame traversal may amortize a uniform query over all pixels. A scalar
/// query must stay scalar throughout its upstream traversal: promoting it back
/// to a frame can rebuild every pixel for each temporal tap of every pixel.
#[derive(Clone, Copy, Eq, PartialEq)]
enum SamplingScope {
    Frame,
    Pixel,
}

/// One effect at one time, sampled over any selection of pixel coordinates.
/// Both sequence playback and sparse editor rasters use this evaluator.
pub(crate) struct EffectSampler<'a> {
    program: &'a crate::dsl::SampleProgram,
    params: &'a BoundParams,
    context: RunContext,
    reuse_uniform: bool,
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
                progress: self.progress(sample_time),
                time: self.local_time(sample_time),
                duration: self.duration,
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            },
            reuse_uniform: false,
            spatial: program.uses_spatial_context(),
            sections: &graph.targets[self.target].sections,
            uses_sections: program.uses_sections(),
        };
        run(&mut sampler)
    }

    pub(crate) fn automation_workspace(&self) -> Option<EffectAutomationWorkspace> {
        let automation = self.automation.as_ref()?;
        let bound_params = &self.bound_params;
        let params = bound_params.clone();
        Some(EffectAutomationWorkspace {
            plan: automation.bindings.clone(),
            params,
            sample_time: None,
        })
    }
    pub(crate) fn resolve_params<'a>(
        &'a self,
        sample_time: SampleTime,
        automation: impl FnOnce(usize) -> &'a mut EffectAutomationWorkspace,
    ) -> &'a BoundParams {
        let bound_params = &self.bound_params;
        match &self.automation {
            None => bound_params,
            Some(binding) => automation(binding.workspace_slot).params_at(sample_time),
        }
    }
}

impl EffectSampler<'_> {
    pub(crate) fn sample_spatial(
        &mut self,
        pixel: &crate::signal::PreparedPixel,
        target_index: usize,
        spatial: &crate::dsl::SpatialContext,
        workspace: &mut VmWorkspace,
    ) -> Color {
        self.sample_spatial_context(
            pixel.pixel_index(),
            pixel.pixel_count(),
            pixel.pixel_fraction,
            self.sections.pixel(target_index),
            spatial,
            workspace,
        )
    }

    pub(crate) fn sample_spatial_context(
        &mut self,
        pixel_index: usize,
        pixel_count: usize,
        pixel_fraction: f32,
        section_pixel: crate::sections::SectionPixel,
        spatial: &crate::dsl::SpatialContext,
        workspace: &mut VmWorkspace,
    ) -> Color {
        self.context.pixel_index = pixel_index as i32;
        self.context.pixel_count = pixel_count as i32;
        self.context.pixel_fraction = pixel_fraction;
        let result = self.program.sample(
            self.params,
            &self.context,
            spatial,
            crate::sections::SectionContext::Prepared {
                target: self.sections,
                pixel: section_pixel,
            },
            workspace,
            self.reuse_uniform,
        );
        self.reuse_uniform = true;
        result
    }

    fn uniform(&self) -> bool {
        !self.program.bytecode().uses_pixel_context
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
    workspace.effect_vm_sample = None;
    // Prefix registers belong to one node/time in each shared depth slot.
    // Never carry this identity across frame evaluations or parameter changes.
    for (_, sample) in &mut workspace.operator_vm {
        *sample = None;
    }
    for state in &mut workspace.operator_automation {
        state.sample_time = None;
    }
    for frames in &mut workspace.operator_frames {
        for frame in frames {
            frame.key = None;
        }
    }
    // Recursive signal sampling uses the VM and caches, not these frame buffers.
    // Keep the fixed storage separate during recursive evaluation.
    let mut buffers = core::mem::take(&mut workspace.signal_buffers);
    let mut operator_automation = core::mem::take(&mut workspace.operator_automation);
    for node_index in graph.frame_nodes.iter().copied() {
        let node = &graph.nodes[node_index];
        let destination = frame_range(renderer, node_index);
        match &node.kind {
            PreparedSignalKind::Layer { layer_index } => {
                sample_layer_frame(
                    renderer,
                    *layer_index,
                    sample_time,
                    &mut buffers[destination],
                    workspace,
                );
            }
            PreparedSignalKind::Operator {
                operator,
                inputs,
                automation,
                vm_slot,
            } => {
                let (params, upstream) =
                    operator_params(operator, automation, sample_time, &mut operator_automation);
                let vm_slot = *vm_slot;
                let mut vm_workspace = core::mem::take(&mut workspace.operator_vm[vm_slot]);
                sample_operator_frame(
                    renderer,
                    operator,
                    inputs,
                    params,
                    sample_time,
                    destination,
                    &mut buffers,
                    workspace,
                    vm_slot,
                    &mut vm_workspace.0,
                    upstream,
                );
                workspace.operator_vm[vm_slot] = vm_workspace;
            }
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
        }
    }
    let range = frame_range(renderer, graph.output_index);
    workspace.signal_buffers = buffers;
    workspace.operator_automation = operator_automation;
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
    let graph = &renderer.plan;
    let slot = graph.frame_slots[node_index];
    let start = slot * renderer.pixel_count;
    start..start + renderer.pixel_count
}

// Keep the per-pixel loop out of the large graph dispatcher; inlining it
// produces slower code for simple layered programs in controlled benchmarks.
#[inline(never)]
fn sample_layer_frame(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    sample_time: SampleTime,
    rendered: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) {
    // This traversal owns the effect VM independently of recursive sampling.
    workspace.effect_vm_sample = None;
    let layer = &renderer.layers[layer_index];
    rendered.fill(black());
    if !layer.enabled {
        return;
    }
    let layer_effects = &renderer.effects_by_layer[layer_index];
    for effect_index in layer_effects {
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
            let uniform = sampler.uniform();
            let target = renderer.target(effect.target);
            if uniform {
                if let Some(pixel) = target.first() {
                    let color = sampler.sample_spatial(
                        pixel,
                        0,
                        renderer.spatial_context(
                            sampler.spatial,
                            renderer.targets[effect.target].pixels.start,
                        ),
                        &mut workspace.effect_vm,
                    );
                    for pixel in target {
                        let flat_index = renderer.fixture_pixel_offsets[pixel.fixture_index()]
                            + pixel.fixture_pixel_index();
                        compose_max(&mut rendered[flat_index], color);
                    }
                }
                return;
            }
            // This traversal keeps one program, parameter set and sample time.
            // Scalar registers survive VM cleanup; restart initialization for
            // every effect/time, including backward seeks and automation edits.
            for (target_index, pixel) in target.iter().enumerate() {
                // Pixel indices are already dense. The count distinguishes
                // fixtures of different sizes sharing the same index.
                let cached = (sample_count != 0 && !sampler.spatial && !sampler.uses_sections)
                    .then(|| &mut workspace.effect_samples[pixel.pixel_index()]);
                let color = match cached {
                    Some(sample) if sample.pixel_count == pixel.pixel_count => sample.color,
                    cached => {
                        let color = sampler.sample_spatial(
                            pixel,
                            target_index,
                            renderer.spatial_context(
                                sampler.spatial,
                                renderer.targets[effect.target].pixels.start + target_index,
                            ),
                            &mut workspace.effect_vm,
                        );
                        if let Some(sample) = cached {
                            sample.pixel_count = pixel.pixel_count;
                            sample.color = color;
                        }
                        color
                    }
                };
                let flat_index = renderer.fixture_pixel_offsets[pixel.fixture_index()]
                    + pixel.fixture_pixel_index();
                compose_max(&mut rendered[flat_index], color);
            }
        });
    }
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_frame(
    renderer: SignalGraph<'_>,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    params: &BoundParams,
    sample_time: SampleTime,
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    workspace: &mut EvaluationWorkspace,
    frame_slot: usize,
    vm_workspace: &mut VmWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
) {
    let program = &operator.program;
    let compiled = renderer.operator_program(*program);
    let duration = renderer.duration;
    let progress = if duration.as_ticks() == 0 {
        0.0
    } else {
        (sample_time.as_ticks() as f32 / duration.as_ticks() as f32).clamp(0.0, 1.0)
    };
    let output = &mut buffers[destination];
    let mut cache = core::mem::take(&mut workspace.signal_cache);
    // This detached VM belongs to one operator at one time. Upstream
    // sampling uses separate workspaces, so its uniform slots remain valid.
    let mut reuse_uniform = false;
    for (flat_pixel_index, pixel) in renderer.target(renderer.plan.target).iter().enumerate() {
        cache.fill(None);
        let context = OperatorRunContext {
            progress,
            time: SampleDuration::from_ticks(sample_time.as_ticks()),
            duration,
            pixel_index: pixel.pixel_index() as i32,
            pixel_count: pixel.pixel_count() as i32,
            pixel_fraction: pixel.pixel_fraction,
        };
        let mut sampler = GraphSignalSampler {
            renderer,
            inputs,
            cache: &mut cache,
            scope: SamplingScope::Frame,
            flat_pixel_index,
            frame_slot,
            duration: renderer.duration,
            workspace,
            operator_automation,
        };
        match compiled.sample(
            params,
            &context,
            renderer.spatial_context(
                compiled.uses_spatial_context(),
                renderer.targets[renderer.plan.target].pixels.start + flat_pixel_index,
            ),
            crate::sections::SectionContext::Prepared {
                target: &renderer.targets[renderer.plan.target].sections,
                pixel: renderer.targets[renderer.plan.target]
                    .sections
                    .pixel(flat_pixel_index),
            },
            &mut sampler,
            vm_workspace,
            reuse_uniform,
        ) {
            Ok(color) => {
                output[flat_pixel_index] = color;
                reuse_uniform = true;
            }
            Err(never) => match never {},
        }
    }
    workspace.signal_cache = cache;
}

#[allow(clippy::too_many_arguments)]
fn sample_signal_pixel(
    renderer: SignalGraph<'_>,
    node_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    scope: SamplingScope,
    operator_automation: &mut [EffectAutomationWorkspace],
) -> Color {
    if let Some(Some(cached)) = cache.get(node_index)
        && cached.sample_time == sample_time
        && cached.flat_pixel_index == flat_pixel_index
    {
        return cached.color;
    }
    let node = &renderer.plan.nodes[node_index];
    let color = match &node.kind {
        PreparedSignalKind::Layer { layer_index } => sample_layer_pixel(
            renderer,
            *layer_index,
            sample_time,
            flat_pixel_index,
            workspace,
        ),
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let (params, upstream) =
                operator_params(operator, automation, sample_time, operator_automation);
            let vm_slot = *vm_slot;
            let mut vm_workspace = core::mem::take(&mut workspace.operator_vm[vm_slot]);
            let cached = vm_workspace
                .1
                .take()
                .filter(|sample| sample.index == node_index && sample.time == sample_time);
            let reuse_uniform = cached.is_some();
            let progress = cached.map_or_else(
                || {
                    if renderer.duration.as_ticks() != 0 {
                        (sample_time.as_ticks() as f32 / renderer.duration.as_ticks() as f32)
                            .clamp(0.0, 1.0)
                    } else {
                        0.0
                    }
                },
                |sample| sample.progress,
            );
            let sampled = sample_operator_pixel(
                renderer,
                operator,
                inputs,
                params,
                sample_time,
                flat_pixel_index,
                cache,
                workspace,
                vm_slot,
                &mut vm_workspace.0,
                reuse_uniform,
                progress,
                scope,
                upstream,
            );
            vm_workspace.1 = Some(CachedVmSample {
                index: node_index,
                time: sample_time,
                progress,
            });
            workspace.operator_vm[vm_slot] = vm_workspace;
            sampled
        }
        PreparedSignalKind::Output { inputs } => {
            let mut output = black();
            for input in inputs {
                compose_max(
                    &mut output,
                    sample_signal_pixel(
                        renderer,
                        *input,
                        sample_time,
                        flat_pixel_index,
                        cache,
                        workspace,
                        scope,
                        operator_automation,
                    ),
                );
            }
            output
        }
    };
    // One latest sample per node: different times replace rather than grow
    // storage. Stateless signals may be recomputed without changing results.
    cache[node_index] = Some(CachedSignal {
        sample_time,
        flat_pixel_index,
        color,
    });
    color
}

fn sample_layer_pixel(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    workspace: &mut EvaluationWorkspace,
) -> Color {
    let layer = &renderer.layers[layer_index];
    if !layer.enabled {
        return black();
    }
    let pixel = &renderer.target(renderer.plan.target)[flat_pixel_index];
    let mut rendered = black();
    let layer_effects = &renderer.effects_by_layer[layer_index];
    for effect_index in layer_effects {
        let effect = &renderer.effects[*effect_index];
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let effect_pixel = if effect.target == renderer.plan.target {
            // Elaboration interns exact pixel contexts, not just addresses.
            Some((flat_pixel_index, pixel))
        } else {
            let target = renderer.target(effect.target);
            target
                .binary_search_by_key(
                    &(pixel.fixture_index(), pixel.fixture_pixel_index()),
                    |effect_pixel| {
                        (
                            effect_pixel.fixture_index(),
                            effect_pixel.fixture_pixel_index(),
                        )
                    },
                )
                .ok()
                .map(|index| (index, &target[index]))
        };
        if let Some((target_index, effect_pixel)) = effect_pixel {
            let cached = workspace
                .effect_vm_sample
                .filter(|(sample, ..)| sample.index == *effect_index && sample.time == sample_time);
            if let Some((_, _, color)) = cached
                && !renderer
                    .sample_program(effect.program)
                    .bytecode()
                    .uses_pixel_context
            {
                compose_max(&mut rendered, color);
                continue;
            }
            // A different effect must not expose old slots.
            workspace.effect_vm_sample = None;
            let reuse_uniform = cached.is_some();
            let (progress, local_time) = cached.map_or_else(
                || (effect.progress(sample_time), effect.local_time(sample_time)),
                |(sample, local_time, _)| (sample.progress, local_time),
            );
            let params =
                effect.resolve_params(sample_time, |slot| &mut workspace.effect_automation[slot]);
            let color = effect.with_sampler(renderer, sample_time, params, |sampler| {
                sampler.context.progress = progress;
                sampler.context.time = local_time;
                sampler.reuse_uniform = reuse_uniform;
                sampler.sample_spatial(
                    effect_pixel,
                    target_index,
                    renderer.spatial_context(
                        sampler.spatial,
                        renderer.targets[effect.target].pixels.start + target_index,
                    ),
                    &mut workspace.effect_vm,
                )
            });
            workspace.effect_vm_sample = Some((
                CachedVmSample {
                    index: *effect_index,
                    time: sample_time,
                    progress,
                },
                local_time,
                color,
            ));
            compose_max(&mut rendered, color);
        }
    }
    rendered
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_pixel(
    renderer: SignalGraph<'_>,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    params: &BoundParams,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    frame_slot: usize,
    vm_workspace: &mut VmWorkspace,
    reuse_uniform: bool,
    progress: f32,
    scope: SamplingScope,
    operator_automation: &mut [EffectAutomationWorkspace],
) -> Color {
    let program = &operator.program;
    let compiled = renderer.operator_program(*program);
    let pixel = &renderer.target(renderer.plan.target)[flat_pixel_index];
    let duration = renderer.duration;
    let context = OperatorRunContext {
        progress,
        time: SampleDuration::from_ticks(sample_time.as_ticks()),
        duration,
        pixel_index: pixel.pixel_index() as i32,
        pixel_count: pixel.pixel_count() as i32,
        pixel_fraction: pixel.pixel_fraction,
    };
    let mut sampler = GraphSignalSampler {
        renderer,
        inputs,
        cache,
        scope,
        flat_pixel_index,
        frame_slot,
        duration: renderer.duration,
        workspace,
        operator_automation,
    };
    match compiled.sample(
        params,
        &context,
        renderer.spatial_context(
            compiled.uses_spatial_context(),
            renderer.targets[renderer.plan.target].pixels.start + flat_pixel_index,
        ),
        crate::sections::SectionContext::Prepared {
            target: &renderer.targets[renderer.plan.target].sections,
            pixel: renderer.targets[renderer.plan.target]
                .sections
                .pixel(flat_pixel_index),
        },
        &mut sampler,
        vm_workspace,
        reuse_uniform,
    ) {
        Ok(color) => color,
        Err(never) => match never {},
    }
}

struct GraphSignalSampler<'a> {
    renderer: SignalGraph<'a>,
    inputs: &'a [usize],
    cache: &'a mut [Option<CachedSignal>],
    scope: SamplingScope,
    flat_pixel_index: usize,
    frame_slot: usize,
    duration: SampleDuration,
    workspace: &'a mut EvaluationWorkspace,
    operator_automation: &'a mut [EffectAutomationWorkspace],
}

impl SignalSampler<Infallible> for GraphSignalSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, Infallible> {
        let index = match pixel {
            SignalPixel::Current => self.flat_pixel_index,
            SignalPixel::Global(index) => {
                let Ok(index) = usize::try_from(index) else {
                    return Ok(black());
                };
                if index >= self.renderer.pixel_count {
                    return Ok(black());
                }
                index
            }
            SignalPixel::Local(index) => {
                let Ok(index) = usize::try_from(index) else {
                    return Ok(black());
                };
                let current =
                    &self.renderer.target(self.renderer.plan.target)[self.flat_pixel_index];
                if index >= current.pixel_count {
                    return Ok(black());
                }
                self.flat_pixel_index - current.pixel_index + index
            }
        };
        Ok(self.sample_at_pixel(input, sample_time, index, frame_cache))
    }
}

impl GraphSignalSampler<'_> {
    fn sample_at_pixel(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        flat_pixel_index: usize,
        frame_cache: Option<usize>,
    ) -> Color {
        if sample_time.as_ticks() >= self.duration.as_ticks() {
            return black();
        }
        let node = self.inputs[input];
        if self.scope == SamplingScope::Frame
            && let Some(frame_cache) = frame_cache
        {
            let stored = &mut self.workspace.operator_frames[self.frame_slot][frame_cache];
            let mut stored = core::mem::replace(
                stored,
                CachedSignalFrame {
                    key: None,
                    colors: Box::new([]),
                },
            );
            if stored.key != Some((node, sample_time)) {
                stored.key = None;
                sample_signal_frame(
                    self.renderer,
                    node,
                    sample_time,
                    &mut stored.colors,
                    self.cache,
                    self.workspace,
                    self.operator_automation,
                );
                stored.key = Some((node, sample_time));
            }
            let color = stored.colors[flat_pixel_index];
            self.workspace.operator_frames[self.frame_slot][frame_cache] = stored;
            return color;
        }
        sample_signal_pixel(
            self.renderer,
            node,
            sample_time,
            flat_pixel_index,
            self.cache,
            self.workspace,
            SamplingScope::Pixel,
            self.operator_automation,
        )
    }
}

fn sample_signal_frame(
    renderer: SignalGraph<'_>,
    node_index: usize,
    sample_time: SampleTime,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
) {
    let node = &renderer.plan.nodes[node_index];
    match &node.kind {
        PreparedSignalKind::Layer { layer_index } => {
            return sample_layer_frame(renderer, *layer_index, sample_time, output, workspace);
        }
        PreparedSignalKind::Operator { .. } => {}
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            if let Some((&first, rest)) = inputs.split_first() {
                sample_signal_frame(
                    renderer,
                    first,
                    sample_time,
                    output,
                    cache,
                    workspace,
                    operator_automation,
                );
                if !rest.is_empty() {
                    let slot = workspace.frame_scratch_used;
                    workspace.frame_scratch_used += 1;
                    let mut frame = core::mem::take(&mut workspace.frame_scratch[slot]);
                    for &input in rest {
                        sample_signal_frame(
                            renderer,
                            input,
                            sample_time,
                            &mut frame,
                            cache,
                            workspace,
                            operator_automation,
                        );
                        for (a, b) in output.iter_mut().zip(frame.iter()) {
                            compose_max(a, *b);
                        }
                    }
                    workspace.frame_scratch[slot] = frame;
                    workspace.frame_scratch_used -= 1;
                    return;
                }
            }
            return;
        }
    }
    for (flat_pixel_index, color) in output.iter_mut().enumerate() {
        cache.fill(None);
        *color = sample_signal_pixel(
            renderer,
            node_index,
            sample_time,
            flat_pixel_index,
            cache,
            workspace,
            SamplingScope::Frame,
            operator_automation,
        );
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
