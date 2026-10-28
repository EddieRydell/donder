//! Playback invocations of a rig's clips and operators, admitted with the
//! project like a sequence's. Each `Control` mapping, each value a trigger's
//! presses set, and each marks parameter reading a live collection is an
//! automation binding on its clip that playback sets.
use super::inputs::{invalid, resolve_values};
use super::parameters::{ClipTiming, MarkOrigin};
use super::*;
use crate::effect::ParamValue;
use crate::rig::{ControlTarget, MappingAction, Rig, RigClip, RigId, RigMappingId};
use crate::sequence::CompositionGraphNodeId;
use crate::validation::{AdmissionProblem, Member, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, Curve, CurvePoint, PreparedAutomation, SampleDuration,
    SampleTime, SequenceTiming,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::Arc;

/// A rig session's length: the playback clock's range.
pub const RIG_SESSION: SampleDuration = SampleDuration::from_ticks(u32::MAX);

#[derive(Debug)]
pub(super) struct RigInputs {
    pub(super) timing: SequenceTiming,
    /// In [`Rig::clips`] order.
    pub(super) clips: Box<[RigClipInputs]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, Invocation>,
}

#[derive(Debug)]
pub(super) struct RigClipInputs {
    pub(super) invocation: Invocation,
    /// What sets each of the invocation's automation bindings, in binding
    /// order.
    pub(super) bindings: Box<[RigBinding]>,
}

/// What sets an automation binding of a rig clip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RigBinding {
    /// A `Control` mapping, for every voice of the clip.
    Control(RigMappingId),
    /// The trigger's `index`th played value, for the voice each press starts.
    Played(usize),
    /// The rig's `index`th live mark collection, for every voice of the clip.
    LiveMarks(usize),
}

pub(super) fn admit(
    project: &DonderProject,
) -> Result<IndexMap<RigId, RigInputs>, ProjectValidationError> {
    project
        .objects::<crate::Rig>()
        .map(|rig| Ok((rig.id, admit_rig(project, rig).map_err(at(rig.id))?)))
        .collect()
}

fn admit_rig(project: &DonderProject, rig: &Rig) -> Result<RigInputs, ValidationProblem> {
    let bindings = rig
        .mappings
        .iter()
        .flat_map(|mapping| match &mapping.action {
            MappingAction::Control { target, param } => {
                vec![(*target, RigBinding::Control(mapping.id), param)]
            }
            MappingAction::Trigger(trigger) => trigger
                .values
                .iter()
                .enumerate()
                .map(|(index, value)| {
                    (
                        ControlTarget::Trigger(mapping.id),
                        RigBinding::Played(index),
                        &value.param,
                    )
                })
                .collect(),
            MappingAction::Mark(_) => Vec::new(),
        });
    let mut controls: IndexMap<
        ControlTarget,
        Vec<(RigBinding, &donder_runtime_types::Identifier)>,
    > = IndexMap::new();
    for (target, binding, param) in bindings {
        let bound = controls.entry(target).or_default();
        if bound.iter().any(|(_, existing)| *existing == param) {
            return Err(invalid(AdmissionProblem::ControlSetTwice(param.clone())));
        }
        bound.push((binding, param));
    }
    let targets = rig
        .voices
        .iter()
        .map(|voice| ControlTarget::Voice(voice.id))
        .chain(rig.mappings.iter().filter_map(|mapping| {
            matches!(mapping.action, MappingAction::Trigger(_))
                .then_some(ControlTarget::Trigger(mapping.id))
        }));
    let clips = rig
        .clips()
        .zip(targets)
        .map(|(clip, target)| {
            admit_clip(
                project,
                &rig.live_marks,
                clip,
                match target {
                    ControlTarget::Voice(voice) => Member::Voice(voice),
                    ControlTarget::Trigger(mapping) => Member::Mapping(mapping),
                },
                controls.get(&target).map_or(&[][..], Vec::as_slice),
            )
        })
        .collect::<Result<_, _>>()?;
    let mut operators = IndexMap::new();
    for node in &rig.composition_graph.nodes {
        let CompositionGraphNodeKind::Operator(operator) = &node.kind else {
            continue;
        };
        if operator
            .params
            .values()
            .any(|value| matches!(value, ParamValue::Marks(Some(_))))
        {
            return Err(invalid(AdmissionProblem::LiveMarksInOperator(
                node.id.clone(),
            )));
        }
        let id = &operator.operator;
        let definition = &project.reusable::<crate::OperatorDefinition>()[id];
        let values = resolve_values(
            project,
            MarkOrigin::Live,
            definition.params(),
            &operator.params,
            session(),
        )?;
        let compiled = definition.compiled();
        let invocation = compiled
            .invoke(values.into_vec(), Box::new([]))
            .map_err(|error| {
                invalid(AdmissionProblem::Binding(
                    Member::Node(node.id.clone()),
                    error,
                ))
            })?;
        operators.insert(node.id.clone(), invocation);
    }
    Ok(RigInputs {
        timing: timing(rig)?,
        clips,
        operators,
    })
}

fn admit_clip(
    project: &DonderProject,
    live_marks: &[crate::rig::LiveMarks],
    clip: &RigClip,
    member: Member,
    controls: &[(RigBinding, &donder_runtime_types::Identifier)],
) -> Result<RigClipInputs, ValidationProblem> {
    let id = &clip.definition;
    let definition = &project.reusable::<crate::EffectDefinition>()[id];
    let params = definition.params();
    let values = resolve_values(
        project,
        MarkOrigin::Live,
        params,
        &clip.param_overrides,
        session(),
    )?;
    let mut bindings: Vec<RigBinding> = controls.iter().map(|(binding, _)| *binding).collect();
    let mut automation = controls
        .iter()
        .map(|(_, name)| {
            let index = params
                .iter()
                .position(|param| &param.name == *name)
                .ok_or_else(|| invalid(AdmissionProblem::UnknownControlParam((*name).clone())))?;
            let mapping = params[index]
                .automation_mapping()
                .ok_or_else(|| invalid(AdmissionProblem::ControlNotAutomatable((*name).clone())))?;
            Ok(session_binding(mapping, param_index(index)?))
        })
        .collect::<Result<Vec<_>, ValidationProblem>>()?;
    for (index, param) in params.iter().enumerate() {
        if let Some(ParamValue::Marks(Some(key))) = clip.param_overrides.get(&param.name) {
            let collection = live_marks
                .iter()
                .position(|marks| marks.key == *key)
                .unwrap_or_else(|| unreachable!("validation resolves live mark collections"));
            automation.push(session_binding(
                AutomationMapping::LiveMarks,
                param_index(index)?,
            ));
            bindings.push(RigBinding::LiveMarks(collection));
        }
    }
    let compiled = definition.compiled();
    let invocation = compiled
        .invoke(values.into_vec(), automation.into())
        .map_err(|error| invalid(AdmissionProblem::Binding(member, error)))?;
    Ok(RigClipInputs {
        invocation,
        bindings: bindings.into(),
    })
}

/// A binding over the whole session. Every control starts at the bottom of
/// its range, released, and a played value is set before its voice sounds.
fn session_binding(mapping: AutomationMapping, param_index: u16) -> PreparedAutomation {
    PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: RIG_SESSION,
        curve: Arc::new(Curve {
            points: vec![CurvePoint {
                position: 0.0,
                value: 0.0,
            }],
        }),
        mapping,
        quantity: AutomatedQuantity::Value,
        param_index,
    }
}

fn param_index(index: usize) -> Result<u16, ValidationProblem> {
    u16::try_from(index).map_err(|_| invalid(AdmissionProblem::TooManyParams))
}

fn session() -> ClipTiming {
    ClipTiming {
        start: SampleTime::from_ticks(0),
        duration: RIG_SESSION,
    }
}

fn timing(rig: &Rig) -> Result<SequenceTiming, ValidationProblem> {
    let admit = || {
        let frame_rate = NonZeroU32::new(rig.frame_rate)?;
        let frame_count = NonZeroU32::new(
            u32::try_from(
                (u64::from(RIG_SESSION.as_ticks()) * u64::from(rig.frame_rate)).div_ceil(1_000_000),
            )
            .ok()?,
        )?;
        let duration = NonZeroU32::new(RIG_SESSION.as_ticks())?;
        Some(SequenceTiming::new(frame_rate, frame_count, duration))
    };
    admit().ok_or_else(|| invalid(AdmissionProblem::TimingUnrepresentable))
}
