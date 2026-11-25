//! Playback invocations of a console's clips and operators, admitted with the
//! project like a sequence's. Each parameter a `Set` action writes, each
//! parameter a clip's copies take from their presses, and each marks parameter
//! reading a live collection is an automation binding on its clip or operator
//! that playback sets.
use super::inputs::{invalid, resolve_values};
use super::parameters::{ClipTiming, MarkOrigin};
use super::*;
use crate::console::CONSOLE_SESSION;
use crate::console::Console;
use crate::effect::ParamValue;
use crate::effect::{Clip, Effect};
use crate::sequence::AutomationTarget;
use crate::site::Member;
use crate::validation::{AdmissionProblem, ProjectValidationError, ValidationProblem, at};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{
    AutomationMapping, AutomationSource, Identifier, Marks, PreparedAutomation, SampleTime,
    SequenceTiming, Value,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::Arc;

/// What sets a live automation binding of a console clip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LiveBinding {
    /// The console's `index`th set parameter, for every copy of the clip.
    Set(usize),
    /// The clip's `index`th played value, for the copy each press starts.
    Played(usize),
    /// The console's `index`th live mark collection, for every copy of the
    /// clip.
    LiveMarks(usize),
}

pub(super) fn admit(
    project: &DonderProject,
) -> Result<IndexMap<ShowId, CompositionInputs>, ProjectValidationError> {
    project
        .objects::<crate::Console>()
        .map(|(id, console)| {
            Ok((
                ShowId::Console(id),
                admit_console(project, console).map_err(at(id))?,
            ))
        })
        .collect()
}

fn admit_console(
    project: &DonderProject,
    console: &Console,
) -> Result<CompositionInputs, ValidationProblem> {
    let clips = console
        .composition
        .clips
        .iter()
        .filter_map(|clip| Some((clip, clip.effect()?)))
        .map(|(clip, effect)| admit_clip(project, console, clip, effect).map(Arc::new))
        .collect::<Result<_, _>>()?;
    let mut operators = IndexMap::new();
    let mut operator_automations = IndexMap::new();
    for node in &console.composition.graph.nodes {
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
        let definition = &project.reusable::<crate::ScriptDefinition>()[id];
        let values = resolve_values(
            project,
            MarkOrigin::Live,
            definition.compiled(),
            &operator.params,
            session(),
        );
        // Each of its parameters a `Set` writes.
        let mut automation = Vec::new();
        let mut automations = Vec::new();
        for (index, target) in console.set_targets().into_iter().enumerate() {
            if let AutomationTarget::CompositionNodeParam { node_id, param } = target
                && *node_id == node.id
            {
                automation.push(set_binding(definition.params(), param, automations.len())?);
                automations.push(index);
            }
        }
        // Then the envelopes that drive it, after the bindings `Set`s set.
        automation.extend(super::automation::admit(
            &console.composition,
            definition.params(),
            |target| matches!(target, AutomationTarget::CompositionNodeParam { node_id, .. } if node_id == &node.id),
        )?);
        let compiled = definition.compiled();
        let invocation = compiled
            .invoke(values.into_vec(), automation.into())
            .map_err(|error| {
                invalid(AdmissionProblem::Binding(
                    Member::Node(node.id.clone()),
                    error,
                ))
            })?;
        operators.insert(node.id.clone(), invocation);
        operator_automations.insert(node.id.clone(), automations.into());
    }
    Ok(CompositionInputs {
        timing: timing(console),
        clips,
        operators,
        operator_automations,
    })
}

fn admit_clip(
    project: &DonderProject,
    console: &Console,
    clip: &Clip,
    effect: &Effect,
) -> Result<ClipInputs, ValidationProblem> {
    let id = &effect.definition;
    let definition = &project.reusable::<crate::ScriptDefinition>()[id];
    let params = definition.params();
    let values = resolve_values(
        project,
        MarkOrigin::Live,
        definition.compiled(),
        &effect.params,
        clip.timing()
            .map_or_else(session, |(start, duration)| ClipTiming {
                start: crate::clock::sample_time(start),
                duration: crate::clock::sample_duration(duration),
            }),
    );
    // Each set parameter; validation checked that each is automatable and that the clip does not play it.
    let mut set = Vec::new();
    for (index, target) in console.set_targets().into_iter().enumerate() {
        if let AutomationTarget::ClipParam { clip_id, param } = target
            && *clip_id == clip.id
        {
            set.push((LiveBinding::Set(index), param));
        }
    }
    for (index, value) in clip.played().iter().enumerate() {
        set.push((LiveBinding::Played(index), &value.param));
    }
    let mut live: Vec<LiveBinding> = set.iter().map(|(binding, _)| *binding).collect();
    // The envelopes that drive a timed clip come first; the live bindings follow.
    let mut automation = super::automation::admit(
        &console.composition,
        params,
        |target| matches!(target, AutomationTarget::ClipParam { clip_id, .. } if *clip_id == clip.id),
    )?
    .into_vec();
    for (index, (_, name)) in set.iter().enumerate() {
        automation.push(set_binding(params, name, index)?);
    }
    let mut values = values.into_vec();
    for (index, param) in params.iter().enumerate() {
        if let Some(ParamValue::Marks(Some(key))) = effect.params.get(&param.name) {
            let collection = console
                .composition
                .marks
                .iter()
                .position(|marks| marks.id == *key)
                .unwrap_or_else(|| unreachable!("validation resolves live mark collections"));
            // Playback reads the collection's marks in the value's place.
            values[index] = Value::Marks(Arc::new(Marks::live(collection as u16)));
            automation.push(PreparedAutomation {
                source: AutomationSource::Live(live.len() as u16),
                mapping: AutomationMapping::LiveMarks,
                param_index: param_index(index)?,
            });
            live.push(LiveBinding::LiveMarks(collection));
        }
    }
    let compiled = definition.compiled();
    let invocation = compiled
        .invoke(values, automation.into())
        .map_err(|error| invalid(AdmissionProblem::Binding(Member::Clip(clip.id), error)))?;
    Ok(ClipInputs {
        invocation,
        live: live.into(),
    })
}

/// The binding of parameter `name` of `params` that a `Set` or a played
/// value sets: the invocation's `live`th live value.
fn set_binding(
    params: &[ParamDecl],
    name: &Identifier,
    live: usize,
) -> Result<PreparedAutomation, ValidationProblem> {
    let index = params
        .iter()
        .position(|param| &param.name == name)
        .unwrap_or_else(|| unreachable!("validation resolves set parameters"));
    let mapping = params[index]
        .automation_mapping()
        .unwrap_or_else(|| unreachable!("validation checks set parameters automate"));
    Ok(PreparedAutomation {
        source: AutomationSource::Live(live as u16),
        mapping,
        param_index: param_index(index)?,
    })
}

fn param_index(index: usize) -> Result<u16, ValidationProblem> {
    u16::try_from(index).map_err(|_| invalid(AdmissionProblem::TooManyParams))
}

fn session() -> ClipTiming {
    ClipTiming {
        start: SampleTime::from_ticks(0),
        duration: CONSOLE_SESSION,
    }
}

/// Validation bounds the console's frame rate, so its session fits the playback clock.
fn timing(console: &Console) -> SequenceTiming {
    fn proven<T>() -> T {
        unreachable!("validation bounds the console's frame rate")
    }
    let frame_count = u32::try_from(
        (u64::from(CONSOLE_SESSION.as_ticks()) * u64::from(console.frame_rate)).div_ceil(1_000_000),
    )
    .unwrap_or_else(|_| proven());
    SequenceTiming::new(
        NonZeroU32::new(console.frame_rate).unwrap_or_else(proven),
        NonZeroU32::new(frame_count).unwrap_or_else(proven),
        NonZeroU32::new(CONSOLE_SESSION.as_ticks()).unwrap_or_else(proven),
    )
}
