pub(super) fn copy_sequence_selection(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<SequenceClipboard>, u32, u32), GuiMutationError> {
    match selection {
        SequenceSelection::Clips { clips } => {
            let (ids, automation_ids) = &split_clips(clips);
            let sequence = session
                .project
                .get::<donder_model::Sequence>(*sequence_id)
                .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for id in ids {
                let Some(clip) = sequence.clips.iter().find(|clip| clip.id.0 == *id) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardClip {
                    clip: (**clip).clone(),
                    start_seconds: clip.start.as_seconds_f64(),
                    lane_index: target_lane_index(session, &clip.target)
                        .ok_or_else(|| GuiMutationError::Invalid("Clip row is missing.".into()))?,
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
                    clips: copied,
                    automation,
                    sequence: *sequence_id,
                    cut: false,
                }),
                copied_count,
                skipped,
            ))
        }
        SequenceSelection::Marks { marks } => {
            let sequence = session
                .project
                .get::<donder_model::Sequence>(*sequence_id)
                .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
            let mut copied = Vec::new();
            let mut skipped = 0u32;
            for mark in marks {
                let Some(authored) = sequence_mark(sequence, mark) else {
                    skipped = skipped.saturating_add(1);
                    continue;
                };
                copied.push(ClipboardMark {
                    collection_key: mark.collection_key.clone(),
                    time_seconds: authored.time.as_seconds_f64(),
                    label: authored.label.clone(),
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
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let sequence = &mut draft;
    match selection {
        SequenceSelection::Clips { clips } => {
            let (ids, automation_ids) = &split_clips(clips);
            sequence.clips.retain(|clip| !ids.contains(&clip.id.0));
            sequence
                .automation_clips
                .retain(|clip| !automation_ids.contains(&clip.id.0));
            for clip in &mut sequence.automation_clips {
                clip.remove_bindings(|target| {
                    matches!(target, AutomationTarget::ClipParam { clip_id, .. } if ids.contains(&clip_id.0))
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
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(())
}

pub(super) fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    anchor: SequencePasteAnchor,
    clipboard: Option<&SequenceClipboard>,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let document = super::model::object_address(session, *sequence_id)
        .document_id()
        .clone();
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let result: Result<GuiEditOutcome, GuiMutationError> = {
        let clipboard = clipboard.ok_or_else(|| {
            GuiMutationError::Invalid("Copy clips or marks before pasting.".into())
        })?;
        if !anchor.time_seconds.is_finite() || anchor.time_seconds < 0.0 {
            return Err(GuiMutationError::Invalid(
                "Paste time must be finite and nonnegative.".into(),
            ));
        }
        match clipboard {
            SequenceClipboard::Clips {
                clips,
                automation,
                sequence: copied_from,
                cut,
            } => {
                let targets = lane_targets(session)
                    .ok_or_else(|| GuiMutationError::Invalid("Active layout is missing.".into()))?;
                let anchor_lane = anchor.lane.ok_or_else(|| {
                    GuiMutationError::Invalid("Select a target row before pasting clips.".into())
                })? as usize;
                if anchor_lane >= targets.len() {
                    return Err(GuiMutationError::Invalid("Paste target is missing.".into()));
                }
                let min_start = clips
                    .iter()
                    .map(|clip| clip.start_seconds)
                    .chain(
                        automation
                            .iter()
                            .map(|entry| entry.clip.start.as_seconds_f64()),
                    )
                    .fold(f64::INFINITY, f64::min);
                let min_lane = clips
                    .iter()
                    .map(|clip| clip.lane_index)
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
                let clip_targets = clips
                    .iter()
                    .map(|entry| destination(entry.lane_index))
                    .collect::<Result<Vec<_>, _>>()?;
                let automation_targets = automation
                    .iter()
                    .map(|entry| destination(entry.lane_index))
                    .collect::<Result<Vec<_>, _>>()?;
                donder_project_io::ensure_document_can_reference_object(
                    session,
                    &document,
                    targets[anchor_lane].layout.into(),
                )
                .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
                let sequence = &mut draft;
                let mut next_id = sequence
                    .clips
                    .iter()
                    .map(|clip| clip.id.0)
                    .max()
                    .unwrap_or(0);
                let mut clip_ids = Vec::new();
                let mut id_map = BTreeMap::new();
                for (entry, target) in clips.iter().zip(clip_targets) {
                    next_id = next_id
                        .checked_add(1)
                        .ok_or_else(|| GuiMutationError::Invalid("Clip IDs exhausted.".into()))?;
                    let mut clip = entry.clip.clone();
                    id_map.insert(clip.id.0, next_id);
                    clip.id = ClipId(next_id);
                    clip.name = donder_language::unique_name(clip.name.as_str(), |name| {
                        sequence
                            .clips
                            .iter()
                            .any(|other| other.name.as_str() == name)
                    });
                    clip.start = super::checked_gui_time(
                        anchor.time_seconds + entry.start_seconds - min_start,
                    )?;
                    clip.target = target;
                    sequence.clips.push(std::sync::Arc::new(clip));
                    clip_ids.push(next_id);
                }
                let mut next_id = sequence
                    .automation_clips
                    .iter()
                    .map(|clip| clip.id.0)
                    .max()
                    .unwrap_or(0);
                let mut automation_ids = Vec::new();
                for (entry, target) in automation.iter().zip(automation_targets) {
                    next_id = next_id.checked_add(1).ok_or_else(|| {
                        GuiMutationError::Invalid("Automation IDs exhausted.".into())
                    })?;
                    let mut clip = entry.clip.clone();
                    clip.id = donder_model::AutomationClipId(next_id);
                    clip.row_target = target;
                    clip.start = super::checked_gui_time(
                        anchor.time_seconds + entry.clip.start.as_seconds_f64() - min_start,
                    )?;
                    // Copy bindings only within the copied selection. Cut may retain existing bindings
                    // in the same sequence when no overlapping clip has claimed them since the cut.
                    let placed = clip.clone();
                    let remap = |target: &mut AutomationTarget| {
                        if let AutomationTarget::ClipParam { clip_id, .. } = target
                            && let Some(id) = id_map.get(&clip_id.0)
                        {
                            clip_id.0 = *id;
                            return true;
                        }
                        *cut && copied_from == sequence_id
                            && !sequence.automation_clips.iter().any(|other| {
                                other.overlaps(&placed)
                                    && other.targets().any(|claimed| claimed == target)
                            })
                    };
                    clip.bindings
                        .retain_mut(|binding| remap(&mut binding.target));
                    clip.detached_bindings
                        .retain_mut(|binding| remap(&mut binding.target));
                    sequence.automation_clips.push(clip);
                    automation_ids.push(next_id);
                }
                Ok(GuiEditOutcome {
                    selection: Some(SequenceSelection::Clips {
                        clips: clip_refs(&clip_ids, &automation_ids),
                    }),
                    copied_count: (clips.len() + automation.len()) as u32,
                    skipped_count: 0,
                    created: Vec::new(),
                })
            }
            SequenceClipboard::Marks(marks) => {
                let min_time = marks
                    .iter()
                    .map(|mark| mark.time_seconds)
                    .fold(f64::INFINITY, f64::min);
                let mut pasted = Vec::new();
                let mut skipped = 0u32;
                let sequence = &mut draft;
                for mark in marks {
                    let collection = match mark_collection_mut(sequence, &mark.collection_key) {
                        Ok(collection) => collection,
                        Err(_) => {
                            skipped = skipped.saturating_add(1);
                            continue;
                        }
                    };
                    let time = super::checked_gui_time(
                        (anchor.time_seconds + mark.time_seconds - min_time).max(0.0),
                    )?;
                    collection.marks.push(donder_model::Mark {
                        time: time.clone(),
                        label: mark.label.clone(),
                    });
                    collection.marks.sort_by_key(|mark| mark.time.0);
                    let index = collection
                        .marks
                        .iter()
                        .position(|value| value.time == time)
                        .ok_or_else(|| {
                            GuiMutationError::Invalid("Pasted mark is missing.".into())
                        })?;
                    pasted.push(SequenceMarkRef {
                        collection_key: mark.collection_key.clone(),
                        index: index as u32,
                    });
                }
                Ok(GuiEditOutcome {
                    selection: Some(SequenceSelection::Marks { marks: pasted }),
                    copied_count: marks.len() as u32,
                    skipped_count: skipped,
                    created: Vec::new(),
                })
            }
        }
    };
    let result = result?;
    session.project.put(draft).map_err(GuiMutationError::from)?;
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    Ok(result)
}

pub(super) fn edit_clip_selection(
    session: &mut ProjectSession,
    owner: &donder_project_io::DocumentId,
    sequence_id: &SequenceId,
    clip_ids: &[u32],
    edit: SequenceClipCommonEdit,
) -> Result<(), GuiMutationError> {
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    if clip_ids.is_empty() {
        return Err(GuiMutationError::Invalid(
            "At least one clip must be selected.".into(),
        ));
    }

    let param_edit = if let SequenceClipCommonEdit::Param { name, value } = &edit {
        let name = identifier(name)?;
        let sequence = session
            .project
            .get::<donder_model::Sequence>(*sequence_id)
            .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
        for id in clip_ids {
            let clip = sequence
                .clips
                .iter()
                .find(|clip| clip.id.0 == *id)
                .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))?;
            let definition = session
                .project
                .get::<donder_model::EffectDefinition>(clip.definition)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Effect definition was not found.".into())
                })?;
            if !definition.params().iter().any(|param| param.name == name) {
                return Err(GuiMutationError::Invalid(format!(
                    "Clip {id} does not declare parameter `{}`.",
                    name.as_str()
                )));
            }
            if sequence.automation_clips.iter().any(|clip| {
                clip.bindings.iter().any(|binding| {
                    binding
                        .clip_param()
                        .is_some_and(|(clip_id, param)| clip_id.0 == *id && param == &name)
                })
            }) {
                return Err(GuiMutationError::Invalid(format!(
                    "Clip {id} parameter `{}` is automated.",
                    name.as_str()
                )));
            }
        }
        let value = param_value_from_gui(session, owner, value.clone())?;
        let sequence = session
            .project
            .get::<donder_model::Sequence>(*sequence_id)
            .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
        for id in clip_ids {
            let clip = sequence
                .clips
                .iter()
                .find(|clip| clip.id.0 == *id)
                .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))?;
            let definition = session
                .project
                .get::<donder_model::EffectDefinition>(clip.definition)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Effect definition was not found.".into())
                })?;
            let declaration = definition
                .params()
                .iter()
                .find(|param| param.name == name)
                .ok_or_else(|| GuiMutationError::Invalid("Clip parameter was not found.".into()))?;
            if !donder_model::param_matches_type(&value, &declaration.ty) {
                return Err(GuiMutationError::Invalid(format!(
                    "Clip {id} parameter `{}` cannot accept this value.",
                    name.as_str()
                )));
            }
        }
        Some((name, value))
    } else {
        None
    };

    let sequence = &mut draft;
    match edit {
        SequenceClipCommonEdit::Layer { layer_id } => {
            if !sequence.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
            }
            for id in clip_ids {
                clip_mut(sequence, *id)?.layer_id = SequenceLayerId(layer_id);
            }
        }
        SequenceClipCommonEdit::Scope { scope } => {
            let scope = clip_scope(scope);
            for id in clip_ids {
                clip_mut(sequence, *id)?.scope = scope.clone();
            }
        }
        SequenceClipCommonEdit::Start { start_seconds } => {
            if !start_seconds.is_finite() {
                return Err(GuiMutationError::Invalid(
                    "Clip start must be finite.".into(),
                ));
            }
            let start = super::checked_gui_time(start_seconds.max(0.0))?;
            for id in clip_ids {
                clip_mut(sequence, *id)?.start = start.clone();
            }
        }
        SequenceClipCommonEdit::Duration { duration_seconds } => {
            if !duration_seconds.is_finite() {
                return Err(GuiMutationError::Invalid(
                    "Clip duration must be finite.".into(),
                ));
            }
            let duration = super::checked_gui_duration(duration_seconds.max(0.000000001))?;
            for id in clip_ids {
                clip_mut(sequence, *id)?.duration = duration.clone();
            }
        }
        SequenceClipCommonEdit::Param { .. } => {
            let (name, value) = param_edit
                .ok_or_else(|| GuiMutationError::Invalid("Parameter edit is missing.".into()))?;
            for id in clip_ids {
                clip_mut(sequence, *id)?
                    .param_overrides
                    .insert(name.clone(), value.clone());
            }
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(())
}

pub(super) fn move_clip_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    clip_ids: &[u32],
    automation_ids: &[u32],
    time_delta_seconds: f64,
    anchor_lane: usize,
    lane_delta: i32,
) -> Result<(), GuiMutationError> {
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let targets = lane_targets(session)
        .ok_or_else(|| GuiMutationError::Invalid("Active layout is missing.".into()))?;
    let destination = |target: &FixtureTarget| -> Result<FixtureTarget, GuiMutationError> {
        let index = nearest_lane(&targets, target, anchor_lane)
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
    let sequence = &mut draft;
    for id in clip_ids {
        let clip = clip_mut(sequence, *id)?;
        clip.target = destination(&clip.target)?;
        clip.start = shifted_start(&clip.start, time_delta_seconds)?;
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
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(())
}

fn shifted_start(start: &DonderTime, delta: f64) -> Result<DonderTime, GuiMutationError> {
    let seconds = start.as_seconds_f64() + delta;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(GuiMutationError::Invalid(
            "Clip start must be finite and nonnegative.".into(),
        ));
    }
    super::checked_gui_time(seconds)
}

pub(super) fn resize_clip_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    clip_ids: &[u32],
    automation_ids: &[u32],
    edge: SequenceResizeEdge,
    automation: SequenceAutomationResize,
    time_delta_seconds: f64,
) -> Result<(), GuiMutationError> {
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let resize =
        |start: &mut DonderTime, duration: &mut DonderDuration| -> Result<(), GuiMutationError> {
            let seconds = duration.as_seconds_f64()
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
            *duration = super::checked_gui_duration(seconds)?;
            Ok(())
        };
    let sequence = &mut draft;
    for id in clip_ids {
        let clip = clip_mut(sequence, *id)?;
        resize(&mut clip.start, &mut clip.duration)?;
    }
    for id in automation_ids {
        let clip = sequence
            .automation_clips
            .iter_mut()
            .find(|clip| clip.id.0 == *id)
            .ok_or_else(|| GuiMutationError::Invalid("Automation clip is missing.".into()))?;
        let (mut start, mut duration) = (clip.start.clone(), clip.duration.clone());
        resize(&mut start, &mut duration)?;
        match automation {
            SequenceAutomationResize::Crop => clip.crop(start, duration),
            SequenceAutomationResize::Stretch => {
                clip.start = start;
                clip.duration = duration;
            }
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(())
}

pub(super) fn move_mark_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f64,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let sequence = &mut draft;
    let mut moved = Vec::new();
    for (collection_key, indexes) in mark_indexes_by_collection(marks) {
        let mut moved_times = Vec::new();
        for index in indexes {
            let collection = mark_collection_mut(sequence, &collection_key)?;
            if let Some(value) = collection.marks.get_mut(index) {
                value.time = super::checked_gui_time(
                    (value.time.as_seconds_f64() + time_delta_seconds).max(0.0),
                )?;
                moved_times.push(value.time.clone());
            }
        }
        let collection = mark_collection_mut(sequence, &collection_key)?;
        collection.marks.sort_by_key(|mark| mark.time.0);
        for time in moved_times {
            if let Some(index) = collection.marks.iter().position(|value| value.time == time) {
                moved.push(SequenceMarkRef {
                    collection_key: collection_key.clone(),
                    index: index as u32,
                });
            }
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(moved)
}

fn sequence_mark<'a>(
    sequence: &'a donder_model::Sequence,
    mark: &SequenceMarkRef,
) -> Option<&'a donder_model::Mark> {
    sequence
        .mark_collections
        .iter()
        .find(|collection| collection.key.name.as_str() == mark.collection_key)?
        .marks
        .get(mark.index as usize)
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

/// The target of every timeline lane, in display order.
pub(super) fn lane_targets(session: &ProjectSession) -> Option<Vec<FixtureTarget>> {
    let layout = active_layout(session)?;
    Some(
        super::projection::lane_walk(layout)
            .into_iter()
            .map(|(fixture, _)| FixtureTarget {
                layout: layout.id,
                fixture: fixture.id,
            })
            .collect(),
    )
}

/// The lane of `target` nearest `anchor`; the earlier one on a tie.
pub(super) fn nearest_lane(
    lanes: &[FixtureTarget],
    target: &FixtureTarget,
    anchor: usize,
) -> Option<usize> {
    lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| *lane == target)
        .min_by_key(|(index, _)| index.abs_diff(anchor))
        .map(|(index, _)| index)
}

/// A target's first lane, which clipboard entries use as their row.
fn target_lane_index(session: &ProjectSession, target: &FixtureTarget) -> Option<usize> {
    nearest_lane(&lane_targets(session)?, target, 0)
}

pub(super) fn target_for_lane(
    session: &ProjectSession,
    lane_index: usize,
) -> Option<FixtureTarget> {
    lane_targets(session)?.into_iter().nth(lane_index)
}
use std::collections::BTreeMap;

use donder_language::{DonderDuration, DonderTime};
use donder_model::ClipId;
use donder_model::FixtureTarget;
use donder_model::{AutomationTarget, SequenceId, SequenceLayerId};
use donder_project_io::ProjectSession;

use super::model::{clip_mut, clip_scope, identifier, mark_collection_mut, param_value_from_gui};
use super::projection::active_layout;
use super::{
    ClipboardAutomation, ClipboardClip, ClipboardMark, GuiEditOutcome, GuiMutationError,
    SequenceClipboard,
};
use crate::dto::{
    SequenceAutomationResize, SequenceClipCommonEdit, SequenceClipRef, SequenceMarkRef,
    SequencePasteAnchor, SequenceResizeEdge, SequenceSelection,
};

/// The clip and automation clip ids of `clips`, in order.
pub(super) fn split_clips(clips: &[SequenceClipRef]) -> (Vec<u32>, Vec<u32>) {
    let mut clip_ids = Vec::new();
    let mut automation = Vec::new();
    for clip in clips {
        match *clip {
            SequenceClipRef::Clip { id } => clip_ids.push(id),
            SequenceClipRef::Automation { id } => automation.push(id),
        }
    }
    (clip_ids, automation)
}

/// The clips, then the automation clips, as clip references.
pub(super) fn clip_refs(clip_ids: &[u32], automation: &[u32]) -> Vec<SequenceClipRef> {
    clip_ids
        .iter()
        .map(|&id| SequenceClipRef::Clip { id })
        .chain(
            automation
                .iter()
                .map(|&id| SequenceClipRef::Automation { id }),
        )
        .collect()
}
