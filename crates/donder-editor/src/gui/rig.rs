//! Rigs: effects played live from input devices.
use donder_model::{
    CompositionGraphNodeKind, ControlRef, ControlTarget, EffectRef, EffectScope, FixtureInstanceId,
    FixtureTarget as DomainFixtureTarget, InputControlKind, InputDevice, InputDeviceId, LiveMarks,
    MAX_STACKED_VOICES, Mapping, MappingAction, MarkCollectionKey, PlayedSource, PlayedValue, Rig,
    RigId, RigMappingId, RigSwitchId, RigVoiceId, SequenceLayerId, SourceIdentity, Switch, Trigger,
    TriggerLength, Voice, VoicePolicy, When,
};
use donder_project_io::{ProjectSession, SourceObjectKind, ensure_document_can_reference_source};

use super::model::{
    effect_param_value_from_gui, effect_ref_from_gui, effect_scope, fresh_name, identifier,
    object_identity_from_gui, parse_color, required_param_values, typed_name,
};
use super::projection::{
    active_layout, composition_graph, curve_library, effect_definitions, effect_ref_to_gui,
    effect_target, effect_target_label, gradient_library, invocation_params, layers_to_gui,
    layout_lanes,
};
use super::{GuiMutationError, ResolvedGuiObject, blocked};
use donder_sequence_api::{
    FixtureTarget, GuiDocument, RigActionChoice, RigControlRef, RigControlTarget, RigEffectEdit,
    RigEffectOwner, RigGuiDocument, RigGuiEdit, RigInput, RigInputControl, RigInputDevice, RigItem,
    RigLiveMarks, RigMapping, RigMappingAction, RigPlayedSource, RigPlayedValue, RigSwitch,
    RigTriggerLength, RigVoice, RigVoicePolicy, RigWhen, SequenceEffectReference,
    SequenceEffectScope,
};

pub(super) fn project_rig(session: &ProjectSession, resolved: &ResolvedGuiObject) -> GuiDocument {
    let id = RigId(resolved.object_identity());
    let Some(rig) = session.project.rig(&id) else {
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
            name: id.0.declared_name().to_string(),
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
                    effect: project_effect(session, &voice.effect),
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
            curve_library: curve_library(session, resolved.identity.document_id()),
            gradient_library: gradient_library(session, resolved.identity.document_id()),
            input_devices: input_devices(session),
            max_stacked_voices: u32::from(MAX_STACKED_VOICES),
        },
    }
}

fn project_effect(
    session: &ProjectSession,
    effect: &donder_model::RigEffect,
) -> donder_sequence_api::RigEffect {
    donder_sequence_api::RigEffect {
        layer_id: effect.layer_id.0,
        target: effect_target(&effect.target),
        target_label: effect_target_label(session, &effect.target),
        scope: match effect.scope {
            EffectScope::PerFixture => SequenceEffectScope::PerFixture,
            EffectScope::WholeTarget => SequenceEffectScope::WholeTarget,
        },
        effect: effect_ref_to_gui(&effect.definition),
        effect_name: session
            .project
            .definitions()
            .effects
            .resolve(&effect.definition)
            .map(|definition| definition.display_name.clone())
            .unwrap_or_else(|| "Missing effect".to_string()),
        params: invocation_params(
            session,
            &effect.definition,
            &effect.param_overrides,
            |_, _| None,
        ),
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
                effect: project_effect(session, &trigger.effect),
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
                            PlayedSource::Velocity => RigPlayedSource::Velocity,
                            PlayedSource::Pitch => RigPlayedSource::Pitch,
                            PlayedSource::PitchClass => RigPlayedSource::PitchClass,
                            PlayedSource::Fifths => RigPlayedSource::Fifths,
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
    let device = session.project.input_device(&input.device);
    RigInput {
        control: RigControlRef {
            device: super::patch::object_ref(&input.device.0, SourceObjectKind::InputDevice),
            control: input.control.0,
        },
        device_name: input.device.0.declared_name().to_string(),
        control_name: device
            .and_then(|device| device.control(input.control))
            .map(|control| control.name.as_str().to_string())
            .unwrap_or_else(|| format!("Missing control {}", input.control.0)),
    }
}

/// A device's controls of the kinds `accepts` accepts.
fn controls(device: &InputDevice, accepts: fn(InputControlKind) -> bool) -> Vec<RigInputControl> {
    device
        .controls
        .iter()
        .filter(|control| accepts(control.kind))
        .map(|control| RigInputControl {
            control: control.id.0,
            name: control.name.as_str().to_string(),
            notes: control.kind.notes().is_some(),
        })
        .collect()
}

/// The active setup's input devices and their buttons and dials.
fn input_devices(session: &ProjectSession) -> Vec<RigInputDevice> {
    let Some(setup) = session.project.setup(session.project.root().setup.id()) else {
        return Vec::new();
    };
    setup
        .input_devices
        .iter()
        .filter_map(|source| {
            let id = source.id();
            let device = session.project.input_device(id)?;
            Some(RigInputDevice {
                device: super::patch::object_ref(&id.0, SourceObjectKind::InputDevice),
                name: id.0.declared_name().to_string(),
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
) -> Result<(), GuiMutationError> {
    let id = RigId(resolved.object_identity());
    let mut draft = session
        .project
        .rig(&id)
        .cloned()
        .ok_or_else(|| GuiMutationError::Invalid("Rig was not found.".into()))?;
    let owner = &resolved.identity;
    match edit {
        RigGuiEdit::SetFrameRate { frame_rate } => draft.frame_rate = frame_rate,
        RigGuiEdit::Composition { edit } => {
            super::graph::edit_composition(session, owner, &mut draft, edit)?;
        }
        RigGuiEdit::AddLiveMarks { name, color } => {
            typed_name(&name)?;
            let name = fresh_name(&name, |candidate| {
                draft
                    .live_marks
                    .iter()
                    .any(|marks| marks.key.name.as_str() == candidate)
            });
            draft.live_marks.push(LiveMarks {
                key: MarkCollectionKey { name },
                description: None,
                display_color: parse_color(&color)?,
            });
        }
        RigGuiEdit::SetLiveMarksColor { key, color } => {
            live_marks_mut(&mut draft, &key)?.display_color = parse_color(&color)?;
        }
        RigGuiEdit::AddVoice {
            initial_color,
            effect,
            target,
        } => {
            let effect = new_effect(session, owner, &draft, &initial_color, effect, target)?;
            let name = fresh_name(effect_object(&effect.definition), |candidate| {
                behavior_name_taken(&draft, candidate)
            });
            draft.voices.push(Voice {
                id: RigVoiceId(next_id(draft.voices.iter().map(|voice| voice.id.0))),
                name,
                description: None,
                effect,
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
        RigGuiEdit::Rename { item, name } => {
            let name = typed_name(&name)?;
            match item {
                RigItem::LiveMarks { key } => {
                    let from = live_marks_mut(&mut draft, &key)?.key.clone();
                    let to = MarkCollectionKey { name };
                    live_marks_mut(&mut draft, &key)?.key = to.clone();
                    mark_references_mut(&mut draft, |reference| {
                        if reference.as_ref() == Some(&from) {
                            *reference = Some(to.clone());
                        }
                    });
                    for mapping in &mut draft.mappings {
                        if let MappingAction::Mark(key) = &mut mapping.action
                            && *key == from
                        {
                            *key = to.clone();
                        }
                    }
                }
                RigItem::Voice { id } => voice_mut(&mut draft, id)?.name = name,
                RigItem::Switch { id } => switch_mut(&mut draft, id)?.name = name,
                RigItem::Mapping { id } => mapping_mut(&mut draft, id)?.name = name,
            }
        }
        RigGuiEdit::SetDescription { item, description } => {
            let description = super::description::normalized(description);
            match item {
                RigItem::LiveMarks { key } => {
                    live_marks_mut(&mut draft, &key)?.description = description;
                }
                RigItem::Voice { id } => voice_mut(&mut draft, id)?.description = description,
                RigItem::Switch { id } => switch_mut(&mut draft, id)?.description = description,
                RigItem::Mapping { id } => mapping_mut(&mut draft, id)?.description = description,
            }
        }
        RigGuiEdit::Delete { item } => match item {
            RigItem::LiveMarks { key } => {
                let removed = live_marks_mut(&mut draft, &key)?.key.clone();
                mark_references_mut(&mut draft, |reference| {
                    if reference.as_ref() == Some(&removed) {
                        *reference = None;
                    }
                });
                draft.live_marks.retain(|marks| marks.key != removed);
            }
            RigItem::Voice { id } => {
                voice_mut(&mut draft, id)?;
                draft.voices.retain(|voice| voice.id.0 != id);
            }
            RigItem::Switch { id } => {
                switch_mut(&mut draft, id)?;
                draft.switches.retain(|switch| switch.id.0 != id);
            }
            RigItem::Mapping { id } => {
                mapping_mut(&mut draft, id)?;
                draft.mappings.retain(|mapping| mapping.id.0 != id);
            }
        },
        RigGuiEdit::EditEffect {
            owner: effect,
            edit,
        } => {
            edit_effect(session, owner, &mut draft, effect, edit)?;
        }
        RigGuiEdit::SetSwitchInput { id, input } => {
            switch_mut(&mut draft, id)?.input = control_from_gui(session, owner, &input)?;
        }
        RigGuiEdit::SetMappingInput { id, input } => {
            let input = control_from_gui(session, owner, &input)?;
            let plays_notes = session
                .project
                .input_device(&input.device)
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
                            RigPlayedSource::Velocity => PlayedSource::Velocity,
                            RigPlayedSource::Pitch => PlayedSource::Pitch,
                            RigPlayedSource::PitchClass => PlayedSource::PitchClass,
                            RigPlayedSource::Fifths => PlayedSource::Fifths,
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
    session
        .project
        .replace_rig(&id, draft)
        .map_err(GuiMutationError::Invalid)
}

fn edit_effect(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    rig: &mut Rig,
    effect: RigEffectOwner,
    edit: RigEffectEdit,
) -> Result<(), GuiMutationError> {
    match edit {
        RigEffectEdit::Layer { layer_id } => {
            if !rig.layers.iter().any(|layer| layer.id.0 == layer_id) {
                return Err(GuiMutationError::Invalid("Layer was not found.".into()));
            }
            effect_mut(rig, effect)?.layer_id = SequenceLayerId(layer_id);
        }
        RigEffectEdit::Target { target } => {
            effect_mut(rig, effect)?.target = target_from_gui(session, owner, target)?;
        }
        RigEffectEdit::Scope { scope } => effect_mut(rig, effect)?.scope = effect_scope(scope),
        RigEffectEdit::Definition {
            initial_color,
            effect: reference,
        } => {
            let definition = effect_definition_from_gui(session, owner, reference)?;
            let params = required_param_values(session, &definition, parse_color(&initial_color)?)?;
            // Played values name the old effect's parameters.
            if let RigEffectOwner::Trigger { mapping_id } = &effect {
                trigger_mut(rig, *mapping_id)?.values.clear();
            }
            let effect = effect_mut(rig, effect)?;
            effect.definition = definition;
            effect.param_overrides = params;
        }
        RigEffectEdit::Param { name, value } => {
            let value = effect_param_value_from_gui(session, owner, value)?;
            effect_mut(rig, effect)?
                .param_overrides
                .insert(identifier(&name)?, value);
        }
    }
    Ok(())
}

fn action_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    rig: &Rig,
    action: RigActionChoice,
) -> Result<MappingAction, GuiMutationError> {
    Ok(match action {
        RigActionChoice::Trigger {
            initial_color,
            effect,
            target,
        } => MappingAction::Trigger(Box::new(Trigger {
            effect: new_effect(session, owner, rig, &initial_color, effect, target)?,
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

/// A new effect on the rig's default layer, over the whole target.
fn new_effect(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    rig: &Rig,
    initial_color: &str,
    effect: SequenceEffectReference,
    target: FixtureTarget,
) -> Result<donder_model::RigEffect, GuiMutationError> {
    let definition = effect_definition_from_gui(session, owner, effect)?;
    let param_overrides = required_param_values(session, &definition, parse_color(initial_color)?)?;
    Ok(donder_model::RigEffect {
        layer_id: rig
            .layers
            .first()
            .map(|layer| layer.id.clone())
            .ok_or_else(|| GuiMutationError::Invalid("The rig has no layer.".into()))?,
        target: target_from_gui(session, owner, target)?,
        scope: EffectScope::WholeTarget,
        definition,
        param_overrides,
    })
}

/// An effect the rig's document may name, importing its definition.
fn effect_definition_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    reference: SequenceEffectReference,
) -> Result<EffectRef, GuiMutationError> {
    let definition = effect_ref_from_gui(session, reference)?;
    let EffectRef::Custom(id) = &definition;
    ensure_document_can_reference_source(
        session,
        owner.document_id(),
        SourceObjectKind::EffectDefinition,
        &id.0,
    )
    .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(definition)
}

/// A fixture or group of the active layout, which the rig's document then imports.
fn target_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    target: FixtureTarget,
) -> Result<DomainFixtureTarget, GuiMutationError> {
    let layout = session
        .project
        .setup(session.project.root().setup.id())
        .map(|setup| setup.layout.id().clone())
        .ok_or_else(|| GuiMutationError::Invalid("Active layout was not found.".into()))?;
    donder_project_io::ensure_document_can_reference_object(
        session,
        owner.document_id(),
        &layout.0,
    )
    .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(DomainFixtureTarget {
        layout,
        fixture: FixtureInstanceId(target.fixture),
    })
}

/// A control the rig's document may name, importing its device.
fn control_from_gui(
    session: &mut ProjectSession,
    owner: &SourceIdentity,
    control: &RigControlRef,
) -> Result<ControlRef, GuiMutationError> {
    let device = InputDeviceId(object_identity_from_gui(&control.device)?);
    donder_project_io::ensure_document_can_reference_object(
        session,
        owner.document_id(),
        &device.0,
    )
    .map_err(|error| GuiMutationError::Blocked(error.to_string()))?;
    Ok(ControlRef {
        device,
        control: donder_model::InputControlId(control.control),
    })
}

fn effect_object(definition: &EffectRef) -> &str {
    let EffectRef::Custom(id) = definition;
    id.0.object()
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

/// Visit every marks parameter value in the rig's effects and operator nodes.
fn mark_references_mut(rig: &mut Rig, mut visit: impl FnMut(&mut Option<MarkCollectionKey>)) {
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
    for effect in rig.effects_mut() {
        for value in effect.param_overrides.values_mut() {
            super::model::visit_marks(value, &mut visit);
        }
    }
}

fn effect_mut(
    rig: &mut Rig,
    owner: RigEffectOwner,
) -> Result<&mut donder_model::RigEffect, GuiMutationError> {
    match owner {
        RigEffectOwner::Voice { id } => Ok(&mut voice_mut(rig, id)?.effect),
        RigEffectOwner::Trigger { mapping_id } => Ok(&mut trigger_mut(rig, mapping_id)?.effect),
    }
}

fn live_marks_mut<'a>(rig: &'a mut Rig, key: &str) -> Result<&'a mut LiveMarks, GuiMutationError> {
    rig.live_marks
        .iter_mut()
        .find(|marks| marks.key.name.as_str() == key)
        .ok_or_else(|| GuiMutationError::Invalid("Live mark collection was not found.".into()))
}

fn voice_mut(rig: &mut Rig, id: u32) -> Result<&mut Voice, GuiMutationError> {
    rig.voices
        .iter_mut()
        .find(|voice| voice.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Voice was not found.".into()))
}

fn switch_mut(rig: &mut Rig, id: u32) -> Result<&mut Switch, GuiMutationError> {
    rig.switches
        .iter_mut()
        .find(|switch| switch.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Switch was not found.".into()))
}

fn mapping_mut(rig: &mut Rig, id: u32) -> Result<&mut Mapping, GuiMutationError> {
    rig.mappings
        .iter_mut()
        .find(|mapping| mapping.id.0 == id)
        .ok_or_else(|| GuiMutationError::Invalid("Mapping was not found.".into()))
}

fn trigger_mut(rig: &mut Rig, id: u32) -> Result<&mut Trigger, GuiMutationError> {
    match &mut mapping_mut(rig, id)?.action {
        MappingAction::Trigger(trigger) => Ok(trigger),
        MappingAction::Mark(_) | MappingAction::Control { .. } => Err(GuiMutationError::Invalid(
            "The mapping does not trigger an effect.".into(),
        )),
    }
}
