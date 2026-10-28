//! Rigs: clips played live from input devices.
use donder_model::{
    ClipScope, CompositionGraphNodeKind, ControlRef, ControlTarget, EffectDefinitionId,
    FixtureInstanceId, FixtureTarget as DomainFixtureTarget, InputControlKind, InputDevice,
    InputDeviceId, LiveMarks, MAX_STACKED_VOICES, Mapping, MappingAction, MarkCollectionKey,
    PlayedQuantity, PlayedValue, Rig, RigId, RigMappingId, RigSwitchId, RigVoiceId,
    SequenceLayerId, Switch, Trigger, TriggerLength, Voice, VoicePolicy, When,
};
use donder_project_io::{DocumentId, ProjectSession, ensure_document_can_reference_object};

use super::model::{
    clip_scope, effect_ref_from_gui, fresh_name, identifier, object_address, object_id_from_gui,
    object_ref, param_value_from_gui, parse_color, required_param_values, typed_name,
};
use super::projection::{
    active_layout, composition_graph, curve_library, effect_definitions, effect_ref_to_gui,
    gradient_library, invocation_params, layers_to_gui, layout_lanes, target_label, target_to_gui,
};
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use crate::dto::{
    FixtureTarget, GuiDocument, RigActionChoice, RigClipEdit, RigClipOwner, RigControlRef,
    RigControlTarget, RigGuiDocument, RigGuiEdit, RigInput, RigInputControl, RigInputDevice,
    RigLiveMarks, RigMapping, RigMappingAction, RigPlayedQuantity, RigPlayedValue, RigSwitch,
    RigTriggerLength, RigVoice, RigVoicePolicy, RigWhen, SequenceClipScope,
    SequenceEffectReference,
};

pub(super) fn project_rig(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let id = resolved.id::<RigId>();
    let Some(rig) = session.project.get::<donder_model::Rig>(id) else {
        return blocked("Rig was not found.", Vec::new());
    };
    let mut targets = active_layout(session).map(layout_lanes).unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    targets.retain(|lane| seen.insert(lane.target.fixture));
    GuiDocument::Rig {
        document: RigGuiDocument {
            description: rig.description.clone(),
            path: resolved.identity.document().to_string(),
            source_ref: resolved.source_ref(),
            object_key: resolved.identity.object().to_string(),
            name: object_address(session, id).declared_name().to_string(),
            perform_index: session
                .project
                .root()
                .rigs
                .iter()
                .position(|source| source.id() == &id)
                .map(|index| index as u32),
            frame_rate: rig.frame_rate,
            live_marks: rig
                .live_marks
                .iter()
                .map(|marks| RigLiveMarks {
                    key: marks.key.name.as_str().to_string(),
                    description: marks.description.clone(),
                    color: marks.display_color.to_hex(),
                })
                .collect(),
            layers: layers_to_gui(&rig.layers),
            composition_graph: composition_graph(
                session,
                &rig.layers,
                &rig.composition_graph,
                None,
            ),
            voices: rig
                .voices
                .iter()
                .map(|voice| RigVoice {
                    id: voice.id.0,
                    name: voice.name.as_str().to_string(),
                    description: voice.description.clone(),
                    clip: project_clip(session, &voice.clip),
                })
                .collect(),
            switches: rig
                .switches
                .iter()
                .map(|switch| RigSwitch {
                    id: switch.id.0,
                    name: switch.name.as_str().to_string(),
                    description: switch.description.clone(),
                    input: project_input(session, &switch.input),
                })
                .collect(),
            mappings: rig
                .mappings
                .iter()
                .map(|mapping| project_mapping(session, mapping))
                .collect(),
            targets,
            effect_definitions: effect_definitions(session),
            curve_library: curve_library(session),
            gradient_library: gradient_library(session),
            input_devices: input_devices(session),
            max_stacked_voices: u32::from(MAX_STACKED_VOICES),
        },
    }
}

fn project_clip(session: &ProjectSession, clip: &donder_model::RigClip) -> crate::dto::RigClip {
    crate::dto::RigClip {
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
        params: invocation_params(session, &clip.definition, &clip.param_overrides, |_, _| {
            None
        }),
    }
}

fn project_mapping(session: &ProjectSession, mapping: &Mapping) -> RigMapping {
    RigMapping {
        id: mapping.id.0,
        name: mapping.name.as_str().to_string(),
        description: mapping.description.clone(),
        input: project_input(session, &mapping.input),
        when: mapping.when.map(|when| match when {
            When::On(switch) => RigWhen::On {
                switch_id: switch.0,
            },
            When::Off(switch) => RigWhen::Off {
                switch_id: switch.0,
            },
        }),
        action: match &mapping.action {
            MappingAction::Trigger(trigger) => RigMappingAction::Trigger {
                clip: project_clip(session, &trigger.clip),
                length: match &trigger.length {
                    TriggerLength::Fixed(duration) => RigTriggerLength::Fixed {
                        duration_seconds: duration.as_seconds_f64(),
                    },
                    TriggerLength::Held => RigTriggerLength::Held,
                    TriggerLength::Latched => RigTriggerLength::Latched,
                },
                voices: match trigger.voices {
                    VoicePolicy::Retrigger => RigVoicePolicy::Retrigger,
                    VoicePolicy::Stack { count } => RigVoicePolicy::Stack {
                        count: u32::from(count),
                    },
                    VoicePolicy::Mono => RigVoicePolicy::Mono,
                },
                values: trigger
                    .values
                    .iter()
                    .map(|value| RigPlayedValue {
                        param: value.param.as_str().to_string(),
                        from: match value.from {
                            PlayedQuantity::Velocity => RigPlayedQuantity::Velocity,
                            PlayedQuantity::Pitch => RigPlayedQuantity::Pitch,
                            PlayedQuantity::PitchClass => RigPlayedQuantity::PitchClass,
                            PlayedQuantity::Fifths => RigPlayedQuantity::Fifths,
                        },
                    })
                    .collect(),
            },
            MappingAction::Mark(key) => RigMappingAction::Mark {
                marks: key.name.as_str().to_string(),
            },
            MappingAction::Control { target, param } => RigMappingAction::Control {
                target: match target {
                    ControlTarget::Voice(id) => RigControlTarget::Voice { voice_id: id.0 },
                    ControlTarget::Trigger(id) => RigControlTarget::Trigger { mapping_id: id.0 },
                },
                param: param.as_str().to_string(),
            },
        },
    }
}

fn project_input(session: &ProjectSession, input: &ControlRef) -> RigInput {
    let device = session
        .project
        .get::<donder_model::InputDevice>(input.device);
    RigInput {
        control: RigControlRef {
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

/// A device's controls of the kinds `accepts` accepts.
fn controls(device: &InputDevice, accepts: fn(&InputControlKind) -> bool) -> Vec<RigInputControl> {
    device
        .controls
        .iter()
        .filter(|control| accepts(&control.kind))
        .map(|control| RigInputControl {
            control: control.id.0,
            name: control.name.as_str().to_string(),
            notes: control.kind.notes().is_some(),
        })
        .collect()
}

/// The active setup's input devices and their buttons and dials.
fn input_devices(session: &ProjectSession) -> Vec<RigInputDevice> {
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
            Some(RigInputDevice {
                device: object_ref(session, *id),
                name: object_address(session, *id).declared_name().to_string(),
                buttons: controls(device, InputControlKind::is_button),
                dials: controls(device, |kind| !kind.is_pressed()),
                keys: controls(device, |kind| kind.is_pressed() && !kind.is_button()),
            })
        })
        .collect()
}

pub(super) fn edit_rig(
    session: &mut ProjectSession,
    resolved: &ResolvedGuiObject,
    edit: RigGuiEdit,
) -> Result<Vec<String>, GuiMutationError> {
    let id = resolved.id::<RigId>();
    let mut created = Vec::new();
    let mut draft = session
        .project
        .get::<donder_model::Rig>(id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Rig was not found.".into()))?;
    let owner = resolved.document_id();
    match edit {
        RigGuiEdit::SetFrameRate { frame_rate } => draft.frame_rate = frame_rate,
        RigGuiEdit::Composition { edit } => {
            created = super::graph::edit_composition(session, owner, &mut draft, edit)?;
        }
        RigGuiEdit::AddLiveMarks { name, color } => {
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
        RigGuiEdit::AddVoice {
            initial_color,
            effect,
            target,
        } => {
            let clip = new_clip(session, owner, &draft, &initial_color, effect, target)?;
            let effect_name = session
                .project
                .get::<donder_model::EffectDefinition>(clip.definition)
                .map(|definition| definition.name().to_string())
                .ok_or_else(|| {
                    GuiMutationError::Invalid("Effect definition was not found.".into())
                })?;
            let name = fresh_name(&effect_name, |candidate| {
                behavior_name_taken(&draft, candidate)
            });
            draft.voices.push(Voice {
                id: RigVoiceId(next_id(draft.voices.iter().map(|voice| voice.id.0))),
                name,
                description: None,
                clip,
            });
        }
        RigGuiEdit::AddSwitch { input } => {
            let input = control_from_gui(session, owner, &input)?;
            let name = fresh_name("switch", |candidate| {
                draft
                    .switches
                    .iter()
                    .any(|switch| switch.name.as_str() == candidate)
            });
            draft.switches.push(Switch {
                id: RigSwitchId(next_id(draft.switches.iter().map(|switch| switch.id.0))),
                name,
                description: None,
                input,
            });
        }
        RigGuiEdit::AddMapping { input, action } => {
            let input = control_from_gui(session, owner, &input)?;
            let action = action_from_gui(session, owner, &draft, action)?;
            let name = fresh_name("mapping", |candidate| {
                behavior_name_taken(&draft, candidate)
            });
            draft.mappings.push(Mapping {
                id: RigMappingId(next_id(draft.mappings.iter().map(|mapping| mapping.id.0))),
                name,
                description: None,
                input,
                when: None,
                action,
            });
        }
        RigGuiEdit::EditClip { owner: clip, edit } => {
            edit_clip(session, owner, &mut draft, clip, edit)?;
        }
        RigGuiEdit::SetSwitchInput { id, input } => {
            switch_mut(&mut draft, id)?.input = control_from_gui(session, owner, &input)?;
        }
        RigGuiEdit::SetMappingInput { id, input } => {
            let input = control_from_gui(session, owner, &input)?;
            let plays_notes = session
                .project
                .get::<donder_model::InputDevice>(input.device)
                .and_then(|device| {
                    device
                        .controls
                        .iter()
                        .find(|control| control.id == input.control)
                })
                .is_some_and(|control| control.kind.notes().is_some());
            let mapping = mapping_mut(&mut draft, id)?;
            mapping.input = input;
            // A value played from a note goes with an input that plays none.
            if let MappingAction::Trigger(trigger) = &mut mapping.action
                && !plays_notes
            {
                trigger.values.retain(|value| !value.from.needs_note());
            }
        }
        RigGuiEdit::SetMappingWhen { id, when } => {
            mapping_mut(&mut draft, id)?.when = when.map(|when| match when {
                RigWhen::On { switch_id } => When::On(RigSwitchId(switch_id)),
                RigWhen::Off { switch_id } => When::Off(RigSwitchId(switch_id)),
            });
        }
        RigGuiEdit::SetMappingAction { id, action } => {
            let action = action_from_gui(session, owner, &draft, action)?;
            mapping_mut(&mut draft, id)?.action = action;
        }
        RigGuiEdit::SetTriggerLength { mapping_id, length } => {
            trigger_mut(&mut draft, mapping_id)?.length = match length {
                RigTriggerLength::Fixed { duration_seconds } => {
                    TriggerLength::Fixed(super::checked_gui_duration(duration_seconds)?)
                }
                RigTriggerLength::Held => TriggerLength::Held,
                RigTriggerLength::Latched => TriggerLength::Latched,
            };
        }
        RigGuiEdit::SetTriggerValues { mapping_id, values } => {
            trigger_mut(&mut draft, mapping_id)?.values = values
                .into_iter()
                .map(|value| {
                    Ok(PlayedValue {
                        param: identifier(&value.param)?,
                        from: match value.from {
                            RigPlayedQuantity::Velocity => PlayedQuantity::Velocity,
                            RigPlayedQuantity::Pitch => PlayedQuantity::Pitch,
                            RigPlayedQuantity::PitchClass => PlayedQuantity::PitchClass,
                            RigPlayedQuantity::Fifths => PlayedQuantity::Fifths,
                        },
                    })
                })
                .collect::<Result<_, GuiMutationError>>()?;
        }
        RigGuiEdit::SetTriggerVoices { mapping_id, voices } => {
            trigger_mut(&mut draft, mapping_id)?.voices = match voices {
                RigVoicePolicy::Retrigger => VoicePolicy::Retrigger,
                RigVoicePolicy::Stack { count } => VoicePolicy::Stack {
                    count: u8::try_from(count).map_err(|_| {
                        GuiMutationError::Invalid(format!(
                            "A stack has 1 to {MAX_STACKED_VOICES} voices."
                        ))
                    })?,
                },
                RigVoicePolicy::Mono => VoicePolicy::Mono,
            };
        }
    }
    session.project.put(draft).map_err(GuiMutationError::from)?;
    Ok(created)
}

fn edit_clip(
    session: &mut ProjectSession,
    owner: &DocumentId,
    rig: &mut Rig,
    clip: RigClipOwner,
    edit: RigClipEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        RigClipEdit::Layer { layer_id } => {
            if !rig.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
            }
            clip_mut(rig, clip)?.layer_id = SequenceLayerId(layer_id);
        }
        RigClipEdit::Target { target } => {
            clip_mut(rig, clip)?.target = target_from_gui(session, owner, target)?;
        }
        RigClipEdit::Scope { scope } => clip_mut(rig, clip)?.scope = clip_scope(scope),
        RigClipEdit::Definition {
            initial_color,
            effect: reference,
        } => {
            let definition = effect_definition_from_gui(session, owner, reference)?;
            let params = required_param_values(session, &definition, parse_color(&initial_color)?)?;
            // Played values name the old effect's parameters.
            if let RigClipOwner::Trigger { mapping_id } = &clip {
                trigger_mut(rig, *mapping_id)?.values.clear();
            }
            let authored = clip_mut(rig, clip)?;
            authored.definition = definition;
            authored.param_overrides = params;
        }
        RigClipEdit::Param { name, value } => {
            let value = param_value_from_gui(session, owner, value)?;
            clip_mut(rig, clip)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
    }
    Ok(())
}

fn action_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    rig: &Rig,
    action: RigActionChoice,
) -> Result<MappingAction, GuiMutationError> {
    Ok(match action {
        RigActionChoice::Trigger {
            initial_color,
            effect,
            target,
        } => MappingAction::Trigger(Box::new(Trigger {
            clip: new_clip(session, owner, rig, &initial_color, effect, target)?,
            length: TriggerLength::Fixed(super::checked_gui_duration(1.0)?),
            voices: VoicePolicy::Retrigger,
            values: Vec::new(),
        })),
        RigActionChoice::Mark { marks } => MappingAction::Mark(MarkCollectionKey {
            name: identifier(&marks)?,
        }),
        RigActionChoice::Control { target, param } => MappingAction::Control {
            target: match target {
                RigControlTarget::Voice { voice_id } => ControlTarget::Voice(RigVoiceId(voice_id)),
                RigControlTarget::Trigger { mapping_id } => {
                    ControlTarget::Trigger(RigMappingId(mapping_id))
                }
            },
            param: identifier(&param)?,
        },
    })
}

/// A new clip on the rig's default layer, over the whole target.
fn new_clip(
    session: &mut ProjectSession,
    owner: &DocumentId,
    rig: &Rig,
    initial_color: &str,
    effect: SequenceEffectReference,
    target: FixtureTarget,
) -> Result<donder_model::RigClip, GuiMutationError> {
    let definition = effect_definition_from_gui(session, owner, effect)?;
    let param_overrides = required_param_values(session, &definition, parse_color(initial_color)?)?;
    Ok(donder_model::RigClip {
        layer_id: rig
            .layers
            .first()
            .map(|layer| layer.id.clone())
            .ok_or_else(|| GuiMutationError::Invalid("The rig has no layer.".into()))?,
        target: target_from_gui(session, owner, target)?,
        scope: ClipScope::WholeTarget,
        definition,
        param_overrides,
    })
}

/// An effect the rig's document may name, importing its definition.
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

/// A fixture or group of the active layout, which the rig's document then imports.
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
    donder_project_io::ensure_document_can_reference_object(session, owner, layout.into())
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(DomainFixtureTarget {
        layout,
        fixture: FixtureInstanceId(target.fixture),
    })
}

/// A control the rig's document may name, importing its device.
fn control_from_gui(
    session: &mut ProjectSession,
    owner: &DocumentId,
    control: &RigControlRef,
) -> Result<ControlRef, GuiMutationError> {
    let device: InputDeviceId = object_id_from_gui(session, &control.device)?;
    donder_project_io::ensure_document_can_reference_object(session, owner, device.into())
        .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(ControlRef {
        device,
        control: donder_model::InputControlId(control.control),
    })
}

/// Voices and mappings share one namespace.
fn behavior_name_taken(rig: &Rig, name: &str) -> bool {
    rig.voices.iter().any(|voice| voice.name.as_str() == name)
        || rig
            .mappings
            .iter()
            .any(|mapping| mapping.name.as_str() == name)
}

fn next_id(ids: impl Iterator<Item = u32>) -> u32 {
    ids.max().unwrap_or(0) + 1
}

/// Visit every marks parameter value in the rig's clips and operator nodes.
pub(super) fn rig_mark_references_mut(
    rig: &mut Rig,
    mut visit: impl FnMut(&mut Option<MarkCollectionKey>),
) {
    let operators = rig
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
    for clip in rig.clips_mut() {
        for value in clip.param_overrides.values_mut() {
            super::model::visit_marks(value, &mut visit);
        }
    }
}

fn clip_mut(
    rig: &mut Rig,
    owner: RigClipOwner,
) -> Result<&mut donder_model::RigClip, GuiMutationError> {
    match owner {
        RigClipOwner::Voice { id } => Ok(&mut voice_mut(rig, id)?.clip),
        RigClipOwner::Trigger { mapping_id } => Ok(&mut trigger_mut(rig, mapping_id)?.clip),
    }
}

pub(super) fn live_marks_mut<'a>(
    rig: &'a mut Rig,
    key: &str,
) -> Result<&'a mut LiveMarks, GuiMutationError> {
    rig.live_marks
        .iter_mut()
        .find(|marks| marks.key.name.as_str() == key)
        .ok_or_else(|| GuiMutationError::Invalid("Live mark collection was not found.".into()))
}

pub(super) fn voice_mut(rig: &mut Rig, id: u32) -> Result<&mut Voice, GuiMutationError> {
    rig.voices
        .iter_mut()
        .find(|voice| voice.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Voice was not found.".into()))
}

pub(super) fn switch_mut(rig: &mut Rig, id: u32) -> Result<&mut Switch, GuiMutationError> {
    rig.switches
        .iter_mut()
        .find(|switch| switch.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Switch was not found.".into()))
}

pub(super) fn mapping_mut(rig: &mut Rig, id: u32) -> Result<&mut Mapping, GuiMutationError> {
    rig.mappings
        .iter_mut()
        .find(|mapping| mapping.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Mapping was not found.".into()))
}

fn trigger_mut(rig: &mut Rig, id: u32) -> Result<&mut Trigger, GuiMutationError> {
    match &mut mapping_mut(rig, id)?.action {
        MappingAction::Trigger(trigger) => Ok(trigger),
        MappingAction::Mark(_) | MappingAction::Control { .. } => Err(GuiMutationError::Invalid(
            "The mapping does not trigger a clip.".into(),
        )),
    }
}
