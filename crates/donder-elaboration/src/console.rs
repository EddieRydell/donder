//! Arming a console: its clips prepared once on the active setup, each with
//! its pool of dormant voices and a live automation per voice for each
//! parameter actions play, and its clips and operators with the parameters its
//! `Set` actions write. The host routes input events to the commands that start
//! and stop them.
use crate::Retain;
use crate::sequence::PreparationCache;
use crate::stage::{self, Parts};
use donder_model::{
    ClipUse, ConsoleAutomationTarget, ConsoleBinding, ConsoleClipId, ConsoleId, DonderProject,
    MarkCollectionKey,
};
use donder_runtime::{ClipHandle, PreparedShow, ShowBuilder, SignalHandle};
use donder_runtime_types::{
    BoundProgram, Curve, Identifier, SampleTime, Shared, sample_duration_seconds_f32,
};
use indexmap::IndexMap;

/// Automation bindings one live automation sets: a clip, and its binding.
type Bindings<'a> = Vec<(ClipHandle<'a>, u16)>;

/// A console ready to perform: its playback data and what its actions
/// command.
pub struct ArmedConsole {
    pub prepared: PreparedShow,
    /// Each clip's voices and played parameters.
    pub clips: IndexMap<ConsoleClipId, ArmedClip>,
    /// The live automation of each parameter `Set` actions write.
    pub automations: IndexMap<ConsoleAutomationTarget, u16>,
    /// Each live mark collection.
    pub collections: IndexMap<MarkCollectionKey, u16>,
}

/// A clip's voices: one for a held clip, or as many as the most copies its
/// `Play` actions play. Every voice starts dormant, `always` clips' too.
pub struct ArmedClip {
    pub voices: Box<[u16]>,
    /// The live automation of each parameter actions play, on each voice.
    pub played: IndexMap<Identifier, Box<[u16]>>,
}

/// Prepare `console` against the project's active setup. `None` means the
/// console or the output selection cannot be resolved.
pub fn arm(
    project: &DonderProject,
    console: &ConsoleId,
    retain: Retain<'_>,
    cache: &mut PreparationCache,
) -> Option<ArmedConsole> {
    let compact = !matches!(retain, Retain::All);
    let selected = crate::selection::resolve_console(project, console, retain)?;
    let accepted = selected.show;
    let console = accepted.console();
    let mut armed_clips = IndexMap::new();
    let mut armed_automations = IndexMap::new();
    let mut armed_collections = IndexMap::new();
    let parts = Parts {
        layers: &console.layers,
        graph: &console.composition_graph,
        operators: accepted.operators().collect(),
        timing: accepted.timing(),
    };
    let session = accepted.timing().duration();
    // Each operator binding's curve is its own allocation, which lowering and
    // fusion keep, so the curve finds the live automation that sets it.
    let operator_curves = accepted
        .operators()
        .flat_map(|operator| {
            operator
                .invocation()
                .automation()
                .iter()
                .zip(accepted.operator_automations(&operator.node().id))
                .map(|(binding, &automation)| (Shared::clone(&binding.curve), automation))
        })
        .collect::<Vec<_>>();
    let prepared = stage::build(
        &selected,
        parts,
        compact,
        cache,
        |clips| {
            // Each live mark collection's and set parameter's bindings, in the
            // console's order.
            let set = console.set_targets();
            let mut collections: Vec<Bindings<'_>> = vec![Vec::new(); console.live_marks.len()];
            let mut automations: Vec<Bindings<'_>> = vec![Vec::new(); set.len()];
            // Each clip's played bindings, by voice, then parameter.
            let mut played: Vec<(ConsoleClipId, Vec<Vec<Bindings<'_>>>)> = Vec::new();
            for clip in accepted.clips() {
                let authored = clip.clip();
                // A `Fixed` length is the same for every copy, so it is a program
                // constant; otherwise each start sets it.
                let (count, duration) = match clip.clip_use() {
                    ClipUse::Played(count) => (count, fixed_length(console, authored.id)),
                    ClipUse::Held => (1, None),
                };
                // A dropped clip keeps its voices, so commands still address
                // them, but plays in no layer.
                let dropped = clips.drops(&authored.layer_id, &authored.target);
                let (target, pixel_count) = clips.target(&authored.target, &authored.scope);
                let program = clips.lower(
                    clip.invocation(),
                    donder_language::compiler::ProgramConstants {
                        pixel_count,
                        duration_seconds: duration.map(sample_duration_seconds_f32),
                    },
                );
                // Lowering keeps the invocation's bindings first and appends a copy
                // of a binding for each integral of its parameter.
                let authored_bindings = clip.invocation().automation().len();
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
                let mut values = vec![vec![Vec::new(); clip.played().len()]; usize::from(count)];
                for voice_values in &mut values {
                    let handle = clips.builder.clip(
                        None,
                        &program,
                        SampleTime::from_ticks(0),
                        session,
                        target,
                    );
                    voices.push(clips.builder.voice(handle));
                    for index in 0..lowered.len() {
                        let binding = (handle, index as u16);
                        match clip.bindings()[source(index)] {
                            ConsoleBinding::Set(automation) => {
                                automations[automation].push(binding);
                            }
                            ConsoleBinding::Played(value) => voice_values[value].push(binding),
                            ConsoleBinding::LiveMarks(collection) => {
                                collections[collection].push(binding);
                            }
                        }
                    }
                    if !dropped {
                        clips.layer(&authored.layer_id, handle);
                    }
                }
                armed_clips.insert(
                    authored.id,
                    ArmedClip {
                        voices: voices.into(),
                        played: IndexMap::new(),
                    },
                );
                played.push((authored.id, values));
            }
            let builder = &mut *clips.builder;
            for (marks, targets) in console.live_marks.iter().zip(collections) {
                armed_collections.insert(marks.key.clone(), builder.live_marks(targets));
            }
            let live = set
                .into_iter()
                .zip(automations)
                .map(|(target, targets)| {
                    let index = builder.live_automation(targets);
                    armed_automations.insert(target.clone(), index);
                    index
                })
                .collect::<Vec<_>>();
            for ((id, mut voices), clip) in played.into_iter().zip(accepted.clips()) {
                let armed = &mut armed_clips[&id];
                for (value, param) in clip.played().iter().enumerate() {
                    let automations = voices
                        .iter_mut()
                        .map(|values| builder.live_automation(std::mem::take(&mut values[value])))
                        .collect();
                    armed.played.insert(param.clone(), automations);
                }
            }
            live
        },
        |builder, automated, live| automate_operators(builder, automated, &operator_curves, &live),
    );
    Some(ArmedConsole {
        prepared,
        clips: armed_clips,
        automations: armed_automations,
        collections: armed_collections,
    })
}

/// Bind each built operator's automation bindings that a `Set` writes, found
/// by `curves`, to the set parameter's live automation in `live`.
fn automate_operators<'id>(
    builder: &mut ShowBuilder<'id>,
    automated: &[(SignalHandle<'id>, BoundProgram)],
    curves: &[(Shared<Curve>, usize)],
    live: &[u16],
) {
    for (signal, program) in automated {
        for (index, binding) in program.automation().iter().enumerate() {
            if let Some(&(_, automation)) = curves
                .iter()
                .find(|(curve, _)| Shared::ptr_eq(curve, &binding.curve))
            {
                builder.automate_operator(live[automation], *signal, index as u16);
            }
        }
    }
}

/// The length every copy of a played clip plays, when all its `Play`
/// actions give the same `Fixed` one.
fn fixed_length(
    console: &donder_model::Console,
    clip: ConsoleClipId,
) -> Option<donder_runtime_types::SampleDuration> {
    let mut lengths = console.actions().filter_map(|action| match action {
        donder_model::Action::Play(play) if play.clip == clip => Some(play.length.fixed()),
        _ => None,
    });
    let first = lengths.next()??;
    lengths.all(|length| length == Some(first)).then_some(first)
}
