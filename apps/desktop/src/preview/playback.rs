use std::time::{Duration, Instant};

use donder_runtime::{LoadError, LoadLimits, decode_rig, decode_sequence};
use donder_runtime::{PlaybackRate, PreparedSequence, RigPlayback, SequencePlayback};
use donder_runtime_types::{RigEvent, SampleTime, sample_time_from_seconds_f32};

use super::protocol::PreviewArchive;
use super::scene::PreviewColor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewPlaybackState {
    Unavailable,
    Playing,
    Paused,
    Stopped,
    Ended,
}

/// Audio positions advance once per output buffer and arrive after pipe delay,
/// so smaller differences within one playback run are noise, not drift.
const CLOCK_RESYNC_TOLERANCE_SECONDS: f32 = 0.025;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewClockUpdate {
    Absorbed,
    Reanchored,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PreviewClockSnapshot {
    pub(crate) generation: u32,
    pub(crate) state: PreviewPlaybackState,
    pub(crate) position_seconds: f32,
    pub(crate) start_delay_seconds: f32,
    pub(crate) rate: PlaybackRate,
}

#[derive(Clone, Copy, Debug)]
struct ClockAnchor {
    snapshot: PreviewClockSnapshot,
    received_at: Instant,
}

impl ClockAnchor {
    fn position_at(self, now: Instant) -> f32 {
        if self.snapshot.state == PreviewPlaybackState::Playing {
            let wall = (now
                .saturating_duration_since(self.received_at)
                .as_secs_f32()
                - self.snapshot.start_delay_seconds)
                .max(0.0);
            self.snapshot.position_seconds
                + self.snapshot.rate.show_elapsed((wall * 1_000_000.0) as u64) as f32 / 1_000_000.0
        } else {
            self.snapshot.position_seconds
        }
    }
}

/// What the Preview evaluates: a sequence, or a rig and the commands it has
/// received.
enum PreviewContent {
    Sequence(Box<SequencePlayback>),
    Rig(Box<RigPlayback>),
}

impl PreviewContent {
    fn sequence(&self) -> &PreparedSequence {
        match self {
            Self::Sequence(playback) => playback.sequence(),
            Self::Rig(playback) => playback.sequence(),
        }
    }

    fn evaluate(&mut self, sample_time: SampleTime) -> donder_runtime::SequenceFrame<'_> {
        match self {
            Self::Sequence(playback) => playback.evaluate(sample_time),
            Self::Rig(playback) => playback.evaluate(sample_time),
        }
    }
}

/// The frame last evaluated: the clock generation, the rig commands applied
/// before it and its frame index.
type FrameKey = (u32, u64, u64);

pub(crate) struct PreviewPlayback {
    content: Option<PreviewContent>,
    colors: Vec<PreviewColor>,
    unlit: PreviewColor,
    clock: ClockAnchor,
    /// Counts the rig command batches applied, so a command changes the frame
    /// without changing the time.
    commands: u64,
    last_frame: Option<FrameKey>,
}

impl PreviewPlayback {
    pub(crate) fn new(unlit: PreviewColor) -> Self {
        Self {
            content: None,
            colors: Vec::new(),
            unlit,
            clock: ClockAnchor {
                snapshot: PreviewClockSnapshot {
                    generation: 0,
                    state: PreviewPlaybackState::Unavailable,
                    position_seconds: 0.0,
                    start_delay_seconds: 0.0,
                    rate: PlaybackRate::NORMAL,
                },
                received_at: Instant::now(),
            },
            commands: 0,
            last_frame: None,
        }
    }

    pub(crate) fn set_unlit(&mut self, unlit: PreviewColor) {
        if self.unlit == unlit {
            return;
        }
        self.unlit = unlit;
        if self.content.is_none() {
            self.colors.fill(unlit);
        }
    }

    pub(crate) fn replace_content(
        &mut self,
        archive: Option<&PreviewArchive>,
        instance_count: usize,
    ) -> Result<(), PreviewPlaybackError> {
        let content = match archive {
            None => None,
            Some(PreviewArchive::Sequence(bytes)) => Some(PreviewContent::Sequence(Box::new(
                decode_sequence(bytes, preview_load_limits())
                    .map_err(PreviewPlaybackError::Decode)?
                    .into_playback(),
            ))),
            Some(PreviewArchive::Rig(bytes)) => Some(PreviewContent::Rig(Box::new(
                decode_rig(bytes, preview_load_limits())
                    .map_err(PreviewPlaybackError::Decode)?
                    .into_playback(),
            ))),
        };
        if let Some(content) = content.as_ref()
            && content.sequence().pixel_count() != instance_count
        {
            return Err(PreviewPlaybackError::PixelCount {
                sequence: content.sequence().pixel_count(),
                scene: instance_count,
            });
        }
        self.content = content;
        self.colors.clear();
        self.colors.resize(instance_count, self.unlit);
        self.last_frame = None;
        Ok(())
    }

    /// Apply a performance's commands, in order, to the rig the Preview plays.
    pub(crate) fn apply_rig_events(
        &mut self,
        events: &[RigEvent],
    ) -> Result<(), PreviewPlaybackError> {
        let Some(PreviewContent::Rig(playback)) = self.content.as_mut() else {
            return Err(PreviewPlaybackError::NoRig);
        };
        for event in events {
            playback.apply(*event);
        }
        self.commands = self.commands.wrapping_add(1);
        Ok(())
    }

    pub(crate) fn set_clock(
        &mut self,
        snapshot: PreviewClockSnapshot,
        received_at: Instant,
    ) -> PreviewClockUpdate {
        let anchor = ClockAnchor {
            snapshot,
            received_at,
        };
        let current = self.clock.snapshot;
        if snapshot.state == PreviewPlaybackState::Playing
            && snapshot.state == current.state
            && snapshot.generation == current.generation
            && (anchor.position_at(received_at) - self.clock.position_at(received_at)).abs()
                <= CLOCK_RESYNC_TOLERANCE_SECONDS
        {
            return PreviewClockUpdate::Absorbed;
        }
        self.clock = anchor;
        PreviewClockUpdate::Reanchored
    }

    pub(crate) fn evaluate(&mut self, now: Instant) -> Result<bool, PreviewPlaybackError> {
        let Some(content) = self.content.as_mut() else {
            return Ok(self.last_frame.take().is_some());
        };
        if self.clock.snapshot.state == PreviewPlaybackState::Unavailable {
            let changed = self.colors.iter().any(|color| *color != self.unlit);
            self.colors.fill(self.unlit);
            self.last_frame = None;
            return Ok(changed);
        }
        let position = self.clock.position_at(now).max(0.0);
        let sample_time = sample_time_from_seconds_f32(position)
            .map_err(|_| PreviewPlaybackError::ClockPosition)?;
        let frame = self.clock.snapshot.rate.frame_at(
            u64::from(sample_time.as_ticks()),
            content.sequence().frame_rate(),
        );
        let key = (self.clock.snapshot.generation, self.commands, frame);
        if self.last_frame == Some(key) {
            return Ok(false);
        }
        let evaluated = content.evaluate(sample_time);
        for (target, color) in self.colors.iter_mut().zip(evaluated.colors()) {
            *target = PreviewColor::opaque([color.red, color.green, color.blue]);
        }
        self.last_frame = Some(key);
        Ok(true)
    }

    pub(crate) fn colors(&self) -> &[PreviewColor] {
        &self.colors
    }

    pub(crate) fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let content = self.content.as_ref()?;
        if self.clock.snapshot.state != PreviewPlaybackState::Playing {
            return None;
        }
        let frame_rate = content.sequence().frame_rate();
        if frame_rate == 0 {
            return None;
        }
        let rate = self.clock.snapshot.rate;
        let position = (self.clock.position_at(now).max(0.0) * 1_000_000.0) as u64;
        let next_position = rate.frame_start(rate.frame_at(position, frame_rate) + 1, frame_rate);
        let delay = Duration::from_micros(rate.wall_elapsed(next_position - position).max(1_000));
        Some(now + delay)
    }
}

fn preview_load_limits() -> LoadLimits {
    LoadLimits {
        payload_bytes: 16 * 1024 * 1024,
        pixels: 1_000_000,
        graph_nodes: 100_000,
        workspace_bytes: 64 * 1024 * 1024,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PreviewPlaybackError {
    Decode(LoadError),
    PixelCount {
        sequence: usize,
        scene: usize,
    },
    ClockPosition,
    /// Rig commands arrived while the Preview plays no rig.
    NoRig,
}
