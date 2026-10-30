//! A show in memory and its playback: a prepared show owns its bytes, a show
//! playback owns its buffer. Each call re-borrows the bytes and buffer as the
//! core [`Show`] and workspace.
use crate::format;
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::{ConsoleEvent, SampleDuration, SampleTime, Shared};

use crate::playback::{Playback, SequenceFrame};
use crate::view::{LoadError, Show};
use crate::workspace::Layout;

/// An encoded show's bytes in memory, allocated once and filled in place by
/// whatever reads them, then opened as a [`PreparedShow`].
pub struct ShowBytes {
    words: Shared<[u32]>,
    len: usize,
}

impl ShowBytes {
    /// Room for an encoded show of `len` bytes.
    pub fn new(len: usize) -> Self {
        Self {
            words: core::iter::repeat_n(0, len.div_ceil(4)).collect(),
            len,
        }
    }

    /// The bytes to fill, in whole 4-byte words.
    pub fn as_mut_bytes(&mut self) -> &mut [u8] {
        let Some(words) = Shared::get_mut(&mut self.words) else {
            unreachable!("the words have one owner until they open")
        };
        bytemuck::cast_slice_mut(words)
    }

    /// The show these bytes encode.
    pub fn open(self) -> Result<PreparedShow, LoadError> {
        let bytes: &[u8] = bytemuck::cast_slice(&self.words);
        let layout = Layout::of(Show::open(&bytes[..self.len])?);
        Ok(PreparedShow {
            words: self.words,
            layout,
        })
    }
}

/// A show in memory: a sequence's, or a console's, whose voices start and
/// stop on commands rather than at authored times. Authoring, elaboration,
/// networking and pin timing are external.
#[derive(Clone)]
pub struct PreparedShow {
    words: Shared<[u32]>,
    layout: Layout,
}

impl PreparedShow {
    /// A show this build wrote.
    #[cfg(feature = "host")]
    pub(crate) fn trusted(words: Shared<[u32]>) -> Self {
        let layout = Layout::of(Show::trusted(bytemuck::cast_slice(&words)));
        Self { words, layout }
    }

    /// Keep a copy of an encoded show of this format.
    pub fn decode(bytes: &[u8]) -> Result<Self, LoadError> {
        let mut copy = ShowBytes::new(bytes.len());
        copy.as_mut_bytes()[..bytes.len()].copy_from_slice(bytes);
        copy.open()
    }

    /// The show's bytes, as [`Self::decode`] and the firmware accept them.
    pub fn encode(&self) -> Vec<u8> {
        self.bytes().to_vec()
    }

    fn bytes(&self) -> &[u8] {
        let bytes: &[u8] = bytemuck::cast_slice(&self.words);
        &bytes[..format::show_len(bytes)]
    }

    pub(crate) fn show(&self) -> Show<'_> {
        Show::trusted(bytemuck::cast_slice(&self.words))
    }

    /// The show's identity, as a controller reports it.
    pub fn id(&self) -> u32 {
        format::show_id(self.bytes())
    }

    pub fn fixtures(&self) -> &[format::Fixture] {
        self.show().fixtures()
    }

    pub fn outputs(&self) -> &[format::Output] {
        self.show().outputs()
    }

    pub fn clip_count(&self) -> usize {
        self.show().clip_count()
    }

    /// Start and duration of each retained clip.
    pub fn clip_windows(&self) -> impl ExactSizeIterator<Item = (SampleTime, SampleDuration)> + '_ {
        self.show().s.clips.iter().map(|clip| {
            (
                SampleTime::from_ticks(clip.start),
                SampleDuration::from_ticks(clip.duration),
            )
        })
    }

    pub fn active_clip_count(&self, time: SampleTime) -> usize {
        self.show().active_clip_count(time)
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
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
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

    /// Apply a console command at its time. Commands arrive in time order; one
    /// the show does not address is ignored.
    pub fn apply(&mut self, event: ConsoleEvent) {
        self.playback().apply(event);
    }
}
