//! Playback invocations of a rig's effects and operators, admitted with the
//! project like a sequence's. A rig's live mark collections start empty and
//! are rebound during playback; each `Control` mapping is an automation
//! binding on its effect whose value playback sets.
use super::inputs::{invalid, resolve_values};
use super::parameters::{EffectParamTiming, MarkSource};
use super::*;
use crate::effect::{EffectImplementation, EffectParamValue, EffectRef};
use crate::operator::OperatorImplementation;
use crate::rig::{ControlTarget, MappingAction, Rig, RigEffect, RigId, RigMappingId};
use crate::sequence::{CompositionGraphNodeId, MarkCollectionKey};
use crate::validation::ProjectValidationError;
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
    /// The `Control` mapping each of the invocation's automation bindings
    /// answers, in binding order.
    pub(super) controls: Box<[RigMappingId]>,
}

pub(super) fn admit(
    project: &DonderProject,
) -> Result<IndexMap<RigId, RigInputs>, ProjectValidationError> {
    project
        .rigs()
        .map(|rig| Ok((rig.id.clone(), admit_rig(project, rig)?)))
        .collect()
}

fn admit_rig(project: &DonderProject, rig: &Rig) -> Result<RigInputs, ProjectValidationError> {
    let mut controls: IndexMap<
        ControlTarget,
        Vec<(RigMappingId, &donder_runtime_types::Identifier)>,
    > = IndexMap::new();
    for mapping in &rig.mappings {
        if let MappingAction::Control { target, param } = &mapping.action {
            let bound = controls.entry(*target).or_default();
            if bound.iter().any(|(_, existing)| *existing == param) {
                return Err(invalid(format!(
                    "Two controls set parameter `{}` of the same voice",
                    param.as_str()
                )));
            }
            bound.push((mapping.id, param));
        }
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
            return Err(invalid(
                "Operators in a rig cannot read live mark collections".into(),
            ));
        }
        let OperatorRef::Custom(id) = &operator.operator;
        let definition = &project.definitions().operators.definitions[id];
        let values = resolve_values(
            project,
            MarkSource::Live,
            definition.params(),
            &operator.params,
            session(),
        )?;
        let OperatorImplementation::Dsl(compiled) = definition.implementation();
        let invocation = compiled
            .invoke(values.into_vec(), Box::new([]))
            .map_err(|error| invalid(error.message))?;
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
    controls: &[(RigMappingId, &donder_runtime_types::Identifier)],
) -> Result<RigEffectInputs, ProjectValidationError> {
    let EffectRef::Custom(id) = &effect.definition;
    let definition = &project.definitions().effects.definitions[id];
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
                .ok_or_else(|| invalid("Unknown control parameter".into()))?;
            let mapping = params[index]
                .automation_mapping()
                .ok_or_else(|| invalid("A control's parameter cannot be automated".into()))?;
            Ok(PreparedAutomation {
                start: SampleTime::from_ticks(0),
                duration: RIG_SESSION,
                // Every control starts at the bottom of its range, released.
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
        .collect::<Result<Box<[_]>, ProjectValidationError>>()?;
    let EffectImplementation::Dsl(compiled) = definition.implementation();
    let invocation = compiled
        .invoke_live(values.into_vec(), automation, live_params.into())
        .map_err(|error| invalid(error.message))?;
    Ok(RigEffectInputs {
        invocation,
        live: live.into(),
        controls: controls.iter().map(|(mapping, _)| *mapping).collect(),
    })
}

fn param_index(index: usize) -> Result<u16, ProjectValidationError> {
    u16::try_from(index).map_err(|_| invalid("Too many parameters".into()))
}

fn session() -> EffectParamTiming {
    EffectParamTiming {
        start: SampleTime::from_ticks(0),
        duration: RIG_SESSION,
    }
}

fn timing(rig: &Rig) -> Result<SequenceTiming, ProjectValidationError> {
    let admit = || {
        let frame_rate = NonZeroU32::new(rig.frame_rate)?;
        let frame_count = NonZeroU32::new(
            u32::try_from(
                (u64::from(RIG_SESSION.as_ticks()) * u64::from(rig.frame_rate)).div_ceil(1_000_000),
            )
            .ok()?,
        )?;
        let duration = NonZeroU32::new(RIG_SESSION.as_ticks())?;
        SequenceTiming::admit(frame_rate, frame_count, duration, Box::new([]))
    };
    admit()
        .ok_or_else(|| invalid("Rig frame rate is not representable by the playback clock".into()))
}
