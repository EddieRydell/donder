//! Playback invocations of a console's clips and operators, admitted with the
//! project like a sequence's. Each parameter a `Set` action writes, each
//! parameter a clip's copies take from their presses, and each marks parameter
//! reading a live collection is an automation binding on its clip or operator
//! that playback sets.
use super::inputs::{invalid, resolve_values};
use super::parameters::{ClipTiming, MarkOrigin};
use super::*;
use crate::console::{Console, ConsoleClip, ConsoleId};
use crate::effect::ParamValue;
use crate::sequence::{AutomationTarget, CompositionGraphNodeId};
use crate::validation::{AdmissionProblem, Member, ProjectValidationError, ValidationProblem, at};
use donder_language::compiler::ParamDecl;
use donder_runtime_types::{
    AutomatedQuantity, AutomationMapping, Curve, CurvePoint, Identifier, PreparedAutomation,
    SampleDuration, SampleTime, SequenceTiming,
};
use indexmap::IndexMap;
use std::num::NonZeroU32;
use std::sync::Arc;

/// A console session's length: the playback clock's range.
pub const CONSOLE_SESSION: SampleDuration = SampleDuration::from_ticks(u32::MAX);

#[derive(Debug)]
pub(super) struct ConsoleInputs {
    pub(super) timing: SequenceTiming,
    /// In [`Console::clips`] order.
    pub(super) clips: Box<[ConsoleClipInputs]>,
    pub(super) operators: IndexMap<CompositionGraphNodeId, Invocation>,
    /// The console's set parameter that sets each of an operator's
    /// automation bindings, in binding order.
    pub(super) operator_automations: IndexMap<CompositionGraphNodeId, Box<[usize]>>,
}

#[derive(Debug)]
pub(super) struct ConsoleClipInputs {
    pub(super) invocation: Invocation,
    /// What sets each of the invocation's automation bindings, in binding
    /// order.
    pub(super) bindings: Box<[ConsoleBinding]>,
}

/// What sets an automation binding of a console clip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConsoleBinding {
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
) -> Result<IndexMap<ConsoleId, ConsoleInputs>, ProjectValidationError> {
    project
        .objects::<crate::Console>()
        .map(|console| {
            Ok((
                console.id,
                admit_console(project, console).map_err(at(console.id))?,
            ))
        })
        .collect()
}

fn admit_console(
    project: &DonderProject,
    console: &Console,
) -> Result<ConsoleInputs, ValidationProblem> {
    let clips = console
        .clips
        .iter()
        .map(|clip| admit_clip(project, console, clip))
        .collect::<Result<_, _>>()?;
    let mut operators = IndexMap::new();
    let mut operator_automations = IndexMap::new();
    for node in &console.composition_graph.nodes {
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
        // Each of its parameters a `Set` writes, starting where its control rests.
        let mut automation = Vec::new();
        let mut automations = Vec::new();
        for (index, target) in console.set_targets().into_iter().enumerate() {
            if let AutomationTarget::CompositionNodeParam { node_id, param } = target
                && *node_id == node.id
            {
                automation.push(set_binding(
                    definition.params(),
                    param,
                    console.rest(target),
                )?);
                automations.push(index);
            }
        }
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
    Ok(ConsoleInputs {
        timing: timing(console),
        clips,
        operators,
        operator_automations,
    })
}

fn admit_clip(
    project: &DonderProject,
    console: &Console,
    clip: &ConsoleClip,
) -> Result<ConsoleClipInputs, ValidationProblem> {
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
    // Each set parameter, starting where its control rests; validation
    // checked that each is automatable and that the clip does not play it.
    let mut set = Vec::new();
    for (index, target) in console.set_targets().into_iter().enumerate() {
        if let AutomationTarget::ClipParam { clip_id, param } = target
            && *clip_id == clip.id
        {
            set.push((ConsoleBinding::Set(index), param, console.rest(target)));
        }
    }
    for (index, value) in clip.played.iter().enumerate() {
        set.push((ConsoleBinding::Played(index), &value.param, 0.0));
    }
    let mut bindings: Vec<ConsoleBinding> = set.iter().map(|(binding, ..)| *binding).collect();
    let mut automation = set
        .iter()
        .map(|(_, name, value)| set_binding(params, name, *value))
        .collect::<Result<Vec<_>, ValidationProblem>>()?;
    for (index, param) in params.iter().enumerate() {
        if let Some(ParamValue::Marks(Some(key))) = clip.param_overrides.get(&param.name) {
            let collection = console
                .live_marks
                .iter()
                .position(|marks| marks.key == *key)
                .unwrap_or_else(|| unreachable!("validation resolves live mark collections"));
            automation.push(session_binding(
                AutomationMapping::LiveMarks,
                param_index(index)?,
                0.0,
            ));
            bindings.push(ConsoleBinding::LiveMarks(collection));
        }
    }
    let compiled = definition.compiled();
    let invocation = compiled
        .invoke(values.into_vec(), automation.into())
        .map_err(|error| {
            invalid(AdmissionProblem::Binding(
                Member::ConsoleClip(clip.id),
                error,
            ))
        })?;
    Ok(ConsoleClipInputs {
        invocation,
        bindings: bindings.into(),
    })
}

/// The binding of parameter `name` of `params` that a `Set` or a played
/// value sets, holding `value` until it is set.
fn set_binding(
    params: &[ParamDecl],
    name: &Identifier,
    value: f32,
) -> Result<PreparedAutomation, ValidationProblem> {
    let index = params
        .iter()
        .position(|param| &param.name == name)
        .unwrap_or_else(|| unreachable!("validation resolves set parameters"));
    let mapping = params[index]
        .automation_mapping()
        .unwrap_or_else(|| unreachable!("validation checks set parameters automate"));
    Ok(session_binding(mapping, param_index(index)?, value))
}

/// A binding over the whole session that holds `value` until playback sets
/// it. A played value is set before its copy plays.
fn session_binding(mapping: AutomationMapping, param_index: u16, value: f32) -> PreparedAutomation {
    PreparedAutomation {
        start: SampleTime::from_ticks(0),
        duration: CONSOLE_SESSION,
        curve: Arc::new(Curve {
            points: vec![CurvePoint {
                position: 0.0,
                value,
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
