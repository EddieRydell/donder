//! Host playback: shows that own their image, built or decoded, and
//! playbacks that own their buffer. Each call re-borrows the image and
//! buffer as the core [`Show`] and workspace.
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::image;
use donder_runtime_types::{RigEvent, SampleDuration, SampleTime, SequenceTiming, Shared};

use crate::playback::{OutputFrame, Playback, SequenceFrame};
use crate::show::{LoadError, LoadLimits, Show};
use crate::workspace::Layout;

mod builder;
mod clip;

pub use builder::{
    EffectHandle, FixtureHandle, LookupHandle, OutputHandle, SequenceBuilder, SequenceRoot,
    SignalHandle, TargetHandle, VoiceLength, WindowHandle,
};
pub use clip::{ClipSampler, SequenceClip};

/// An admitted show image in memory; authoring, elaboration, networking and
/// pin timing are external.
#[derive(Clone)]
pub struct PreparedSequence {
    image: Shared<[u64]>,
    layout: Layout,
}

/// 8-byte-aligned words holding `bytes`.
fn aligned(bytes: &[u8]) -> Box<[u64]> {
    let mut words = vec![0u64; bytes.len().div_ceil(8)].into_boxed_slice();
    bytemuck::cast_slice_mut::<u64, u8>(&mut words)[..bytes.len()].copy_from_slice(bytes);
    words
}

impl PreparedSequence {
    /// Lower accepted inputs through owner-branded handles. All graph addresses,
    /// execution slots, and output spans are derived by the builder.
    pub fn build(
        timing: SequenceTiming,
        build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>) -> SequenceRoot<'id>,
    ) -> Self {
        let mut builder = SequenceBuilder::new(timing);
        let root = build(&mut builder);
        let image: Shared<[u64]> = builder.finish(root).into();
        let bytes: &[u8] = bytemuck::cast_slice(&image);
        debug_assert!(Show::admit(bytes, LoadLimits::UNLIMITED).is_ok());
        let layout = Layout::of(Show::trusted(bytes));
        Self { image, layout }
    }

    /// Check an encoded image and keep a copy of it.
    pub fn decode(bytes: &[u8], limits: LoadLimits) -> Result<Self, LoadError> {
        let image: Shared<[u64]> = aligned(bytes).into();
        let words: &[u8] = bytemuck::cast_slice(&image);
        let show = Show::admit(&words[..bytes.len()], limits)?;
        let layout = Layout::of(show);
        Ok(Self { image, layout })
    }

    /// The image's bytes, as [`Self::decode`] and the firmware accept them.
    pub fn encode(&self) -> Vec<u8> {
        let bytes: &[u8] = bytemuck::cast_slice(&self.image);
        let Ok(len) = image::image_len(bytes, crate::show::FORMAT_VERSION) else {
            unreachable!("an admitted image has a header")
        };
        bytes[..len].to_vec()
    }

    pub fn show(&self) -> Show<'_> {
        Show::trusted(bytemuck::cast_slice(&self.image))
    }

    pub fn fixtures(&self) -> &[image::Fixture] {
        self.show().fixtures()
    }

    pub fn effect_count(&self) -> usize {
        self.show().effect_count()
    }

    /// Start and duration of each retained effect.
    pub fn effect_windows(
        &self,
    ) -> impl ExactSizeIterator<Item = (SampleTime, SampleDuration)> + '_ {
        self.show().s.effects.iter().map(|effect| {
            (
                SampleTime::from_ticks(effect.start),
                SampleDuration::from_ticks(effect.duration),
            )
        })
    }

    pub fn active_effect_count(&self, time: SampleTime) -> usize {
        self.show().active_effect_count(time)
    }

    pub fn outputs(&self) -> &[image::Output] {
        self.show().outputs()
    }

    pub fn frame_rate(&self) -> u32 {
        self.show().frame_rate()
    }

    pub fn frame_count(&self) -> u32 {
        self.show().frame_count()
    }

    pub fn pixel_count(&self) -> usize {
        self.show().pixel_count()
    }

    pub fn duration(&self) -> SampleDuration {
        self.show().duration()
    }

    /// Bytes of the playback buffer.
    pub fn workspace_bytes(&self) -> usize {
        self.layout.words() * 8
    }

    /// Resolve an authored clip without loading or preparing its source again.
    pub fn clip(&self, id: u32) -> Option<SequenceClip<'_>> {
        let show = self.show();
        let index = show
            .s
            .clips
            .binary_search_by_key(&id, |clip| clip.id)
            .ok()?;
        Some(SequenceClip::new(show, show.s.clips[index].effect as usize))
    }

    pub fn into_playback(self) -> SequencePlayback {
        let mut buffer = vec![0u64; self.layout.words()].into_boxed_slice();
        if Playback::new(self.show(), &self.layout, &mut buffer).is_none() {
            unreachable!("the buffer holds the layout's words")
        }
        SequencePlayback {
            sequence: self,
            buffer,
        }
    }
}

/// Owns an admitted show and its playback buffer.
pub struct SequencePlayback {
    sequence: PreparedSequence,
    buffer: Box<[u64]>,
}

impl SequencePlayback {
    fn playback(&mut self) -> Playback<'_, '_> {
        let show = Show::trusted(bytemuck::cast_slice(&self.sequence.image));
        let Some(playback) = Playback::resume(show, &self.sequence.layout, &mut self.buffer) else {
            unreachable!("the buffer holds the layout's words")
        };
        playback
    }

    pub fn sequence(&self) -> &PreparedSequence {
        &self.sequence
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        self.playback().frame(time)
    }

    /// Borrow the packed bytes of the most recently evaluated frame.
    pub fn outputs(&mut self) -> impl ExactSizeIterator<Item = OutputFrame<'_>> {
        self.playback().outputs()
    }

    /// Apply a rig command at its time. Commands arrive in time order; one
    /// the show does not address is ignored.
    pub fn apply(&mut self, event: RigEvent) {
        self.playback().apply(event);
    }

    /// Effects active at `time` as commands have placed them.
    pub fn active_effect_count(&mut self, time: SampleTime) -> usize {
        self.playback().active_effect_count(time)
    }
}

/// A show whose voices start and stop on commands rather than at authored
/// times, with live mark collections and controls.
#[derive(Clone)]
pub struct PreparedRig {
    sequence: PreparedSequence,
}

impl PreparedRig {
    /// Lower a rig's graph like a sequence's, with the voices, live mark
    /// collections and controls the builder registers.
    pub fn build(
        timing: SequenceTiming,
        build: impl for<'id> FnOnce(&mut SequenceBuilder<'id>) -> SequenceRoot<'id>,
    ) -> Self {
        Self {
            sequence: PreparedSequence::build(timing, build),
        }
    }

    pub fn decode(bytes: &[u8], limits: LoadLimits) -> Result<Self, LoadError> {
        Ok(Self {
            sequence: PreparedSequence::decode(bytes, limits)?,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        self.sequence.encode()
    }

    pub fn sequence(&self) -> &PreparedSequence {
        &self.sequence
    }

    pub fn voice_count(&self) -> usize {
        self.sequence.show().voice_count()
    }

    pub fn collection_count(&self) -> usize {
        self.sequence.show().collection_count()
    }

    pub fn control_count(&self) -> usize {
        self.sequence.show().control_count()
    }

    /// Whether `command` addresses a voice, collection or control of this rig.
    pub fn accepts(&self, command: donder_runtime_types::RigCommand) -> bool {
        self.sequence.show().accepts(command)
    }

    pub fn into_playback(self) -> RigPlayback {
        RigPlayback {
            playback: self.sequence.into_playback(),
        }
    }
}

/// Owns a rig's show and evaluates it as commands arrive.
pub struct RigPlayback {
    playback: SequencePlayback,
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

    pub fn apply(&mut self, event: RigEvent) {
        self.playback.apply(event);
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        self.playback.evaluate(time)
    }

    pub fn active_effect_count(&mut self, time: SampleTime) -> usize {
        self.playback.active_effect_count(time)
    }
}
