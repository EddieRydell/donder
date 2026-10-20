//! Arming a rig: its effects prepared once on the active setup, each trigger
//! with its pool of dormant voices and a control per voice for each value its
//! presses set, and the router that turns presses into the commands that
//! start and stop them.
mod router;
#[cfg(test)]
mod tests;

pub use router::{
    RigRouter, RoutedAction, RoutedLength, RoutedMapping, RoutedNotes, RoutedSource, RoutedValue,
    RoutedVoices,
};

use crate::PrepareOutputs;
use crate::sequence::{PreparationCache, composition, prepare_routes, programs::Programs};
use crate::stage::{Stage, Targets};
use donder_model::{
    ButtonInput, ControlRef, DonderProject, InputControlKind, MappingAction, MarkCollectionKey,
    PlayedSource, RIG_SESSION, RigBinding, RigEffectOrigin, RigId, RigMappingId, TriggerLength,
    VoicePolicy, When,
};
use donder_runtime::{EffectHandle, PreparedRig, VoiceLength};
use donder_runtime_types::{SampleDuration, Shared, sample_duration_seconds_f32};
use indexmap::IndexMap;

/// Automation bindings a control sets: an effect, and its binding.
type Bindings<'a> = Vec<(EffectHandle<'a>, u16)>;

/// A rig ready to perform: its playback data and the routing of its controls.
pub struct ArmedRig {
    pub prepared: PreparedRig,
    pub router: RigRouter,
    /// The controls the router reads, by input index.
    pub inputs: Box<[ControlRef]>,
}

/// Prepare `rig` against the project's active setup. `None` means the rig or
/// the output selection cannot be resolved.
pub fn arm(
    project: &DonderProject,
    rig: &RigId,
    outputs: PrepareOutputs<'_>,
    cache: &mut PreparationCache,
) -> Option<ArmedRig> {
    let compact = !matches!(outputs, PrepareOutputs::All);
    let selected = crate::selection::resolve_rig(project, rig, outputs)?;
    let accepted = selected.show;
    let rig = accepted.rig();
    let stage = Stage::new(
        &selected,
        &rig.composition_graph,
        accepted.operators(),
        compact,
    );
    let required_layers = stage.required_layers(&rig.composition_graph);
    let mut programs = Programs::new(cache);
    // Each trigger mapping's voices, in the order the rig registers them.
    let mut trigger_voices: IndexMap<RigMappingId, Vec<u16>> = IndexMap::new();
    let mut collections: IndexMap<&MarkCollectionKey, u16> = IndexMap::new();
    let mut controls: IndexMap<RigMappingId, u16> = IndexMap::new();
    // Each trigger's controls for its played values, by value, then voice.
    let mut played_controls: IndexMap<RigMappingId, Vec<Box<[u16]>>> = IndexMap::new();
    let prepared = PreparedRig::build(accepted.timing().clone(), |builder| {
        let fixtures = stage.fixtures(builder, &selected);
        let mut targets = Targets::default();
        let mut layer_effects = rig
            .layers
            .iter()
            .map(|layer| (&layer.id, Vec::new()))
            .collect::<IndexMap<_, _>>();
        let mut readers: IndexMap<&MarkCollectionKey, Vec<(EffectHandle<'_>, u16)>> = rig
            .live_marks
            .iter()
            .map(|marks| (&marks.key, Vec::new()))
            .collect();
        let mut bindings: IndexMap<RigMappingId, Vec<(EffectHandle<'_>, u16)>> = IndexMap::new();
        // Each trigger's played bindings, by voice, then value.
        let mut played: IndexMap<RigMappingId, Vec<Vec<Bindings<'_>>>> = IndexMap::new();
        for effect in accepted.effects() {
            let authored = effect.effect();
            let (length, count) = match effect.origin() {
                RigEffectOrigin::Voice(_) => (None, 1),
                RigEffectOrigin::Trigger(_, trigger) => (
                    Some(match &trigger.length {
                        TriggerLength::Fixed(duration) => VoiceLength::Fixed(
                            SampleDuration::from_ticks(duration.as_micros_rounded() as u32),
                        ),
                        TriggerLength::Held | TriggerLength::Latched => VoiceLength::Open,
                    }),
                    match trigger.voices {
                        VoicePolicy::Stack { count } => count,
                        VoicePolicy::Retrigger | VoicePolicy::Mono => 1,
                    },
                ),
            };
            let dropped = compact
                && (!stage.has_pixels
                    || !required_layers.contains(&authored.layer_id)
                    || stage.drops(&authored.target));
            let (target, pixel_count) = targets.get(
                builder,
                &stage,
                &fixtures,
                &authored.target,
                &authored.scope,
            );
            let duration_seconds = match length {
                Some(VoiceLength::Fixed(duration)) => Some(sample_duration_seconds_f32(duration)),
                // Scene voices and open voices play for the rest of the session.
                Some(VoiceLength::Open) | None => None,
            };
            let (invocation, live) = programs.sample_live(
                effect.execution(),
                donder_language::compiler::ProgramConstants {
                    pixel_count,
                    duration_seconds,
                },
            );
            // Lowering keeps the invocation's bindings first and appends a copy
            // of a binding for each integral of its parameter.
            let authored_bindings = effect.execution().automation().len();
            let lowered = invocation.automation();
            let source = |index: usize| {
                if index < authored_bindings {
                    index
                } else {
                    (0..authored_bindings)
                        .find(|&source| {
                            Shared::ptr_eq(&lowered[source].curve, &lowered[index].curve)
                        })
                        .unwrap_or_else(|| unreachable!("an integral copies a binding"))
                }
            };
            let mut voices = Vec::new();
            let value_count = match effect.origin() {
                RigEffectOrigin::Trigger(_, trigger) => trigger.values.len(),
                RigEffectOrigin::Voice(_) => 0,
            };
            let mut values = vec![vec![Vec::new(); value_count]; usize::from(count)];
            for voice_values in &mut values {
                let handle = builder.sample(&invocation, builder.whole_sequence(), target);
                if let Some(length) = length {
                    voices.push(builder.voice(handle, length));
                }
                for (key, &param) in effect.live_marks().iter().zip(live.iter()) {
                    readers[key].push((handle, param));
                }
                for index in 0..lowered.len() {
                    match effect.controls()[source(index)] {
                        RigBinding::Control(mapping) => bindings
                            .entry(mapping)
                            .or_default()
                            .push((handle, index as u16)),
                        RigBinding::Played(value) => {
                            voice_values[value].push((handle, index as u16));
                        }
                    }
                }
                if !dropped {
                    layer_effects[&authored.layer_id].push(handle);
                }
            }
            if let RigEffectOrigin::Trigger(mapping, _) = effect.origin() {
                trigger_voices.insert(mapping.id, voices);
                played.insert(mapping.id, values);
            }
        }
        for (key, readers) in readers {
            collections.insert(key, builder.live_marks(readers));
        }
        for mapping in &rig.mappings {
            if matches!(mapping.action, MappingAction::Control { .. }) {
                let targets = bindings.shift_remove(&mapping.id).unwrap_or_default();
                controls.insert(mapping.id, builder.control(targets));
            }
        }
        for (mapping, mut voices) in played {
            let value_count = voices.first().map_or(0, Vec::len);
            let controls = (0..value_count)
                .map(|value| {
                    voices
                        .iter_mut()
                        .map(|values| builder.control(std::mem::take(&mut values[value])))
                        .collect()
                })
                .collect();
            played_controls.insert(mapping, controls);
        }
        let layers = rig
            .layers
            .iter()
            .filter(|layer| !compact || stage.has_pixels && required_layers.contains(&layer.id))
            .map(|layer| {
                (
                    &layer.id,
                    builder.layer(layer.enabled, layer_effects[&layer.id].iter().copied()),
                )
            })
            .collect::<IndexMap<_, _>>();
        let black = rig
            .layers
            .iter()
            .filter(|layer| !layer.enabled || layer_effects[&layer.id].is_empty())
            .map(|layer| &layer.id)
            .collect::<std::collections::HashSet<_>>();
        let root = if compact && !stage.has_pixels {
            builder.output(None)
        } else {
            composition::prepare(
                builder,
                &rig.composition_graph,
                accepted.operators(),
                RIG_SESSION,
                &layers,
                &black,
                &mut programs,
            )
        };
        prepare_routes(
            builder,
            &selected,
            &stage.geometry,
            &stage.targets,
            &stage.cells,
            &fixtures,
        );
        root
    });
    let mut inputs: Vec<ControlRef> = Vec::new();
    let mut input_index = |input: &ControlRef| match inputs.iter().position(|known| known == input)
    {
        Some(index) => index,
        None => {
            inputs.push(input.clone());
            inputs.len() - 1
        }
    };
    let switches: Box<[usize]> = rig
        .switches
        .iter()
        .map(|switch| input_index(&switch.input))
        .collect();
    let switch_index = |id| rig.switches.iter().position(|switch| switch.id == id);
    let mappings = rig
        .mappings
        .iter()
        .map(|mapping| {
            Some(RoutedMapping {
                input: input_index(&mapping.input),
                when: match mapping.when {
                    None => None,
                    Some(When::On(id)) => Some((switch_index(id)?, true)),
                    Some(When::Off(id)) => Some((switch_index(id)?, false)),
                },
                action: match &mapping.action {
                    MappingAction::Trigger(trigger) => RoutedAction::Trigger {
                        values: trigger
                            .values
                            .iter()
                            .zip(played_controls.get(&mapping.id)?)
                            .map(|(value, controls)| RoutedValue {
                                source: match value.from {
                                    PlayedSource::Velocity => RoutedSource::Velocity,
                                    PlayedSource::Pitch => RoutedSource::Pitch,
                                    PlayedSource::PitchClass => RoutedSource::PitchClass,
                                    PlayedSource::Fifths => RoutedSource::Fifths,
                                },
                                controls: controls.clone(),
                            })
                            .collect(),
                        voices: trigger_voices.get(&mapping.id)?.clone().into(),
                        length: match &trigger.length {
                            TriggerLength::Fixed(duration) => RoutedLength::Fixed(
                                SampleDuration::from_ticks(duration.as_micros_rounded() as u32),
                            ),
                            TriggerLength::Held => RoutedLength::Held,
                            TriggerLength::Latched => RoutedLength::Latched,
                        },
                        policy: match trigger.voices {
                            VoicePolicy::Retrigger => RoutedVoices::Retrigger,
                            VoicePolicy::Stack { .. } => RoutedVoices::Stack,
                            VoicePolicy::Mono => RoutedVoices::Mono,
                        },
                    },
                    MappingAction::Mark(key) => RoutedAction::Mark {
                        collection: *collections.get(key)?,
                    },
                    MappingAction::Control { .. } => RoutedAction::Control {
                        control: *controls.get(&mapping.id)?,
                    },
                },
            })
        })
        .collect::<Option<Box<[_]>>>()?;
    let notes = inputs
        .iter()
        .map(|input| {
            let control = project
                .input_device(&input.device)?
                .control(input.control)?;
            Some(match control.kind {
                InputControlKind::Button(ButtonInput::Note(note)) => RoutedNotes::Note(note),
                InputControlKind::Keys(range) => RoutedNotes::Keys {
                    low: range.low,
                    high: range.high,
                },
                InputControlKind::Button(ButtonInput::Key(_) | ButtonInput::ControlChange(_))
                | InputControlKind::Dial(_) => RoutedNotes::None,
            })
        })
        .collect::<Option<Box<[_]>>>()?;
    Some(ArmedRig {
        prepared,
        router: RigRouter::new(notes, switches, mappings),
        inputs: inputs.into(),
    })
}
