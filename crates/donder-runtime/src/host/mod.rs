//! Host playback: shows that own their image, built or decoded, and
//! playbacks that own their buffer. Each call re-borrows the image and
//! buffer as the core [`Show`] and workspace.
use crate::image;
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::{RigEvent, SampleDuration, SampleTime, SequenceTiming, Shared};

use crate::playback::{OutputFrame, Playback, SequenceFrame};
use crate::show::{LoadError, Show};
use crate::workspace::Layout;

mod builder;
mod clip;

pub use builder::{EffectHandle, OutputHandle, ShowBuilder, SignalHandle, TargetHandle};
pub use clip::{ClipSampler, SequenceClip};

/// A show image in memory: a sequence's, or a rig's, whose voices start and
/// stop on commands rather than at authored times. Authoring, elaboration,
/// networking and pin timing are external.
#[derive(Clone)]
pub struct PreparedShow {
    image: Shared<[u32]>,
    layout: Layout,
}

/// 4-byte-aligned words holding `bytes`.
fn aligned(bytes: &[u8]) -> Box<[u32]> {
    let mut words = vec![0u32; bytes.len().div_ceil(4)].into_boxed_slice();
    bytemuck::cast_slice_mut::<u32, u8>(&mut words)[..bytes.len()].copy_from_slice(bytes);
    words
}

impl PreparedShow {
    /// Lower accepted inputs through owner-branded handles. All graph addresses,
    /// execution slots, and output spans are derived by the builder.
    pub fn build(
        timing: SequenceTiming,
        build: impl for<'id> FnOnce(&mut ShowBuilder<'id>) -> Option<SignalHandle<'id>>,
    ) -> Self {
        let mut builder = ShowBuilder::new(timing);
        let root = build(&mut builder);
        let image: Shared<[u32]> = builder.finish(root).into();
        let bytes: &[u8] = bytemuck::cast_slice(&image);
        let layout = Layout::of(Show::trusted(bytes));
        Self { image, layout }
    }

    /// Keep a copy of an encoded image of this format.
    pub fn decode(bytes: &[u8]) -> Result<Self, LoadError> {
        let image: Shared<[u32]> = aligned(bytes).into();
        let words: &[u8] = bytemuck::cast_slice(&image);
        let show = Show::open(&words[..bytes.len()])?;
        let layout = Layout::of(show);
        Ok(Self { image, layout })
    }

    /// The image's bytes, as [`Self::decode`] and the firmware accept them.
    pub fn encode(&self) -> Vec<u8> {
        let bytes: &[u8] = bytemuck::cast_slice(&self.image);
        let len = image::image_len(bytes);
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
        self.layout.words() * 4
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

    pub fn into_playback(self) -> ShowPlayback {
        let mut buffer = vec![0u32; self.layout.words()].into_boxed_slice();
        if Playback::new(self.show(), &self.layout, &mut buffer).is_none() {
            unreachable!("the buffer holds the layout's words")
        }
        ShowPlayback {
            prepared: self,
            buffer,
        }
    }
}

/// Owns a show and its playback buffer.
pub struct ShowPlayback {
    prepared: PreparedShow,
    buffer: Box<[u32]>,
}

impl ShowPlayback {
    fn playback(&mut self) -> Playback<'_, '_> {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.image));
        let Some(playback) = Playback::resume(show, &self.prepared.layout, &mut self.buffer) else {
            unreachable!("the buffer holds the layout's words")
        };
        playback
    }

    pub fn prepared(&self) -> &PreparedShow {
        &self.prepared
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
