//! Bounded color-block traversal. Each temporal query initializes its upstream
//! program once for the block. No sampled frames or temporal results are retained.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn operator(
    renderer: SignalGraph<'_>,
    operator: &PreparedOperatorNode,
    inputs: &[usize],
    params: &BoundParams,
    time: SampleTime,
    first_pixel: usize,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    vm: &mut VmWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
    operator_vms: &mut [OperatorVmWorkspace],
    reuse_uniform: bool,
) -> f32 {
    let program = renderer.operator_program(operator.program);
    let mut context = RunContext {
        progress: if program.uses_progress() && renderer.duration.as_ticks() != 0 {
            (time.as_ticks() as f32 / renderer.duration.as_ticks() as f32).clamp(0.0, 1.0)
        } else {
            0.0
        },
        time: SampleDuration::from_ticks(time.as_ticks()),
        duration: renderer.duration,
        pixel_index: 0,
        pixel_count: 0,
        pixel_fraction: 0.0,
    };
    let mut sampler = BlockSampler {
        renderer,
        inputs,
        first_pixel,
        cache,
        workspace,
        operator_automation,
        operator_vms,
    };
    if program.supports_color_blocks() {
        // Color-lane admission excludes pixel/spatial context reads.
        let spatial = crate::dsl::SpatialContext {
            position: [0.0; 2],
            min: [0.0; 2],
            max: [0.0; 2],
        };
        program.sample_block(
            params,
            &context,
            &spatial,
            &mut sampler,
            vm,
            output,
            reuse_uniform,
        );
    } else if program.supports_numeric_blocks() {
        let contexts: [_; crate::dsl::COLOR_BLOCK_WIDTH] = core::array::from_fn(|lane| {
            let flat = first_pixel + lane.min(output.len() - 1);
            let pixel = renderer.target(renderer.plan.target).pixel(flat);
            crate::dsl::LaneContext {
                context: RunContext {
                    pixel_index: pixel.pixel_index() as i32,
                    pixel_count: pixel.pixel_count() as i32,
                    pixel_fraction: pixel.pixel_fraction,
                    ..context
                },
                spatial: *renderer.spatial_context(
                    program.uses_spatial_context(),
                    renderer.plan.target,
                    flat,
                    &pixel,
                ),
                sections: crate::sections::SectionContext::Prepared {
                    target: &renderer.targets[renderer.plan.target].sections,
                    pixel: if program.uses_sections() {
                        renderer.targets[renderer.plan.target].sections.pixel(flat)
                    } else {
                        Default::default()
                    },
                },
            }
        });
        program.sample_numeric_block(
            params,
            &contexts[..output.len()],
            &mut sampler,
            vm,
            output,
            reuse_uniform,
        );
    } else {
        // Admission proves that every source call has the same input/time and
        // current-pixel addressing. The temporary lives only for this block;
        // scalar continuations retain their own control flow and quantization.
        let mut source = SingleQuerySampler {
            upstream: sampler,
            colors: [Color::BLACK; crate::dsl::COLOR_BLOCK_WIDTH],
            width: output.len(),
            lane: 0,
            initialized: false,
        };
        for (lane, destination) in output.iter_mut().enumerate() {
            source.lane = lane;
            let flat = first_pixel + lane;
            let pixel = renderer.target(renderer.plan.target).pixel(flat);
            context.pixel_index = pixel.pixel_index() as i32;
            context.pixel_count = pixel.pixel_count() as i32;
            context.pixel_fraction = pixel.pixel_fraction;
            let spatial = renderer.spatial_context(
                program.uses_spatial_context(),
                renderer.plan.target,
                flat,
                &pixel,
            );
            let sections = crate::sections::SectionContext::Prepared {
                target: &renderer.targets[renderer.plan.target].sections,
                pixel: if program.uses_sections() {
                    renderer.targets[renderer.plan.target].sections.pixel(flat)
                } else {
                    Default::default()
                },
            };
            *destination = match program.sample(
                params,
                &context,
                &spatial,
                sections,
                &mut source,
                vm,
                reuse_uniform || lane != 0,
            ) {
                Ok(color) => color,
                Err(never) => match never {},
            };
        }
    }
    context.progress
}

struct SingleQuerySampler<'a> {
    upstream: BlockSampler<'a>,
    colors: [Color; crate::dsl::COLOR_BLOCK_WIDTH],
    width: usize,
    lane: usize,
    initialized: bool,
}

impl SignalSampler<Infallible> for SingleQuerySampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, Infallible> {
        if !self.initialized {
            self.upstream.sample_signal_block(
                input,
                time,
                pixel,
                &mut self.colors[..self.width],
            )?;
            self.initialized = true;
        }
        Ok(self.colors[self.lane])
    }
}

struct BlockSampler<'a> {
    renderer: SignalGraph<'a>,
    inputs: &'a [usize],
    first_pixel: usize,
    cache: &'a mut [Option<CachedSignal>],
    workspace: &'a mut EvaluationWorkspace,
    operator_automation: &'a mut [EffectAutomationWorkspace],
    operator_vms: &'a mut [OperatorVmWorkspace],
}

impl SignalSampler<Infallible> for BlockSampler<'_> {
    fn sample_signal(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        _: Option<usize>,
    ) -> Result<Color, Infallible> {
        let mut color = [Color::BLACK];
        self.sample_signal_block(input, time, pixel, &mut color)?;
        Ok(color[0])
    }

    fn sample_signal_block(
        &mut self,
        input: usize,
        time: SampleTime,
        pixel: SignalPixel<i32>,
        output: &mut [Color],
    ) -> Result<(), Infallible> {
        if time.as_ticks() >= self.renderer.duration.as_ticks() {
            output.fill(Color::BLACK);
            return Ok(());
        }
        let node = self.inputs[input];
        if matches!(pixel, SignalPixel::Current) {
            signal(
                self.renderer,
                node,
                time,
                self.first_pixel,
                output,
                self.cache,
                self.workspace,
                self.operator_automation,
                self.operator_vms,
            );
        } else {
            // Explicit local addressing is relative to each lane's fixture.
            // Global addressing denotes the same source for every lane.
            for (lane, color) in output.iter_mut().enumerate() {
                let current = self.first_pixel + lane;
                let target = signal_pixel(self.renderer, current, pixel);
                *color = target.map_or(Color::BLACK, |index| {
                    sample_signal_pixel(
                        self.renderer,
                        node,
                        time,
                        index,
                        self.cache,
                        self.workspace,
                        SamplingScope::Pixel,
                        self.operator_automation,
                        self.operator_vms,
                    )
                });
            }
        }
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn signal(
    renderer: SignalGraph<'_>,
    node: usize,
    time: SampleTime,
    first_pixel: usize,
    output: &mut [Color],
    cache: &mut [Option<CachedSignal>],
    workspace: &mut EvaluationWorkspace,
    operator_automation: &mut [EffectAutomationWorkspace],
    operator_vms: &mut [OperatorVmWorkspace],
) {
    match &renderer.plan.nodes[node].kind {
        PreparedSignalKind::Layer { layer_index } => {
            layer(renderer, *layer_index, time, first_pixel, output, workspace);
        }
        PreparedSignalKind::Operator {
            operator: op,
            inputs,
            automation,
            vm_slot,
        } if renderer.operator_program(op.program).supports_blocks() => {
            let (params, upstream) = operator_params(op, automation, time, operator_automation);
            let (upstream_vms, current) = operator_vms.split_at_mut(*vm_slot);
            let reuse_uniform = current[0]
                .sample
                .take()
                .is_some_and(|sample| sample.index == node && sample.time == time);
            let progress = operator(
                renderer,
                op,
                inputs,
                params,
                time,
                first_pixel,
                output,
                cache,
                workspace,
                &mut current[0].vm,
                upstream,
                upstream_vms,
                reuse_uniform,
            );
            current[0].sample = Some(CachedVmSample {
                index: node,
                time,
                progress,
            });
        }
        PreparedSignalKind::Output { inputs } => {
            output.fill(Color::BLACK);
            let mut colors = [Color::BLACK; crate::dsl::COLOR_BLOCK_WIDTH];
            for &input in inputs {
                let colors = &mut colors[..output.len()];
                signal(
                    renderer,
                    input,
                    time,
                    first_pixel,
                    colors,
                    cache,
                    workspace,
                    operator_automation,
                    operator_vms,
                );
                for (dst, src) in output.iter_mut().zip(colors) {
                    compose_max(dst, *src);
                }
            }
        }
        _ => {
            for (lane, color) in output.iter_mut().enumerate() {
                *color = sample_signal_pixel(
                    renderer,
                    node,
                    time,
                    first_pixel + lane,
                    cache,
                    workspace,
                    SamplingScope::Pixel,
                    operator_automation,
                    operator_vms,
                );
            }
        }
    }
}

fn layer(
    renderer: SignalGraph<'_>,
    layer_index: usize,
    time: SampleTime,
    first_pixel: usize,
    output: &mut [Color],
    workspace: &mut EvaluationWorkspace,
) {
    output.fill(Color::BLACK);
    if !renderer.layers[layer_index].enabled {
        return;
    }
    for &effect_index in &renderer.effects_by_layer[layer_index] {
        let effect = &renderer.effects[effect_index];
        if effect.start_time > time {
            break;
        }
        if !effect.is_active(time) {
            continue;
        }
        let cached = workspace
            .effect_vm_sample
            .take()
            .filter(|(sample, ..)| sample.index == effect_index && sample.time == time);
        let params = effect.resolve_params(time, |slot| &mut workspace.effect_automation[slot]);
        effect.with_sampler(renderer, time, params, |sampler| {
            // Continue the existing workspace's query prefix across adjacent
            // blocks when no other effect/time has overwritten its registers.
            sampler.reuse_uniform = cached.is_some();
            if !sampler.uniform() && sampler.program.supports_numeric_blocks() {
                let target = renderer.target(effect.target);
                let Some(first) = target.first() else { return };
                let mut selected = [(0, first); crate::dsl::COLOR_BLOCK_WIDTH];
                let mut destinations = [0; crate::dsl::COLOR_BLOCK_WIDTH];
                let mut width = 0;
                for lane in 0..output.len() {
                    let flat = first_pixel + lane;
                    let pixel = renderer.target(renderer.plan.target).pixel(flat);
                    let index = if effect.target == renderer.plan.target {
                        Some(flat)
                    } else {
                        target.find(pixel.fixture_index, pixel.fixture_pixel_index)
                    };
                    if let Some(index) = index {
                        selected[width] = (index, target.pixel(index));
                        destinations[width] = lane;
                        width += 1;
                    }
                }
                if width != 0 {
                    let mut colors = [Color::BLACK; crate::dsl::COLOR_BLOCK_WIDTH];
                    sampler.sample_block(
                        renderer,
                        effect.target,
                        &selected[..width],
                        &mut workspace.effect_vm,
                        &mut colors[..width],
                    );
                    for lane in 0..width {
                        compose_max(&mut output[destinations[lane]], colors[lane]);
                    }
                    workspace.effect_vm_sample = Some((
                        CachedVmSample {
                            index: effect_index,
                            time,
                            progress: sampler.context.progress,
                        },
                        sampler.context.time,
                        colors[width - 1],
                    ));
                }
                return;
            }
            let mut uniform_color = if sampler.uniform() {
                cached.map(|(_, _, color)| color)
            } else {
                None
            };
            let mut last_color = None;
            for (lane, destination) in output.iter_mut().enumerate() {
                let flat = first_pixel + lane;
                let pixel = renderer.target(renderer.plan.target).pixel(flat);
                let target = renderer.target(effect.target);
                let selected = if effect.target == renderer.plan.target {
                    Some((flat, pixel))
                } else {
                    target
                        .find(pixel.fixture_index, pixel.fixture_pixel_index)
                        .map(|index| (index, target.pixel(index)))
                };
                let Some((index, pixel)) = selected else {
                    continue;
                };
                let color = match uniform_color {
                    Some(color) => color,
                    None => sampler.sample_spatial(
                        &pixel,
                        index,
                        &renderer.spatial_context(sampler.spatial, effect.target, index, &pixel),
                        &mut workspace.effect_vm,
                    ),
                };
                if sampler.uniform() {
                    uniform_color = Some(color);
                }
                last_color = Some(color);
                compose_max(destination, color);
            }
            if let Some(color) = last_color {
                workspace.effect_vm_sample = Some((
                    CachedVmSample {
                        index: effect_index,
                        time,
                        progress: sampler.context.progress,
                    },
                    sampler.context.time,
                    color,
                ));
            }
        });
    }
}
