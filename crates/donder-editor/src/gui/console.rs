//! Consoles: sequences built live from input devices.
use donder_model::{
    Action, Always, AutomationTarget, ClipId, Console, ConsoleId, ControlRef, InputDevice,
    InputDeviceId, LengthSource, MAX_COPIES, MarkCollectionId, On, OnId, Placement, Play,
    PlayLength, PlayedValue, Window,
};
use donder_project_io::ProjectSession;
use donder_runtime_types::{InputHappening, PlayedSource, TargetScope};

use super::edit::automation_target_from_gui;
use super::graph::new_effect_clip;
use super::input_device::gui_control;
use super::model::{object_id_from_gui, object_ref};
use super::projection::{
    automation_target_to_gui, composition_graph, curve_library, effect_definitions,
    gradient_library, layers_to_gui, layout_lanes,
};
use super::{GuiMutationError, ResolvedGuiObject, checked_gui_duration, next_id};
use crate::dto::{
    ConsoleAction, ConsoleControlRef, ConsoleEventKind, ConsoleGuiDocument, ConsoleGuiEdit,
    ConsoleInput, ConsoleInputControl, ConsoleInputDevice, ConsoleLengthSource, ConsoleOn,
    ConsoleParamSource, ConsolePlayLength, ConsolePlayMode, ConsolePlayedQuantity,
    ConsolePlayedValue, FixtureTarget, GuiDocument, SequenceEffectReference,
};

/// The length a clip newly started from the session's start, or by a new
/// `Start` or `Toggle`, repeats every.
const HELD_LENGTH_SECONDS: f64 = 4.0;

pub(super) fn project_console(
    session: &ProjectSession,
    resolved: &ResolvedGuiObject,
) -> GuiDocument {
    let id = resolved.id::<ConsoleId>();
    let console = resolved.view::<Console>(session).object();
    let mut targets = layout_lanes(&session.project().layout());
    let mut seen = std::collections::HashSet::new();
    targets.retain(|lane| seen.insert(lane.target.fixture));
    GuiDocument::Console {
        document: ConsoleGuiDocument {
            description: console.description.clone(),
            source_ref: resolved.source_ref(),
            name: session.address_of(id).object().to_string(),
            perform_index: session
                .project()
                .reusable::<donder_model::Console>()
                .get_index_of(&id)
                .unwrap_or_else(|| unreachable!("the console is the project's"))
                as u32,
            frame_rate: console.frame_rate,
            mark_collections: super::projection::mark_collections_to_gui(
                &console.composition.marks,
            ),
            layers: layers_to_gui(&console.composition.layers),
            composition_graph: composition_graph(
                session,
                &console.composition.layers,
                &console.composition.graph,
                |_, _, _| None,
            ),
            // The console editor shows its effects; its envelopes are edited as text.
            clips: console
                .composition
                .clips
                .iter()
                .filter(|clip| clip.effect().is_some())
                .map(|clip| super::projection::project_clip(session, &console.always, clip))
                .collect(),
            always: console.always.iter().map(|always| always.clip.0).collect(),
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
            held_length_seconds: HELD_LENGTH_SECONDS,
        },
    }
}

pub(super) fn played_value_to_gui(value: &PlayedValue) -> ConsolePlayedValue {
    ConsolePlayedValue {
        param: value.param.as_str().to_string(),
        from: match value.from {
            PlayedSource::Velocity => ConsolePlayedQuantity::Velocity,
            PlayedSource::PitchClass => ConsolePlayedQuantity::PitchClass,
        },
        window: value.window.into(),
    }
}

fn project_event(session: &ProjectSession, on: &On) -> ConsoleOn {
    ConsoleOn {
        id: on.id.0,
        event: match on.happening {
            InputHappening::Press => ConsoleEventKind::Press,
            InputHappening::Release => ConsoleEventKind::Release,
            InputHappening::Move => ConsoleEventKind::Move,
        },
        input: project_input(session, &on.control),
        actions: on
            .actions
            .iter()
            .map(|action| action_to_gui(session, action))
            .collect(),
    }
}

fn action_to_gui(session: &ProjectSession, action: &Action) -> ConsoleAction {
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
                        LengthSource::Axis(control) => ConsoleLengthSource::Axis {
                            control: control_ref_to_gui(session, control),
                        },
                    },
                    shortest_seconds: shortest.as_seconds_f64(),
                    longest_seconds: longest.as_seconds_f64(),
                },
            },
        },
        Action::Start { clip, length } => ConsoleAction::Start {
            clip_id: clip.0,
            length_seconds: length.as_seconds_f64(),
        },
        Action::Stop { clip } => ConsoleAction::Stop { clip_id: clip.0 },
        Action::Toggle { clip, length } => ConsoleAction::Toggle {
            clip_id: clip.0,
            length_seconds: length.as_seconds_f64(),
        },
        Action::Mark(id) => ConsoleAction::Mark { into: id.0 },
        Action::Set { target, window } => ConsoleAction::Set {
            target: automation_target_to_gui(target),
            window: (*window).into(),
        },
    }
}

pub(super) fn action_from_gui(
    session: &ProjectSession,
    action: ConsoleAction,
) -> Result<Action, GuiMutationError> {
    Ok(match action {
        ConsoleAction::Play {
            clip_id,
            copies,
            length,
        } => Action::Play(Play {
            clip: ClipId(clip_id),
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
                        ConsoleLengthSource::Axis { control } => {
                            LengthSource::Axis(control_from_gui(session, &control)?)
                        }
                    },
                    shortest: checked_gui_duration(shortest_seconds)?,
                    longest: checked_gui_duration(longest_seconds)?,
                },
            },
        }),
        ConsoleAction::Start {
            clip_id,
            length_seconds,
        } => Action::Start {
            clip: ClipId(clip_id),
            length: checked_gui_duration(length_seconds)?,
        },
        ConsoleAction::Stop { clip_id } => Action::Stop {
            clip: ClipId(clip_id),
        },
        ConsoleAction::Toggle {
            clip_id,
            length_seconds,
        } => Action::Toggle {
            clip: ClipId(clip_id),
            length: checked_gui_duration(length_seconds)?,
        },
        ConsoleAction::Mark { into } => Action::Mark(MarkCollectionId(into)),
        ConsoleAction::Set { target, window } => Action::Set {
            target: automation_target_from_gui(target)?,
            window: window.into(),
        },
    })
}

fn control_ref_to_gui(session: &ProjectSession, control: &ControlRef) -> ConsoleControlRef {
    ConsoleControlRef {
        device: object_ref(session, control.device),
        control: control.control.0,
    }
}

fn project_input(session: &ProjectSession, input: &ControlRef) -> ConsoleInput {
    let device = session
        .project()
        .get::<donder_model::InputDevice>(input.device);
    ConsoleInput {
        control: control_ref_to_gui(session, input),
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
            input: gui_control(control).input,
        })
        .collect()
}

/// The project's input devices and their buttons and axes.
fn input_devices(session: &ProjectSession) -> Vec<ConsoleInputDevice> {
    session
        .project()
        .input_devices()
        .map(|device| ConsoleInputDevice {
            device: object_ref(session, device.id()),
            name: session.address_of(device.id()).object().to_string(),
            buttons: controls(&device, true),
            axes: controls(&device, false),
        })
        .collect()
}

pub(super) fn edit_console(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: ConsoleGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    let mut created = Vec::new();
    let mut draft = resolved.view::<Console>(session).object().clone();
    match edit {
        ConsoleGuiEdit::Composition { edit } => {
            created = super::graph::edit_composition(session, &mut draft.composition, edit)?;
        }
        ConsoleGuiEdit::AddAlwaysEffect {
            initial_color,
            effect,
            target,
        } => {
            let (id, name) = add_clip(session, &mut draft, &initial_color, effect, target)?;
            created.push(name);
            draft.always.push(Always {
                clip: id,
                length: checked_gui_duration(HELD_LENGTH_SECONDS)?,
            });
        }
        ConsoleGuiEdit::AddClip {
            initial_color,
            effect,
            target,
        } => {
            let (_, name) = add_clip(session, &mut draft, &initial_color, effect, target)?;
            created.push(name);
        }
        ConsoleGuiEdit::AddEffectAction {
            control,
            event,
            mode,
            initial_color,
            effect,
            target,
        } => {
            let control = control_from_gui(session, &control)?;
            let (clip, name) = add_clip(session, &mut draft, &initial_color, effect, target)?;
            created.push(name);
            match mode {
                ConsolePlayMode::ForTime => push_action(
                    &mut draft,
                    event,
                    control,
                    Action::Play(Play {
                        clip,
                        copies: 1,
                        length: PlayLength::Fixed(checked_gui_duration(1.0)?),
                    }),
                )?,
                ConsolePlayMode::Toggle => push_action(
                    &mut draft,
                    event,
                    control,
                    Action::Toggle {
                        clip,
                        length: checked_gui_duration(HELD_LENGTH_SECONDS)?,
                    },
                )?,
                ConsolePlayMode::Held => {
                    push_action(
                        &mut draft,
                        ConsoleEventKind::Press,
                        control.clone(),
                        Action::Start {
                            clip,
                            length: checked_gui_duration(HELD_LENGTH_SECONDS)?,
                        },
                    )?;
                    push_action(
                        &mut draft,
                        ConsoleEventKind::Release,
                        control,
                        Action::Stop { clip },
                    )?;
                }
            }
        }
        ConsoleGuiEdit::AddAction {
            control,
            event,
            action,
        } => {
            let control = control_from_gui(session, &control)?;
            let action = action_from_gui(session, action)?;
            push_action(&mut draft, event, control, action)?;
        }
        ConsoleGuiEdit::ControlParam {
            target,
            source,
            window,
        } => {
            let target = automation_target_from_gui(target)?;
            let window = Window::from(window);
            // Whatever set the parameter before lets go of it.
            for on in &mut draft.events {
                on.actions.retain(
                    |action| !matches!(action, Action::Set { target: set, .. } if *set == target),
                );
            }
            if let AutomationTarget::ClipParam { clip_id, param } = &target {
                played_mut(&mut draft, *clip_id)?.retain(|value| value.param != *param);
            }
            if let Some(ConsoleParamSource::Played { from }) = source {
                let AutomationTarget::ClipParam { clip_id, param } = target else {
                    return Err(GuiMutationError::Invalid(
                        "Only an effect's parameter is played.".into(),
                    ));
                };
                played_mut(&mut draft, clip_id)?.push(PlayedValue {
                    param,
                    from: match from {
                        ConsolePlayedQuantity::Velocity => PlayedSource::Velocity,
                        ConsolePlayedQuantity::PitchClass => PlayedSource::PitchClass,
                    },
                    window,
                });
            } else if let Some(ConsoleParamSource::Control { control }) = source {
                let control = control_from_gui(session, &control)?;
                let button = session
                    .project()
                    .get::<InputDevice>(control.device)
                    .and_then(|device| device.control(control.control))
                    .map(|control| control.kind.is_button())
                    .ok_or_else(|| GuiMutationError::Invalid("Control was not found.".into()))?;
                if button {
                    // The velocity of a press, and 0 when it lets go.
                    push_action(
                        &mut draft,
                        ConsoleEventKind::Press,
                        control.clone(),
                        Action::Set {
                            target: target.clone(),
                            window,
                        },
                    )?;
                    push_action(
                        &mut draft,
                        ConsoleEventKind::Release,
                        control,
                        Action::Set { target, window },
                    )?;
                } else {
                    push_action(
                        &mut draft,
                        ConsoleEventKind::Move,
                        control,
                        Action::Set { target, window },
                    )?;
                }
            }
            draft.events.retain(|on| !on.actions.is_empty());
        }
    }
    session
        .put((resolved.id(), draft))
        .map_err(GuiMutationError::from)?;
    Ok(created)
}

/// A new clip on the first layer over `target`, named after its effect: its id
/// and name.
fn add_clip(
    session: &ProjectSession,
    draft: &mut Console,
    initial_color: &str,
    effect: SequenceEffectReference,
    target: FixtureTarget,
) -> Result<(ClipId, String), GuiMutationError> {
    let clip = new_effect_clip(
        session,
        &draft.composition,
        super::model::target_from_gui(target),
        Placement::Played(Vec::new()),
        (effect, TargetScope::WholeTarget, initial_color),
    )?;
    let (id, shown) = (clip.id, clip.name.as_str().to_string());
    draft.composition.clips.push(std::sync::Arc::new(clip));
    Ok((id, shown))
}

/// Append `action` to what `control` does on `kind`, adding the event when it has none.
fn push_action(
    console: &mut Console,
    kind: ConsoleEventKind,
    control: ControlRef,
    action: Action,
) -> Result<(), GuiMutationError> {
    let happening = match kind {
        ConsoleEventKind::Press => InputHappening::Press,
        ConsoleEventKind::Release => InputHappening::Release,
        ConsoleEventKind::Move => InputHappening::Move,
    };
    let index = match console
        .events
        .iter()
        .position(|on| on.happening == happening && on.control == control)
    {
        Some(index) => index,
        None => {
            console.events.push(On {
                id: OnId(next_id(console.events.iter().map(|on| on.id.0))?),
                happening,
                control,
                actions: Vec::new(),
            });
            console.events.len() - 1
        }
    };
    console.events[index].actions.push(action);
    Ok(())
}

/// A control of an input device of the project.
fn control_from_gui(
    session: &ProjectSession,
    control: &ConsoleControlRef,
) -> Result<ControlRef, GuiMutationError> {
    let device: InputDeviceId = object_id_from_gui(session, &control.device)?;
    Ok(ControlRef {
        device,
        control: donder_model::InputControlId(control.control),
    })
}

/// The values each copy of a played clip takes from its press.
fn played_mut(
    console: &mut Console,
    id: ClipId,
) -> Result<&mut Vec<PlayedValue>, GuiMutationError> {
    let clip = console
        .composition
        .clips
        .iter_mut()
        .find(|clip| clip.id == id)
        .ok_or_else(|| GuiMutationError::Invalid("Clip was not found.".into()))?;
    match &mut std::sync::Arc::make_mut(clip).placement {
        Placement::Played(values) => Ok(values),
        Placement::Timed { .. } => Err(GuiMutationError::Invalid(
            "A timed clip takes nothing from a press.".into(),
        )),
    }
}
