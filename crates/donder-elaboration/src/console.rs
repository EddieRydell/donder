//! Arming a console: its clips prepared once on the active setup, each with
//! its pool of dormant voices and a live automation per voice for each
//! parameter its copies take from their presses, and its clips and operators with the parameters its
//! `Set` actions write, and its events routed to the actions that start and
//! stop them.
use crate::Retain;
use crate::sequence::PreparationCache;
use crate::stage::{self, Parts};
use donder_model::{
    Action, ClipUse, ConsoleBinding, ConsoleClipId, ConsoleId, ControlRef, DonderProject,
    InputEvent, LengthSource, PlayLength, PlayedQuantity, sample_duration,
};
use donder_runtime::{
    ClipHandle, ConsoleRouting, PlayedSource, PreparedShow, RoutedAction, RoutedClip, RoutedLength,
    RoutedOn, RoutedValue, ShowBuilder, SignalHandle,
};
use donder_runtime_types::{
    BoundProgram, Curve, InputHappening, SampleTime, Shared, sample_duration_seconds_f32,
};
use indexmap::IndexMap;

/// Automation bindings one live automation sets: a clip, and its binding.
type Bindings<'a> = Vec<(ClipHandle<'a>, u16)>;

/// A console ready to perform: its playback data, which routes its inputs.
pub struct ArmedConsole {
    pub prepared: PreparedShow,
    /// The controls its inputs name, by index.
    pub controls: Vec<ControlRef>,
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
    let mut controls: Vec<ControlRef> = Vec::new();
    for control in console.inputs() {
        if !controls.contains(control) {
            controls.push(control.clone());
        }
    }
    let control = |control: &ControlRef| {
        controls
            .iter()
            .position(|read| read == control)
            .unwrap_or_else(|| unreachable!("the controls list every control the console reads"))
            as u16
    };
    let mut armed_clips = IndexMap::new();
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
            let mut played: Vec<Vec<Vec<Bindings<'_>>>> = Vec::new();
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
                let mut values = vec![vec![Vec::new(); authored.played.len()]; usize::from(count)];
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
                // A held clip has one voice; a played clip as many as its
                // most copies. Every voice starts dormant, `always` clips' too.
                armed_clips.insert(
                    authored.id,
                    RoutedClip {
                        voices,
                        values: Vec::new(),
                    },
                );
                played.push(values);
            }
            let builder = &mut *clips.builder;
            let collections = console
                .live_marks
                .iter()
                .zip(collections)
                .map(|(marks, targets)| (&marks.key, builder.live_marks(targets)))
                .collect::<IndexMap<_, _>>();
            let live = automations
                .into_iter()
                .map(|targets| builder.live_automation(targets))
                .collect::<Vec<_>>();
            for ((mut voices, clip), armed) in played
                .into_iter()
                .zip(&console.clips)
                .zip(armed_clips.values_mut())
            {
                armed.values = clip
                    .played
                    .iter()
                    .enumerate()
                    .map(|(index, value)| RoutedValue {
                        source: match value.from {
                            PlayedQuantity::Velocity => PlayedSource::Velocity,
                            PlayedQuantity::PitchClass => PlayedSource::PitchClass,
                        },
                        window: (value.window.low, value.window.high),
                        automations: voices
                            .iter_mut()
                            .map(|values| {
                                builder.live_automation(std::mem::take(&mut values[index]))
                            })
                            .collect(),
                    })
                    .collect();
            }
            let clip = |id| {
                armed_clips.get_index_of(&id).unwrap_or_else(|| {
                    unreachable!("validation keeps actions on the console's clips")
                }) as u16
            };
            let ons = console
                .events
                .iter()
                .map(|on| RoutedOn {
                    control: control(on.event.control()),
                    happening: match on.event {
                        InputEvent::Press(_) => InputHappening::Press,
                        InputEvent::Release(_) => InputHappening::Release,
                        InputEvent::Move(_) => InputHappening::Move,
                    },
                    actions: on
                        .actions
                        .iter()
                        .map(|action| match action {
                            Action::Play(play) => RoutedAction::Play {
                                clip: clip(play.clip),
                                copies: u16::from(play.copies),
                                length: match &play.length {
                                    PlayLength::Fixed(duration) => {
                                        RoutedLength::Fixed(sample_duration(duration))
                                    }
                                    PlayLength::Scaled {
                                        by,
                                        shortest,
                                        longest,
                                    } => RoutedLength::Scaled {
                                        by: match by {
                                            LengthSource::Velocity => {
                                                donder_runtime::LengthSource::Velocity
                                            }
                                            LengthSource::Axis(axis) => {
                                                donder_runtime::LengthSource::Axis(control(axis))
                                            }
                                        },
                                        shortest: sample_duration(shortest),
                                        longest: sample_duration(longest),
                                    },
                                },
                            },
                            Action::Start { clip: id } => RoutedAction::Start { clip: clip(*id) },
                            Action::Stop { clip: id } => RoutedAction::Stop { clip: clip(*id) },
                            Action::Toggle { clip: id } => RoutedAction::Toggle { clip: clip(*id) },
                            Action::Mark(key) => RoutedAction::Mark {
                                collection: collections[key],
                            },
                            Action::Set { target, window } => RoutedAction::Set {
                                automation: live[set
                                    .iter()
                                    .position(|set| *set == target)
                                    .unwrap_or_else(|| {
                                        unreachable!("set targets list every action's target")
                                    })],
                                window: (window.low, window.high),
                            },
                        })
                        .collect(),
                })
                .collect();
            builder.console(ConsoleRouting {
                controls: controls.len() as u16,
                always: console.always.iter().map(|&id| clip(id)).collect(),
                clips: armed_clips.into_values().collect(),
                ons,
            });
            live
        },
        |builder, automated, live| automate_operators(builder, automated, &operator_curves, &live),
    );
    Some(ArmedConsole { prepared, controls })
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
