use crate::composition::{Composition, CompositionGraphNodeId};
use crate::effect::{Clip, ClipContent, ClipId, Placement};
pub(crate) use crate::identity::SequenceId;
use donder_language::{DonderDuration, DonderTime};
use donder_runtime_types::Identifier;
use donder_runtime_types::sampling::sample_curve;
use donder_runtime_types::{Curve, CurvePoint};

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub description: Option<String>,
    pub duration: DonderDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub composition: Composition,
}

impl Sequence {
    pub(crate) fn frame_count(&self) -> u128 {
        (self.duration.as_nanos() * u128::from(self.frame_rate))
            .div_ceil(u128::from(donder_language::NANOS_PER_SECOND))
    }
}

/// A curve that drives the parameters it binds while its clip plays.
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub curve: Curve,
    pub bindings: Vec<Binding>,
}

/// Well under one frame at any supported frame rate.
const CROP_TOLERANCE_SECONDS: f64 = 1e-4;

/// Envelope curves hold normalized values that parameters map onto their
/// ranges.
pub(crate) fn curve_is_normalized(curve: &Curve) -> bool {
    curve
        .points
        .iter()
        .all(|point| (0.0..=1.0).contains(&point.value))
}

/// The part of a normalized `0..1` value a binding uses: 0 lands on `low` and
/// 1 on `high`, so a window of `0.0..0.7` caps the parameter at 70% of its
/// range and `low > high` inverts it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Window {
    pub low: f32,
    pub high: f32,
}

impl Window {
    pub const FULL: Self = Self {
        low: 0.0,
        high: 1.0,
    };

    pub fn is_normalized(self) -> bool {
        (0.0..=1.0).contains(&self.low) && (0.0..=1.0).contains(&self.high)
    }

    pub fn apply(self, value: f32) -> f32 {
        self.low + (self.high - self.low) * value
    }

    fn apply_to_curve(self, curve: &Curve) -> Curve {
        Curve {
            points: curve
                .points
                .iter()
                .map(|point| CurvePoint {
                    position: point.position,
                    value: self.apply(point.value),
                })
                .collect(),
        }
    }
}

impl Clip {
    fn end(&self) -> Option<core::time::Duration> {
        self.timing()
            .map(|(start, duration)| start.0.saturating_add(duration.0))
    }

    /// Whether both are timed and play at once.
    pub fn overlaps(&self, other: &Clip) -> bool {
        match (self.timing(), other.timing(), self.end(), other.end()) {
            (Some((start, _)), Some((other_start, _)), Some(end), Some(other_end)) => {
                start.0 < other_end && other_start.0 < end
            }
            _ => false,
        }
    }

    /// Move a timed clip's window over fixed content: an envelope's points
    /// outside the window are dropped, and an edge that cuts through its curve
    /// gains a point holding the value it cut through.
    pub fn crop(&mut self, start: DonderTime, duration: DonderDuration) {
        let Placement::Timed {
            start: old_start,
            duration: old_duration,
        } = &mut self.placement
        else {
            return;
        };
        if let ClipContent::Envelope(envelope) = &mut self.content {
            envelope.curve = crop_curve(
                &envelope.curve,
                (old_start.0.as_secs_f64(), old_duration.0.as_secs_f64()),
                (start.0.as_secs_f64(), duration.0.as_secs_f64()),
            );
        }
        *old_start = start;
        *old_duration = duration;
    }

    /// Keep the part of a timed clip before `at` and return the part after it
    /// as clip `id`.
    pub fn split_off(&mut self, at: DonderTime, id: ClipId) -> Option<Clip> {
        let (start, end) = (self.timing()?.0.clone(), self.end()?);
        if at.0 <= start.0 || at.0 >= end {
            return None;
        }
        let mut right = self.clone();
        right.id = id;
        right.crop(at.clone(), DonderDuration(end - at.0));
        self.crop(start.clone(), DonderDuration(at.0 - start.0));
        Some(right)
    }
}

impl Envelope {
    /// The binding of `target`, when the envelope drives it.
    pub fn binding(&self, target: &AutomationTarget) -> Option<&Binding> {
        self.bindings
            .iter()
            .find(|binding| &binding.target == target)
    }

    /// Drive `target` through `window`, in place of any binding it had.
    pub fn bind(&mut self, target: AutomationTarget, window: Window) {
        self.bindings.retain(|binding| binding.target != target);
        self.bindings.push(Binding { target, window });
    }

    /// The curve as `target` sees it, through the window of its binding.
    fn windowed_curve(&self, target: &AutomationTarget) -> Option<Curve> {
        Some(self.binding(target)?.window.apply_to_curve(&self.curve))
    }
}

/// `curve` over the window `(start, duration)`, in seconds, cropped to the
/// window `to`. GUI timing passes through f32 seconds, so an edge that did not
/// move can shift slightly: points this close to an edge stay inside it.
fn crop_curve(curve: &Curve, (old_start, old_duration): (f64, f64), to: (f64, f64)) -> Curve {
    let (new_start, new_duration) = to;
    let new_end = new_start + new_duration;
    let (inner_start, inner_end) = (
        new_start - CROP_TOLERANCE_SECONDS,
        new_end + CROP_TOLERANCE_SECONDS,
    );
    let time = |position: f32| old_start + f64::from(position) * old_duration;
    let value_at =
        |seconds: f64| sample_curve(curve, ((seconds - old_start) / old_duration) as f32);
    let cut_left = curve
        .points
        .iter()
        .any(|point| time(point.position) < inner_start);
    let cut_right = curve
        .points
        .iter()
        .any(|point| time(point.position) > inner_end);
    let kept = curve
        .points
        .iter()
        .map(|point| (time(point.position), point.value))
        .filter(|(seconds, _)| (inner_start..=inner_end).contains(seconds))
        .collect::<Vec<_>>();
    let mut points = Vec::with_capacity(kept.len() + 2);
    if cut_left && kept.first().is_none_or(|(seconds, _)| *seconds > new_start) {
        points.push((new_start, value_at(new_start)));
    }
    points.extend(kept.iter().copied());
    if cut_right && kept.last().is_none_or(|(seconds, _)| *seconds < new_end) {
        points.push((new_end, value_at(new_end)));
    }
    Curve {
        points: points
            .into_iter()
            .map(|(seconds, value)| CurvePoint {
                position: ((seconds - new_start) / new_duration).clamp(0.0, 1.0) as f32,
                value,
            })
            .collect(),
    }
}

/// Every envelope bound to one target, merged into a single curve.
pub struct AutomationEnvelope {
    pub(crate) start: DonderTime,
    pub(crate) duration: DonderDuration,
    pub(crate) curve: Curve,
}

impl Composition {
    /// The timed envelopes bound to `target`, merged. Envelopes bound to one
    /// target never overlap. Before the first, the merged curve holds its
    /// first value, and each gap holds the value the previous one ended on.
    pub fn automation_envelope(&self, target: &AutomationTarget) -> Option<AutomationEnvelope> {
        // Each envelope's timing, with its curve through its own window.
        let mut clips = self
            .clips
            .iter()
            .filter_map(|clip| {
                let (start, duration) = clip.timing()?;
                Some((start, duration, clip.envelope()?.windowed_curve(target)?))
            })
            .collect::<Vec<_>>();
        if let [(start, duration, curve)] = clips.as_slice() {
            return Some(AutomationEnvelope {
                start: (*start).clone(),
                duration: (*duration).clone(),
                curve: curve.clone(),
            });
        }
        clips.sort_by_key(|(start, ..)| start.0);
        let start = clips.first()?.0.0;
        let end = clips
            .iter()
            .map(|(start, duration, _)| start.0.saturating_add(duration.0))
            .max()?;
        let span = (end - start).as_secs_f64();
        let position =
            |seconds: f64| ((seconds - start.as_secs_f64()) / span).clamp(0.0, 1.0) as f32;
        let last = clips.len() - 1;
        let mut points = Vec::new();
        let mut held = None;
        for (index, (clip_start, clip_duration, curve)) in clips.iter().enumerate() {
            if curve.points.is_empty() {
                continue;
            }
            let clip_start = clip_start.0.as_secs_f64();
            let clip_duration = clip_duration.0.as_secs_f64();
            if index > 0 {
                let start_position = position(clip_start);
                if let Some(value) = held {
                    points.push(CurvePoint {
                        position: start_position,
                        value,
                    });
                }
                points.push(CurvePoint {
                    position: start_position,
                    value: sample_curve(curve, 0.0),
                });
            }
            points.extend(curve.points.iter().map(|point| CurvePoint {
                position: position(clip_start + f64::from(point.position) * clip_duration),
                value: point.value,
            }));
            if index < last {
                let value = sample_curve(curve, 1.0);
                points.push(CurvePoint {
                    position: position(clip_start + clip_duration),
                    value,
                });
                held = Some(value);
            }
        }
        Some(AutomationEnvelope {
            start: DonderTime(start),
            duration: DonderDuration(end - start),
            curve: Curve { points },
        })
    }
}

impl AutomationEnvelope {
    /// The envelope over a target's time range, with target-relative positions.
    /// Preserve coincident points: they encode steps, and the last point wins at a boundary.
    pub fn curve_in_range(&self, start: &DonderTime, duration: &DonderDuration) -> Curve {
        let envelope_duration = self.duration.as_seconds_f64().max(f64::EPSILON);
        let range_duration = duration.as_seconds_f64().max(f64::EPSILON);
        let start_position =
            (start.as_seconds_f64() - self.start.as_seconds_f64()) / envelope_duration;
        let end_position = start_position + range_duration / envelope_duration;
        let mut points = vec![CurvePoint {
            position: 0.0,
            value: sample_curve(&self.curve, start_position as f32),
        }];
        points.extend(self.curve.points.iter().filter_map(|point| {
            let position = ((f64::from(point.position) - start_position) * envelope_duration
                / range_duration) as f32;
            (position > 0.0 && position <= 1.0).then_some(CurvePoint {
                position,
                value: point.value,
            })
        }));
        if points.last().is_none_or(|point| point.position < 1.0) {
            points.push(CurvePoint {
                position: 1.0,
                value: sample_curve(&self.curve, end_position as f32),
            });
        }
        Curve { points }
    }
}

/// An envelope's drive of one parameter, through a window of its curve.
#[derive(Clone, Debug, PartialEq)]
pub struct Binding {
    pub target: AutomationTarget,
    pub window: Window,
}

/// A parameter an envelope drives or a console's `Set` sets: of a clip, or of
/// an operator node of the composition graph.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AutomationTarget {
    ClipParam {
        clip_id: ClipId,
        param: Identifier,
    },
    CompositionNodeParam {
        node_id: CompositionGraphNodeId,
        param: Identifier,
    },
}

impl AutomationTarget {
    pub fn param(&self) -> &Identifier {
        match self {
            Self::ClipParam { param, .. } | Self::CompositionNodeParam { param, .. } => param,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);
