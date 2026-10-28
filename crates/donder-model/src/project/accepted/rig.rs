//! Playback invocations of a rig's effects and operators, admitted with the
//! project like a sequence's. A rig's live mark collections start empty and
//! are rebound during playback; each `Control` mapping, and each value a
//! trigger's presses set, is an automation binding on its effect whose value
//! playback sets.
use super::inputs::{invalid, resolve_values};
use super::parameters::{EffectParamTiming, MarkSource};
use super::*;
use crate::effect::EffectParamValue;
use crate::rig::{ControlTarget, MappingAction, Rig, RigEffect, RigId, RigMappingId};
use crate::sequence::{CompositionGraphNodeId, MarkCollectionKey};
use crate::validation::{AdmissionProblem, Member, ProjectValidationError, ValidationProblem, at};
use donder_runtime_types::{
    AutomatedQuantity, Curve, CurvePoint, PreparedAutomation, SampleDuration, SampleTime,
    SequenceTiming,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::Arc;

/// A rig session's length: the playback clock's range.
pub const RIG_SESSION: SampleDuration = SampleDuration::from_ticks(u32::MAX);

#[derive(Debug)]
pub(super) struct RigInputs {
    pub(super) timing: SequenceTiming,
    /// In [`Rig::effects`] order.
    pub(super) effects: Box<[RigEffectInputs]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, Invocation>,
}

#[derive(Debug)]
pub(super) struct RigEffectInputs {
    pub(super) invocation: Invocation,
    /// The live collection each live marks parameter reads, in the order the
    /// invocation lists them.
    pub(super) live: Box<[MarkCollectionKey]>,
    /// What sets each of the invocation's automation bindings, in binding
    /// order.
    pub(super) controls: Box<[RigBinding]>,
}

/// What sets an automation binding of a rig effect.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RigBinding {
    /// A `Control` mapping, for every voice of the effect.
    Control(RigMappingId),
    /// The trigger's `index`th played value, for the voice each press starts.
    Played(usize),
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
    let effects = rig
        .effects()
        .zip(targets)
        .map(|(effect, target)| {
            admit_effect(
                project,
                effect,
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
            .any(|value| matches!(value, EffectParamValue::Marks(Some(_))))
        {
            return Err(invalid(AdmissionProblem::LiveMarksInOperator(
                node.id.clone(),
            )));
        }
        let id = &operator.operator;
        let definition = &project.reusable::<crate::OperatorDefinition>()[id];
        let values = resolve_values(
            project,
            MarkSource::Live,
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
        effects,
        operators,
    })
}

fn admit_effect(
    project: &DonderProject,
    effect: &RigEffect,
    member: Member,
    controls: &[(RigBinding, &donder_runtime_types::Identifier)],
) -> Result<RigEffectInputs, ValidationProblem> {
    let id = &effect.definition;
    let definition = &project.reusable::<crate::EffectDefinition>()[id];
    let params = definition.params();
    let values = resolve_values(
        project,
        MarkSource::Live,
        params,
        &effect.param_overrides,
        session(),
    )?;
    let mut live_params = Vec::new();
    let mut live = Vec::new();
    for (index, param) in params.iter().enumerate() {
        if let Some(EffectParamValue::Marks(Some(key))) = effect.param_overrides.get(&param.name) {
            live_params.push(param_index(index)?);
            live.push(key.clone());
        }
    }
    let automation = controls
        .iter()
        .map(|(_, name)| {
            let index = params
                .iter()
                .position(|param| &param.name == *name)
                .ok_or_else(|| invalid(AdmissionProblem::UnknownControlParam((*name).clone())))?;
            let mapping = params[index]
                .automation_mapping()
                .ok_or_else(|| invalid(AdmissionProblem::ControlNotAutomatable((*name).clone())))?;
            Ok(PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: RIG_SESSION,
                // Every control starts at the bottom of its range, released,
                // and a played value is set before its voice sounds.
                curve: Arc::new(Curve {
                    points: vec![CurvePoint {
                        position: 0.0,
                        value: 0.0,
                    }],
                }),
                mapping,
                quantity: AutomatedQuantity::Value,
                param_index: param_index(index)?,
            })
        })
        .collect::<Result<Box<[_]>, ValidationProblem>>()?;
    let compiled = definition.compiled();
    let invocation = compiled
        .invoke_live(values.into_vec(), automation, live_params.into())
        .map_err(|error| invalid(AdmissionProblem::Binding(member, error)))?;
    Ok(RigEffectInputs {
        invocation,
        live: live.into(),
        controls: controls.iter().map(|(binding, _)| *binding).collect(),
    })
}

fn param_index(index: usize) -> Result<u16, ValidationProblem> {
    u16::try_from(index).map_err(|_| invalid(AdmissionProblem::TooManyParams))
}

fn session() -> EffectParamTiming {
    EffectParamTiming {
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
