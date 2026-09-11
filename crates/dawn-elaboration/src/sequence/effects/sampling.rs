use dawn_language::dsl::{BytecodeProgram, RuntimeError, VmWorkspace};
use dawn_language::values::Color;
use std::collections::HashMap;
use std::sync::Arc;

use crate::sequence::color::compose_max;
use crate::sequence::targets::PreparedTargetPixel;
use crate::{PreparedEffect, RenderError};

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreparedSampleContext {
    pub(crate) pixel_index: usize,
    pub(crate) pixel_count: usize,
    pub(crate) pixel_fraction: f32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct PreparedSampleContextKey {
    pixel_index: usize,
    pixel_count: usize,
    pixel_fraction_bits: u32,
}

impl From<PreparedSampleContext> for PreparedSampleContextKey {
    fn from(context: PreparedSampleContext) -> Self {
        Self {
            pixel_index: context.pixel_index,
            pixel_count: context.pixel_count,
            pixel_fraction_bits: context.pixel_fraction.to_bits(),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixels {
    pub(crate) pixels: Vec<PreparedSampledEffectPixel>,
    pub(crate) groups: Option<Vec<PreparedSampledEffectPixelGroup>>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixel {
    pub(crate) pixel: PreparedTargetPixel,
    pub(crate) rows: Vec<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct PreparedSampledEffectPixelGroup {
    context: PreparedSampleContext,
    rows: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct TargetColorAddress {
    pub(crate) fixture_index: usize,
    pub(crate) fixture_pixel_index: usize,
}

pub(crate) fn render_sampled_effect_target_colors(
    programs: &[Arc<BytecodeProgram>],
    effect: &PreparedEffect,
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    sample_time: dawn_language::values::SampleTime,
    workspace: &mut VmWorkspace,
    parameters: (
        Option<&mut dawn_runtime::signal::EffectAutomationWorkspace>,
        Option<&dawn_language::dsl::BoundParams>,
    ),
) -> Result<(), RenderError> {
    effect
        .with_sampler(
            programs,
            sample_time,
            parameters.0,
            parameters.1,
            |sampler| {
                render_sampled_effect_pixels(effect_pixels, rendered, |context| {
                    sampler.sample(
                        context.pixel_index,
                        context.pixel_count,
                        context.pixel_fraction,
                        workspace,
                    )
                })
            },
        )
        .map_err(RenderError::from)
}

#[inline(always)]
fn render_sampled_effect_pixels(
    effect_pixels: &PreparedSampledEffectPixels,
    rendered: &mut [Color],
    mut sample: impl FnMut(PreparedSampleContext) -> Result<Color, RuntimeError>,
) -> Result<(), RuntimeError> {
    if let Some(groups) = &effect_pixels.groups {
        for group in groups {
            let color = sample(group.context)?;
            for row in &group.rows {
                if let Some(target) = rendered.get_mut(*row) {
                    compose_max(target, color);
                }
            }
        }
        return Ok(());
    }
    for sampled in &effect_pixels.pixels {
        let pixel = &sampled.pixel;
        let color = sample(PreparedSampleContext {
            pixel_index: pixel.pixel_index(),
            pixel_count: pixel.pixel_count(),
            pixel_fraction: pixel.pixel_fraction,
        })?;
        for row in &sampled.rows {
            if let Some(target) = rendered.get_mut(*row) {
                compose_max(target, color);
            }
        }
    }
    Ok(())
}

pub(crate) fn prepare_sampled_effect_pixel_groups(
    pixels: &[PreparedSampledEffectPixel],
) -> Option<Vec<PreparedSampledEffectPixelGroup>> {
    let mut group_indexes = HashMap::<PreparedSampleContextKey, usize>::new();
    let mut groups = Vec::<PreparedSampledEffectPixelGroup>::new();
    let mut has_repeated_context = false;

    for sampled in pixels {
        let context = PreparedSampleContext {
            pixel_index: sampled.pixel.pixel_index(),
            pixel_count: sampled.pixel.pixel_count(),
            pixel_fraction: sampled.pixel.pixel_fraction,
        };
        let key = PreparedSampleContextKey::from(context);
        if let Some(group_index) = group_indexes.get(&key) {
            has_repeated_context = true;
            groups[*group_index]
                .rows
                .extend(sampled.rows.iter().copied());
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(PreparedSampledEffectPixelGroup {
                context,
                rows: sampled.rows.clone(),
            });
        }
    }

    has_repeated_context.then_some(groups)
}

pub(crate) fn evenly_sample_indices(source_count: usize, sample_count: usize) -> Vec<usize> {
    if source_count == 0 || sample_count == 0 {
        return Vec::new();
    }
    let sample_count = source_count.min(sample_count);
    if sample_count == 1 {
        return vec![0];
    }
    let last_source_index = source_count - 1;
    let last_sample_index = sample_count - 1;
    (0..sample_count)
        .map(|sample_index| {
            (sample_index * last_source_index + last_sample_index / 2) / last_sample_index
        })
        .collect()
}
