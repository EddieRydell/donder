pub(super) fn required_operator_param_value(
    ty: Type,
    sequence: &dawn_language::sequence::Sequence,
    color: dawn_language::values::Color,
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
        SequenceSelection::Effects { ids } => {
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
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
                    lane_index: effect_lane_index(session, &effect.target),
                });
            }
            let copied_count = copied.len() as u32;
            Ok((
                (!copied.is_empty()).then_some(SequenceClipboard::Effects(copied)),
                copied_count,
                skipped,
            ))
        }
        SequenceSelection::Marks { marks } => {
            let sequence =
                session.project.sequences.get(sequence_id).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence was not found.".to_string())
                })?;
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
        SequenceSelection::Effects { ids } => {
            sequence
                .effects
                .retain(|effect| !ids.contains(&effect.id.0));
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
    let Some(clipboard) = clipboard else {
        return Ok(SequenceSelectionMutation {
            selection: None,
            copied_count: 0,
            skipped_count: 0,
        });
    };
    let lane_count = sequence_lane_count(session);
    let lane_targets = (0..lane_count)
        .map(|lane| target_for_lane(session, lane))
        .collect::<Vec<_>>();
    match clipboard {
        SequenceClipboard::Effects(effects) => {
            let min_start = effects
                .iter()
                .map(|effect| effect.start_seconds)
                .fold(f32::INFINITY, f32::min);
            let min_lane = effects
                .iter()
                .map(|effect| effect.lane_index)
                .min()
                .unwrap_or_default();
            let sequence = sequence_mut(session, sequence_id)?;
            let mut next_id = sequence
                .effects
                .iter()
                .map(|effect| effect.id.0)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut pasted_ids = Vec::with_capacity(effects.len());
            for effect in effects {
                let mut value = effect.effect.clone();
                let target_lane = anchored_lane(
                    anchor.lane_index as usize,
                    effect.lane_index,
                    min_lane,
                    lane_count,
                );
                value.id = EffectInstId(next_id);
                value.start = DawnTime::from_seconds_f32(
                    (anchor.time_seconds + effect.start_seconds - min_start).max(0.0),
                );
                if let Some(Some(target)) = lane_targets.get(target_lane) {
                    value.target = target.clone();
                }
                sequence.effects.push(value);
                pasted_ids.push(next_id);
                next_id = next_id.saturating_add(1);
            }
            Ok(SequenceSelectionMutation {
                selection: Some(SequenceSelection::Effects { ids: pasted_ids }),
                copied_count: effects.len() as u32,
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
                    .push(DawnTime::from_seconds_f32(time_seconds));
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

pub(super) fn move_effect_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    time_delta_seconds: f32,
    lane_delta: i32,
) -> Result<Vec<u32>, GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let lane = shifted_lane(
            effect_lane_index(session, &effect.target),
            lane_delta,
            sequence_lane_count(session),
        );
        (
            effect.start.as_seconds_f32() + time_delta_seconds,
            effect.duration.as_seconds_f32(),
            lane,
        )
    })?;
    apply_effect_updates(session, sequence_id, effect_updates)
}

pub(super) fn resize_effect_selection(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    edge: SequenceResizeEdge,
    time_delta_seconds: f32,
) -> Result<(), GuiMutationError> {
    let effect_updates = effect_selection_updates(session, sequence_id, ids, |session, effect| {
        let start_seconds = effect.start.as_seconds_f32();
        let duration_seconds = effect.duration.as_seconds_f32();
        let lane = effect_lane_index(session, &effect.target);
        match edge {
            SequenceResizeEdge::Left => (
                start_seconds + time_delta_seconds,
                duration_seconds - time_delta_seconds,
                lane,
            ),
            SequenceResizeEdge::Right => {
                (start_seconds, duration_seconds + time_delta_seconds, lane)
            }
        }
    })?;
    apply_effect_updates(session, sequence_id, effect_updates)?;
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
                *value = DawnTime::from_seconds_f32(time_seconds);
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

pub(super) struct EffectUpdate {
    id: u32,
    start_seconds: f32,
    duration_seconds: f32,
    lane_index: usize,
}

fn effect_selection_updates(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    ids: &[u32],
    update: impl Fn(&ProjectSession, &dawn_language::effect::EffectInst) -> (f32, f32, usize),
) -> Result<Vec<EffectUpdate>, GuiMutationError> {
    let sequence = session
        .project
        .sequences
        .get(sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    Ok(sequence
        .effects
        .iter()
        .filter(|effect| ids.contains(&effect.id.0))
        .map(|effect| {
            let (start_seconds, duration_seconds, lane_index) = update(session, effect);
            EffectUpdate {
                id: effect.id.0,
                start_seconds,
                duration_seconds,
                lane_index,
            }
        })
        .collect())
}

fn apply_effect_updates(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    updates: Vec<EffectUpdate>,
) -> Result<Vec<u32>, GuiMutationError> {
    let targets = updates
        .iter()
        .map(|update| (update.id, target_for_lane(session, update.lane_index)))
        .collect::<Vec<_>>();
    let sequence = sequence_mut(session, sequence_id)?;
    let mut moved = Vec::new();
    for update in updates {
        let effect = effect_mut(sequence, update.id)?;
        effect.start = DawnTime::from_seconds_f32(update.start_seconds.max(0.0));
        effect.duration = DawnDuration::from_seconds_f32(update.duration_seconds.max(0.000000001));
        if let Some((_, Some(target))) = targets.iter().find(|(id, _)| *id == update.id) {
            effect.target = target.clone();
        }
        moved.push(update.id);
    }
    Ok(moved)
}

fn mark_time_seconds(
    sequence: &dawn_language::sequence::Sequence,
    mark: &SequenceMarkRef,
) -> Option<f32> {
    sequence
        .mark_collections
        .iter()
        .find(|collection| collection.key.name == mark.collection_key)?
        .marks
        .get(mark.index as usize)
        .map(DawnTime::as_seconds_f32)
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

fn effect_lane_index(session: &ProjectSession, target: &FixtureTarget) -> usize {
    effect_lane_index_resolved(session, target).unwrap_or_default()
}

pub(super) fn effect_lane_index_resolved(
    session: &ProjectSession,
    target: &FixtureTarget,
) -> Option<usize> {
    let layout = active_layout(session)?;
    if layout.id != target.layout {
        return None;
    }
    layout
        .iter_fixtures()
        .position(|fixture| fixture.id == target.fixture)
}

fn sequence_lane_count(session: &ProjectSession) -> usize {
    active_layout(session)
        .map(|layout| layout.iter_fixtures().count())
        .unwrap_or_default()
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

fn shifted_lane(lane_index: usize, lane_delta: i32, lane_count: usize) -> usize {
    if lane_count == 0 {
        return 0;
    }
    (lane_index as i32 + lane_delta).clamp(0, lane_count.saturating_sub(1) as i32) as usize
}

fn anchored_lane(
    anchor_lane: usize,
    lane_index: usize,
    min_lane: usize,
    lane_count: usize,
) -> usize {
    if lane_count == 0 {
        return lane_index;
    }
    (anchor_lane + lane_index.saturating_sub(min_lane)).min(lane_count.saturating_sub(1))
}

pub(super) fn mark_param_names(
    session: &ProjectSession,
    reference: &SequenceEffectReference,
) -> Result<Vec<String>, GuiMutationError> {
    let reference = match reference {
        SequenceEffectReference::Builtin { effect } => EffectRef::Builtin(match effect {
            SequenceBuiltinEffect::Pulse => BuiltinEffect::Pulse,
            SequenceBuiltinEffect::Chase => BuiltinEffect::Chase,
            SequenceBuiltinEffect::Spin => BuiltinEffect::Spin,
            SequenceBuiltinEffect::MarkPulse => BuiltinEffect::MarkPulse,
            SequenceBuiltinEffect::MarkChase => BuiltinEffect::MarkChase,
        }),
        SequenceEffectReference::Custom {
            module_id,
            path,
            effect_name,
        } => {
            let identity = source_identity_from_gui(module_id, path, effect_name)?;
            if session.source.module(identity.module_id()).is_none() {
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

use dawn_language::dsl::Type;
use dawn_language::effect::{
    BuiltinEffect, EffectDefinitionId, EffectInstId, EffectParamValue, EffectRef,
};
use dawn_language::layout::FixtureTarget;
use dawn_language::sequence::{AutomationDetachmentReason, AutomationTarget, SequenceId};
use dawn_language::values::{DawnDuration, DawnTime};
use dawn_project_io::ProjectSession;

use super::model::{effect_mut, mark_collection_mut, sequence_mut, source_identity_from_gui};
use super::projection::active_layout;
use super::{
    ClipboardEffect, ClipboardMark, GuiMutationError, SequenceClipboard, SequenceSelectionMutation,
};
use crate::dto::{
    SequenceBuiltinEffect, SequenceEffectReference, SequenceMarkRef, SequencePasteAnchor,
    SequenceResizeEdge, SequenceSelection,
};
