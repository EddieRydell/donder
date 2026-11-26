pub(super) fn edit_sequence(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    let mut created = Vec::new();
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
    match edit {
        SequenceGuiEdit::DeleteSelection { .. }
        | SequenceGuiEdit::MoveClips { .. }
        | SequenceGuiEdit::ResizeClips { .. }
        | SequenceGuiEdit::ArrangeClips { .. }
        | SequenceGuiEdit::MoveMarks { .. } => {
            unreachable!("selection edits are applied to the selection")
        }
        SequenceGuiEdit::Composition { edit } => {
            created = super::graph::edit_composition(session, &mut draft.composition, edit)?;
        }
        SequenceGuiEdit::SplitClips { ids, time_seconds } => {
            let at = super::checked_gui_time(time_seconds)?;
            let clips = &mut draft.composition.clips;
            let mut ids_after = IdAllocator::after(clips.iter().map(|clip| clip.id.0));
            for id in ids {
                let index = clips
                    .iter()
                    .position(|clip| clip.id.0 == id)
                    .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))?;
                let mut left = Clip::clone(&clips[index]);
                let mut right = left
                    .split_off(at.clone(), ClipId(ids_after.next()?))
                    .ok_or_else(|| {
                        GuiMutationError::Invalid(
                            "Split time must fall inside the clip.".to_string(),
                        )
                    })?;
                right.name = donder_language::unique_name(left.name.as_str(), |name| {
                    clips.iter().any(|other| other.name.as_str() == name)
                });
                // The envelopes that drive the left half drive the right too.
                let rebind = |target: &AutomationTarget| match target {
                    AutomationTarget::ClipParam { clip_id, param } if *clip_id == left.id => {
                        Some(AutomationTarget::ClipParam {
                            clip_id: right.id,
                            param: param.clone(),
                        })
                    }
                    _ => None,
                };
                for clip in clips.iter_mut() {
                    let Some(envelope) = clip.envelope() else {
                        continue;
                    };
                    let bindings = envelope
                        .bindings
                        .iter()
                        .filter_map(|binding| {
                            Some(Binding {
                                target: rebind(&binding.target)?,
                                window: binding.window,
                            })
                        })
                        .collect::<Vec<_>>();
                    if let Some(envelope) = (!bindings.is_empty())
                        .then(|| std::sync::Arc::make_mut(clip).envelope_mut())
                        .flatten()
                    {
                        envelope.bindings.extend(bindings);
                    }
                }
                clips[index] = std::sync::Arc::new(left);
                clips.insert(index + 1, std::sync::Arc::new(right));
            }
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_id,
            index,
            target_collection_id,
        } => {
            if collection_id != target_collection_id {
                let sequence = &mut draft;
                let mark = {
                    let collection = mark_collection_mut(sequence, collection_id)?;
                    if (index as usize) >= collection.marks.len() {
                        return Err(GuiMutationError::Invalid("Mark was not found.".to_string()));
                    }
                    collection.marks.remove(index as usize)
                };
                let target_collection = mark_collection_mut(sequence, target_collection_id)?;
                target_collection.marks.push(mark);
                target_collection.marks.sort_by_key(|mark| mark.time.0);
            }
        }
        SequenceGuiEdit::AddMarks {
            collection_id,
            times_seconds,
        } => {
            let collection = mark_collection_mut(&mut draft, collection_id)?;
            for seconds in times_seconds {
                collection
                    .marks
                    .push(Mark::at(super::checked_gui_time(seconds)?));
            }
            collection.marks.sort_by_key(|mark| mark.time.0);
        }
        SequenceGuiEdit::AddClip {
            target,
            start_seconds,
            duration_seconds,
            content,
        } => {
            let target = target_from_gui(target);
            let placement = Placement::Timed {
                start: super::checked_gui_time(start_seconds)?,
                duration: super::checked_gui_duration(duration_seconds)?,
            };
            let clip = match content {
                NewClipContent::Effect {
                    initial_color,
                    effect,
                    scope,
                } => super::graph::new_effect_clip(
                    session,
                    &draft.composition,
                    target,
                    placement,
                    (effect, clip_scope(scope), &initial_color),
                )?,
                NewClipContent::Envelope => {
                    new_envelope(&draft.composition, vec![target], placement)?
                }
            };
            draft.composition.clips.push(std::sync::Arc::new(clip));
        }
        SequenceGuiEdit::AddEnvelopeFor { target } => {
            let target = automation_target_from_gui(target)?;
            automation_target_mapping(&resolved.view(session), &target)?;
            let (placement, row) = envelope_timing(session, &draft, &target)?;
            let mut clip = new_envelope(&draft.composition, row, placement)?;
            if let Some(envelope) = clip.envelope_mut() {
                envelope.bind(target, Window::FULL);
            }
            draft.composition.clips.push(std::sync::Arc::new(clip));
        }
        SequenceGuiEdit::Bind { clip, target } => {
            let target = automation_target_from_gui(target)?;
            automation_target_mapping(&resolved.view(session), &target)?;
            clip_mut(&mut draft, clip)?
                .envelope_mut()
                .ok_or_else(|| GuiMutationError::Invalid("Choose an envelope.".to_string()))?
                .bind(target, Window::FULL);
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(created)
}

/// A new envelope of `composition` that rises from 0 to 1 and binds nothing.
fn new_envelope(
    composition: &donder_model::Composition,
    rows: Vec<donder_model::FixtureId>,
    placement: Placement,
) -> Result<Clip, GuiMutationError> {
    super::graph::new_clip(
        composition,
        "envelope",
        rows,
        placement,
        ClipContent::Envelope(Envelope {
            curve: Curve {
                points: vec![
                    CurvePoint {
                        position: 0.0,
                        value: 0.0,
                    },
                    CurvePoint {
                        position: 1.0,
                        value: 1.0,
                    },
                ],
            },
            bindings: Vec::new(),
        }),
    )
}

pub(super) fn automation_target_from_gui(
    target: SequenceAutomationTarget,
) -> Result<AutomationTarget, GuiMutationError> {
    Ok(match target {
        SequenceAutomationTarget::ClipParam { clip_id, param } => AutomationTarget::ClipParam {
            clip_id: ClipId(clip_id),
            param: identifier(&param)?,
        },
        SequenceAutomationTarget::CompositionNodeParam { node_id, param } => {
            AutomationTarget::CompositionNodeParam {
                node_id: donder_model::CompositionGraphNodeId(node_id),
                param: identifier(&param)?,
            }
        }
    })
}

/// Where a new envelope for `target` plays and is drawn: over its clip, or
/// over the whole sequence in the first lane for an operator's parameter.
fn envelope_timing(
    session: &ProjectSession,
    sequence: &donder_model::Sequence,
    target: &AutomationTarget,
) -> Result<(Placement, Vec<donder_model::FixtureId>), GuiMutationError> {
    match target {
        AutomationTarget::ClipParam { clip_id, .. } => {
            let clip = sequence
                .composition
                .clip(*clip_id)
                .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))?;
            Ok((clip.placement.clone(), clip.targets.clone()))
        }
        AutomationTarget::CompositionNodeParam { .. } => Ok((
            Placement::Timed {
                start: super::checked_gui_time(0.0)?,
                duration: sequence.duration.clone(),
            },
            vec![
                super::selection::target_for_lane(session, 0).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence has no target for automation.".into())
                })?,
            ],
        )),
    }
}

/// The target param's declared automation mapping.
fn automation_target_mapping(
    sequence: &donder_model::View<'_, donder_model::Sequence>,
    target: &AutomationTarget,
) -> Result<AutomationMapping, GuiMutationError> {
    let declaration =
        donder_model::automation_target_param(sequence.project(), &sequence.composition, target)
            .map_err(|problem| {
                GuiMutationError::Invalid(
                    donder_model::EnvelopeProblem::Binding(target.clone(), problem).to_string(),
                )
            })?;
    declaration
        .automation_mapping()
        .ok_or_else(|| GuiMutationError::Invalid("Param does not support automation.".to_string()))
}

use donder_model::{AutomationTarget, Binding, ClipContent, Envelope, Mark, Window};
use donder_model::{Clip, ClipId, Placement};
use donder_project_io::ProjectSession;
use donder_runtime_types::{AutomationMapping, Curve, CurvePoint};

use super::model::{clip_mut, clip_scope, identifier, mark_collection_mut, target_from_gui};
use super::{GuiMutationError, IdAllocator, ResolvedGuiObject};
use crate::dto::{NewClipContent, SequenceAutomationTarget, SequenceGuiEdit};
