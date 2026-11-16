//! A show in memory and its playback. A prepared show owns its bytes and a
//! show playback owns its buffer; a region playback keeps both in memory its
//! caller owns. Each call re-borrows the bytes and buffer as the core
//! [`Show`] and workspace.
use crate::format;
use alloc::{boxed::Box, vec, vec::Vec};
use donder_runtime_types::{ConsoleEvent, SampleDuration, SampleTime, Shared};

use crate::console::Router;
use crate::playback::{Playback, SequenceFrame};
use crate::view::{LoadError, Show};
use crate::workspace::Layout;

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
        let mut words = vec![0u32; bytes.len().div_ceil(4)];
        bytemuck::cast_slice_mut::<u32, u8>(&mut words)[..bytes.len()].copy_from_slice(bytes);
        let layout = Layout::of(Show::open(bytes_of(&words, bytes.len())?)?);
        Ok(Self {
            words: words.into(),
            layout,
        })
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

    /// Play the show from its start; a console's session starts its
    /// `always` clips.
    pub fn into_playback(self) -> ShowPlayback {
        let mut buffer = vec![0u32; self.layout.words()].into_boxed_slice();
        let show = self.show();
        let mut router = Router::new(show);
        restart(show, &self.layout, &mut buffer, &mut router);
        ShowPlayback {
            prepared: self,
            buffer,
            router,
        }
    }
}

/// The first `len` bytes of `words` when they hold exactly one show's
/// header, table and payload.
fn bytes_of(words: &[u32], len: usize) -> Result<&[u8], LoadError> {
    let bytes = bytemuck::cast_slice::<u32, u8>(words)
        .get(..len)
        .ok_or(LoadError::Version)?;
    Show::open(bytes)?;
    if format::show_len(bytes) != len {
        return Err(LoadError::Version);
    }
    Ok(bytes)
}

/// Owns a show, its playback buffer and a console's routing state.
pub struct ShowPlayback {
    prepared: PreparedShow,
    buffer: Box<[u32]>,
    router: Router,
}

impl ShowPlayback {
    /// Play the show from its start again, as [`PreparedShow::into_playback`]
    /// does, in the buffer it has.
    pub fn restart(&mut self) {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
        restart(
            show,
            &self.prepared.layout,
            &mut self.buffer,
            &mut self.router,
        );
    }

    pub fn prepared(&self) -> &PreparedShow {
        &self.prepared
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
        resume(show, &self.prepared.layout, &mut self.buffer).frame(time)
    }

    /// Route a console input at its time. Inputs arrive in time order; one of
    /// a control the console does not read is ignored.
    pub fn apply(&mut self, event: ConsoleEvent) {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
        route(
            show,
            &self.prepared.layout,
            &mut self.buffer,
            &mut self.router,
            event,
        );
    }

    /// Release every held control at `time`, as when the input that held them
    /// goes away.
    pub fn release_all(&mut self, time: SampleTime) {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
        release_all(
            show,
            &self.prepared.layout,
            &mut self.buffer,
            &mut self.router,
            time,
        );
    }

    /// Apply a console command at `time`, without routing.
    #[cfg(test)]
    pub(crate) fn command(
        &mut self,
        (time, command): (SampleTime, crate::playback::ConsoleCommand),
    ) {
        let show = Show::trusted(bytemuck::cast_slice(&self.prepared.words));
        resume(show, &self.prepared.layout, &mut self.buffer).apply(time, command);
    }
}

/// Memory a caller owns for a show and its playback buffer: the show's words
/// first, then the buffer. Playing from it allocates nothing but a console's
/// routing state.
pub struct ShowRegion<'r> {
    words: &'r mut [u32],
}

impl<'r> ShowRegion<'r> {
    pub fn new(words: &'r mut [u32]) -> Self {
        Self { words }
    }

    /// Bytes the region holds.
    pub fn capacity(&self) -> usize {
        self.words.len() * 4
    }

    /// The region's bytes, to fill with a show from its start.
    pub fn bytes_mut(&mut self) -> &mut [u8] {
        bytemuck::cast_slice_mut(self.words)
    }

    /// The region's words, to rewrite the show they hold in place.
    pub fn words_mut(&mut self) -> &mut [u32] {
        self.words
    }

    /// Play the show the region's first `len` bytes hold, from its start.
    /// The region comes back with the reason when they hold no show of this
    /// format, or the show and its buffer do not fit.
    pub fn open(self, len: usize) -> Result<RegionPlayback<'r>, (Self, LoadError)> {
        let layout = match fit(self.words, len) {
            Ok(layout) => layout,
            Err(error) => return Err((self, error)),
        };
        let mut router = Router::new(Show::trusted(bytemuck::cast_slice(&self.words[..len / 4])));
        let (show, buffer) = self.words.split_at_mut(len / 4);
        restart(
            Show::trusted(bytemuck::cast_slice(show)),
            &layout,
            buffer,
            &mut router,
        );
        Ok(RegionPlayback {
            words: self.words,
            len,
            layout,
            router,
        })
    }
}

/// The layout of the show `words` hold in their first `len` bytes, when it
/// and its buffer fit in them.
fn fit(words: &[u32], len: usize) -> Result<Layout, LoadError> {
    let capacity = words.len() * 4;
    if len > capacity {
        return Err(LoadError::Capacity {
            needed: len,
            capacity,
        });
    }
    let layout = Layout::of(Show::open(bytes_of(words, len)?)?);
    let needed = len + layout.words() * 4;
    if needed > capacity {
        return Err(LoadError::Capacity { needed, capacity });
    }
    Ok(layout)
}

/// A show playing from a [`ShowRegion`].
pub struct RegionPlayback<'r> {
    words: &'r mut [u32],
    /// The show's bytes; its buffer follows them.
    len: usize,
    layout: Layout,
    router: Router,
}

impl<'r> RegionPlayback<'r> {
    fn show(&self) -> Show<'_> {
        Show::trusted(bytemuck::cast_slice(&self.words[..self.len / 4]))
    }

    /// Stop playing and give the region back, still holding the show's bytes.
    pub fn into_region(self) -> ShowRegion<'r> {
        ShowRegion { words: self.words }
    }

    /// The show's length in bytes.
    pub fn show_bytes(&self) -> usize {
        self.len
    }

    /// Bytes the region holds.
    pub fn capacity(&self) -> usize {
        self.words.len() * 4
    }

    /// The show's identity, as a controller reports it.
    pub fn id(&self) -> u32 {
        format::show_id(bytemuck::cast_slice(&self.words[..self.len / 4]))
    }

    pub fn outputs(&self) -> &[format::Output] {
        self.show().outputs()
    }

    pub fn frame_rate(&self) -> u32 {
        self.show().frame_rate()
    }

    pub fn duration(&self) -> SampleDuration {
        self.show().duration()
    }

    /// Play the show from its start again.
    pub fn restart(&mut self) {
        let (show, buffer) = self.words.split_at_mut(self.len / 4);
        restart(
            Show::trusted(bytemuck::cast_slice(show)),
            &self.layout,
            buffer,
            &mut self.router,
        );
    }

    pub fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        let (show, buffer) = self.words.split_at_mut(self.len / 4);
        resume(
            Show::trusted(bytemuck::cast_slice(show)),
            &self.layout,
            buffer,
        )
        .frame(time)
    }

    /// Route a console input at its time, as [`ShowPlayback::apply`] does.
    pub fn apply(&mut self, event: ConsoleEvent) {
        let (show, buffer) = self.words.split_at_mut(self.len / 4);
        route(
            Show::trusted(bytemuck::cast_slice(show)),
            &self.layout,
            buffer,
            &mut self.router,
            event,
        );
    }

    /// Release every held control at `time`.
    pub fn release_all(&mut self, time: SampleTime) {
        let (show, buffer) = self.words.split_at_mut(self.len / 4);
        release_all(
            Show::trusted(bytemuck::cast_slice(show)),
            &self.layout,
            buffer,
            &mut self.router,
            time,
        );
    }
}

/// Continue `show` in the buffer an earlier call left.
fn resume<'a, 'w>(show: Show<'a>, layout: &Layout, buffer: &'w mut [u32]) -> Playback<'a, 'w> {
    let Some(playback) = Playback::resume(show, layout, buffer) else {
        unreachable!("the buffer holds the layout's words")
    };
    playback
}

/// Start `show` in a fresh buffer; a console's session starts its `always`
/// clips.
fn restart(show: Show<'_>, layout: &Layout, buffer: &mut [u32], router: &mut Router) {
    buffer.fill(0);
    let Some(mut playback) = Playback::new(show, layout, buffer) else {
        unreachable!("the buffer holds the layout's words")
    };
    *router = Router::new(show);
    router.start(show, SampleTime::from_ticks(0), &mut |command| {
        playback.apply(SampleTime::from_ticks(0), command);
    });
}

fn route(
    show: Show<'_>,
    layout: &Layout,
    buffer: &mut [u32],
    router: &mut Router,
    event: ConsoleEvent,
) {
    let mut playback = resume(show, layout, buffer);
    router.route(show, event, &mut |command| {
        playback.apply(event.time, command);
    });
}

fn release_all(
    show: Show<'_>,
    layout: &Layout,
    buffer: &mut [u32],
    router: &mut Router,
    time: SampleTime,
) {
    let mut playback = resume(show, layout, buffer);
    router.release_all(show, time, &mut |command| {
        playback.apply(time, command);
    });
}
