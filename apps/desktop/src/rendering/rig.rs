use std::time::{Duration, Instant};

use donder_model::ControllerId;
use donder_output::OutputFrame;
use donder_runtime::{PreparedShow, ShowPlayback};
use donder_runtime_types::{RigEvent, SampleDuration, SampleTime};

use super::sequence::controller_frames;

/// A performance's clock: wall time since the performance started, as
/// session time. It stops at the end of the session.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SessionClock {
    started: Instant,
    duration: SampleDuration,
}

impl SessionClock {
    pub(crate) fn start(duration: SampleDuration) -> Self {
        Self {
            started: Instant::now(),
            duration,
        }
    }

    pub(crate) fn now(&self) -> SampleTime {
        let elapsed = self
            .started
            .elapsed()
            .as_micros()
            .min(u128::from(self.duration.as_ticks()));
        SampleTime::from_ticks(elapsed as u32)
    }
}

/// A rig played to controllers: its playback, the commands applied so far,
/// and the frame last rendered.
pub(crate) struct RigOutput {
    playback: ShowPlayback,
    controllers: Vec<ControllerId>,
    clock: SessionClock,
    /// Counts the event batches applied, so a frame reflects its commands.
    revision: u64,
    rendered: Option<RenderedRigFrame>,
}

struct RenderedRigFrame {
    frame: u64,
    revision: u64,
    controller_frames: Vec<OutputFrame>,
}

impl RigOutput {
    /// `events` are the performance's commands so far, applied in order.
    pub(crate) fn new(
        prepared: PreparedShow,
        events: &[RigEvent],
        controllers: Vec<ControllerId>,
        clock: SessionClock,
    ) -> Self {
        let mut output = Self {
            playback: prepared.into_playback(),
            controllers,
            clock,
            revision: 0,
            rendered: None,
        };
        output.apply(events);
        output
    }

    pub(crate) fn apply(&mut self, events: &[RigEvent]) {
        for event in events {
            self.playback.apply(*event);
        }
        self.revision = self.revision.saturating_add(1);
    }

    pub(crate) fn frame_interval(&self) -> Duration {
        Duration::from_secs(1) / self.playback.prepared().frame_rate()
    }

    /// The controller frames at the session's current time.
    pub(crate) fn render(&mut self) -> &[OutputFrame] {
        let time = self.clock.now();
        let frame = u64::from(time.as_ticks()) * u64::from(self.playback.prepared().frame_rate())
            / 1_000_000;
        let rendered = match self.rendered.take() {
            Some(rendered) if rendered.frame == frame && rendered.revision == self.revision => {
                rendered
            }
            _ => RenderedRigFrame {
                frame,
                revision: self.revision,
                controller_frames: controller_frames(
                    &self.playback.evaluate(time),
                    &self.controllers,
                ),
            },
        };
        &self.rendered.insert(rendered).controller_frames
    }
}
