pub use crate::automation::{
    AutomationMapping, AutomationValue,
    automation_value_at_position as automation_mapping_value_at_position,
    curve_window_into as curve_window_into_at_position,
};
use crate::dsl::types::Identifier;
use crate::effect::{EffectInst, EffectInstId};
use crate::identity::ObjectIdentity;
use crate::operator::GraphOperatorNode;
use crate::sampling::sample_curve;
use crate::values::{Color, Curve, CurvePoint, DonderDuration, DonderTime};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceId(pub ObjectIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub id: SequenceId,
    pub description: Option<String>,
    pub duration: DonderDuration,
    pub frame_rate: u32,
    pub audio: SequenceAudio,
    pub mark_collections: Vec<MarkCollection>,
    pub layers: Vec<SequenceLayer>,
    pub effects: Vec<EffectInst>,
    pub composition_graph: SequenceCompositionGraph,
    pub automation_clips: Vec<AutomationClip>,
}

impl Sequence {
    pub fn frame_count(&self) -> u128 {
        (self.duration.as_nanos() * u128::from(self.frame_rate))
            .div_ceil(u128::from(crate::values::NANOS_PER_SECOND))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceLayerId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceLayer {
    pub id: SequenceLayerId,
    /// Unique among the sequence's layers and graph nodes.
    pub name: Identifier,
    pub description: Option<String>,
    pub color: Color,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SequenceCompositionGraph {
    pub nodes: Vec<CompositionGraphNode>,
    pub edges: Vec<EffectGraphEdge>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompositionGraphNode {
    pub id: CompositionGraphNodeId,
    pub position: GraphNodePosition,
    pub kind: CompositionGraphNodeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CompositionGraphNodeKind {
    Layer { layer_id: SequenceLayerId },
    Operator(GraphOperatorNode),
    Output,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, PartialOrd, Ord)]
pub struct CompositionGraphNodeId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct GraphNodePosition {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EffectGraphEdge {
    pub from: CompositionGraphNodeId,
    pub from_port: GraphPortId,
    pub to: CompositionGraphNodeId,
    pub to_port: GraphPortId,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct GraphPortId(pub String);

#[derive(Clone, Debug, PartialEq)]
pub struct MarkCollection {
    pub key: MarkCollectionKey,
    pub description: Option<String>,
    pub display_color: Color,
    pub marks: Vec<DonderTime>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MarkCollectionKey {
    pub name: Identifier,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationClip {
    pub id: AutomationClipId,
    pub start: DonderTime,
    pub duration: DonderDuration,
    /// Visual placement, independent of parameter bindings.
    pub row_target: crate::layout::FixtureTarget,
    pub curve: Curve,
    pub bindings: Vec<AutomationBinding>,
    pub detached_bindings: Vec<DetachedAutomationBinding>,
}

/// Well under one frame at any supported frame rate.
const CROP_TOLERANCE_SECONDS: f64 = 1e-4;

impl AutomationClip {
    pub fn end(&self) -> core::time::Duration {
        self.start.0.saturating_add(self.duration.0)
    }

    pub fn overlaps(&self, other: &AutomationClip) -> bool {
        self.start.0 < other.end() && other.start.0 < self.end()
    }

    /// Active and detached binding targets.
    pub fn targets(&self) -> impl Iterator<Item = &AutomationTarget> {
        self.bindings
            .iter()
            .map(|binding| &binding.target)
            .chain(self.detached_bindings.iter().map(|binding| &binding.target))
    }

    /// Move the clip window over fixed content. Points outside the window are
    /// dropped, and an edge that cuts through content gains a point holding the
    /// value it cut through.
    pub fn crop(&mut self, start: DonderTime, duration: DonderDuration) {
        let old_start = self.start.0.as_secs_f64();
        let old_duration = self.duration.0.as_secs_f64();
        let new_start = start.0.as_secs_f64();
        let new_duration = duration.0.as_secs_f64();
        let new_end = new_start + new_duration;
        // GUI timing passes through f32 seconds, so an edge that did not move can
        // shift slightly. Points this close to an edge stay inside it.
        let (inner_start, inner_end) = (
            new_start - CROP_TOLERANCE_SECONDS,
            new_end + CROP_TOLERANCE_SECONDS,
        );
        let time = |position: f32| old_start + f64::from(position) * old_duration;
        let value_at =
            |seconds: f64| sample_curve(&self.curve, ((seconds - old_start) / old_duration) as f32);
        let cut_left = self
            .curve
            .points
            .iter()
            .any(|point| time(point.position) < inner_start);
        let cut_right = self
            .curve
            .points
            .iter()
            .any(|point| time(point.position) > inner_end);
        let kept = self
            .curve
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
        self.curve.points = points
            .into_iter()
            .map(|(seconds, value)| CurvePoint {
                position: ((seconds - new_start) / new_duration).clamp(0.0, 1.0) as f32,
                value,
            })
            .collect();
        self.start = start;
        self.duration = duration;
    }

    /// Keep the part before `at` and return the part after it as clip `id`.
    pub fn split_off(&mut self, at: DonderTime, id: AutomationClipId) -> Option<AutomationClip> {
        let end = self.end();
        if at.0 <= self.start.0 || at.0 >= end {
            return None;
        }
        let mut right = self.clone();
        right.id = id;
        right.crop(at.clone(), DonderDuration(end - at.0));
        self.crop(self.start.clone(), DonderDuration(at.0 - self.start.0));
        Some(right)
    }

    pub fn detach_bindings(
        &mut self,
        reason: AutomationDetachmentReason,
        matches: impl Fn(&AutomationTarget) -> bool,
    ) {
        let mut retained = Vec::with_capacity(self.bindings.len());
        for binding in self.bindings.drain(..) {
            if matches(&binding.target) {
                self.detached_bindings.push(DetachedAutomationBinding {
                    target: binding.target,
                    reason: reason.clone(),
                });
            } else {
                retained.push(binding);
            }
        }
        self.bindings = retained;
    }

    pub fn bind(&mut self, target: AutomationTarget) {
        self.detached_bindings
            .retain(|binding| binding.target != target);
        self.bindings.push(AutomationBinding { target });
    }
}

/// Every clip actively bound to one target, merged into a single curve.
pub struct AutomationEnvelope {
    pub start: DonderTime,
    pub duration: DonderDuration,
    pub curve: Curve,
}

impl Sequence {
    /// Clips bound to one target never overlap. Before the first clip the
    /// envelope holds its first value, and each gap holds the value the
    /// previous clip ended on.
    pub fn automation_envelope(&self, target: &AutomationTarget) -> Option<AutomationEnvelope> {
        let mut clips = self
            .automation_clips
            .iter()
            .filter(|clip| {
                clip.bindings
                    .iter()
                    .any(|binding| &binding.target == target)
            })
            .collect::<Vec<_>>();
        if let [clip] = clips.as_slice() {
            return Some(AutomationEnvelope {
                start: clip.start.clone(),
                duration: clip.duration.clone(),
                curve: clip.curve.clone(),
            });
        }
        clips.sort_by_key(|clip| clip.start.0);
        let start = clips.first()?.start.0;
        let end = clips.iter().map(|clip| clip.end()).max()?;
        let span = (end - start).as_secs_f64();
        let position =
            |seconds: f64| ((seconds - start.as_secs_f64()) / span).clamp(0.0, 1.0) as f32;
        let last = clips.len() - 1;
        let mut points = Vec::new();
        let mut held = None;
        for (index, clip) in clips.iter().enumerate() {
            if clip.curve.points.is_empty() {
                continue;
            }
            let clip_start = clip.start.0.as_secs_f64();
            let clip_duration = clip.duration.0.as_secs_f64();
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
                    value: sample_curve(&clip.curve, 0.0),
                });
            }
            points.extend(clip.curve.points.iter().map(|point| CurvePoint {
                position: position(clip_start + f64::from(point.position) * clip_duration),
                value: point.value,
            }));
            if index < last {
                let value = sample_curve(&clip.curve, 1.0);
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
        let envelope_duration = self.duration.as_seconds_f32().max(f32::EPSILON);
        let range_duration = duration.as_seconds_f32().max(f32::EPSILON);
        let start_position =
            (start.as_seconds_f32() - self.start.as_seconds_f32()) / envelope_duration;
        let end_position = start_position + range_duration / envelope_duration;
        let mut points = vec![CurvePoint {
            position: 0.0,
            value: sample_curve(&self.curve, start_position),
        }];
        points.extend(self.curve.points.iter().filter_map(|point| {
            let position = (point.position - start_position) * envelope_duration / range_duration;
            (position > 0.0 && position <= 1.0).then_some(CurvePoint {
                position,
                value: point.value,
            })
        }));
        if points.last().is_none_or(|point| point.position < 1.0) {
            points.push(CurvePoint {
                position: 1.0,
                value: sample_curve(&self.curve, end_position),
            });
        }
        Curve { points }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationBinding {
    pub target: AutomationTarget,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetachedAutomationBinding {
    pub target: AutomationTarget,
    pub reason: AutomationDetachmentReason,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AutomationDetachmentReason {
    TargetDeleted,
    DefinitionChanged,
}

impl AutomationBinding {
    pub fn effect_param(&self) -> Option<(&EffectInstId, &Identifier)> {
        match &self.target {
            AutomationTarget::EffectParam { effect_id, param } => Some((effect_id, param)),
            AutomationTarget::CompositionNodeParam { .. } => None,
        }
    }

    pub fn composition_node_param(&self) -> Option<(&CompositionGraphNodeId, &Identifier)> {
        match &self.target {
            AutomationTarget::CompositionNodeParam { node_id, param } => Some((node_id, param)),
            AutomationTarget::EffectParam { .. } => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum AutomationTarget {
    EffectParam {
        effect_id: EffectInstId,
        param: Identifier,
    },
    CompositionNodeParam {
        node_id: CompositionGraphNodeId,
        param: Identifier,
    },
}

/// `mapping` comes from the target parameter declaration.
pub fn automation_value_at<'a>(
    clip: &AutomationClip,
    mapping: &'a AutomationMapping,
    sample_seconds: f32,
) -> Option<AutomationValue<'a>> {
    let duration = clip.duration.as_seconds_f32();
    let position = if duration <= 0.0 {
        0.0
    } else {
        ((sample_seconds - clip.start.as_seconds_f32()) / duration).clamp(0.0, 1.0)
    };
    automation_mapping_value_at_position(&clip.curve, mapping, position)
}

pub fn curve_window_into(
    output: &mut Curve,
    clip: &AutomationClip,
    min: f32,
    max: f32,
    sample_seconds: f32,
) {
    let duration = clip.duration.as_seconds_f32().max(f32::EPSILON);
    let sample_position =
        ((sample_seconds - clip.start.as_seconds_f32()) / duration).clamp(0.0, 1.0);
    curve_window_into_at_position(output, &clip.curve, min, max, sample_position);
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SequenceAudio {
    None,
    Asset(AssetId),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct AssetId(pub u32);

impl crate::ownership::Identified for Sequence {
    type Id = SequenceId;
    fn id(&self) -> &Self::Id {
        &self.id
    }
}

pub type SequenceSource = crate::ownership::ValueSource<Box<Sequence>, SequenceId>;

impl AsRef<ObjectIdentity> for SequenceId {
    fn as_ref(&self) -> &ObjectIdentity {
        &self.0
    }
}
