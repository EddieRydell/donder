//! Arming a console: its show, whose played clips each have a pool of dormant
//! voices with a live automation per voice for each parameter its copies take
//! from their presses, whose clips and operators read the live automations
//! its `Set` actions write, and whose events route to the actions that start
//! and stop them.
use crate::Retain;
use crate::sequence::PreparationCache;
use crate::stage::{self, Clips};
use donder_language::compiler::{LiveWords, ProgramConstants};
use donder_model::{
    AcceptedClip, AcceptedShow, Action, AutomationTarget, ClipId, ClipUse, CompositionGraphNodeId,
    Console, ConsoleId, ControlRef, DonderProject, LengthSource, LiveBinding, PlayLength, ShowId,
    sample_duration,
};
use donder_runtime::{
    ConsoleRouting, PreparedShow, RoutedAction, RoutedClip, RoutedLength, RoutedOn, RoutedValue,
    ShowBuilder, TargetHandle,
};
use donder_runtime_types::{AutomationMapping, AutomationSource, SampleDuration, SampleTime};
/// A console ready to perform: its playback data, which routes its inputs.
pub struct ArmedConsole {
    pub prepared: PreparedShow,
    /// The controls its inputs name, by index.
    pub controls: Vec<ControlRef>,
}

/// Prepare `console` against the project. `None` means the
/// console or the output selection cannot be resolved.
pub fn arm(
    project: &DonderProject,
    console: &ConsoleId,
    retain: Retain<'_>,
    cache: &mut PreparationCache,
) -> Option<ArmedConsole> {
    let selected = crate::selection::resolve(project, ShowId::Console(*console), retain)?;
    let plan = RoutingPlan::new(selected.show);
    let prepared = stage::build(
        &selected,
        !matches!(retain, Retain::All),
        cache,
        Some(&plan),
    );
    Some(ArmedConsole {
        prepared,
        controls: plan.controls,
    })
}

/// What routing a console's inputs needs before its show is built: the
/// controls it reads and the parameters its `Set`s write.
pub(crate) struct RoutingPlan<'a> {
    console: &'a Console,
    controls: Vec<ControlRef>,
    set: Vec<&'a AutomationTarget>,
    session: SampleDuration,
}

impl<'a> RoutingPlan<'a> {
    fn new(show: AcceptedShow<'a>) -> Self {
        let Some(console) = show.console() else {
            unreachable!("a console's show is the console");
        };
        let mut controls: Vec<ControlRef> = Vec::new();
        for control in console.inputs() {
            if !controls.contains(control) {
                controls.push(control.clone());
            }
        }
        Self {
            console,
            controls,
            set: console.set_targets(),
            session: show.timing().duration(),
        }
    }

    fn control(&self, control: &ControlRef) -> u16 {
        self.controls
            .iter()
            .position(|read| read == control)
            .unwrap_or_else(|| unreachable!("the controls list every control the console reads"))
            as u16
    }

    /// The length every copy of a clip plays, or a held copy plays before it
    /// repeats, when everything that starts it gives the same fixed one.
    pub(crate) fn fixed_length(&self, clip: ClipId) -> Option<SampleDuration> {
        let always = self
            .console
            .always
            .iter()
            .filter(|always| always.clip == clip)
            .map(|always| Some(sample_duration(&always.length)));
        let mut lengths = self
            .console
            .actions()
            .filter_map(|action| match action {
                Action::Play(play) if play.clip == clip => Some(play.length.fixed()),
                Action::Start { clip: id, length } | Action::Toggle { clip: id, length }
                    if *id == clip =>
                {
                    Some(Some(sample_duration(length)))
                }
                _ => None,
            })
            .chain(always);
        let first = lengths.next()??;
        lengths.all(|length| length == Some(first)).then_some(first)
    }
}

/// A live automation: its index and the first of its live store words.
type Automation = (u16, u32);

/// A console's live automations, issued while its show is built: one per set
/// parameter, and each played clip's voices with an automation per voice for
/// each of its played values.
pub(crate) struct Live<'p> {
    plan: &'p RoutingPlan<'p>,
    set: Vec<Automation>,
    played: Vec<(ClipId, Vec<u16>, Vec<Vec<u16>>)>,
}

impl<'p> Live<'p> {
    /// The set parameters' live automations, each resting where its control
    /// rests.
    pub(crate) fn new(plan: &'p RoutingPlan<'p>, builder: &mut ShowBuilder<'_>) -> Self {
        let set = plan
            .set
            .iter()
            .map(|target| builder.live_automation(plan.console.rest(target)))
            .collect();
        Self {
            plan,
            set,
            played: Vec::new(),
        }
    }

    /// The live store words of `clip`'s live bindings, with `played` the
    /// automations of its copy's played values, and the snapshots its float
    /// bindings' integrals count from, which the clip must place.
    pub(crate) fn words(
        &self,
        clip: AcceptedClip<'_>,
        played: &[Automation],
        builder: &mut ShowBuilder<'_>,
    ) -> (Vec<LiveWords>, Vec<(u16, u32)>) {
        let mut snapshots = Vec::new();
        let words = clip
            .live()
            .iter()
            .enumerate()
            .map(|(index, binding)| {
                let (automation, value) = match *binding {
                    LiveBinding::Set(set) => self.set[set],
                    LiveBinding::Played(value) => played[value],
                    LiveBinding::LiveMarks(_) => {
                        return LiveWords {
                            value: 0,
                            snapshot: None,
                        };
                    }
                };
                let integrates = clip.invocation().automation().iter().any(|bound| {
                    matches!(bound.source, AutomationSource::Live(live) if usize::from(live) == index)
                        && matches!(bound.mapping, AutomationMapping::Float { .. })
                });
                let snapshot = integrates.then(|| {
                    let word = builder.snapshot_word();
                    snapshots.push((automation, word));
                    word
                });
                LiveWords { value, snapshot }
            })
            .collect();
        (words, snapshots)
    }

    /// The live store words of operator `node`'s live bindings.
    pub(crate) fn operator_words(
        &self,
        show: AcceptedShow<'_>,
        node: &CompositionGraphNodeId,
    ) -> Vec<LiveWords> {
        show.operator_automations(node)
            .iter()
            .map(|&set| LiveWords {
                value: self.set[set].1,
                snapshot: None,
            })
            .collect()
    }

    /// The dormant voices of a played clip: as many as its `Play` actions'
    /// most copies, or one held, each with its own played values.
    pub(crate) fn voices<'id>(
        &mut self,
        clips: &mut Clips<'_, 'id>,
        clip: AcceptedClip<'_>,
        constants: ProgramConstants,
        target: TargetHandle<'id>,
        dropped: bool,
    ) {
        let authored = clip.clip();
        let count = match self.plan.console.clip_use(authored.id) {
            ClipUse::Played(count) => count,
            ClipUse::Held => 1,
        };
        let mut voices = Vec::new();
        let mut values = Vec::new();
        for _ in 0..count {
            let played = authored
                .played()
                .iter()
                .map(|_| clips.builder.live_automation(0.0))
                .collect::<Vec<_>>();
            let (words, snapshots) = self.words(clip, &played, clips.builder);
            let program = clips.lower(clip.invocation(), constants, &words);
            let handle = clips.builder.clip(
                None,
                &program,
                SampleTime::from_ticks(0),
                self.plan.session,
                target,
            );
            for (automation, word) in snapshots {
                clips.builder.snapshot(handle, automation, word);
            }
            voices.push(clips.builder.voice(handle));
            values.push(played.iter().map(|(automation, _)| *automation).collect());
            if !dropped {
                clips.layer(&clip.effect().layer, handle);
            }
        }
        self.played.push((authored.id, voices, values));
    }

    /// Route the console's inputs to the voices, mark collections and live
    /// automations issued.
    pub(crate) fn route(self, builder: &mut ShowBuilder<'_>) {
        let plan = self.plan;
        let console = plan.console;
        let set_automations = self
            .set
            .iter()
            .map(|(automation, _)| *automation)
            .collect::<Vec<_>>();
        let ids = self.played.iter().map(|(id, ..)| *id).collect::<Vec<_>>();
        let routed = self
            .played
            .into_iter()
            .map(|(id, voices, values)| {
                let Some(clip) = console.clip(id) else {
                    unreachable!("a played clip is the console's");
                };
                RoutedClip {
                    voices,
                    values: clip
                        .played()
                        .iter()
                        .enumerate()
                        .map(|(index, value)| RoutedValue {
                            source: value.from,
                            window: (value.window.low, value.window.high),
                            automations: values.iter().map(|values| values[index]).collect(),
                        })
                        .collect(),
                }
            })
            .collect();
        let clip = |id| {
            ids.iter()
                .position(|played| *played == id)
                .unwrap_or_else(|| {
                    unreachable!("validation keeps actions on the console's played clips")
                }) as u16
        };
        let collection = |key: &donder_model::MarkCollectionId| {
            console
                .composition
                .marks
                .iter()
                .position(|marks| marks.id == *key)
                .unwrap_or_else(|| unreachable!("validation resolves mark collections"))
                as u16
        };
        let control = |control: &ControlRef| plan.control(control);
        let set = &plan.set;
        let automations = set_automations;
        let ons = console
            .events
            .iter()
            .map(|on| RoutedOn {
                control: control(&on.control),
                happening: on.happening,
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
                        Action::Start { clip: id, length } => RoutedAction::Start {
                            clip: clip(*id),
                            length: sample_duration(length),
                        },
                        Action::Stop { clip: id } => RoutedAction::Stop { clip: clip(*id) },
                        Action::Toggle { clip: id, length } => RoutedAction::Toggle {
                            clip: clip(*id),
                            length: sample_duration(length),
                        },
                        Action::Mark(key) => RoutedAction::Mark {
                            collection: collection(key),
                        },
                        Action::Set { target, window } => RoutedAction::Set {
                            automation: automations[set
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
            controls: plan.controls.len() as u16,
            collections: console.composition.marks.len() as u16,
            always: console
                .always
                .iter()
                .map(|always| (clip(always.clip), sample_duration(&always.length)))
                .collect(),
            clips: routed,
            ons,
        });
    }
}
