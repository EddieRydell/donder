//! A rig: a prepared graph whose voices start and stop on commands rather
//! than at authored times, with live mark collections and controls.
//!
//! Commands change only this playback's own graph and workspace: a voice's
//! start and duration, its layer's start order, and the parameter values the
//! automation path already writes each frame. Frame evaluation is unchanged.

use super::{PreparedSequence, SequenceFrame, SequencePlayback};
use crate::dsl::bytecode::param_bank;
use alloc::{boxed::Box, vec::Vec};
use donder_runtime_types::{Marks, RigCommand, RigEvent, SampleDuration, SampleTime, Shared};

/// The most marks a live collection keeps; older marks are dropped.
pub const LIVE_MARK_CAPACITY: usize = 1024;

/// A dormant voice starts at the end of time, so it is never active and sorts
/// after every started effect of its layer.
const DORMANT: SampleTime = SampleTime::from_ticks(u32::MAX);

#[derive(Clone, Copy, Debug, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub enum VoiceLength {
    /// Plays for this long from each start.
    Fixed(#[rkyv(with = donder_runtime_types::Microseconds)] SampleDuration),
    /// Plays until stopped.
    Open,
}

#[derive(Clone, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct PreparedVoice {
    pub(crate) effect: u32,
    pub(crate) length: VoiceLength,
}

/// An effect's marks parameter that reads a live collection.
#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct MarksReader {
    pub(crate) effect: u32,
    pub(crate) param: u16,
}

/// An effect's automation binding that a control sets.
#[derive(Clone, Copy, Debug, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct ControlTarget {
    pub(crate) effect: u32,
    pub(crate) binding: u16,
}

#[derive(Clone, Debug, Default, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct RigData {
    pub(crate) voices: Box<[PreparedVoice]>,
    pub(crate) collections: Box<[Box<[MarksReader]>]>,
    pub(crate) controls: Box<[Box<[ControlTarget]>]>,
}

/// A rig's archived form: its graph's sequence data and its command tables.
#[derive(rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
pub(crate) struct RigArchiveData {
    pub(crate) sequence: super::SequenceData,
    pub(crate) rig: RigData,
}

/// A prepared graph with the voices, live collections and controls its
/// commands address.
#[derive(Clone)]
pub struct PreparedRig {
    pub(crate) sequence: PreparedSequence,
    pub(crate) rig: RigData,
}

impl PreparedRig {
    /// Lower a rig's graph like a sequence's, with the voices, live mark
    /// collections and controls the builder registers.
    pub fn build(
        timing: donder_runtime_types::SequenceTiming,
        build: impl for<'id> FnOnce(&mut super::SequenceBuilder<'id>) -> super::SequenceRoot<'id>,
    ) -> Self {
        let mut builder = super::SequenceBuilder::new(timing);
        let root = build(&mut builder);
        let rig = builder.take_rig();
        Self {
            sequence: PreparedSequence::assembled(builder.finish(root)),
            rig,
        }
    }

    pub(crate) fn archive_data(&self) -> RigArchiveData {
        RigArchiveData {
            sequence: self.sequence.archive_data(),
            rig: self.rig.clone(),
        }
    }

    /// Admit an archived rig: its graph like a sequence's, and command tables
    /// that address only its own effects, marks parameters and bindings.
    pub(crate) fn from_archive(data: RigArchiveData) -> Result<Self, crate::archive::LoadError> {
        let sequence = PreparedSequence::from_archive(data.sequence)?;
        let signals = &sequence.data.signals;
        let effect = |index: u32| signals.effects.get(index as usize);
        let voices = data
            .rig
            .voices
            .iter()
            .all(|voice| effect(voice.effect).is_some());
        let readers = data.rig.collections.iter().flatten().all(|reader| {
            effect(reader.effect).is_some_and(|effect| {
                signals
                    .sample_program(effect.program)
                    .bytecode()
                    .params
                    .get(usize::from(reader.param))
                    == Some(&donder_runtime_types::Type::Marks)
            })
        });
        let controls = data.rig.controls.iter().flatten().all(|target| {
            effect(target.effect).is_some_and(|effect| {
                effect.automation.as_ref().is_some_and(|automation| {
                    usize::from(target.binding) < automation.bindings.binding_count()
                })
            })
        });
        if !(voices && readers && controls) {
            return Err(crate::archive::LoadError::Archive);
        }
        Ok(Self {
            sequence,
            rig: data.rig,
        })
    }

    pub fn sequence(&self) -> &PreparedSequence {
        &self.sequence
    }

    pub fn voice_count(&self) -> usize {
        self.rig.voices.len()
    }

    pub fn collection_count(&self) -> usize {
        self.rig.collections.len()
    }

    pub fn control_count(&self) -> usize {
        self.rig.controls.len()
    }

    /// Whether `command` addresses a voice, collection or control of this rig.
    pub fn accepts(&self, command: RigCommand) -> bool {
        match command {
            RigCommand::Start { voice } | RigCommand::Stop { voice } => {
                usize::from(voice) < self.voice_count()
            }
            RigCommand::Mark { collection } => usize::from(collection) < self.collection_count(),
            RigCommand::Control { control, value } => {
                usize::from(control) < self.control_count() && (0.0..=1.0).contains(&value)
            }
        }
    }

    pub fn into_playback(self) -> RigPlayback {
        let mut playback = self.sequence.into_playback();
        let signals = &mut playback.sequence.data.signals;
        for voice in &self.rig.voices {
            signals.effects[voice.effect as usize].start_time = DORMANT;
        }
        let mut layer_of = alloc::vec![usize::MAX; signals.effects.len()];
        for (layer, effects) in signals.effects_by_layer.iter_mut().enumerate() {
            for &effect in effects.iter() {
                layer_of[effect] = layer;
            }
        }
        let effects = &signals.effects;
        for layer in signals.effects_by_layer.iter_mut() {
            layer.sort_by_key(|&index| effects[index].start_time);
        }
        let banks = self
            .rig
            .collections
            .iter()
            .map(|readers| {
                readers
                    .iter()
                    .map(|reader| {
                        let effect = &signals.effects[reader.effect as usize];
                        let params = &signals.sample_program(effect.program).bytecode().params;
                        (
                            reader.effect as usize,
                            usize::from(param_bank(params, usize::from(reader.param))),
                        )
                    })
                    .collect()
            })
            .collect();
        RigPlayback {
            playback,
            tracks: alloc::vec![Vec::new(); self.rig.collections.len()],
            rig: self.rig,
            layer_of,
            banks,
        }
    }
}

/// A live collection's readers as (effect, marks resource bank).
type CollectionReaders = Box<[(usize, usize)]>;

/// Owns a rig's graph and evaluates it as commands arrive.
pub struct RigPlayback {
    playback: SequencePlayback,
    rig: RigData,
    /// The layer each effect belongs to.
    layer_of: Vec<usize>,
    banks: Box<[CollectionReaders]>,
    /// Each collection's mark times in ticks, ascending.
    tracks: Vec<Vec<u32>>,
}

impl RigPlayback {
    pub fn duration(&self) -> SampleDuration {
        self.playback.sequence.duration()
    }

    pub fn frame_rate(&self) -> u32 {
        self.playback.sequence.frame_rate()
    }

    pub fn sequence(&self) -> &PreparedSequence {
        self.playback.sequence()
    }

    /// Apply a command at its time. Commands arrive in time order; one the
    /// rig does not address is ignored.
    pub fn apply(&mut self, event: RigEvent) {
        let time = event.time;
        match event.command {
            RigCommand::Start { voice } => {
                if let Some(voice) = self.rig.voices.get(usize::from(voice)).cloned() {
                    self.start(voice, time);
                }
            }
            RigCommand::Stop { voice } => {
                if let Some(voice) = self.rig.voices.get(usize::from(voice)) {
                    let effect =
                        &mut self.playback.sequence.data.signals.effects[voice.effect as usize];
                    if effect.is_active(time) {
                        effect.duration = time
                            .checked_duration_since(effect.start_time)
                            .unwrap_or(SampleDuration::from_ticks(0));
                    }
                }
            }
            RigCommand::Mark { collection } => {
                let index = usize::from(collection);
                let Some(track) = self.tracks.get_mut(index) else {
                    return;
                };
                if track.len() == LIVE_MARK_CAPACITY {
                    track.remove(0);
                }
                track.push(time.as_ticks());
                for &(effect, _) in self.banks[index].clone().iter() {
                    self.bind_marks(effect);
                }
            }
            RigCommand::Control { control, value } => {
                let Some(targets) = self.rig.controls.get(usize::from(control)) else {
                    return;
                };
                if !(0.0..=1.0).contains(&value) {
                    return;
                }
                let signals = &self.playback.sequence.data.signals;
                let workspace = &mut self.playback.workspace.sampling.effect_automation;
                for target in targets.iter() {
                    let effect = &signals.effects[target.effect as usize];
                    let Some(automation) = &effect.automation else {
                        continue;
                    };
                    workspace[automation.workspace_slot].plan.set_value(
                        usize::from(target.binding),
                        value,
                        time,
                    );
                }
            }
        }
        self.playback.workspace.forget_samples();
    }

    fn start(&mut self, voice: PreparedVoice, time: SampleTime) {
        let effect_index = voice.effect as usize;
        let signals = &mut self.playback.sequence.data.signals;
        let session_end = signals.duration.as_ticks();
        let effect = &mut signals.effects[effect_index];
        effect.start_time = time;
        effect.duration = match voice.length {
            VoiceLength::Fixed(duration) => duration,
            VoiceLength::Open => {
                SampleDuration::from_ticks(session_end.saturating_sub(time.as_ticks()))
            }
        };
        if let Some(automation) = &effect.automation {
            self.playback.workspace.sampling.effect_automation[automation.workspace_slot]
                .plan
                .restart_at(time);
        }
        let layer = self.layer_of[effect_index];
        if let Some(order) = signals.effects_by_layer.get_mut(layer) {
            let effects = &signals.effects;
            order.sort_by_key(|&index| effects[index].start_time);
        }
        self.bind_marks(effect_index);
    }

    /// Bind each live collection `effect` reads, measured from its start.
    fn bind_marks(&mut self, effect: usize) {
        let signals = &mut self.playback.sequence.data.signals;
        let prepared = &mut signals.effects[effect];
        if prepared.start_time == DORMANT {
            return;
        }
        let origin = prepared.start_time.as_ticks();
        for (collection, readers) in self.banks.iter().enumerate() {
            for &(_, bank) in readers.iter().filter(|(reader, _)| *reader == effect) {
                let track: Shared<[u32]> = self.tracks[collection].as_slice().into();
                let marks = Shared::new(Marks::window(track, origin, u32::MAX));
                prepared.bound_params.set_marks(bank, Shared::clone(&marks));
                if let Some(automation) = &prepared.automation {
                    self.playback.workspace.sampling.effect_automation[automation.workspace_slot]
                        .params
                        .set_marks(bank, marks);
                }
            }
        }
    }

    pub fn evaluate(&mut self, sample_time: SampleTime) -> SequenceFrame<'_> {
        self.playback.evaluate(sample_time)
    }

    pub fn active_effect_count(&self, sample_time: SampleTime) -> usize {
        self.playback.sequence.active_effect_count(sample_time)
    }
}
