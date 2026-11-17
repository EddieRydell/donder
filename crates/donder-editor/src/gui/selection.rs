/// `selection` of a sequence as clipboard text: the sequence with only the
/// selected clips, automation clips and marks, automation keeping the
/// bindings of the copied clips and of the graph's nodes. None when nothing
/// was copied, with the counts of items copied and of selected items missing.
pub(super) fn copy_sequence_selection(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    selection: &SequenceSelection,
) -> Result<(Option<String>, u32, u32), GuiMutationError> {
    let sequence = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    let mut copied = sequence.clone();
    copied.description = None;
    copied.audio = donder_model::SequenceAudio::None;
    let (copied_count, skipped) = match selection {
        SequenceSelection::Clips { clips } => {
            let (ids, automation_ids) = &split_clips(clips);
            let skipped = ids
                .iter()
                .filter(|id| !sequence.clips.iter().any(|clip| clip.id.0 == **id))
                .count();
            if automation_ids.iter().any(|id| {
                !sequence
                    .automation_clips
                    .iter()
                    .any(|clip| clip.id.0 == *id)
            }) {
                return Err(GuiMutationError::Invalid(
                    "Selected automation clip is missing.".into(),
                ));
            }
            copied.clips.retain(|clip| ids.contains(&clip.id.0));
            copied
                .automation_clips
                .retain(|clip| automation_ids.contains(&clip.id.0));
            for clip in &mut copied.automation_clips {
                clip.remove_bindings(|target| {
                    matches!(target, AutomationTarget::ClipParam { clip_id, .. } if !ids.contains(&clip_id.0))
                });
            }
            for collection in &mut copied.mark_collections {
                collection.marks.clear();
            }
            (copied.clips.len() + copied.automation_clips.len(), skipped)
        }
        SequenceSelection::Marks { marks } => {
            copied.clips.clear();
            copied.automation_clips.clear();
            let skipped = marks
                .iter()
                .filter(|mark| sequence_mark(sequence, mark).is_none())
                .count();
            for collection in &mut copied.mark_collections {
                let key = collection.key.name.as_str().to_string();
                collection.marks = std::mem::take(&mut collection.marks)
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        marks
                            .iter()
                            .any(|mark| mark.collection_key == key && mark.index as usize == *index)
                    })
                    .map(|(_, mark)| mark)
                    .collect();
            }
            (marks.len() - skipped, skipped)
        }
    };
    let text = if copied_count == 0 {
        None
    } else {
        Some(
            donder_project_io::write_sequence_clipboard(session, copied)
                .map_err(GuiMutationError::Invalid)?,
        )
    };
    Ok((text, copied_count as u32, skipped as u32))
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

/// Paste the clipboard's `text` at `anchor`: its clips and automation clips
/// from the anchor's lane and time, each on this sequence's layer of its
/// layer's name, or else its marks into this sequence's collections of the
/// same names. An automation clip keeps the bindings of the clips pasted with
/// it, and of this sequence's graph nodes that no automation clip it overlaps
/// binds.
pub(super) fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    anchor: SequencePasteAnchor,
    text: &str,
    texts: &mut donder_project_io::DocumentTextCache,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let pasted = donder_project_io::read_sequence_clipboard(session, text, texts)
        .map_err(GuiMutationError::Invalid)?;
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    super::checked_gui_time(anchor.time_seconds)?;
    let outcome = if pasted.clips.is_empty() && pasted.automation_clips.is_empty() {
        paste_marks(&mut draft, &pasted, &anchor)?
    } else {
        paste_clips(session, &mut draft, &pasted, &anchor)?
    };
    session.project.put(draft).map_err(GuiMutationError::from)?;
    donder_project_io::maintain_ownership_sources(session)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    Ok(outcome)
}

fn paste_clips(
    session: &ProjectSession,
    sequence: &mut donder_model::Sequence,
    pasted: &donder_model::Sequence,
    anchor: &SequencePasteAnchor,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let targets = lane_targets(session)
        .ok_or_else(|| GuiMutationError::Invalid("This sequence has no rows.".into()))?;
    let anchor_lane = anchor
        .lane
        .ok_or_else(|| GuiMutationError::Invalid("Select a row to paste into.".into()))?
        as usize;
    if anchor_lane >= targets.len() {
        return Err(GuiMutationError::Invalid(
            "Select a row to paste into.".into(),
        ));
    }
    let lane = |target: &FixtureTarget| {
        target_lane_index(session, target).ok_or_else(|| {
            GuiMutationError::Invalid("A copied clip's row isn't in this sequence.".into())
        })
    };
    let clip_lanes = pasted
        .clips
        .iter()
        .map(|clip| lane(&clip.target))
        .collect::<Result<Vec<_>, _>>()?;
    let automation_lanes = pasted
        .automation_clips
        .iter()
        .map(|clip| lane(&clip.row_target))
        .collect::<Result<Vec<_>, _>>()?;
    let min_start = pasted
        .clips
        .iter()
        .map(|clip| clip.start.as_seconds_f64())
        .chain(
            pasted
                .automation_clips
                .iter()
                .map(|clip| clip.start.as_seconds_f64()),
        )
        .fold(f64::INFINITY, f64::min);
    let min_lane = clip_lanes
        .iter()
        .chain(&automation_lanes)
        .copied()
        .min()
        .ok_or_else(|| GuiMutationError::Invalid("Nothing to paste.".into()))?;
    let destination = |lane: usize| {
        targets
            .get(anchor_lane + lane - min_lane)
            .cloned()
            .ok_or_else(|| GuiMutationError::Invalid("The clips don't fit below this row.".into()))
    };
    // Resolve every destination before mutation; never collapse distinct rows at the boundary.
    let clip_targets = clip_lanes
        .iter()
        .map(|&lane| destination(lane))
        .collect::<Result<Vec<_>, _>>()?;
    let automation_targets = automation_lanes
        .iter()
        .map(|&lane| destination(lane))
        .collect::<Result<Vec<_>, _>>()?;
    let mut clip_ids_after = IdAllocator::after(sequence.clips.iter().map(|clip| clip.id.0));
    let mut clip_ids = Vec::new();
    let mut id_map = BTreeMap::new();
    for (pasted_clip, target) in pasted.clips.iter().zip(clip_targets) {
        let id = clip_ids_after.next()?;
        let mut clip = (**pasted_clip).clone();
        id_map.insert(clip.id.0, id);
        clip.id = ClipId(id);
        clip.name = donder_language::unique_name(clip.name.as_str(), |name| {
            sequence
                .clips
                .iter()
                .any(|other| other.name.as_str() == name)
        });
        clip.layer_id = pasted_layer(sequence, pasted, clip.layer_id)?;
        clip.start = super::checked_gui_time(
            anchor.time_seconds + pasted_clip.start.as_seconds_f64() - min_start,
        )?;
        clip.target = target;
        sequence.clips.push(std::sync::Arc::new(clip));
        clip_ids.push(id);
    }
    let mut automation_ids_after =
        IdAllocator::after(sequence.automation_clips.iter().map(|clip| clip.id.0));
    let mut automation_ids = Vec::new();
    for (pasted_clip, target) in pasted.automation_clips.iter().zip(automation_targets) {
        let id = automation_ids_after.next()?;
        let mut clip = pasted_clip.clone();
        clip.id = donder_model::AutomationClipId(id);
        clip.row_target = target;
        clip.start = super::checked_gui_time(
            anchor.time_seconds + pasted_clip.start.as_seconds_f64() - min_start,
        )?;
        let placed = clip.clone();
        let remap = |target: &mut AutomationTarget| match target {
            AutomationTarget::ClipParam { clip_id, .. } => match id_map.get(&clip_id.0) {
                Some(id) => {
                    clip_id.0 = *id;
                    true
                }
                None => false,
            },
            AutomationTarget::CompositionNodeParam { node_id, .. } => {
                sequence
                    .composition_graph
                    .nodes
                    .iter()
                    .any(|node| node.id == *node_id)
                    && !sequence.automation_clips.iter().any(|other| {
                        other.overlaps(&placed) && other.targets().any(|claimed| claimed == target)
                    })
            }
        };
        clip.bindings
            .retain_mut(|binding| remap(&mut binding.target));
        clip.detached_bindings
            .retain_mut(|binding| remap(&mut binding.target));
        sequence.automation_clips.push(clip);
        automation_ids.push(id);
    }
    Ok(GuiEditOutcome {
        selection: Some(SequenceSelection::Clips {
            clips: clip_refs(&clip_ids, &automation_ids),
        }),
        copied_count: (clip_ids.len() + automation_ids.len()) as u32,
        skipped_count: 0,
        created: Vec::new(),
    })
}

/// This sequence's layer of the name `layer` has in `pasted`.
fn pasted_layer(
    sequence: &donder_model::Sequence,
    pasted: &donder_model::Sequence,
    layer: SequenceLayerId,
) -> Result<SequenceLayerId, GuiMutationError> {
    let name = &pasted
        .layers
        .iter()
        .find(|candidate| candidate.id == layer)
        .unwrap_or_else(|| unreachable!("loading resolves a clip's layer"))
        .name;
    sequence
        .layers
        .iter()
        .find(|candidate| candidate.name == *name)
        .map(|candidate| candidate.id.clone())
        .ok_or_else(|| {
            GuiMutationError::Invalid(format!(
                "This sequence has no layer named {}.",
                name.as_str()
            ))
        })
}

fn paste_marks(
    sequence: &mut donder_model::Sequence,
    pasted: &donder_model::Sequence,
    anchor: &SequencePasteAnchor,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let marks = pasted
        .mark_collections
        .iter()
        .flat_map(|collection| {
            collection
                .marks
                .iter()
                .map(move |mark| (&collection.key, mark))
        })
        .collect::<Vec<_>>();
    if marks.is_empty() {
        return Err(GuiMutationError::Invalid("Nothing to paste.".into()));
    }
    let min_time = marks
        .iter()
        .map(|(_, mark)| mark.time.as_seconds_f64())
        .fold(f64::INFINITY, f64::min);
    let mut selection = Vec::new();
    let mut skipped = 0u32;
    for (key, mark) in &marks {
        let collection_key = key.name.as_str().to_string();
        let Ok(collection) = mark_collection_mut(sequence, &collection_key) else {
            skipped = skipped.saturating_add(1);
            continue;
        };
        let time =
            super::checked_gui_time(anchor.time_seconds + mark.time.as_seconds_f64() - min_time)?;
        collection.marks.push(donder_model::Mark {
            time: time.clone(),
            label: mark.label.clone(),
        });
        collection.marks.sort_by_key(|mark| mark.time.0);
        let index = collection
            .marks
            .iter()
            .position(|value| value.time == time)
            .ok_or_else(|| GuiMutationError::Invalid("Pasted mark is missing.".into()))?;
        selection.push(SequenceMarkRef {
            collection_key,
            index: index as u32,
        });
    }
    Ok(GuiEditOutcome {
        selection: Some(SequenceSelection::Marks { marks: selection }),
        copied_count: marks.len() as u32,
        skipped_count: skipped,
        created: Vec::new(),
    })
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
                    "The clip's effect has no parameter {}.",
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
                    "An envelope drives {}; edit the envelope instead.",
                    name.as_str()
                )));
            }
        }
        let value = param_value_from_gui(session, owner, value.clone())?;
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
            let start = super::checked_gui_time(start_seconds)?;
            for id in clip_ids {
                clip_mut(sequence, *id)?.start = start.clone();
            }
        }
        SequenceClipCommonEdit::Duration { duration_seconds } => {
            let duration = super::checked_gui_duration(duration_seconds)?;
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
        clip.start = shifted_time(&clip.start, time_delta_seconds)?;
    }
    for id in automation_ids {
        let clip = sequence
            .automation_clips
            .iter_mut()
            .find(|clip| clip.id.0 == *id)
            .ok_or_else(|| GuiMutationError::Invalid("Automation clip is missing.".into()))?;
        clip.row_target = destination(&clip.row_target)?;
        clip.start = shifted_time(&clip.start, time_delta_seconds)?;
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(())
}

fn shifted_time(time: &DonderTime, delta: f64) -> Result<DonderTime, GuiMutationError> {
    super::checked_gui_time(time.as_seconds_f64() + delta)
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
            *duration = super::checked_gui_duration(seconds)?;
            if matches!(edge, SequenceResizeEdge::Left) {
                *start = shifted_time(start, time_delta_seconds)?;
            }
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
                value.time = shifted_time(&value.time, time_delta_seconds)?;
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
use super::{GuiEditOutcome, GuiMutationError, IdAllocator};
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
