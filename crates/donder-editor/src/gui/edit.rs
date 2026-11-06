pub(super) fn edit_sequence(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: SequenceGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    let sequence_id = resolved.id::<SequenceId>();
    let mut created = Vec::new();
    let mut draft = session
        .project
        .get::<donder_model::Sequence>(sequence_id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".into()))?;
    let layout = active_layout(session)
        .map(|layout| layout.id)
        .ok_or_else(|| GuiMutationError::Invalid("Active layout was not found.".to_string()))?;
    if matches!(
        &edit,
        SequenceGuiEdit::AddClip { .. }
            | SequenceGuiEdit::AddAutomationClip { .. }
            | SequenceGuiEdit::CreateAndBindAutomationClip { .. }
            | SequenceGuiEdit::RetargetClip { .. }
    ) {
        donder_project_io::ensure_document_can_reference_object(
            session,
            resolved.document_id(),
            layout.into(),
        )
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    }
    match edit {
        SequenceGuiEdit::CutSelection { .. }
        | SequenceGuiEdit::DeleteSelection { .. }
        | SequenceGuiEdit::Paste { .. }
        | SequenceGuiEdit::MoveClips { .. }
        | SequenceGuiEdit::ResizeClips { .. }
        | SequenceGuiEdit::EditClips { .. }
        | SequenceGuiEdit::ArrangeClips { .. }
        | SequenceGuiEdit::MoveMarks { .. } => {
            unreachable!("selection edits are applied to the selection")
        }
        SequenceGuiEdit::Composition { edit } => {
            created =
                super::graph::edit_composition(session, resolved.document_id(), &mut draft, edit)?;
        }
        SequenceGuiEdit::SetDuration { duration_seconds } => {
            draft.duration = super::checked_gui_duration(duration_seconds)?;
        }
        SequenceGuiEdit::SetAudio { import_path } => {
            let audio = match import_path {
                Some(import_path) => {
                    let root = session.source.project_root().to_path_buf();
                    let id = donder_project_io::reference_asset(
                        &mut session.source.referenced_assets,
                        &root,
                        resolved.document_id(),
                        &import_path,
                    )
                    .map_err(|error| {
                        GuiMutationError::Invalid(format!("Audio file {import_path}: {error}"))
                    })?;
                    DomainSequenceAudio::Asset(id)
                }
                None => DomainSequenceAudio::None,
            };
            draft.audio = audio;
        }
        SequenceGuiEdit::RetargetClip { id, target } => {
            let sequence = &mut draft;
            let target = layout_target_to_clip_target(&layout, target);
            clip_mut(sequence, id)?.target = target;
        }
        SequenceGuiEdit::SplitClips { ids, time_seconds } => {
            let at = super::checked_gui_time(time_seconds)?;
            let sequence = &mut draft;
            let mut ids_after = IdAllocator::after(sequence.clips.iter().map(|clip| clip.id.0));
            for id in ids {
                let index = sequence
                    .clips
                    .iter()
                    .position(|clip| clip.id.0 == id)
                    .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))?;
                let left = &sequence.clips[index];
                let end = left.start.0.saturating_add(left.duration.0);
                if at.0 <= left.start.0 || at.0 >= end {
                    return Err(GuiMutationError::Invalid(
                        "Split time must fall inside the clip.".to_string(),
                    ));
                }
                let mut right = Clip::clone(left);
                right.id = ClipId(ids_after.next()?);
                right.name = donder_language::unique_name(left.name.as_str(), |name| {
                    sequence
                        .clips
                        .iter()
                        .any(|other| other.name.as_str() == name)
                });
                right.start = at.clone();
                right.duration = DonderDuration(end - at.0);
                let left = std::sync::Arc::make_mut(&mut sequence.clips[index]);
                left.duration = DonderDuration(at.0 - left.start.0);
                let left_id = left.id.clone();
                let rebind = |target: &AutomationTarget| match target {
                    AutomationTarget::ClipParam { clip_id, param } if *clip_id == left_id => {
                        Some(AutomationTarget::ClipParam {
                            clip_id: right.id.clone(),
                            param: param.clone(),
                        })
                    }
                    _ => None,
                };
                for clip in &mut sequence.automation_clips {
                    let bindings = clip
                        .bindings
                        .iter()
                        .filter_map(|binding| {
                            Some(AutomationBinding {
                                target: rebind(&binding.target)?,
                                window: binding.window,
                            })
                        })
                        .collect::<Vec<_>>();
                    let detached = clip
                        .detached_bindings
                        .iter()
                        .filter_map(|binding| rebind(&binding.target))
                        .map(|target| DetachedAutomationBinding { target })
                        .collect::<Vec<_>>();
                    clip.bindings.extend(bindings);
                    clip.detached_bindings.extend(detached);
                }
                sequence.clips.insert(index + 1, std::sync::Arc::new(right));
            }
        }
        SequenceGuiEdit::ReassignMarkCollection {
            collection_key,
            index,
            target_collection_key,
        } => {
            if collection_key != target_collection_key {
                let sequence = &mut draft;
                let mark = {
                    let collection = mark_collection_mut(sequence, &collection_key)?;
                    if (index as usize) >= collection.marks.len() {
                        return Err(GuiMutationError::Invalid("Mark was not found.".to_string()));
                    }
                    collection.marks.remove(index as usize)
                };
                let target_collection = mark_collection_mut(sequence, &target_collection_key)?;
                target_collection.marks.push(mark);
                target_collection.marks.sort_by_key(|mark| mark.time.0);
            }
        }
        SequenceGuiEdit::AddMarks {
            collection_key,
            times_seconds,
        } => {
            let collection = mark_collection_mut(&mut draft, &collection_key)?;
            for seconds in times_seconds {
                collection
                    .marks
                    .push(Mark::at(super::checked_gui_time(seconds)?));
            }
            collection.marks.sort_by_key(|mark| mark.time.0);
        }
        SequenceGuiEdit::SetMarkLabel {
            collection_key,
            index,
            label,
        } => {
            let collection = mark_collection_mut(&mut draft, &collection_key)?;
            let mark = collection
                .marks
                .get_mut(index as usize)
                .ok_or_else(|| GuiMutationError::Invalid("Mark was not found.".to_string()))?;
            mark.label = label
                .map(|label| label.trim().to_string())
                .filter(|label| !label.is_empty());
        }
        SequenceGuiEdit::CreateMarkCollections { collections } => {
            let sequence = &mut draft;
            for collection in collections {
                super::model::typed_name(&collection.name)?;
                let name = super::model::fresh_name(&collection.name, |candidate| {
                    sequence
                        .mark_collections
                        .iter()
                        .any(|existing| existing.key.name.as_str() == candidate)
                });
                let mut marks = collection
                    .marks_seconds
                    .iter()
                    .map(|&seconds| super::checked_gui_time(seconds).map(Mark::at))
                    .collect::<Result<Vec<_>, _>>()?;
                marks.sort_by_key(|mark| mark.time.0);
                created.push(name.as_str().to_string());
                sequence.mark_collections.push(MarkCollection {
                    key: MarkCollectionKey { name },
                    description: None,
                    display_color: parse_color(&collection.color)?,
                    marks,
                });
            }
        }
        SequenceGuiEdit::AddClip {
            effect: effect_reference,
            target,
            initial_color,
            scope,
            start_seconds,
        } => {
            let clip = super::graph::new_clip(
                session,
                resolved.document_id(),
                &draft,
                effect_reference,
                &initial_color,
            )?;
            draft.clips.push(std::sync::Arc::new(Clip {
                id: ClipId(clip.id),
                name: clip.name,
                description: None,
                layer_id: clip.layer_id,
                start: super::checked_gui_time(start_seconds)?,
                duration: super::checked_gui_duration(1.0)?,
                target: layout_target_to_clip_target(&layout, target),
                scope: clip_scope(scope),
                definition: clip.definition,
                param_overrides: clip.param_overrides,
            }));
        }
        SequenceGuiEdit::ChangeEffectDefinition {
            id,
            effect: effect_reference,
            initial_color,
        } => {
            let (definition, _, param_overrides) = imported_effect(
                session,
                resolved.document_id(),
                effect_reference,
                &initial_color,
            )?;
            let sequence = &mut draft;
            let clip = clip_mut(sequence, id)?;
            clip.definition = definition;
            clip.param_overrides = param_overrides;
            for clip in &mut sequence.automation_clips {
                clip.detach_bindings(|target| {
                    matches!(target, AutomationTarget::ClipParam { clip_id, .. } if clip_id.0 == id)
                });
            }
        }
        SequenceGuiEdit::AddAutomationClip {
            start_seconds,
            duration_seconds,
            row_target,
        } => {
            let sequence = &mut draft;
            let new_id = next_id(sequence.automation_clips.iter().map(|clip| clip.id.0))?;
            sequence.automation_clips.push(AutomationClip {
                id: AutomationClipId(new_id),
                start: super::checked_gui_time(start_seconds)?,
                duration: super::checked_gui_duration(duration_seconds)?,
                row_target: layout_target_to_clip_target(&layout, row_target),
                curve: default_automation_curve(),
                bindings: Vec::new(),
                detached_bindings: Vec::new(),
            });
        }
        SequenceGuiEdit::CreateAndBindAutomationClip { target } => {
            let target = automation_target_from_gui(target)?;
            automation_target_mapping(session, &sequence_id, &target)?;
            let (start, duration, row_target) = {
                let sequence = session
                    .project
                    .get::<donder_model::Sequence>(sequence_id)
                    .ok_or_else(|| {
                        GuiMutationError::Invalid("Sequence was not found.".to_string())
                    })?;
                automation_target_timing(session, sequence, &target)?
            };
            let new_id = next_id(draft.automation_clips.iter().map(|clip| clip.id.0))?;
            let mut clip = AutomationClip {
                id: AutomationClipId(new_id),
                start,
                duration,
                row_target,
                curve: default_automation_curve(),
                bindings: Vec::new(),
                detached_bindings: Vec::new(),
            };
            clip.bind(target, Window::FULL);
            draft.automation_clips.push(clip);
        }
        SequenceGuiEdit::SplitAutomationClip { id, time_seconds } => {
            let new_id = next_id(draft.automation_clips.iter().map(|clip| clip.id.0))?;
            let at = super::checked_gui_time(time_seconds)?;
            let index = draft
                .automation_clips
                .iter()
                .position(|clip| clip.id.0 == id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let right = draft.automation_clips[index]
                .split_off(at, AutomationClipId(new_id))
                .ok_or_else(|| {
                    GuiMutationError::Invalid(
                        "Split time must fall inside the automation clip.".to_string(),
                    )
                })?;
            draft.automation_clips.insert(index + 1, right);
        }
        SequenceGuiEdit::UpdateAutomationCurve { id, curve } => {
            let mut curve = curve_from_points(curve);
            super::model::collapse_coincident_points(&mut curve);
            automation_clip_mut(&mut draft, id)?.curve = curve;
        }
        SequenceGuiEdit::BindAutomationParam {
            automation_clip_id,
            target,
        } => {
            let target = automation_target_from_gui(target)?;
            automation_target_mapping(session, &sequence_id, &target)?;
            automation_clip_mut(&mut draft, automation_clip_id)?.bind(target, Window::FULL);
        }
        SequenceGuiEdit::SetAutomationWindow {
            automation_clip_id,
            target,
            window,
        } => {
            let target = automation_target_from_gui(target)?;
            let binding = automation_clip_mut(&mut draft, automation_clip_id)?
                .bindings
                .iter_mut()
                .find(|binding| binding.target == target)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation binding was not found.".to_string())
                })?;
            binding.window = window.into();
        }
        SequenceGuiEdit::UnbindAutomationParam {
            automation_clip_id,
            target,
        } => {
            let target = automation_target_from_gui(target)?;
            let sequence = &mut draft;
            let clip = sequence
                .automation_clips
                .iter()
                .find(|clip| clip.id.0 == automation_clip_id)
                .cloned()
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation clip was not found.".to_string())
                })?;
            let Some(binding) = clip
                .bindings
                .iter()
                .find(|binding| binding.target == target)
                .cloned()
            else {
                return Err(GuiMutationError::Invalid(
                    "Automation binding was not found.".to_string(),
                ));
            };
            let sample_seconds = match &target {
                AutomationTarget::ClipParam { clip_id, .. } => sequence
                    .clips
                    .iter()
                    .find(|clip| &clip.id == clip_id)
                    .map(|clip| clip.start.as_seconds_f64())
                    .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))?,
                AutomationTarget::CompositionNodeParam { .. } => clip.start.as_seconds_f64(),
            };
            let mapping = automation_target_mapping(session, &sequence_id, &binding.target)?;
            let value =
                automation_binding_value_at(&clip, binding.window, &mapping, sample_seconds)?;
            match &target {
                AutomationTarget::ClipParam { clip_id, param } => {
                    clip_mut(sequence, clip_id.0)?
                        .param_overrides
                        .insert(param.clone(), value);
                }
                AutomationTarget::CompositionNodeParam { node_id, param } => {
                    let node = super::graph::node_mut(sequence, node_id)?;
                    let CompositionGraphNodeKind::Operator(operator) = &mut node.kind else {
                        return Err(GuiMutationError::Invalid(
                            "Automation graph node is not an operator.".to_string(),
                        ));
                    };
                    operator.params.insert(param.clone(), value);
                }
            }
            automation_clip_mut(sequence, automation_clip_id)?
                .bindings
                .retain(|binding| binding.target != target);
        }
        SequenceGuiEdit::RebindDetachedAutomation {
            automation_clip_id,
            detached_index,
            target,
        } => {
            let target = automation_target_from_gui(target)?;
            automation_target_mapping(session, &sequence_id, &target)?;
            let clip = automation_clip_mut(&mut draft, automation_clip_id)?;
            if detached_index as usize >= clip.detached_bindings.len() {
                return Err(GuiMutationError::Invalid(
                    "Detached automation binding was not found.".to_string(),
                ));
            }
            clip.detached_bindings.remove(detached_index as usize);
            clip.bind(target, Window::FULL);
        }
        SequenceGuiEdit::DiscardDetachedAutomation {
            automation_clip_id,
            detached_index,
        } => {
            let clip = automation_clip_mut(&mut draft, automation_clip_id)?;
            if detached_index as usize >= clip.detached_bindings.len() {
                return Err(GuiMutationError::Invalid(
                    "Detached automation binding was not found.".to_string(),
                ));
            }
            clip.detached_bindings.remove(detached_index as usize);
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(created)
}

pub(super) fn automation_target_from_gui<Clip: GuiClipId>(
    target: SequenceAutomationTarget,
) -> Result<AutomationTarget<Clip>, GuiMutationError> {
    Ok(match target {
        SequenceAutomationTarget::ClipParam { clip_id, param } => AutomationTarget::ClipParam {
            clip_id: Clip::from_gui(clip_id),
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

fn automation_target_timing(
    session: &ProjectSession,
    sequence: &donder_model::Sequence,
    target: &AutomationTarget,
) -> Result<(DonderTime, DonderDuration, donder_model::FixtureTarget), GuiMutationError> {
    match target {
        AutomationTarget::ClipParam { clip_id, .. } => {
            let clip = sequence
                .clips
                .iter()
                .find(|clip| &clip.id == clip_id)
                .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".to_string()))?;
            Ok((
                clip.start.clone(),
                clip.duration.clone(),
                clip.target.clone(),
            ))
        }
        AutomationTarget::CompositionNodeParam { node_id, .. } => {
            let node = sequence
                .composition_graph
                .nodes
                .iter()
                .find(|node| &node.id == node_id)
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Automation graph node is missing.".to_string())
                })?;
            if !matches!(node.kind, CompositionGraphNodeKind::Operator(_)) {
                return Err(GuiMutationError::Invalid(
                    "Automation graph node is not an operator.".to_string(),
                ));
            }
            if sequence.layers.is_empty() {
                return Err(GuiMutationError::Invalid(
                    "Sequence has no lane for automation.".to_string(),
                ));
            }
            Ok((
                super::checked_gui_time(0.0)?,
                sequence.duration.clone(),
                super::selection::target_for_lane(session, 0).ok_or_else(|| {
                    GuiMutationError::Invalid("Sequence has no target for automation.".into())
                })?,
            ))
        }
    }
}

/// The target param's declared automation mapping.
fn automation_target_mapping(
    session: &ProjectSession,
    sequence_id: &SequenceId,
    target: &AutomationTarget,
) -> Result<AutomationMapping, GuiMutationError> {
    let sequence = session
        .project
        .get::<donder_model::Sequence>(*sequence_id)
        .ok_or_else(|| GuiMutationError::Invalid("Sequence was not found.".to_string()))?;
    donder_model::automation_target_param(&session.project, sequence, target)
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?
        .automation_mapping()
        .ok_or_else(|| GuiMutationError::Invalid("Param does not support automation.".to_string()))
}

use donder_language::{DonderDuration, DonderTime};
use donder_model::{
    AutomationBinding, AutomationClip, AutomationClipId, AutomationTarget,
    CompositionGraphNodeKind, DetachedAutomationBinding, Mark, MarkCollection, MarkCollectionKey,
    SequenceAudio as DomainSequenceAudio, SequenceId, Window,
};
use donder_model::{Clip, ClipId};
use donder_project_io::ProjectSession;
use donder_runtime_types::AutomationMapping;

use super::model::{
    automation_binding_value_at, automation_clip_mut, clip_mut, clip_scope, curve_from_points,
    default_automation_curve, identifier, imported_effect, layout_target_to_clip_target,
    mark_collection_mut, parse_color,
};
use super::projection::{GuiClipId, active_layout};
use super::{GuiMutationError, IdAllocator, ResolvedGuiObject, next_id};
use crate::dto::{SequenceAutomationTarget, SequenceGuiEdit};
