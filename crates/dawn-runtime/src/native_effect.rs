use crate::BuiltinEffect;
use crate::dsl::{BoundParams, CurveCrossings, RunContext, RuntimeError};
use crate::sampling::{hsv, mix_colors, sample_curve, sample_gradient, scale_color};
use crate::values::{Color, Curve, Gradient, SampleDuration, SampleTime};
#[cfg(not(feature = "atomic"))]
use alloc::rc::Rc as Arc;
use alloc::string::String;
#[cfg(feature = "atomic")]
use alloc::sync::Arc;

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum GradientMode {
    ThroughEffect,
    AcrossItems,
    PerPulse,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum NativeSample {
    Pulse {
        gradient: Arc<Gradient>,
        pulse_shape: Arc<Curve>,
    },
    Chase(Chase),
    Spin {
        chase: Chase,
        revolutions: i32,
    },
    MarkPulseChild(MarkPulseChild),
    MarkChaseChild(MarkChaseChild),
}

/// Fixed child identity with rendering inputs resolved from its lexical
/// generator environment at the requested time.
#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum NativeParameterSample {
    Sample(BuiltinEffect),
    MarkPulse {
        #[rkyv(with = crate::wire::Microseconds)]
        parent_start: SampleTime,
        #[rkyv(with = crate::wire::Microseconds)]
        parent_duration: SampleDuration,
    },
    MarkChase {
        mark_index: u32,
        #[rkyv(with = crate::wire::Microseconds)]
        parent_start: SampleTime,
        #[rkyv(with = crate::wire::Microseconds)]
        parent_duration: SampleDuration,
    },
}

impl NativeParameterSample {
    pub fn resolve(&self, params: &BoundParams) -> Result<NativeSample, RuntimeError> {
        Ok(match *self {
            Self::Sample(builtin) => return prepare_sample(builtin, params),
            Self::MarkPulse {
                parent_start,
                parent_duration,
            } => NativeSample::MarkPulseChild(MarkPulseChild {
                base: params.color(1)?,
                accent: params.gradient(2)?,
                hue: params.curve(3)?,
                hue_mix: params.float(4)?,
                section_width_pixels: params.int(7)?,
                section_edge_fade_pixels: params.float(8)?,
                parent_start,
                parent_duration,
            }),
            Self::MarkChase {
                mark_index,
                parent_start,
                parent_duration,
            } => {
                let gradients = params.array_len(3)?;
                let positions = params.array_len(10)?;
                if gradients == 0 || positions == 0 {
                    return Err(error(
                        "mark chase requires non-empty gradients and chase_positions",
                    ));
                }
                NativeSample::MarkChaseChild(MarkChaseChild {
                    base: params.color(1)?,
                    gradient_mode: parse_gradient_mode(params.enum_name(2)?)?,
                    gradient: params.gradient_at(3, mark_index as usize % gradients)?,
                    hue: params.curve(4)?,
                    hue_mix: params.float(5)?,
                    pulse_overlap: params.float(8)?,
                    section_width_pixels: params.int(9)?,
                    chase_position: params.curve_at(10, mark_index as usize % positions)?,
                    pulse_shape: params.curve(11)?,
                    parent_start,
                    parent_duration,
                })
            }
        })
    }
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct MarkPulseChild {
    pub base: Color,
    pub accent: Arc<Gradient>,
    pub hue: Arc<Curve>,
    pub hue_mix: f32,
    pub section_width_pixels: i32,
    pub section_edge_fade_pixels: f32,
    #[rkyv(with = crate::wire::Microseconds)]
    pub parent_start: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub parent_duration: SampleDuration,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct MarkChaseChild {
    pub base: Color,
    pub gradient_mode: GradientMode,
    pub gradient: Arc<Gradient>,
    pub hue: Arc<Curve>,
    pub hue_mix: f32,
    pub pulse_overlap: f32,
    pub section_width_pixels: i32,
    pub chase_position: Arc<Curve>,
    pub pulse_shape: Arc<Curve>,
    #[rkyv(with = crate::wire::Microseconds)]
    pub parent_start: SampleTime,
    #[rkyv(with = crate::wire::Microseconds)]
    pub parent_duration: SampleDuration,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub struct Chase {
    gradient: Arc<Gradient>,
    gradient_mode: GradientMode,
    pulse_overlap: f32,
    section_width_pixels: i32,
    chase_position: CurveCrossings,
    reverse: bool,
    extend_to_start: bool,
    extend_to_end: bool,
    pulse_shape: Arc<Curve>,
}

pub fn prepare_sample(
    builtin: BuiltinEffect,
    params: &BoundParams,
) -> Result<NativeSample, RuntimeError> {
    Ok(match builtin {
        BuiltinEffect::Pulse => NativeSample::Pulse {
            gradient: params.gradient(0)?,
            pulse_shape: params.curve(1)?,
        },
        BuiltinEffect::Chase => NativeSample::Chase(prepare_chase(params)?),
        BuiltinEffect::Spin => NativeSample::Spin {
            chase: prepare_chase(params)?,
            revolutions: params.int(9)?,
        },
        BuiltinEffect::MarkPulse | BuiltinEffect::MarkChase => {
            return Err(error("generator effect cannot be sampled"));
        }
    })
}

fn prepare_chase(params: &BoundParams) -> Result<Chase, RuntimeError> {
    Ok(Chase {
        gradient: params.gradient(0)?,
        gradient_mode: parse_gradient_mode(params.enum_name(1)?)?,
        pulse_overlap: params.float(2)?,
        section_width_pixels: params.int(3)?,
        chase_position: params.prepared_curve_crossings(4)?,
        reverse: params.boolean(5)?,
        extend_to_start: params.boolean(6)?,
        extend_to_end: params.boolean(7)?,
        pulse_shape: params.curve(8)?,
    })
}

impl NativeSample {
    pub(crate) fn uses_pixel_context(&self) -> bool {
        match self {
            Self::Pulse { .. } => false,
            Self::MarkPulseChild(child) => child.section_edge_fade_pixels > 0.0,
            _ => true,
        }
    }

    pub fn sample(
        &self,
        context: &RunContext,
        sample_time: SampleTime,
    ) -> Result<Color, RuntimeError> {
        let mut cache = NativeSampleCache::default();
        self.sample_cached(context, sample_time, &mut cache)
    }

    pub(crate) fn sample_cached(
        &self,
        context: &RunContext,
        sample_time: SampleTime,
        cache: &mut NativeSampleCache,
    ) -> Result<Color, RuntimeError> {
        match self {
            Self::Pulse {
                gradient,
                pulse_shape,
            } => gradient_scaled(
                gradient,
                context.progress,
                sample_curve(pulse_shape, context.progress),
            ),
            Self::Chase(chase) => {
                let geometry = cache.chase(chase, context.pixel_count, None);
                sample_chase(chase, context, geometry)
            }
            Self::Spin { chase, revolutions } => {
                let geometry = cache.chase(chase, context.pixel_count, Some(*revolutions));
                sample_chase(chase, context, geometry)
            }
            Self::MarkPulseChild(child) => child.sample(context, sample_time, cache),
            Self::MarkChaseChild(child) => child.sample(context, sample_time, cache),
        }
    }
}

#[derive(Default)]
pub(crate) struct NativeSampleCache {
    chase: Option<ChaseGeometry>,
    hue: Option<Color>,
}

impl NativeSampleCache {
    fn hue(
        &mut self,
        curve: &Curve,
        time: SampleTime,
        start: SampleTime,
        duration: SampleDuration,
    ) -> Color {
        // One cache belongs to one immutable effect at one time. Hue is independent
        // of pixel coordinates, just like the DSL's hoisted resource expressions.
        *self.hue.get_or_insert_with(|| {
            hsv(
                sample_curve(curve, parent_progress(time, start, duration)) / 360.0,
                1.0,
                1.0,
            )
        })
    }

    fn chase(
        &mut self,
        chase: &Chase,
        pixel_count: i32,
        revolutions: Option<i32>,
    ) -> &ChaseGeometry {
        if self
            .chase
            .as_ref()
            .is_none_or(|cached| cached.pixel_count != pixel_count)
        {
            self.chase = Some(ChaseGeometry::new(chase, pixel_count, revolutions));
        }
        self.chase.as_ref().unwrap()
    }
}

struct ChaseGeometry {
    pixel_count: i32,
    width: i32,
    position_denominator: f32,
    revolutions: Option<i32>,
    revolution_scale: f32,
    duration: f32,
    duration_scale: f32,
    start: f32,
    end: f32,
}

impl ChaseGeometry {
    fn new(chase: &Chase, pixel_count: i32, revolutions: Option<i32>) -> Self {
        let width = chase.section_width_pixels.max(1);
        let count = (1 + (pixel_count.max(1) - 1) / width) as f32;
        let revolutions = revolutions.map(|value| value.max(1));
        let revolution_scale = revolutions.map_or(0.0, |value| 1.0 / value as f32);
        let virtual_count = count * revolutions.unwrap_or(1) as f32;
        let duration = (chase.pulse_overlap.max(1.0) / virtual_count).max(1e-9);
        let start = if chase.extend_to_start {
            -duration
        } else {
            0.0
        };
        let end = if chase.extend_to_end {
            1.0
        } else {
            (1.0 - duration).max(0.0)
        };
        Self {
            pixel_count,
            width,
            position_denominator: (count - 1.0).max(1.0),
            revolutions,
            revolution_scale,
            duration,
            duration_scale: 1.0 / duration,
            start,
            end,
        }
    }
}

fn sample_chase(
    chase: &Chase,
    context: &RunContext,
    geometry: &ChaseGeometry,
) -> Result<Color, RuntimeError> {
    let position =
        context.pixel_index.div_euclid(geometry.width) as f32 / geometry.position_denominator;
    if let Some(revolutions) = geometry.revolutions {
        let mut level = 0.0;
        let mut gradient_position = 0.0;
        for revolution in 0..revolutions {
            let mut virtual_position = (revolution as f32 + position) * geometry.revolution_scale;
            if chase.reverse {
                virtual_position = 1.0 - virtual_position;
            }
            let hit = geometry.start
                + (geometry.end - geometry.start)
                    * chase
                        .chase_position
                        .crossing(virtual_position, virtual_position)?
                        .clamp(0.0, 1.0);
            let elapsed = context.progress - hit;
            if (0.0..=geometry.duration).contains(&elapsed) {
                let pulse_progress = elapsed * geometry.duration_scale;
                let candidate = sample_curve(&chase.pulse_shape, pulse_progress).clamp(0.0, 1.0);
                if candidate > level {
                    level = candidate;
                    gradient_position = match chase.gradient_mode {
                        GradientMode::ThroughEffect => context.progress,
                        GradientMode::AcrossItems => position,
                        GradientMode::PerPulse => pulse_progress,
                    };
                }
            }
        }
        return gradient_scaled(&chase.gradient, gradient_position.clamp(0.0, 1.0), level);
    }

    let chase_value = if chase.reverse {
        1.0 - position
    } else {
        position
    };
    let hit = geometry.start
        + (geometry.end - geometry.start)
            * chase
                .chase_position
                .crossing(chase_value, chase_value)?
                .clamp(0.0, 1.0);
    let pulse_progress = (context.progress - hit) / geometry.duration;
    let level = if (0.0..=1.0).contains(&pulse_progress) {
        sample_curve(&chase.pulse_shape, pulse_progress).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let gradient_position = match chase.gradient_mode {
        GradientMode::ThroughEffect => context.progress,
        GradientMode::AcrossItems => position,
        GradientMode::PerPulse => pulse_progress,
    };
    gradient_scaled(&chase.gradient, gradient_position.clamp(0.0, 1.0), level)
}

impl MarkPulseChild {
    fn sample(
        &self,
        c: &RunContext,
        sample_time: SampleTime,
        cache: &mut NativeSampleCache,
    ) -> Result<Color, RuntimeError> {
        let fade = self.section_edge_fade_pixels.max(0.0);
        let active = if fade > 0.0 {
            let width = self.section_width_pixels.max(1);
            let choice = c.pixel_index.div_euclid(width) as f32;
            let width = width as f32;
            let pixel = c.pixel_index as f32;
            let start = choice * width;
            let end = (start + width - 1.0).min(c.pixel_count as f32 - 1.0);
            ((pixel - start).min(end - pixel) / fade).clamp(0.0, 1.0)
        } else {
            1.0
        };
        let progress = c.progress.clamp(0.0, 1.0);
        let accent = sample_gradient(&self.accent, progress)
            .ok_or_else(|| error("cannot sample empty gradient"))?;
        let pulse = mix_colors(
            accent,
            cache.hue(
                &self.hue,
                sample_time,
                self.parent_start,
                self.parent_duration,
            ),
            self.hue_mix.clamp(0.0, 1.0),
        );
        Ok(mix_colors(self.base, pulse, active * (1.0 - progress)))
    }
}
impl MarkChaseChild {
    fn sample(
        &self,
        c: &RunContext,
        sample_time: SampleTime,
        cache: &mut NativeSampleCache,
    ) -> Result<Color, RuntimeError> {
        let span = self.pulse_overlap / (c.pixel_count as f32).max(1.0);
        let travel_start = -span;
        let travel_end = 1.0 + span;
        let chase = travel_start
            + (travel_end - travel_start)
                * sample_curve(&self.chase_position, c.progress).clamp(0.0, 1.0);
        let pulse_progress = (c.pixel_fraction - chase).abs() / span.max(1e-9);
        let level = sample_curve(&self.pulse_shape, pulse_progress.clamp(0.0, 1.0));
        let gp = match self.gradient_mode {
            GradientMode::ThroughEffect => c.progress,
            GradientMode::AcrossItems => c.pixel_fraction,
            GradientMode::PerPulse => {
                let width = self.section_width_pixels.max(1);
                c.pixel_index.rem_euclid(width) as f32 / width as f32
            }
        };
        let value = gradient_scaled(&self.gradient, gp.clamp(0.0, 1.0), level)?;
        let hue = cache.hue(
            &self.hue,
            sample_time,
            self.parent_start,
            self.parent_duration,
        );
        Ok(mix_colors(
            self.base,
            mix_colors(value, hue, self.hue_mix.clamp(0.0, 1.0)),
            level,
        ))
    }
}

fn gradient_scaled(g: &Gradient, p: f32, level: f32) -> Result<Color, RuntimeError> {
    sample_gradient(g, p)
        .map(|c| scale_color(c, level))
        .ok_or_else(|| error("cannot sample empty gradient"))
}
fn parent_progress(
    sample_time: SampleTime,
    parent_start: SampleTime,
    parent_duration: SampleDuration,
) -> f32 {
    let elapsed = sample_time
        .checked_duration_since(parent_start)
        .map_or(0, |duration| duration.as_ticks());
    (elapsed as f32 / parent_duration.as_ticks().max(1) as f32).clamp(0.0, 1.0)
}
pub fn parse_gradient_mode(value: &str) -> Result<GradientMode, RuntimeError> {
    match value {
        "through_effect" => Ok(GradientMode::ThroughEffect),
        "across_items" => Ok(GradientMode::AcrossItems),
        "per_pulse" => Ok(GradientMode::PerPulse),
        _ => Err(error("unsupported gradient mode")),
    }
}
fn error(message: impl Into<String>) -> RuntimeError {
    RuntimeError {
        message: message.into(),
    }
}
