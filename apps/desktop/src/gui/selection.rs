pub(super) fn required_operator_param_value(
    ty: Type,
    sequence: &donder_language::sequence::Sequence,
    color: donder_language::values::Color,
) -> Result<EffectParamValue, GuiMutationError> {
    if ty == Type::Marks {
        return sequence
            .mark_collections
            .first()
            .map(|collection| EffectParamValue::Marks(collection.key.clone()))
            .ok_or_else(|| {
                GuiMutationError::Invalid(
                    "A required marks parameter needs a mark collection.".to_string(),
                )
            });
    }
    EffectParamValue::initial_for_type(&ty, color).ok_or_else(|| {
        GuiMutationError::Invalid(
            "A valid required operator parameter could not be created.".to_string(),
        )
    })
}

pub(crate) fn copy_sequence_selection(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, u32, u32), GuiMutationError> {
    match selection {
        SequenceSelection::Clips {
            effect_ids: ids,
            automation_ids,
        } => {
            let sequence = session
                .project
                .sequence(sequence_id)
                .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for id in ids {
                let Some(effect) = sequence.effects.iter().find(|effect| effect.id.0 == *id) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardEffect {
                    effect: effect.clone(),
                    start_seconds: effect.start.as_seconds_f32(),
                    lane_index: target_lane_index(session, &effect.target).ok_or_else(|| {
                        GuiMutationError::Invalid("Effect row is missing.".into())
                    })?,
                });
            }
            let mut automation = Vec::new();
            for id in automation_ids {
                let clip = sequence
                    .automation_clips
                    .iter()
                    .find(|clip| clip.id.0 == *id)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Selected automation clip is missing.".into())
                    })?;
                let lane_index = target_lane_index(session, &clip.row_target).ok_or_else(|| {
                    GuiMutationError::Invalid("Automation row is missing.".into())
                })?;
                automation.push(ClipboardAutomation {
                    clip: clip.clone(),
                    lane_index,
                });
            }
            let copied_count = (copied.len() + automation.len()) as u32;
            Ok((
                (copied_count > 0).then_some(SequenceClipboard::Clips {
                    effects: copied,
                    automation,
                    source: sequence_id.clone(),
                    cut: false,
                }),
                copied_count,
                skipped,
            ))
        }
        SequenceSelection::Marks { marks } => {
            let sequence = session
                .project
                .sequence(sequence_id)
                .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let Some(time_seconds) = mark_time_seconds(sequence, mark) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardMark {
                    collection_key: mark.collection_key.clone(),
                    time_seconds,
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Marks(copied)),
                copied_count,
                skipped,
            ))
        }
    }
}

pub(super) fn delete_sequence_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(), GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    match selection {
        SequenceSelection::Clips {
            effect_ids: ids,
            automation_ids,
        } => {
            sequence
                .effects
                .retain(|effect| !ids.contains(&effect.id.0));
            sequence
                .automation_clips
                .retain(|clip| !automation_ids.contains(&clip.id.0));
            for clip in &mut sequence.automation_clips {
                clip.detach_bindings(AutomationDetachmentReason::TargetDeleted, |target| {
                    matches!(target, AutomationTarget::EffectParam { effect_id, .. } if ids.contains(&effect_id.0))
                });
            }
        }
        SequenceSelection::Marks { marks } => {
            for (collection_key, indexes) in mark_indexes_by_collection(marks) {
                for index in indexes.into_iter().rev() {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if index < collection.marks.len() {
                        collection.marks.remove(index);
                    }
                }
            }
        }
    }
    Ok(())
}

pub(super) fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    anchor: SequencePasteAnchor,
    clipboard: Option<&SequenceClipboard>,
) -> Result<SequenceSelectionMutation, GuiMutationError> {
    let clipboard = clipboard
        .ok_or_else(|| GuiMutationError::Invalid("Copy clips or marks before pasting.".into()))?;
    if !anchor.time_seconds.is_finite() || anchor.time_seconds < 0.0 {
        return Err(GuiMutationError::Invalid(
            "Paste time must be finite and nonnegative.".into(),
        ));
    }
    match clipboard {
        SequenceClipboard::Clips {
            effects,
            automation,
            source,
            cut,
        } => {
            let layout = active_layout(session)
                .ok_or_else(|| GuiMutationError::Invalid("Active layout is missing.".into()))?;
            let targets = layout
                .iter_fixtures()
                .map(|fixture| FixtureTarget {
                    layout: layout.id.clone(),
                    fixture: fixture.id,
                })
                .collect::<Vec<_>>();
            let anchor_target = anchor.target.as_ref().ok_or_else(|| {
                GuiMutationError::Invalid("Select a target row before pasting clips.".into())
            })?;
            let anchor_lane = targets
                .iter()
                .position(|target| target.fixture.0 == anchor_target.fixture)
                .ok_or_else(|| GuiMutationError::Invalid("Paste target is missing.".into()))?;
            let min_start = effects
                .iter()
                .map(|effect| effect.start_seconds)
                .chain(
                    automation
                        .iter()
                        .map(|entry| entry.clip.start.as_seconds_f32()),
                )
                .fold(f32::INFINITY, f32::min);
            let min_lane = effects
                .iter()
                .map(|effect| effect.lane_index)
                .chain(automation.iter().map(|entry| entry.lane_index))
                .min()
                .ok_or_else(|| GuiMutationError::Invalid("Clipboard is empty.".into()))?;
            let destination = |lane: usize| {
                targets
                    .get(anchor_lane + lane - min_lane)
                    .cloned()
                    .ok_or_else(|| {
                        GuiMutationError::Invalid(
                            "The copied clips do not fit below this target.".into(),
                        )
                    })
            };
            // Resolve every destination before mutation; never collapse distinct rows at the boundary.
            let effect_targets = effects
                .iter()
                .map(|entry| destination(entry.lane_index))
                .collect::<Result<Vec<_>, _>>()?;
            let automation_targets = automation
                .iter()
                .map(|entry| destination(entry.lane_index))
                .collect::<Result<Vec<_>, _>>()?;
            donder_project_io::ensure_document_can_reference_object(
                session,
                sequence_id.0.document_id(),
                &targets[anchor_lane].layout.0,
            )
            .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            let sequence = sequence_mut(session, sequence_id)?;
            let mut next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0);
            let mut effect_ids = Vec::new();
            let mut id_map = BTreeMap::new();
            for (entry, target) in effects.iter().zip(effect_targets) {
                next_id = next_id
                    .checked_add(1)
                    .ok_or_else(|| GuiMutationError::Invalid("Effect IDs exhausted.".into()))?;
                let mut effect = entry.effect.clone();
                id_map.insert(effect.id.0, next_id);
                effect.id = EffectInstId(next_id);
                effect.start = DonderTime::from_seconds_f32(
                    anchor.time_seconds + entry.start_seconds - min_start,
                );
                effect.target = target;
                sequence.effects.push(effect);
                effect_ids.push(next_id);
            }
            let mut next_id = sequence
                .automation_clips
                .iter()
                .map(|clip| clip.id.0)
                .max()
                .unwrap_or(0);
            let mut automation_ids = Vec::new();
            for (entry, target) in automation.iter().zip(automation_targets) {
                next_id = next_id
                    .checked_add(1)
                    .ok_or_else(|| GuiMutationError::Invalid("Automation IDs exhausted.".into()))?;
                let mut clip = entry.clip.clone();
                clip.id = donder_language::sequence::AutomationClipId(next_id);
                clip.row_target = target;
                clip.start = DonderTime::from_seconds_f32(
                    anchor.time_seconds + entry.clip.start.as_seconds_f32() - min_start,
                );
                // Copy bindings only within the copied selection. Cut may retain existing bindings
                // in the same sequence when no other clip has claimed them since the cut.
                let remap = |target: &mut AutomationTarget| {
                    if let AutomationTarget::EffectParam { effect_id, .. } = target
                        && let Some(id) = id_map.get(&effect_id.0)
                    {
                        effect_id.0 = *id;
                        return true;
                    }
                    *cut && source == sequence_id
                        && !sequence.automation_clips.iter().any(|clip| {
                            clip.bindings
                                .iter()
                                .any(|binding| &binding.target == target)
                                || clip
                                    .detached_bindings
                                    .iter()
                                    .any(|binding| &binding.target == target)
                        })
                };
                clip.bindings
                    .retain_mut(|binding| remap(&mut binding.target));
                clip.detached_bindings
                    .retain_mut(|binding| remap(&mut binding.target));
                sequence.automation_clips.push(clip);
                automation_ids.push(next_id);
            }
            donder_project_io::maintain_ownership_sources(session)
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Clips {
                    effect_ids,
                    automation_ids,
                }),
                copied_count: (effects.len() + automation.len()) as u32,
                skipped_count: 0,
            })
        }
        SequenceClipboard::Marks(marks) => {
            let min_time = marks
                .iter()
                .map(|mark| mark.time_seconds)
                .fold(f32::INFINITY, f32::min);
            let mut pasted = Vec::new();
            let mut skipped = 0u32;
            let sequence = sequence_mut(session, sequence_id)?;
            for mark in marks {
                let collection = match mark_collection_mut(sequence, &mark.collection_key) {
                    Ok(collection) => collection,
                    Err(_) => {
                        skipped = skipped.saturating_add(1);
                        continue;
                    }
                };
                let time_seconds = (anchor.time_seconds + mark.time_seconds - min_time).max(0.0);
                collection
                    .marks
                    .push(DonderTime::from_seconds_f32(time_seconds));
                collection.marks.sort_by_key(|time| time.0);
                let index = collection
                    .marks
                    .iter()
                    .position(|value| (value.as_seconds_f32() - time_seconds).abs() < f32::EPSILON)
                    .unwrap_or_else(|| collection.marks.len().saturating_sub(1));
                pasted.push(SequenceMarkRef {
                    collection_key: mark.collection_key.clone(),
                    index: index as u32,
                });
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Marks { marks: pasted }),
                copied_count: marks.len() as u32,
                skipped_count: skipped,
            })
        }
    }
}

pub(super) fn move_clip_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    effect_ids: &[u32],
    automation_ids: &[u32],
    time_delta_seconds: f32,
    lane_delta: i32,
) -> Result<(), GuiMutationError> {
    let layout = active_layout(session)
        .ok_or_else(|| GuiMutationError::Invalid("Active layout is missing.".into()))?;
    let targets = layout
        .iter_fixtures()
        .map(|fixture| FixtureTarget {
            layout: layout.id.clone(),
            fixture: fixture.id,
        })
        .collect::<Vec<_>>();
    let destination = |target: &FixtureTarget| -> Result<FixtureTarget, GuiMutationError> {
        let index = targets
            .iter()
            .position(|candidate| candidate == target)
            .ok_or_else(|| GuiMutationError::Invalid("Clip row target is missing.".into()))?;
        let destination = index as i64 + i64::from(lane_delta);
        usize::try_from(destination)
            .ok()
            .and_then(|index| targets.get(index))
            .cloned()
            .ok_or_else(|| {
                GuiMutationError::Invalid("The selected clips do not fit at this target.".into())
            })
    };
    let sequence = sequence_mut(session, sequence_id)?;
    for id in effect_ids {
        let effect = effect_mut(sequence, *id)?;
        effect.target = destination(&effect.target)?;
        effect.start = shifted_start(&effect.start, time_delta_seconds)?;
    }
    for id in automation_ids {
        let clip = sequence
            .automation_clips
            .iter_mut()
            .find(|clip| clip.id.0 == *id)
            .ok_or_else(|| GuiMutationError::Invalid("Automation clip is missing.".into()))?;
        clip.row_target = destination(&clip.row_target)?;
        clip.start = shifted_start(&clip.start, time_delta_seconds)?;
    }
    Ok(())
}

fn shifted_start(start: &DonderTime, delta: f32) -> Result<DonderTime, GuiMutationError> {
    let seconds = start.as_seconds_f32() + delta;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(GuiMutationError::Invalid(
            "Clip start must be finite and nonnegative.".into(),
        ));
    }
    Ok(DonderTime::from_seconds_f32(seconds))
}

pub(super) fn resize_clip_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    effect_ids: &[u32],
    automation_ids: &[u32],
    edge: SequenceResizeEdge,
    time_delta_seconds: f32,
) -> Result<(), GuiMutationError> {
    let resize =
        |start: &mut DonderTime, duration: &mut DonderDuration| -> Result<(), GuiMutationError> {
            let seconds = duration.as_seconds_f32()
                + match edge {
                    SequenceResizeEdge::Left => -time_delta_seconds,
                    SequenceResizeEdge::Right => time_delta_seconds,
                };
            if !seconds.is_finite() || seconds <= 0.0 {
                return Err(GuiMutationError::Invalid(
                    "Clip duration must be positive and finite.".into(),
                ));
            }
            if matches!(edge, SequenceResizeEdge::Left) {
                *start = shifted_start(start, time_delta_seconds)?;
            }
            *duration = DonderDuration::from_seconds_f32(seconds);
            Ok(())
        };
    let sequence = sequence_mut(session, sequence_id)?;
    for id in effect_ids {
        let effect = effect_mut(sequence, *id)?;
        resize(&mut effect.start, &mut effect.duration)?;
    }
    for id in automation_ids {
        let clip = sequence
            .automation_clips
            .iter_mut()
            .find(|clip| clip.id.0 == *id)
            .ok_or_else(|| GuiMutationError::Invalid("Automation clip is missing.".into()))?;
        resize(&mut clip.start, &mut clip.duration)?;
    }
    Ok(())
}

pub(super) fn move_mark_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f32,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for (collection_key, indexes) in mark_indexes_by_collection(marks) {
        let mut moved_times = Vec::new();
        for index in indexes {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            if let Some(value) = collection.marks.get_mut(index) {
                let time_seconds = (value.as_seconds_f32() + time_delta_seconds).max(0.0);
                *value = DonderTime::from_seconds_f32(time_seconds);
                moved_times.push(time_seconds);
            }
        }
        let collection = mark_collection_mut(sequence, &collection_key)?;
        collection.marks.sort_by_key(|time| time.0);
        for time_seconds in moved_times {
            if let Some(index) = collection
                .marks
                .iter()
                .position(|value| (value.as_seconds_f32() - time_seconds).abs() < f32::EPSILON)
            {
                moved.push(SequenceMarkRef {
                    collection_key: collection_key.clone(),
                    index: index as u32,
                });
            }
        }
    }
    Ok(moved)
}

fn mark_time_seconds(
    sequence: &donder_language::sequence::Sequence,
    mark: &SequenceMarkRef,
) -> Option<f32> {
    sequence
        .mark_collections
        .iter()
        .find(|collection| collection.key.name == mark.collection_key)?
        .marks
        .get(mark.index as usize)
        .map(DonderTime::as_seconds_f32)
}

fn mark_indexes_by_collection(marks: &[SequenceMarkRef]) -> BTreeMap<String, Vec<usize>> {
    let mut grouped = BTreeMap::<String, Vec<usize>>::new();
    for mark in marks {
        grouped
            .entry(mark.collection_key.clone())
            .or_default()
            .push(mark.index as usize);
    }
    for indexes in grouped.values_mut() {
        indexes.sort_unstable();
        indexes.dedup();
    }
    grouped
}

fn target_lane_index(session: &ProjectSession, target: &FixtureTarget) -> Option<usize> {
    let layout = active_layout(session)?;
    if layout.id != target.layout {
        return None;
    }
    layout
        .iter_fixtures()
        .position(|fixture| fixture.id == target.fixture)
}

pub(super) fn target_for_lane(
    session: &ProjectSession,
    lane_index: usize,
) -> Option<FixtureTarget> {
    let layout = active_layout(session)?;
    let fixture = layout.iter_fixtures().nth(lane_index)?.id;
    Some(FixtureTarget {
        layout: layout.id.clone(),
        fixture,
    })
}

pub(super) fn mark_param_names(
    session: &ProjectSession,
    reference: &SequenceEffectReference,
) -> Result<Vec<String>, GuiMutationError> {
    let reference = match reference {
        SequenceEffectReference::Custom {
            module_id,
            path,
            effect_name,
        } => {
            let identity = source_identity_from_gui(module_id, path, effect_name)?;
            if !session.source.is_project_owned(identity.document_id()) {
                return Err(GuiMutationError::Invalid(
                    "Effect source module was not found.".to_string(),
                ));
            }
            EffectRef::Custom(EffectDefinitionId(identity))
        }
    };
    let definition = session
        .project
        .definitions
        .effects
        .resolve(&reference)
        .ok_or_else(|| GuiMutationError::Invalid("Effect was not found.".to_string()))?;
    Ok(definition
        .params
        .iter()
        .filter(|param| matches!(param.ty, Type::Marks))
        .map(|param| param.name.as_str().to_string())
        .collect())
}
use std::collections::BTreeMap;

use donder_language::dsl::Type;
use donder_language::effect::{EffectDefinitionId, EffectInstId, EffectParamValue, EffectRef};
use donder_language::layout::FixtureTarget;
use donder_language::sequence::{AutomationDetachmentReason, AutomationTarget, SequenceId};
use donder_language::values::{DonderDuration, DonderTime};
use donder_project_io::ProjectSession;

use super::model::{effect_mut, mark_collection_mut, sequence_mut, source_identity_from_gui};
use super::projection::active_layout;
use super::{
    ClipboardAutomation, ClipboardEffect, ClipboardMark, GuiMutationError, SequenceClipboard,
    SequenceSelectionMutation,
};
use crate::dto::{
    SequenceEffectReference, SequenceMarkRef, SequencePasteAnchor, SequenceResizeEdge,
    SequenceSelection,
};
