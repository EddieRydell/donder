//! Consoles: sequences built live from input devices.
use donder_model::{
    Action, AutomationTarget, ClipScope, CompositionGraphNodeKind, Console,
    ConsoleAutomationTarget, ConsoleClipId, ConsoleId, ControlRef, EffectDefinitionId,
    FixtureInstanceId, FixtureTarget as DomainFixtureTarget, InputDevice, InputDeviceId,
    InputEvent, LengthSource, LiveAutomation, LiveAutomationId, LiveMarks, MAX_COPIES,
    MarkCollectionKey, On, OnId, Play, PlayLength, PlayedQuantity, PlayedValue, SequenceLayerId,
};
use donder_project_io::{DocumentId, ProjectSession, ensure_document_can_reference_object};

use super::edit::automation_target_from_gui;
use super::model::{
    clip_scope, effect_ref_from_gui, fresh_name, identifier, object_address, object_id_from_gui,
    object_ref, param_value_from_gui, parse_color, required_param_values, typed_name,
};
use super::projection::{
    active_layout, automation_target_to_gui, composition_graph, curve_library, effect_definitions,
    effect_ref_to_gui, gradient_library, invocation_params, layers_to_gui, layout_lanes,
    target_label, target_to_gui,
};
use super::{GuiMutationError, ResolvedGuiObject, blocked, checked_gui_duration};
use crate::dto::{
    ConsoleAction, ConsoleAutomation, ConsoleClipEdit, ConsoleControlRef, ConsoleEventKind,
    ConsoleGuiDocument, ConsoleGuiEdit, ConsoleInput, ConsoleInputControl, ConsoleInputDevice,
    ConsoleLengthSource, ConsoleLiveMarks, ConsoleOn, ConsolePlayLength, ConsolePlayedQuantity,
    ConsolePlayedValue, FixtureTarget, GuiDocument, SequenceClipScope, SequenceEffectReference,
    SequenceParamAutomation,
};

pub(super) fn project_console(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = resolved.id::<ConsoleId>();
    let Some(console) = session.project.get::<Console>(id) else {
        return blocked("Console was not found.", Vec::new());
    };
    let mut targets = active_layout(session).map(layout_lanes).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    targets.retain(|lane| seen.insert(lane.target.fixture));
    GuiDocument::Console {
        document: ConsoleGuiDocument {
            description: console.description.clone(),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            name: object_address(session, id).declared_name().to_string(),
            perform_index: session
                .project
                .root()
                .consoles
                .iter()
                .position(|source| source.id() == &id)
                .map(|index| index as u32),
            frame_rate: console.frame_rate,
            live_marks: console
                .live_marks
                .iter()
                .map(|marks| ConsoleLiveMarks {
                    key: marks.key.name.as_str().to_string(),
                    description: marks.description.clone(),
                    color: marks.display_color.to_hex(),
                })
                .collect(),
            automations: console
                .automations
                .iter()
                .map(|automation| ConsoleAutomation {
                    id: automation.id.0,
                    name: automation.name.as_str().to_string(),
                    description: automation.description.clone(),
                    value: automation.value,
                    bindings: automation
                        .bindings
                        .iter()
                        .map(automation_target_to_gui)
                        .collect(),
                })
                .collect(),
            layers: layers_to_gui(&console.layers),
            composition_graph: composition_graph(
                session,
                &console.layers,
                &console.composition_graph,
                |node, param, _| {
                    live_automation(
                        console,
                        &AutomationTarget::CompositionNodeParam {
                            node_id: node.clone(),
                            param: param.clone(),
                        },
                    )
                },
            ),
            clips: console
                .clips
                .iter()
                .map(|clip| project_clip(session, console, clip))
                .collect(),
            always: console.always.iter().map(|clip| clip.0).collect(),
            events: console
                .events
                .iter()
                .map(|on| project_event(session, on))
                .collect(),
            targets,
            effect_definitions: effect_definitions(session),
            curve_library: curve_library(session),
            gradient_library: gradient_library(session),
            input_devices: input_devices(session),
            max_copies: u32::from(MAX_COPIES),
        },
    }
}

fn project_clip(
    session: &ProjectSession,
    console: &Console,
    clip: &donder_model::ConsoleClip,
) -> crate::dto::ConsoleClip {
    crate::dto::ConsoleClip {
        id: clip.id.0,
        name: clip.name.as_str().to_string(),
        description: clip.description.clone(),
        layer_id: clip.layer_id.0,
        target: target_to_gui(&clip.target),
        target_label: target_label(session, &clip.target),
        scope: match clip.scope {
            ClipScope::PerFixture => SequenceClipScope::PerFixture,
            ClipScope::WholeTarget => SequenceClipScope::WholeTarget,
        },
        effect: effect_ref_to_gui(session, &clip.definition),
        effect_name: session
            .project
            .get::<donder_model::EffectDefinition>(clip.definition)
            .map(|definition| definition.name().to_string())
            .unwrap_or_else(|| "Missing effect".to_string()),
        params: invocation_params(
            session,
            &clip.definition,
            &clip.param_overrides,
            |name, _| {
                live_automation(
                    console,
                    &AutomationTarget::ClipParam {
                        clip_id: clip.id,
                        param: name.clone(),
                    },
                )
            },
        ),
    }
}

/// The live automation that sets `target`, which the parameter editor shows
/// as automated.
fn live_automation(
    console: &Console,
    target: &ConsoleAutomationTarget,
) -> Option<SequenceParamAutomation> {
    console
        .automation_setting(target)
        .map(|automation| SequenceParamAutomation {
            automation_clip_id: automation.id.0,
        })
}

fn project_event(session: &ProjectSession, on: &On) -> ConsoleOn {
    ConsoleOn {
        id: on.id.0,
        event: match on.event {
            InputEvent::Press(_) => ConsoleEventKind::Press,
            InputEvent::Release(_) => ConsoleEventKind::Release,
            InputEvent::Move(_) => ConsoleEventKind::Move,
        },
        input: project_input(session, on.event.control()),
        actions: on.actions.iter().map(action_to_gui).collect(),
    }
}

fn action_to_gui(action: &Action) -> ConsoleAction {
    let values = |values: &[PlayedValue]| {
        values
            .iter()
            .map(|value| ConsolePlayedValue {
                param: value.param.as_str().to_string(),
                from: match value.from {
                    PlayedQuantity::Velocity => ConsolePlayedQuantity::Velocity,
                    PlayedQuantity::Pitch => ConsolePlayedQuantity::Pitch,
                    PlayedQuantity::PitchClass => ConsolePlayedQuantity::PitchClass,
                    PlayedQuantity::Fifths => ConsolePlayedQuantity::Fifths,
                },
            })
            .collect()
    };
    match action {
        Action::Play(play) => ConsoleAction::Play {
            clip_id: play.clip.0,
            copies: u32::from(play.copies),
            length: match &play.length {
                PlayLength::Fixed(duration) => ConsolePlayLength::Fixed {
                    duration_seconds: duration.as_seconds_f64(),
                },
                PlayLength::Scaled {
                    by,
                    shortest,
                    longest,
                } => ConsolePlayLength::Scaled {
                    by: match by {
                        LengthSource::Velocity => ConsoleLengthSource::Velocity,
                        LengthSource::Automation(id) => ConsoleLengthSource::Automation {
                            automation_id: id.0,
                        },
                    },
                    shortest_seconds: shortest.as_seconds_f64(),
                    longest_seconds: longest.as_seconds_f64(),
                },
            },
            values: values(&play.values),
        },
        Action::Start {
            clip,
            values: played,
        } => ConsoleAction::Start {
            clip_id: clip.0,
            values: values(played),
        },
        Action::Stop { clip } => ConsoleAction::Stop { clip_id: clip.0 },
        Action::Toggle {
            clip,
            values: played,
        } => ConsoleAction::Toggle {
            clip_id: clip.0,
            values: values(played),
        },
        Action::Mark(key) => ConsoleAction::Mark {
            into: key.name.as_str().to_string(),
        },
        Action::Set(id) => ConsoleAction::Set { into: id.0 },
    }
}

fn action_from_gui(action: ConsoleAction) -> Result<Action, GuiMutationError> {
    let values = |values: Vec<ConsolePlayedValue>| {
        values
            .into_iter()
            .map(|value| {
                Ok(PlayedValue {
                    param: identifier(&value.param)?,
                    from: match value.from {
                        ConsolePlayedQuantity::Velocity => PlayedQuantity::Velocity,
                        ConsolePlayedQuantity::Pitch => PlayedQuantity::Pitch,
                        ConsolePlayedQuantity::PitchClass => PlayedQuantity::PitchClass,
                        ConsolePlayedQuantity::Fifths => PlayedQuantity::Fifths,
                    },
                })
            })
            .collect::<Result<Vec<_>, GuiMutationError>>()
    };
    Ok(match action {
        ConsoleAction::Play {
            clip_id,
            copies,
            length,
            values: played,
        } => Action::Play(Play {
            clip: ConsoleClipId(clip_id),
            copies: u8::try_from(copies).map_err(|_| {
                GuiMutationError::Invalid(format!("A clip plays 1 to {MAX_COPIES} copies."))
            })?,
            length: match length {
                ConsolePlayLength::Fixed { duration_seconds } => {
                    PlayLength::Fixed(checked_gui_duration(duration_seconds)?)
                }
                ConsolePlayLength::Scaled {
                    by,
                    shortest_seconds,
                    longest_seconds,
                } => PlayLength::Scaled {
                    by: match by {
                        ConsoleLengthSource::Velocity => LengthSource::Velocity,
                        ConsoleLengthSource::Automation { automation_id } => {
                            LengthSource::Automation(LiveAutomationId(automation_id))
                        }
                    },
                    shortest: checked_gui_duration(shortest_seconds)?,
                    longest: checked_gui_duration(longest_seconds)?,
                },
            },
            values: values(played)?,
        }),
        ConsoleAction::Start {
            clip_id,
            values: played,
        } => Action::Start {
            clip: ConsoleClipId(clip_id),
            values: values(played)?,
        },
        ConsoleAction::Stop { clip_id } => Action::Stop {
            clip: ConsoleClipId(clip_id),
        },
        ConsoleAction::Toggle {
            clip_id,
            values: played,
        } => Action::Toggle {
            clip: ConsoleClipId(clip_id),
            values: values(played)?,
        },
        ConsoleAction::Mark { into } => Action::Mark(MarkCollectionKey {
            name: identifier(&into)?,
        }),
        ConsoleAction::Set { into } => Action::Set(LiveAutomationId(into)),
    })
}

fn project_input(session: &ProjectSession, input: &ControlRef) -> ConsoleInput {
    let device = session
        .project
        .get::<donder_model::InputDevice>(input.device);
    ConsoleInput {
        control: ConsoleControlRef {
            device: object_ref(session, input.device),
            control: input.control.0,
        },
        device_name: object_address(session, input.device)
            .declared_name()
            .to_string(),
        control_name: device
            .and_then(|device| device.control(input.control))
            .map(|control| control.name.as_str().to_string())
            .unwrap_or_else(|| format!("Missing control {}", input.control.0)),
    }
}

/// A device's buttons, or its axes.
fn controls(device: &InputDevice, buttons: bool) -> Vec<ConsoleInputControl> {
    device
        .controls
        .iter()
        .filter(|control| control.kind.is_button() == buttons)
        .map(|control| ConsoleInputControl {
            control: control.id.0,
            name: control.name.as_str().to_string(),
            notes: control.kind.notes().is_some(),
        })
        .collect()
}

/// The active setup's input devices and their buttons and axes.
fn input_devices(session: &ProjectSession) -> Vec<ConsoleInputDevice> {
    let Some(setup) = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
    else {
        return Vec::new();
    };
    setup
        .input_devices
        .iter()
        .filter_map(|source| {
            let id = source.id();
            let device = session.project.get::<donder_model::InputDevice>(*id)?;
            Some(ConsoleInputDevice {
                device: object_ref(session, *id),
                name: object_address(session, *id).declared_name().to_string(),
                buttons: controls(device, true),
                axes: controls(device, false),
            })
        })
        .collect()
}

pub(super) fn edit_console(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: ConsoleGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    let id = resolved.id::<ConsoleId>();
    let mut created = Vec::new();
    let mut draft = session
        .project
        .get::<Console>(id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Console was not found.".into()))?;
    let owner = resolved.document_id();
    match edit {
        ConsoleGuiEdit::SetFrameRate { frame_rate } => draft.frame_rate = frame_rate,
        ConsoleGuiEdit::Composition { edit } => {
            created = super::graph::edit_composition(session, owner, &mut draft, edit)?;
        }
        ConsoleGuiEdit::AddLiveMarks { name, color } => {
            typed_name(&name)?;
            let name = fresh_name(&name, |candidate| {
                draft
                    .live_marks
                    .iter()
                    .any(|marks| marks.key.name.as_str() == candidate)
            });
            created.push(name.as_str().to_string());
            draft.live_marks.push(LiveMarks {
                key: MarkCollectionKey { name },
                description: None,
                display_color: parse_color(&color)?,
            });
        }
        ConsoleGuiEdit::AddAutomation { name } => {
            typed_name(&name)?;
            let name = fresh_name(&name, |candidate| {
                draft
                    .automations
                    .iter()
                    .any(|automation| automation.name.as_str() == candidate)
            });
            created.push(name.as_str().to_string());
            draft.automations.push(LiveAutomation {
                id: LiveAutomationId(next_id(draft.automations.iter().map(|a| a.id.0))),
                name,
                description: None,
                value: 0.0,
                bindings: Vec::new(),
            });
        }
        ConsoleGuiEdit::SetAutomationValue { id, value } => {
            automation_mut(&mut draft, id)?.value = value;
        }
        ConsoleGuiEdit::SetAutomationBindings { id, bindings } => {
            automation_mut(&mut draft, id)?.bindings = bindings
                .into_iter()
                .map(automation_target_from_gui)
                .collect::<Result<_, GuiMutationError>>()?;
        }
        ConsoleGuiEdit::AddClip {
            initial_color,
            effect,
            target,
        } => {
            let definition = effect_definition_from_gui(session, owner, effect)?;
            let param_overrides =
                required_param_values(session, &definition, parse_color(&initial_color)?)?;
            let effect_name = session
                .project
                .get::<donder_model::EffectDefinition>(definition)
                .map(|definition| definition.name().to_string())
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Effect definition was not found.".into())
                })?;
            let name = fresh_name(&effect_name, |candidate| {
                draft
                    .clips
                    .iter()
                    .any(|clip| clip.name.as_str() == candidate)
            });
            created.push(name.as_str().to_string());
            draft.clips.push(donder_model::ConsoleClip {
                id: ConsoleClipId(next_id(draft.clips.iter().map(|clip| clip.id.0))),
                name,
                description: None,
                layer_id: draft
                    .layers
                    .first()
                    .map(|layer| layer.id.clone())
                    .ok_or_else(|| GuiMutationError::Invalid("The console has no layer.".into()))?,
                target: target_from_gui(session, owner, target)?,
                scope: ClipScope::WholeTarget,
                definition,
                param_overrides,
            });
        }
        ConsoleGuiEdit::EditClip { id, edit } => {
            edit_clip(session, owner, &mut draft, ConsoleClipId(id), edit)?;
        }
        ConsoleGuiEdit::SetAlways { clip_ids } => {
            draft.always = clip_ids.into_iter().map(ConsoleClipId).collect();
        }
        ConsoleGuiEdit::AddEvent { event, input } => {
            let input = control_from_gui(session, owner, &input)?;
            draft.events.push(On {
                id: OnId(next_id(draft.events.iter().map(|on| on.id.0))),
                event: input_event(event, input),
                actions: Vec::new(),
            });
        }
        ConsoleGuiEdit::SetEvent { id, event, input } => {
            let input = control_from_gui(session, owner, &input)?;
            let notes = session
                .project
                .get::<InputDevice>(input.device)
                .and_then(|device| device.control(input.control))
                .and_then(|control| control.kind.notes())
                .is_some();
            let on = event_mut(&mut draft, id)?;
            on.event = input_event(event, input);
            match event {
                ConsoleEventKind::Move => {
                    on.actions.retain(|action| matches!(action, Action::Set(_)));
                }
                ConsoleEventKind::Release => {
                    on.actions.iter_mut().for_each(|action| {
                        if let Some(values) = values_mut(action) {
                            values.clear();
                        }
                    });
                }
                ConsoleEventKind::Press if !notes => {
                    on.actions.iter_mut().for_each(|action| {
                        if let Some(values) = values_mut(action) {
                            values.retain(|value| !value.from.needs_note());
                        }
                    });
                }
                ConsoleEventKind::Press => {}
            }
        }
        ConsoleGuiEdit::SetActions { id, actions } => {
            event_mut(&mut draft, id)?.actions = actions
                .into_iter()
                .map(action_from_gui)
                .collect::<Result<_, _>>()?;
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(created)
}

fn input_event(kind: ConsoleEventKind, control: ControlRef) -> InputEvent {
    match kind {
        ConsoleEventKind::Press => InputEvent::Press(control),
        ConsoleEventKind::Release => InputEvent::Release(control),
        ConsoleEventKind::Move => InputEvent::Move(control),
    }
}

/// The parameters an action sets on the copy it starts.
fn values_mut(action: &mut Action) -> Option<&mut Vec<PlayedValue>> {
    match action {
        Action::Play(play) => Some(&mut play.values),
        Action::Start { values, .. } | Action::Toggle { values, .. } => Some(values),
        Action::Stop { .. } | Action::Mark(_) | Action::Set(_) => None,
    }
}

fn edit_clip(
    session: &mut ProjectSession,
    owner: &DocumentId,
    console: &mut Console,
    id: ConsoleClipId,
    edit: ConsoleClipEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        ConsoleClipEdit::Layer { layer_id } => {
            if !console.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
            }
            clip_mut(console, id)?.layer_id = SequenceLayerId(layer_id);
        }
        ConsoleClipEdit::Target { target } => {
            clip_mut(console, id)?.target = target_from_gui(session, owner, target)?;
        }
        ConsoleClipEdit::Scope { scope } => clip_mut(console, id)?.scope = clip_scope(scope),
        ConsoleClipEdit::Definition {
            initial_color,
            effect: reference,
        } => {
            let definition = effect_definition_from_gui(session, owner, reference)?;
            let params = required_param_values(session, &definition, parse_color(&initial_color)?)?;
            let authored = clip_mut(console, id)?;
            authored.definition = definition;
            authored.param_overrides = params;
            // Played values and bindings name the old effect's parameters.
            for action in console.events.iter_mut().flat_map(|on| &mut on.actions) {
                if action.clip() == Some(id)
                    && let Some(values) = values_mut(action)
                {
                    values.clear();
                }
            }
            console.forget_clip(id);
        }
        ConsoleClipEdit::Param { name, value } => {
            let value = param_value_from_gui(session, owner, value)?;
            clip_mut(console, id)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
    }
    Ok(())
}

/// An effect the console's document may name, importing its definition.
fn effect_definition_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    reference: SequenceEffectReference,
) -> Result<EffectDefinitionId, GuiMutationError> {
    let definition = effect_ref_from_gui(session, reference)?;
    ensure_document_can_reference_object(session, owner, definition.into())
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(definition)
}

/// A fixture or group of the active layout, which the console's document then imports.
fn target_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    target: FixtureTarget,
) -> Result<DomainFixtureTarget, GuiMutationError> {
    let layout = session
        .project
        .get::<donder_model::Setup>(*session.project.root().setup.id())
        .map(|setup| *setup.layout.id())
        .ok_or_else(|| GuiMutationError::Invalid("Active layout was not found.".into()))?;
    ensure_document_can_reference_object(session, owner, layout.into())
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(DomainFixtureTarget {
        layout,
        fixture: FixtureInstanceId(target.fixture),
    })
}

/// A control the console's document may name, importing its device.
fn control_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    control: &ConsoleControlRef,
) -> Result<ControlRef, GuiMutationError> {
    let device: InputDeviceId = object_id_from_gui(session, &control.device)?;
    ensure_document_can_reference_object(session, owner, device.into())
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(ControlRef {
        device,
        control: donder_model::InputControlId(control.control),
    })
}

fn next_id(ids: impl Iterator<Item = u32>) -> u32 {
    ids.max().unwrap_or(0) + 1
}

/// Visit every marks parameter value in the console's clips and operator nodes.
pub(super) fn console_mark_references_mut(
    console: &mut Console,
    mut visit: impl FnMut(&mut Option<MarkCollectionKey>),
) {
    let operators = console
        .composition_graph
        .nodes
        .iter_mut()
        .filter_map(|node| match &mut node.kind {
            CompositionGraphNodeKind::Operator(operator) => Some(operator.params.values_mut()),
            _ => None,
        })
        .flatten();
    for value in operators {
        super::model::visit_marks(value, &mut visit);
    }
    for clip in &mut console.clips {
        for value in clip.param_overrides.values_mut() {
            super::model::visit_marks(value, &mut visit);
        }
    }
}

pub(super) fn live_marks_mut<'a>(
    console: &'a mut Console,
    key: &str,
) -> Result<&'a mut LiveMarks, GuiMutationError> {
    console
        .live_marks
        .iter_mut()
        .find(|marks| marks.key.name.as_str() == key)
        .ok_or_else(|| GuiMutationError::Invalid("Live mark collection was not found.".into()))
}

pub(super) fn clip_mut(
    console: &mut Console,
    id: ConsoleClipId,
) -> Result<&mut donder_model::ConsoleClip, GuiMutationError> {
    console
        .clips
        .iter_mut()
        .find(|clip| clip.id == id)
        .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))
}

pub(super) fn automation_mut(
    console: &mut Console,
    id: u32,
) -> Result<&mut LiveAutomation, GuiMutationError> {
    console
        .automations
        .iter_mut()
        .find(|automation| automation.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Live automation was not found.".into()))
}

fn event_mut(console: &mut Console, id: u32) -> Result<&mut On, GuiMutationError> {
    console
        .events
        .iter_mut()
        .find(|on| on.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Event was not found.".into()))
}
