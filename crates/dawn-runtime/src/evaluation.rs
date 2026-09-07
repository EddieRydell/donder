use crate::dsl::bytecode::SignalPixel;
use crate::dsl::{
    BoundParams, OperatorRunContext, RunContext, RuntimeError, SignalSampler, VmWorkspace,
};
use crate::native_effect::{self, NativeSample, NativeSampleCache};
use crate::operator::{Echo, NativeOperator};
use crate::sampling::scale_color;
use crate::signal::{
    CachedSignal, CachedSignalFrame, CachedVmSample, EffectAutomationWorkspace, EvaluationError,
    EvaluationWorkspace, PreparedEffect, PreparedEffectImplementation, PreparedOperator,
    PreparedOperatorNode, PreparedSignalGraph, PreparedSignalKind,
};
use crate::values::{Color, SampleDuration, SampleTime};
use alloc::string::ToString;
use alloc::{boxed::Box, format};

enum SampleImplementation<'a> {
    Dsl(&'a crate::dsl::bytecode::BytecodeProgram, &'a BoundParams),
    Native(&'a NativeSample),
}

/// One effect at one time, sampled over any selection of pixel coordinates.
/// Both sequence playback and sparse editor rasters use this evaluator.
pub struct EffectSampler<'a> {
    implementation: SampleImplementation<'a>,
    context: RunContext,
    sample_time: SampleTime,
    native_cache: NativeSampleCache,
    reuse_uniform: bool,
}

impl PreparedEffect {
    #[inline(always)]
    pub fn with_sampler<P: core::borrow::Borrow<crate::dsl::bytecode::BytecodeProgram>, R>(
        &self,
        programs: &[P],
        sample_time: SampleTime,
        automation: Option<&mut EffectAutomationWorkspace>,
        bound: Option<&BoundParams>,
        run: impl FnOnce(&mut EffectSampler<'_>) -> Result<R, RuntimeError>,
    ) -> Result<R, EvaluationError> {
        if self.automation.is_some() && automation.is_none() {
            return Err(EvaluationError::InvalidWorkspace);
        }
        // This local owns automated resources only for the duration of sampling.
        // The pixel loop always sees a plain borrow, never an ownership branch.
        let prepared_native;
        let implementation = match &self.implementation {
            PreparedEffectImplementation::Bound { implementation, .. } => {
                let params = bound.ok_or(EvaluationError::InvalidWorkspace)?;
                match implementation {
                    crate::signal::BoundEffectImplementation::Dsl(program) => {
                        SampleImplementation::Dsl(programs[*program as usize].borrow(), params)
                    }
                    crate::signal::BoundEffectImplementation::Native(recipe) => {
                        prepared_native = recipe.resolve(params)?;
                        SampleImplementation::Native(&prepared_native)
                    }
                }
            }
            PreparedEffectImplementation::Dsl {
                program,
                bound_params,
            } => {
                let params = match automation {
                    Some(state) => prepare_effect_params(self, sample_time, state)?,
                    None => bound_params,
                };
                SampleImplementation::Dsl(programs[*program as usize].borrow(), params)
            }
            PreparedEffectImplementation::Native { sample, params } => {
                let sample = match automation {
                    Some(state) => {
                        let builtin = params
                            .as_ref()
                            .ok_or_else(|| EvaluationError::InvalidGraph {
                                message: "automated native effect has no bound parameters"
                                    .to_string(),
                            })?
                            .0;
                        let params = prepare_effect_params(self, sample_time, state)?;
                        prepared_native = native_effect::prepare_sample(builtin, params)?;
                        &prepared_native
                    }
                    None => sample,
                };
                SampleImplementation::Native(sample)
            }
        };
        let mut sampler = EffectSampler {
            implementation,
            context: RunContext {
                progress: self.progress(sample_time),
                time: self.local_time(sample_time),
                duration: self.duration,
                pixel_index: 0,
                pixel_count: 0,
                pixel_fraction: 0.0,
            },
            sample_time,
            native_cache: NativeSampleCache::default(),
            reuse_uniform: false,
        };
        run(&mut sampler).map_err(EvaluationError::from)
    }

    pub fn automation_workspace(&self) -> Option<EffectAutomationWorkspace> {
        let automation = self.automation.as_ref()?;
        let params = match &self.implementation {
            PreparedEffectImplementation::Dsl { bound_params, .. }
            | PreparedEffectImplementation::Native {
                params: Some((_, bound_params)),
                ..
            } => Some(crate::signal::automation_params(
                bound_params,
                &automation.bindings,
            )),
            PreparedEffectImplementation::Native { params: None, .. }
            | PreparedEffectImplementation::Bound { .. } => None,
        };
        Some(EffectAutomationWorkspace {
            params,
            sample_time: None,
        })
    }
}

impl EffectSampler<'_> {
    fn uniform(&self) -> bool {
        match &self.implementation {
            SampleImplementation::Dsl(program, _) => !program.uses_pixel_context,
            SampleImplementation::Native(sample) => !sample.uses_pixel_context(),
        }
    }

    #[inline(always)]
    pub fn sample(
        &mut self,
        pixel_index: usize,
        pixel_count: usize,
        pixel_fraction: f32,
        workspace: &mut VmWorkspace,
    ) -> Result<Color, RuntimeError> {
        self.context.pixel_index = pixel_index as i32;
        self.context.pixel_count = pixel_count as i32;
        self.context.pixel_fraction = pixel_fraction;
        let result = match &self.implementation {
            SampleImplementation::Dsl(program, params) => {
                program.sample_effect_from(params, &self.context, workspace, self.reuse_uniform)
            }
            SampleImplementation::Native(sample) => {
                sample.sample_cached(&self.context, self.sample_time, &mut self.native_cache)
            }
        };
        self.reuse_uniform = result.is_ok();
        result
    }
}

// Keep graph loops separate from patch/output loops; combining them worsens
// Xtensa code generation even though it removes one call per frame.
#[inline(never)]
pub(crate) fn sample_signal_graph<'a>(
    renderer: &PreparedSignalGraph,
    sample_time: SampleTime,
    workspace: &'a mut EvaluationWorkspace,
) -> Result<&'a [Color], EvaluationError> {
    let graph = &renderer.plan;
    workspace.effect_vm_sample = None;
    // Prefix registers belong to one node/time in each shared depth slot.
    // Never carry this identity across frame evaluations or parameter changes.
    for (_, sample) in &mut workspace.operator_vm {
        *sample = None;
    }
    for (_, time) in &mut workspace.operator_automation {
        *time = None;
    }
    for frames in &mut workspace.operator_frames {
        for frame in frames {
            frame.key = None;
        }
    }
    // Recursive signal sampling uses the VM and caches, not these frame buffers.
    // Keep the fixed storage separate while evaluating, and restore it on errors too.
    let mut buffers = core::mem::take(&mut workspace.signal_buffers);
    let result = (|| {
        for node_index in graph.frame_nodes.iter().copied() {
            let node =
                graph
                    .nodes
                    .get(node_index)
                    .ok_or_else(|| EvaluationError::InvalidGraph {
                        message: "graph node index is out of bounds".to_string(),
                    })?;
            let destination = frame_range(renderer, node_index)?;
            match &node.kind {
                PreparedSignalKind::Layer { layer_index } => {
                    sample_layer_frame(
                        renderer,
                        *layer_index,
                        sample_time,
                        &mut buffers[destination],
                        workspace,
                    )?;
                }
                PreparedSignalKind::Operator {
                    operator,
                    inputs,
                    automation,
                    vm_slot,
                } => {
                    let slot = operator.automation_slot as usize;
                    let mut automation_workspace = (!automation.is_empty())
                        .then(|| core::mem::take(&mut workspace.operator_automation[slot]));
                    if let Some((state, time)) = &mut automation_workspace {
                        if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                            workspace.operator_automation[slot] = automation_workspace.unwrap();
                            return Err(error);
                        }
                        *time = Some(sample_time);
                    }
                    let vm_slot = usize::from(*vm_slot);
                    let uses_vm = matches!(operator.implementation, PreparedOperator::Dsl(_));
                    let mut vm_workspace = if uses_vm {
                        core::mem::take(&mut workspace.operator_vm[vm_slot])
                    } else {
                        Default::default()
                    };
                    let sampled = sample_operator_frame(
                        renderer,
                        operator,
                        inputs,
                        automation_workspace.as_ref().map(|(params, _)| params),
                        sample_time,
                        destination,
                        &mut buffers,
                        workspace,
                        vm_slot,
                        &mut vm_workspace.0,
                    );
                    if uses_vm {
                        workspace.operator_vm[vm_slot] = vm_workspace;
                    }
                    if let Some(state) = automation_workspace {
                        workspace.operator_automation[slot] = state;
                    }
                    sampled?;
                }
                PreparedSignalKind::Output { inputs } => {
                    buffers[destination.clone()].fill(black());
                    for input in inputs {
                        let source = frame_range(renderer, *input)?;
                        for (target, source) in destination.clone().zip(source) {
                            let color = buffers[source];
                            compose_max(&mut buffers[target], color);
                        }
                    }
                }
            }
        }
        frame_range(renderer, graph.output_index)
    })();
    workspace.signal_buffers = buffers;
    result.map(|range| &workspace.signal_buffers[range])
}

pub(crate) fn frame_range(
    renderer: &PreparedSignalGraph,
    node_index: usize,
) -> Result<core::ops::Range<usize>, EvaluationError> {
    let graph = &renderer.plan;
    let slot = graph
        .frame_slots
        .get(node_index)
        .copied()
        .unwrap_or(u16::MAX);
    if slot >= graph.frame_buffer_count {
        return Err(EvaluationError::InvalidGraph {
            message: "signal has no prepared frame buffer".to_string(),
        });
    }
    let start = usize::from(slot) * renderer.pixel_count;
    Ok(start..start + renderer.pixel_count)
}

// Keep the per-pixel loop out of the large graph dispatcher; inlining it
// produces slower code for simple layered programs in controlled benchmarks.
#[inline(never)]
fn sample_layer_frame(
    renderer: &PreparedSignalGraph,
    layer_index: usize,
    sample_time: SampleTime,
    rendered: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    // This traversal owns the effect VM independently of recursive sampling.
    workspace.effect_vm_sample = None;
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(EvaluationError::InvalidGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    rendered.fill(black());
    if !layer.enabled {
        return Ok(());
    }
    let Some(layer_effects) = renderer.effects_by_layer.get(layer_index) else {
        return Ok(());
    };
    for effect_index in layer_effects {
        let Some(effect) = renderer.effects.get(*effect_index) else {
            continue;
        };
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let sample_count = renderer.targets[effect.target as usize].sample_count as usize;
        for sample in &mut workspace.effect_samples[..sample_count] {
            sample.pixel_count = 0;
        }
        let state = effect
            .automation
            .as_ref()
            .map(|automation| &mut workspace.effect_automation[automation.workspace_slot as usize]);
        let bound = match effect.implementation {
            PreparedEffectImplementation::Bound { environment, .. } => {
                Some(workspace.parameters.resolve(
                    &renderer.parameter_environments,
                    environment,
                    sample_time,
                )?)
            }
            _ => None,
        };
        effect.with_sampler(&renderer.programs, sample_time, state, bound, |sampler| {
            let uniform = sampler.uniform();
            let target = renderer.target(effect.target);
            if uniform {
                if let Some(pixel) = target.first() {
                    let color = sampler.sample(
                        pixel.pixel_index(),
                        pixel.pixel_count(),
                        pixel.pixel_fraction,
                        &mut workspace.effect_vm,
                    )?;
                    for pixel in target {
                        let flat_index = renderer.element_cell_offsets[pixel.element_index()]
                            + pixel.element_cell_index();
                        compose_max(&mut rendered[flat_index], color);
                    }
                }
                return Ok(());
            }
            // This traversal keeps one program, parameter set and sample time.
            // Scalar registers survive VM cleanup; restart initialization for
            // every effect/time, including backward seeks and automation edits.
            for pixel in target {
                // Pixel indices are already dense. The count distinguishes
                // fixtures of different sizes sharing the same index.
                let cached =
                    (sample_count != 0).then(|| &mut workspace.effect_samples[pixel.pixel_index()]);
                let color = match cached {
                    Some(sample) if sample.pixel_count == pixel.pixel_count => sample.color,
                    cached => {
                        let color = sampler.sample(
                            pixel.pixel_index(),
                            pixel.pixel_count(),
                            pixel.pixel_fraction,
                            &mut workspace.effect_vm,
                        )?;
                        if let Some(sample) = cached {
                            sample.pixel_count = pixel.pixel_count;
                            sample.color = color;
                        }
                        color
                    }
                };
                let flat_index = renderer.element_cell_offsets[pixel.element_index()]
                    + pixel.element_cell_index();
                compose_max(&mut rendered[flat_index], color);
            }
            Ok(())
        })?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_frame(
    renderer: &PreparedSignalGraph,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    workspace: &mut EvaluationWorkspace,
    frame_slot: usize,
    vm_workspace: &mut VmWorkspace,
) -> Result<(), EvaluationError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(program) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let compiled = &renderer.programs[*program as usize];
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
                flat_pixel_index,
                frame_slot,
                duration: renderer.duration,
                workspace,
            };
            match compiled.sample_operator_from(
                params,
                &context,
                &mut sampler,
                vm_workspace,
                reuse_uniform,
            ) {
                Ok(color) => {
                    output[flat_pixel_index] = color;
                    reuse_uniform = true;
                }
                Err(error) => {
                    workspace.signal_cache = cache;
                    return Err(error.into());
                }
            }
        }
        workspace.signal_cache = cache;
        return Ok(());
    };

    let operation = builtin.bind(params)?;
    if matches!(
        operation,
        NativeOperator::Delay(_) | NativeOperator::Echo(_)
    ) {
        let mut cache = core::mem::take(&mut workspace.signal_cache);
        let result = match operation {
            NativeOperator::Delay(delay) => sample_delay_frame(
                renderer,
                inputs[0],
                delay,
                sample_time,
                &mut buffers[destination],
                &mut cache,
                workspace,
            ),
            NativeOperator::Echo(echo) => sample_echo_frame(
                renderer,
                inputs[0],
                echo,
                sample_time,
                &mut buffers[destination],
                &mut cache,
                workspace,
            ),
            _ => unreachable!(),
        };
        workspace.signal_cache = cache;
        return result;
    }
    match operation {
        NativeOperator::Binary(op) => {
            binary_graph_op(renderer, inputs, destination, buffers, |a, b| {
                op.apply(a, b)
            })
        }
        NativeOperator::Unary(op) => {
            map_graph_op(renderer, inputs[0], destination, buffers, |color| {
                op.apply(color)
            })
        }
        NativeOperator::Delay(_) | NativeOperator::Echo(_) => unreachable!(),
    }
}

fn binary_graph_op(
    renderer: &PreparedSignalGraph,
    inputs: &[usize],
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    op: impl Fn(Color, Color) -> Color,
) -> Result<(), EvaluationError> {
    let left = frame_range(renderer, inputs[0])?;
    let right = frame_range(renderer, inputs[1])?;
    for ((target, left), right) in destination.zip(left).zip(right) {
        buffers[target] = op(buffers[left], buffers[right]);
    }
    Ok(())
}

fn map_graph_op(
    renderer: &PreparedSignalGraph,
    input: usize,
    destination: core::ops::Range<usize>,
    buffers: &mut [Color],
    op: impl Fn(Color) -> Color,
) -> Result<(), EvaluationError> {
    let input = frame_range(renderer, input)?;
    for (target, source) in destination.zip(input) {
        buffers[target] = op(buffers[source]);
    }
    Ok(())
}

fn sample_echo_frame(
    renderer: &PreparedSignalGraph,
    input: usize,
    echo: Echo,
    sample_time: SampleTime,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    output.fill(black());
    let mut frame = workspace
        .frame_scratch
        .pop()
        .ok_or(EvaluationError::InvalidWorkspace)?;
    let result = (|| {
        for (delayed_time, amount) in echo.samples(sample_time) {
            sample_signal_frame(renderer, input, delayed_time, &mut frame, cache, workspace)?;
            for (target, source) in output.iter_mut().zip(frame.iter()) {
                compose_max(target, scale_color(*source, amount));
            }
        }
        Ok(())
    })();
    workspace.frame_scratch.push(frame);
    result
}

fn sample_delay_frame(
    renderer: &PreparedSignalGraph,
    input: usize,
    delay: SampleDuration,
    sample_time: SampleTime,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    output.fill(black());
    let Some(delayed_time) = sample_time.checked_sub_duration(delay) else {
        return Ok(());
    };
    sample_signal_frame(renderer, input, delayed_time, output, cache, workspace)
}

fn sample_signal_pixel(
    renderer: &PreparedSignalGraph,
    node_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
) -> Result<Color, EvaluationError> {
    if let Some(Some(cached)) = cache.get(node_index)
        && cached.sample_time == sample_time
        && cached.flat_pixel_index == flat_pixel_index
    {
        return Ok(cached.color);
    }
    let node =
        renderer
            .plan
            .nodes
            .get(node_index)
            .ok_or_else(|| EvaluationError::InvalidGraph {
                message: "graph node index is out of bounds".to_string(),
            })?;
    let color = match &node.kind {
        PreparedSignalKind::Layer { layer_index } => sample_layer_pixel(
            renderer,
            *layer_index,
            sample_time,
            flat_pixel_index,
            workspace,
        )?,
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            vm_slot,
        } => {
            let slot = operator.automation_slot as usize;
            let mut automation_workspace = (!automation.is_empty())
                .then(|| core::mem::take(&mut workspace.operator_automation[slot]));
            if let Some((state, time)) = &mut automation_workspace
                && *time != Some(sample_time)
            {
                // Invalidate before mutation so a partial error cannot reuse old data.
                *time = None;
                if let Err(error) = apply_bound_automation(state, automation, sample_time) {
                    workspace.operator_automation[slot] = automation_workspace.unwrap();
                    return Err(error);
                }
                *time = Some(sample_time);
            }
            let vm_slot = usize::from(*vm_slot);
            let uses_vm = matches!(operator.implementation, PreparedOperator::Dsl(_));
            let mut vm_workspace = if uses_vm {
                core::mem::take(&mut workspace.operator_vm[vm_slot])
            } else {
                Default::default()
            };
            let cached = vm_workspace
                .1
                .take()
                .filter(|sample| sample.index == node_index && sample.time == sample_time);
            let reuse_uniform = cached.is_some();
            let progress = cached.map_or_else(
                || {
                    if uses_vm && renderer.duration.as_ticks() != 0 {
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
                automation_workspace.as_ref().map(|(params, _)| params),
                sample_time,
                flat_pixel_index,
                cache,
                workspace,
                vm_slot,
                &mut vm_workspace.0,
                reuse_uniform,
                progress,
            );
            if sampled.is_ok() {
                vm_workspace.1 = Some(CachedVmSample {
                    index: node_index,
                    time: sample_time,
                    progress,
                });
            }
            if uses_vm {
                workspace.operator_vm[vm_slot] = vm_workspace;
            }
            if let Some(state) = automation_workspace {
                workspace.operator_automation[slot] = state;
            }
            sampled?
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
                    )?,
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
    Ok(color)
}

fn sample_layer_pixel(
    renderer: &PreparedSignalGraph,
    layer_index: usize,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    workspace: &mut EvaluationWorkspace,
) -> Result<Color, EvaluationError> {
    let Some(layer) = renderer.layers.get(layer_index) else {
        return Err(EvaluationError::InvalidGraph {
            message: "layer index is out of bounds".to_string(),
        });
    };
    if !layer.enabled {
        return Ok(black());
    }
    let Some(pixel) = renderer.target(renderer.plan.target).get(flat_pixel_index) else {
        return Ok(black());
    };
    let mut rendered = black();
    let Some(layer_effects) = renderer.effects_by_layer.get(layer_index) else {
        return Ok(rendered);
    };
    for effect_index in layer_effects {
        let Some(effect) = renderer.effects.get(*effect_index) else {
            continue;
        };
        if effect.start_time > sample_time {
            break;
        }
        if !effect.is_active(sample_time) {
            continue;
        }
        let effect_pixel = if effect.target == renderer.plan.target {
            // Elaboration interns exact pixel contexts, not just addresses.
            Some(pixel)
        } else {
            let target = renderer.target(effect.target);
            target
                .binary_search_by_key(
                    &(pixel.element_index(), pixel.element_cell_index()),
                    |effect_pixel| {
                        (
                            effect_pixel.element_index(),
                            effect_pixel.element_cell_index(),
                        )
                    },
                )
                .ok()
                .map(|index| &target[index])
        };
        if let Some(effect_pixel) = effect_pixel {
            let cached = workspace
                .effect_vm_sample
                .filter(|(sample, ..)| sample.index == *effect_index && sample.time == sample_time);
            if let Some((_, _, color)) = cached
                && let Some(program) = effect.implementation.dsl_program()
                && !renderer.programs[program as usize].uses_pixel_context
            {
                compose_max(&mut rendered, color);
                continue;
            }
            // A failed evaluation or a different effect must not expose old slots.
            workspace.effect_vm_sample = None;
            let reuse_uniform = cached.is_some();
            let (progress, local_time) = cached.map_or_else(
                || (effect.progress(sample_time), effect.local_time(sample_time)),
                |(sample, local_time, _)| (sample.progress, local_time),
            );
            let state = effect.automation.as_ref().map(|automation| {
                &mut workspace.effect_automation[automation.workspace_slot as usize]
            });
            let bound = match effect.implementation {
                PreparedEffectImplementation::Bound { environment, .. } => {
                    Some(workspace.parameters.resolve(
                        &renderer.parameter_environments,
                        environment,
                        sample_time,
                    )?)
                }
                _ => None,
            };
            let color =
                effect.with_sampler(&renderer.programs, sample_time, state, bound, |sampler| {
                    sampler.context.progress = progress;
                    sampler.context.time = local_time;
                    sampler.reuse_uniform = reuse_uniform;
                    sampler.sample(
                        effect_pixel.pixel_index(),
                        effect_pixel.pixel_count(),
                        effect_pixel.pixel_fraction,
                        &mut workspace.effect_vm,
                    )
                })?;
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
    Ok(rendered)
}

#[allow(clippy::too_many_arguments)]
fn sample_operator_pixel(
    renderer: &PreparedSignalGraph,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    automation: Option<&BoundParams>,
    sample_time: SampleTime,
    flat_pixel_index: usize,
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    frame_slot: usize,
    vm_workspace: &mut VmWorkspace,
    reuse_uniform: bool,
    progress: f32,
) -> Result<Color, EvaluationError> {
    let params = automation.unwrap_or(&operator.params);
    let PreparedOperator::Native(builtin) = &operator.implementation else {
        let PreparedOperator::Dsl(program) = &operator.implementation else {
            unreachable!("operator implementation is native or DSL")
        };
        let compiled = &renderer.programs[*program as usize];
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
            flat_pixel_index,
            frame_slot,
            duration: renderer.duration,
            workspace,
        };
        return Ok(compiled.sample_operator_from(
            params,
            &context,
            &mut sampler,
            vm_workspace,
            reuse_uniform,
        )?);
    };
    let sample = |input: usize,
                  time: SampleTime,
                  cache: &mut [Option<CachedSignal>],
                  workspace: &mut EvaluationWorkspace| {
        sample_signal_pixel(
            renderer,
            inputs[input],
            time,
            flat_pixel_index,
            cache,
            workspace,
        )
    };
    Ok(match builtin.bind(params)? {
        NativeOperator::Binary(op) => op.apply(
            sample(0, sample_time, cache, workspace)?,
            sample(1, sample_time, cache, workspace)?,
        ),
        NativeOperator::Unary(op) => op.apply(sample(0, sample_time, cache, workspace)?),
        NativeOperator::Delay(delay) => {
            let Some(delayed_time) = sample_time.checked_sub_duration(delay) else {
                return Ok(black());
            };
            sample(0, delayed_time, cache, workspace)?
        }
        NativeOperator::Echo(echo) => {
            let mut output = black();
            for (delayed_time, amount) in echo.samples(sample_time) {
                compose_max(
                    &mut output,
                    scale_color(sample(0, delayed_time, cache, workspace)?, amount),
                );
            }
            output
        }
    })
}

struct GraphSignalSampler<'a> {
    renderer: &'a PreparedSignalGraph,
    inputs: &'a [usize],
    cache: &'a mut [Option<CachedSignal>],
    flat_pixel_index: usize,
    frame_slot: usize,
    duration: SampleDuration,
    workspace: &'a mut EvaluationWorkspace,
}

impl SignalSampler for GraphSignalSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        pixel: SignalPixel<i32>,
        frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
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
                if index >= current.pixel_count as usize {
                    return Ok(black());
                }
                self.flat_pixel_index - current.pixel_index as usize + index
            }
        };
        self.sample_at_pixel(input, sample_time, index, frame_cache)
    }
}

impl GraphSignalSampler<'_> {
    fn sample_at_pixel(
        &mut self,
        input: usize,
        sample_time: SampleTime,
        flat_pixel_index: usize,
        frame_cache: Option<usize>,
    ) -> Result<Color, RuntimeError> {
        if sample_time.as_ticks() >= self.duration.as_ticks() {
            return Ok(black());
        }
        let node = self
            .inputs
            .get(input)
            .copied()
            .ok_or_else(|| RuntimeError {
                message: "Signal input index is out of bounds".to_string(),
            })?;
        if let Some(frame_cache) = frame_cache {
            let Some(frames) = self.workspace.operator_frames.get_mut(self.frame_slot) else {
                return Err(RuntimeError {
                    message: "Signal frame cache slot is out of bounds".to_string(),
                });
            };
            let Some(stored) = frames.get_mut(frame_cache) else {
                return Err(RuntimeError {
                    message: "Signal frame cache index is out of bounds".to_string(),
                });
            };
            let mut stored = core::mem::replace(
                stored,
                CachedSignalFrame {
                    key: None,
                    colors: Box::new([]),
                },
            );
            if stored.key != Some((node, sample_time)) {
                stored.key = None;
                let sampled = sample_signal_frame(
                    self.renderer,
                    node,
                    sample_time,
                    &mut stored.colors,
                    self.cache,
                    self.workspace,
                );
                if let Err(error) = sampled {
                    self.workspace.operator_frames[self.frame_slot][frame_cache] = stored;
                    return Err(RuntimeError {
                        message: format!("failed to sample Signal frame: {error:?}"),
                    });
                }
                stored.key = Some((node, sample_time));
            }
            let color = stored
                .colors
                .get(flat_pixel_index)
                .copied()
                .ok_or_else(|| RuntimeError {
                    message: "Signal frame pixel is out of bounds".to_string(),
                });
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
        )
        .map_err(|error| RuntimeError {
            message: format!("failed to sample Signal: {error:?}"),
        })
    }
}

fn sample_signal_frame(
    renderer: &PreparedSignalGraph,
    node_index: usize,
    sample_time: SampleTime,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
) -> Result<(), EvaluationError> {
    if output.len() != renderer.pixel_count {
        return Err(EvaluationError::InvalidWorkspace);
    }
    let node =
        renderer
            .plan
            .nodes
            .get(node_index)
            .ok_or_else(|| EvaluationError::InvalidGraph {
                message: "graph node index is out of bounds".to_string(),
            })?;
    match &node.kind {
        PreparedSignalKind::Layer { layer_index } => {
            return sample_layer_frame(renderer, *layer_index, sample_time, output, workspace);
        }
        PreparedSignalKind::Operator {
            operator,
            inputs,
            automation,
            ..
        } => {
            if let PreparedOperator::Native(builtin) = operator.implementation {
                let slot = operator.automation_slot as usize;
                let mut state = (!automation.is_empty())
                    .then(|| core::mem::take(&mut workspace.operator_automation[slot]));
                let result = (|| {
                    if let Some((params, time)) = &mut state {
                        apply_bound_automation(params, automation, sample_time)?;
                        *time = Some(sample_time);
                    }
                    let params = state
                        .as_ref()
                        .map_or(&operator.params, |(params, _)| params);
                    let operation = builtin.bind(params)?;
                    match operation {
                        NativeOperator::Delay(delay) => {
                            return sample_delay_frame(
                                renderer,
                                inputs[0],
                                delay,
                                sample_time,
                                output,
                                cache,
                                workspace,
                            );
                        }
                        NativeOperator::Echo(echo) => {
                            return sample_echo_frame(
                                renderer,
                                inputs[0],
                                echo,
                                sample_time,
                                output,
                                cache,
                                workspace,
                            );
                        }
                        _ => {}
                    }
                    sample_signal_frame(
                        renderer,
                        inputs[0],
                        sample_time,
                        output,
                        cache,
                        workspace,
                    )?;
                    match operation {
                        NativeOperator::Unary(op) => {
                            for color in output {
                                *color = op.apply(*color);
                            }
                        }
                        NativeOperator::Binary(op) => {
                            let mut right = workspace
                                .frame_scratch
                                .pop()
                                .ok_or(EvaluationError::InvalidWorkspace)?;
                            let sampled = sample_signal_frame(
                                renderer,
                                inputs[1],
                                sample_time,
                                &mut right,
                                cache,
                                workspace,
                            );
                            if sampled.is_ok() {
                                for (a, b) in output.iter_mut().zip(right.iter()) {
                                    *a = op.apply(*a, *b);
                                }
                            }
                            workspace.frame_scratch.push(right);
                            sampled?;
                        }
                        NativeOperator::Delay(_) | NativeOperator::Echo(_) => unreachable!(),
                    }
                    Ok(())
                })();
                if let Some(state) = state {
                    workspace.operator_automation[slot] = state;
                }
                return result;
            }
        }
        PreparedSignalKind::Output { inputs } => {
            output.fill(black());
            if let Some((&first, rest)) = inputs.split_first() {
                sample_signal_frame(renderer, first, sample_time, output, cache, workspace)?;
                if !rest.is_empty() {
                    let mut frame = workspace
                        .frame_scratch
                        .pop()
                        .ok_or(EvaluationError::InvalidWorkspace)?;
                    let result = (|| {
                        for &input in rest {
                            sample_signal_frame(
                                renderer,
                                input,
                                sample_time,
                                &mut frame,
                                cache,
                                workspace,
                            )?;
                            for (a, b) in output.iter_mut().zip(frame.iter()) {
                                compose_max(a, *b);
                            }
                        }
                        Ok(())
                    })();
                    workspace.frame_scratch.push(frame);
                    return result;
                }
            }
            return Ok(());
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
        )?;
    }
    Ok(())
}

fn prepare_effect_params<'a>(
    effect: &PreparedEffect,
    sample_time: SampleTime,
    workspace: &'a mut EffectAutomationWorkspace,
) -> Result<&'a BoundParams, EvaluationError> {
    let automation = effect
        .automation
        .as_ref()
        .ok_or_else(|| EvaluationError::InvalidGraph {
            message: "automated effect is missing its prepared automation".to_string(),
        })?;
    if workspace.sample_time != Some(sample_time) {
        workspace.sample_time = None;
        let params = workspace
            .params
            .as_mut()
            .ok_or_else(|| EvaluationError::InvalidGraph {
                message: "automated effect parameters are missing".to_string(),
            })?;
        apply_bound_automation(params, &automation.bindings, sample_time)?;
        workspace.sample_time = Some(sample_time);
    }
    workspace
        .params
        .as_ref()
        .ok_or_else(|| EvaluationError::InvalidGraph {
            message: "automated effect parameters are missing".to_string(),
        })
}

pub fn apply_bound_automation(
    params: &mut BoundParams,
    automation: &[crate::signal::PreparedAutomation],
    sample_time: SampleTime,
) -> Result<(), EvaluationError> {
    for binding in automation {
        params.apply_automation(
            usize::from(binding.param_index),
            &binding.curve,
            &binding.mapping,
            binding.position(sample_time),
        )?;
    }
    Ok(())
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
