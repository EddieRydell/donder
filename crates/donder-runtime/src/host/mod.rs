//! Host playback: prepared shows that own their image, built or decoded, and
//! playbacks that own their buffer. Each call re-borrows the image and
//! buffer as the core [`Image`] and workspace.
use crate::format;
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::{RigEvent, SampleDuration, SampleTime, SequenceTiming, Shared};

use crate::image::{Image, LoadError};
use crate::playback::{OutputFrame, Playback, SequenceFrame};
use crate::workspace::Layout;

mod builder;
mod clip;

pub use builder::{ClipHandle, OutputHandle, ShowBuilder, SignalHandle, TargetHandle};
pub use clip::{ClipRaster, ImageClip};

/// An image in memory: a sequence's, or a rig's, whose voices start and
/// stop on commands rather than at authored times. Authoring, elaboration,
/// networking and pin timing are external.
#[derive(Clone)]
pub struct PreparedShow {
    words: Shared<[u32]>,
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
        let words: Shared<[u32]> = builder.finish(root).into();
        let bytes: &[u8] = bytemuck::cast_slice(&words);
        let layout = Layout::of(Image::trusted(bytes));
        Self { words, layout }
    }

    /// Keep a copy of an encoded image of this format.
    pub fn decode(bytes: &[u8]) -> Result<Self, LoadError> {
        let words: Shared<[u32]> = aligned(bytes).into();
        let bytes_aligned: &[u8] = bytemuck::cast_slice(&words);
        let image = Image::open(&bytes_aligned[..bytes.len()])?;
        let layout = Layout::of(image);
        Ok(Self { words, layout })
    }

    /// The image's bytes, as [`Self::decode`] and the firmware accept them.
    pub fn encode(&self) -> Vec<u8> {
        let bytes: &[u8] = bytemuck::cast_slice(&self.words);
        let len = format::image_len(bytes);
        bytes[..len].to_vec()
    }

    pub fn image(&self) -> Image<'_> {
        Image::trusted(bytemuck::cast_slice(&self.words))
    }

    pub fn fixtures(&self) -> &[format::Fixture] {
        self.image().fixtures()
    }

    pub fn clip_count(&self) -> usize {
        self.image().clip_count()
    }

    /// Start and duration of each retained clip.
    pub fn clip_windows(&self) -> impl ExactSizeIterator<Item = (SampleTime, SampleDuration)> + '_ {
        self.image().s.clips.iter().map(|clip| {
            (
                SampleTime::from_ticks(clip.start),
                SampleDuration::from_ticks(clip.duration),
            )
        })
    }

    pub fn active_clip_count(&self, time: SampleTime) -> usize {
        self.image().active_clip_count(time)
    }

    pub fn outputs(&self) -> &[format::Output] {
        self.image().outputs()
    }

    pub fn frame_rate(&self) -> u32 {
        self.image().frame_rate()
    }

    pub fn frame_count(&self) -> u32 {
        self.image().frame_count()
    }

    pub fn pixel_count(&self) -> usize {
        self.image().pixel_count()
    }

    pub fn duration(&self) -> SampleDuration {
        self.image().duration()
    }

    /// Bytes of the playback buffer.
    pub fn workspace_bytes(&self) -> usize {
        self.layout.words() * 4
    }

    /// Resolve an authored clip without loading or preparing its source again.
    pub fn clip(&self, id: u32) -> Option<ImageClip<'_>> {
        let image = self.image();
        let index = image
            .s
            .clip_index
            .binary_search_by_key(&id, |entry| entry.id)
            .ok()?;
        Some(ImageClip::new(
            image,
            image.s.clip_index[index].clip as usize,
        ))
    }

    pub fn into_playback(self) -> ShowPlayback {
        let mut buffer = vec![0u32; self.layout.words()].into_boxed_slice();
        if Playback::new(self.image(), &self.layout, &mut buffer).is_none() {
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
        let image = Image::trusted(bytemuck::cast_slice(&self.prepared.words));
        let Some(playback) = Playback::resume(image, &self.prepared.layout, &mut self.buffer)
        else {
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

    /// Clips active at `time` as commands have placed them.
    pub fn active_clip_count(&mut self, time: SampleTime) -> usize {
        self.playback().active_clip_count(time)
    }
}
