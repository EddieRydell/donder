//! Arming a rig: its effects prepared once on the active setup, each trigger
//! with its pool of dormant voices and a control per voice for each value its
//! presses set. The host routes presses to the commands that start and stop
//! them.
use crate::PrepareOutputs;
use crate::sequence::PreparationCache;
use crate::stage::{self, Parts};
use donder_model::{
    DonderProject, MappingAction, MarkCollectionKey, RigBinding, RigEffectOrigin, RigId,
    RigMappingId, VoicePolicy,
};
use donder_runtime::{EffectHandle, PreparedShow};
use donder_runtime_types::{SampleTime, Shared, sample_duration_seconds_f32};
use indexmap::IndexMap;

/// Automation bindings a control sets: an effect, and its binding.
type Bindings<'a> = Vec<(EffectHandle<'a>, u16)>;

/// A rig ready to perform: its playback data and what its mappings command.
pub struct ArmedRig {
    pub prepared: PreparedShow,
    /// Each trigger mapping's voices: one, or its stack's.
    pub voices: IndexMap<RigMappingId, Box<[u16]>>,
    /// Each trigger mapping's controls for the values a press sets, by
    /// value, then by voice.
    pub played: IndexMap<RigMappingId, Box<[Box<[u16]>]>>,
    /// Each control mapping's control.
    pub controls: IndexMap<RigMappingId, u16>,
    /// Each live mark collection.
    pub collections: IndexMap<MarkCollectionKey, u16>,
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
    let mut armed_voices = IndexMap::new();
    let mut armed_played = IndexMap::new();
    let mut armed_controls = IndexMap::new();
    let mut armed_collections = IndexMap::new();
    let parts = Parts {
        layers: &rig.layers,
        graph: &rig.composition_graph,
        operators: accepted.operators().collect(),
        timing: accepted.timing(),
    };
    let session = accepted.timing().duration();
    let prepared = stage::build(&selected, parts, compact, cache, |effects| {
        let mut readers: IndexMap<&MarkCollectionKey, Bindings<'_>> = rig
            .live_marks
            .iter()
            .map(|marks| (&marks.key, Vec::new()))
            .collect();
        let mut bindings: IndexMap<RigMappingId, Bindings<'_>> = IndexMap::new();
        // Each trigger's played bindings, by voice, then value.
        let mut played: IndexMap<RigMappingId, Vec<Vec<Bindings<'_>>>> = IndexMap::new();
        for effect in accepted.effects() {
            let authored = effect.effect();
            // Scene voices play for the whole session, triggers' as long as
            // their length says.
            let (trigger, duration, count) = match effect.origin() {
                RigEffectOrigin::Voice(_) => (None, None, 1),
                RigEffectOrigin::Trigger(mapping, trigger) => (
                    Some((mapping, trigger)),
                    trigger.length.duration(),
                    match trigger.voices {
                        VoicePolicy::Stack { count } => count,
                        VoicePolicy::Retrigger | VoicePolicy::Mono => 1,
                    },
                ),
            };
            // A dropped effect keeps its voices, so commands still address
            // them, but plays in no layer.
            let dropped = effects.drops(&authored.layer_id, &authored.target);
            let (target, pixel_count) = effects.target(&authored.target, &authored.scope);
            let (program, live) = effects.lower(
                effect.execution(),
                donder_language::compiler::ProgramConstants {
                    pixel_count,
                    duration_seconds: duration.map(sample_duration_seconds_f32),
                },
            );
            // Lowering keeps the invocation's bindings first and appends a copy
            // of a binding for each integral of its parameter.
            let authored_bindings = effect.execution().automation().len();
            let lowered = program.automation();
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
            let value_count = trigger.map_or(0, |(_, trigger)| trigger.values.len());
            let mut values = vec![vec![Vec::new(); value_count]; usize::from(count)];
            for voice_values in &mut values {
                let handle = effects.builder.clip(
                    None,
                    &program,
                    SampleTime::from_ticks(0),
                    session,
                    target,
                );
                if trigger.is_some() {
                    voices.push(effects.builder.voice(handle, duration));
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
                    effects.layer(&authored.layer_id, handle);
                }
            }
            if let Some((mapping, _)) = trigger {
                armed_voices.insert(mapping.id, voices.into());
                played.insert(mapping.id, values);
            }
        }
        let builder = &mut *effects.builder;
        for (key, readers) in readers {
            armed_collections.insert(key.clone(), builder.live_marks(readers));
        }
        for mapping in &rig.mappings {
            if matches!(mapping.action, MappingAction::Control { .. }) {
                let targets = bindings.shift_remove(&mapping.id).unwrap_or_default();
                armed_controls.insert(mapping.id, builder.control(targets));
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
            armed_played.insert(mapping, controls);
        }
    });
    Some(ArmedRig {
        prepared,
        voices: armed_voices,
        played: armed_played,
        controls: armed_controls,
        collections: armed_collections,
    })
}
