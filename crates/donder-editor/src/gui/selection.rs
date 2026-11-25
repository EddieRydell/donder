/// `selection` of a sequence as clipboard text: the sequence with only the
/// selected clips and marks, envelopes keeping the bindings of the copied
/// clips and of the graph's nodes. None when nothing was copied.
pub(super) fn copy_sequence_selection(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
    selection: &SequenceSelection,
) -> Result<Option<String>, GuiMutationError> {
    let sequence = resolved.view::<donder_model::Sequence>(session).object();
    let mut copied = sequence.clone();
    copied.description = None;
    copied.audio = donder_model::SequenceAudio::None;
    let copied_count = match selection {
        SequenceSelection::Clips { ids } => {
            if ids
                .iter()
                .any(|id| sequence.composition.clip(ClipId(*id)).is_none())
            {
                return Err(GuiMutationError::Invalid(
                    "Selected clip is missing.".into(),
                ));
            }
            copied
                .composition
                .clips
                .retain(|clip| ids.contains(&clip.id.0));
            for clip in &mut copied.composition.clips {
                let copies = |target: &AutomationTarget| !matches!(target, AutomationTarget::ClipParam { clip_id, .. } if !ids.contains(&clip_id.0));
                if clip.envelope().is_some_and(|envelope| {
                    !envelope
                        .bindings
                        .iter()
                        .all(|binding| copies(&binding.target))
                }) && let Some(envelope) = std::sync::Arc::make_mut(clip).envelope_mut()
                {
                    envelope.bindings.retain(|binding| copies(&binding.target));
                }
            }
            for collection in &mut copied.composition.marks {
                collection.marks.clear();
            }
            copied.composition.clips.len()
        }
        SequenceSelection::Marks { marks } => {
            copied.composition.clips.clear();
            for collection in &mut copied.composition.marks {
                let id = collection.id.0;
                collection.marks = std::mem::take(&mut collection.marks)
                    .into_iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        marks
                            .iter()
                            .any(|mark| mark.collection_id == id && mark.index as usize == *index)
                    })
                    .map(|(_, mark)| mark)
                    .collect();
            }
            copied
                .composition
                .marks
                .iter()
                .map(|collection| collection.marks.len())
                .sum()
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
    Ok(text)
}

pub(super) fn delete_sequence_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    selection: &SequenceSelection,
) -> Result<(), GuiMutationError> {
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
    let sequence = &mut draft;
    match selection {
        SequenceSelection::Clips { ids } => {
            sequence
                .composition
                .clips
                .retain(|clip| !ids.contains(&clip.id.0));
        }
        SequenceSelection::Marks { marks } => {
            for (collection_id, indexes) in mark_indexes_by_collection(marks) {
                for index in indexes.into_iter().rev() {
                    let collection = mark_collection_mut(sequence, collection_id)?;
                    if index < collection.marks.len() {
                        collection.marks.remove(index);
                    }
                }
            }
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(())
}

/// Paste the clipboard's `text` at `anchor`: its clips from the anchor's lane
/// and time, each effect on this sequence's layer of its layer's name, or
/// else its marks into this sequence's collections of the same names. An
/// envelope keeps the bindings of the clips pasted with it, and of this
/// sequence's graph nodes that no envelope it overlaps binds.
pub(super) fn paste_sequence_clipboard(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    anchor: SequencePasteAnchor,
    text: &str,
    texts: &mut donder_project_io::DocumentTextCache,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let pasted = donder_project_io::read_sequence_clipboard(session, text, texts)
        .map_err(GuiMutationError::Invalid)?;
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
    super::checked_gui_time(anchor.time_seconds)?;
    let outcome = if pasted.composition.clips.is_empty() {
        paste_marks(&mut draft, &pasted, &anchor)?
    } else {
        paste_clips(session, &mut draft, &pasted, &anchor)?
    };
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(outcome)
}

fn paste_clips(
    session: &ProjectSession,
    sequence: &mut donder_model::Sequence,
    pasted: &donder_model::Sequence,
    anchor: &SequencePasteAnchor,
) -> Result<GuiEditOutcome, GuiMutationError> {
    let targets = lane_targets(session);
    let anchor_lane = anchor
        .lane
        .ok_or_else(|| GuiMutationError::Invalid("Select a row to paste into.".into()))?
        as usize;
    if anchor_lane >= targets.len() {
        return Err(GuiMutationError::Invalid(
            "Select a row to paste into.".into(),
        ));
    }
    let lane = |target: &FixtureId| {
        target_lane_index(session, target).ok_or_else(|| {
            GuiMutationError::Invalid("A copied clip's row isn't in this sequence.".into())
        })
    };
    let lanes = pasted
        .composition
        .clips
        .iter()
        .map(|clip| lane(&clip.target))
        .collect::<Result<Vec<_>, _>>()?;
    let min_start = pasted
        .composition
        .clips
        .iter()
        .map(|clip| timed(clip).0.as_seconds_f64())
        .fold(f64::INFINITY, f64::min);
    let min_lane = lanes
        .iter()
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
    let destinations = lanes
        .iter()
        .map(|&lane| destination(lane))
        .collect::<Result<Vec<_>, _>>()?;
    let mut ids_after = IdAllocator::after(sequence.composition.clips.iter().map(|clip| clip.id.0));
    let id_map = pasted
        .composition
        .clips
        .iter()
        .map(|clip| Ok((clip.id, ClipId(ids_after.next()?))))
        .collect::<Result<BTreeMap<_, _>, GuiMutationError>>()?;
    let existing = sequence.composition.clips.len();
    let mut ids = Vec::new();
    for (pasted_clip, target) in pasted.composition.clips.iter().zip(destinations) {
        let mut clip = (**pasted_clip).clone();
        clip.id = id_map[&pasted_clip.id];
        clip.name = donder_language::unique_name(clip.name.as_str(), |name| {
            sequence
                .composition
                .clips
                .iter()
                .any(|other| other.name.as_str() == name)
        });
        *timed_mut(&mut clip).0 = super::checked_gui_time(
            anchor.time_seconds + timed(pasted_clip).0.as_seconds_f64() - min_start,
        )?;
        clip.target = target;
        let placed = clip.clone();
        match &mut clip.content {
            ClipContent::Effect(effect) => {
                effect.layer = pasted_layer(sequence, pasted, effect.layer.clone())?;
            }
            ClipContent::Envelope(envelope) => {
                let composition = &sequence.composition;
                envelope
                    .bindings
                    .retain_mut(|binding| match &mut binding.target {
                        AutomationTarget::ClipParam { clip_id, .. } => match id_map.get(clip_id) {
                            Some(id) => {
                                *clip_id = *id;
                                true
                            }
                            None => false,
                        },
                        AutomationTarget::CompositionNodeParam { node_id, .. } => {
                            let node_id = node_id.clone();
                            let target = &binding.target;
                            composition
                                .graph
                                .nodes
                                .iter()
                                .any(|node| node.id == node_id)
                                && !composition.clips[..existing].iter().any(|other| {
                                    other.overlaps(&placed)
                                        && other
                                            .envelope()
                                            .is_some_and(|other| other.binding(target).is_some())
                                })
                        }
                    });
            }
        }
        ids.push(clip.id.0);
        sequence.composition.clips.push(std::sync::Arc::new(clip));
    }
    Ok(GuiEditOutcome {
        selection: Some(SequenceSelection::Clips { ids }),
        ..GuiEditOutcome::default()
    })
}

/// This sequence's layer of the name `layer` has in `pasted`.
fn pasted_layer(
    sequence: &donder_model::Sequence,
    pasted: &donder_model::Sequence,
    layer: LayerId,
) -> Result<LayerId, GuiMutationError> {
    let name = &pasted
        .composition
        .layers
        .iter()
        .find(|candidate| candidate.id == layer)
        .unwrap_or_else(|| unreachable!("loading resolves a clip's layer"))
        .name;
    sequence
        .composition
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
        .composition
        .marks
        .iter()
        .flat_map(|collection| {
            collection
                .marks
                .iter()
                .map(move |mark| (&collection.name, mark))
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
    for (name, mark) in &marks {
        // The clipboard names its collections: marks paste into the
        // collection of the same name.
        let Some(collection) = sequence
            .composition
            .marks
            .iter_mut()
            .find(|collection| &collection.name == *name)
        else {
            continue;
        };
        let collection_id = collection.id.0;
        let time =
            super::checked_gui_time(anchor.time_seconds + mark.time.as_seconds_f64() - min_time)?;
        collection.marks.push(donder_model::Mark {
            time: time.clone(),
            duration: mark.duration.clone(),
            label: mark.label.clone(),
        });
        collection.marks.sort_by_key(|mark| mark.time.0);
        let index = collection
            .marks
            .iter()
            .position(|value| value.time == time)
            .ok_or_else(|| GuiMutationError::Invalid("Pasted mark is missing.".into()))?;
        selection.push(SequenceMarkRef {
            collection_id,
            index: index as u32,
        });
    }
    Ok(GuiEditOutcome {
        selection: Some(SequenceSelection::Marks { marks: selection }),
        ..GuiEditOutcome::default()
    })
}

pub(super) fn move_clip_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    ids: &[u32],
    time_delta_seconds: f64,
    anchor_lane: usize,
    lane_delta: i32,
) -> Result<(), GuiMutationError> {
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
    let targets = lane_targets(session);
    let destination = |target: &FixtureId| -> Result<FixtureId, GuiMutationError> {
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
    for id in ids {
        let clip = clip_mut(sequence, *id)?;
        clip.target = destination(&clip.target)?;
        let start = timed_mut(clip).0;
        *start = shifted_time(start, time_delta_seconds)?;
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(())
}

fn shifted_time(time: &DonderTime, delta: f64) -> Result<DonderTime, GuiMutationError> {
    super::checked_gui_time(time.as_seconds_f64() + delta)
}

pub(super) fn resize_clip_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    ids: &[u32],
    edge: SequenceResizeEdge,
    automation: SequenceAutomationResize,
    time_delta_seconds: f64,
) -> Result<(), GuiMutationError> {
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
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
    for id in ids {
        let clip = clip_mut(sequence, *id)?;
        let (mut start, mut duration) = {
            let (start, duration) = timed(clip);
            (start.clone(), duration.clone())
        };
        resize(&mut start, &mut duration)?;
        // An effect plays as long as its clip; an envelope's curve is cropped
        // or stretched.
        match automation {
            SequenceAutomationResize::Crop => clip.crop(start, duration),
            SequenceAutomationResize::Stretch => {
                clip.placement = donder_model::Placement::Timed { start, duration };
            }
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(())
}

pub(super) fn move_mark_selection(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    marks: &[SequenceMarkRef],
    time_delta_seconds: f64,
) -> Result<Vec<SequenceMarkRef>, GuiMutationError> {
    let mut draft = resolved
        .view::<donder_model::Sequence>(session)
        .object()
        .clone();
    let sequence = &mut draft;
    let mut moved = Vec::new();
    for (collection_id, indexes) in mark_indexes_by_collection(marks) {
        let mut moved_times = Vec::new();
        for index in indexes {
            let collection = mark_collection_mut(sequence, collection_id)?;
            if let Some(value) = collection.marks.get_mut(index) {
                value.time = shifted_time(&value.time, time_delta_seconds)?;
                moved_times.push(value.time.clone());
            }
        }
        let collection = mark_collection_mut(sequence, collection_id)?;
        collection.marks.sort_by_key(|mark| mark.time.0);
        for time in moved_times {
            if let Some(index) = collection.marks.iter().position(|value| value.time == time) {
                moved.push(SequenceMarkRef {
                    collection_id,
                    index: index as u32,
                });
            }
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(moved)
}

fn mark_indexes_by_collection(marks: &[SequenceMarkRef]) -> BTreeMap<u32, Vec<usize>> {
    let mut grouped = BTreeMap::<u32, Vec<usize>>::new();
    for mark in marks {
        grouped
            .entry(mark.collection_id)
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
pub(super) fn lane_targets(session: &ProjectSession) -> Vec<FixtureId> {
    let layout = session.project().layout();
    super::projection::lane_walk(&layout)
        .into_iter()
        .map(|(fixture, _)| fixture.id)
        .collect()
}

/// The lane of `target` nearest `anchor`; the earlier one on a tie.
pub(super) fn nearest_lane(
    lanes: &[FixtureId],
    target: &FixtureId,
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
fn target_lane_index(session: &ProjectSession, target: &FixtureId) -> Option<usize> {
    nearest_lane(&lane_targets(session), target, 0)
}

pub(super) fn target_for_lane(session: &ProjectSession, lane_index: usize) -> Option<FixtureId> {
    lane_targets(session).into_iter().nth(lane_index)
}
use std::collections::BTreeMap;

use donder_language::{DonderDuration, DonderTime};
use donder_model::ClipId;
use donder_model::FixtureId;
use donder_model::{AutomationTarget, ClipContent, LayerId};
use donder_project_io::ProjectSession;

use super::model::{clip_mut, mark_collection_mut, timed, timed_mut};
use super::{GuiEditOutcome, GuiMutationError, IdAllocator, ResolvedGuiObject};
use crate::dto::{
    SequenceAutomationResize, SequenceMarkRef, SequencePasteAnchor, SequenceResizeEdge,
    SequenceSelection,
};
