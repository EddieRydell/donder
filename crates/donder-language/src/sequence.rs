use crate::dsl::types::Identifier;
use crate::effect::{EffectInst, EffectInstId};
use crate::identity::SourceIdentity;
use crate::operator::GraphOperatorNode;
use crate::values::{Color, Curve, DonderDuration, DonderTime};
pub use donder_runtime::automation::{
    AutomationMapping, AutomationValue,
    automation_value_at_position as automation_mapping_value_at_position,
    curve_window_into as curve_window_into_at_position,
};

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct SequenceId(pub SourceIdentity);

#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    pub id: SequenceId,
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
    pub name: String,
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
    pub name: String,
    pub display_color: Color,
    pub marks: Vec<DonderTime>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct MarkCollectionKey {
    pub name: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationClip {
    pub id: AutomationClipId,
    pub start: DonderTime,
    pub duration: DonderDuration,
    pub anchor_lane_index: u32,
    pub lane_index: u32,
    pub curve: Curve,
    pub bindings: Vec<AutomationBinding>,
    pub detached_bindings: Vec<DetachedAutomationBinding>,
}

impl AutomationClip {
    /// The authored envelope over a target's time range, with target-relative positions.
    /// Preserve coincident points: they encode steps, and the last point wins at a boundary.
    pub fn curve_in_range(&self, start: &DonderTime, duration: &DonderDuration) -> Curve {
        let clip_duration = self.duration.as_seconds_f32().max(f32::EPSILON);
        let range_duration = duration.as_seconds_f32().max(f32::EPSILON);
        let start_position = (start.as_seconds_f32() - self.start.as_seconds_f32()) / clip_duration;
        let end_position = start_position + range_duration / clip_duration;
        let mut points = vec![crate::values::CurvePoint {
            position: 0.0,
            value: donder_runtime::sampling::sample_curve(&self.curve, start_position),
        }];
        points.extend(self.curve.points.iter().filter_map(|point| {
            let position = (point.position - start_position) * clip_duration / range_duration;
            (position > 0.0 && position <= 1.0).then_some(crate::values::CurvePoint {
                position,
                value: point.value,
            })
        }));
        if points.last().is_none_or(|point| point.position < 1.0) {
            points.push(crate::values::CurvePoint {
                position: 1.0,
                value: donder_runtime::sampling::sample_curve(&self.curve, end_position),
            });
        }
        Curve { points }
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
                    mapping: binding.mapping,
                    reason: reason.clone(),
                });
            } else {
                retained.push(binding);
            }
        }
        self.bindings = retained;
    }

    pub fn bind(&mut self, target: AutomationTarget, mapping: AutomationMapping) {
        self.detached_bindings
            .retain(|binding| binding.target != target);
        self.bindings.push(AutomationBinding { target, mapping });
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AutomationClipId(pub u32);

#[derive(Clone, Debug, PartialEq)]
pub struct AutomationBinding {
    pub target: AutomationTarget,
    pub mapping: AutomationMapping,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetachedAutomationBinding {
    pub target: AutomationTarget,
    pub mapping: AutomationMapping,
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

pub fn automation_value_at<'a>(
    clip: &AutomationClip,
    binding: &'a AutomationBinding,
    sample_seconds: f32,
) -> Option<AutomationValue<'a>> {
    automation_mapping_value_at(clip, &binding.mapping, sample_seconds)
}

pub fn automation_mapping_value_at<'a>(
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
