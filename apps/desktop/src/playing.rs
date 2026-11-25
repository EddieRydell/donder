//! What plays: the open sequence on the audio transport, or a performance on
//! its session clock. Live output, the Preview and Donder controllers all
//! follow the one show that plays.

use std::time::{Duration, Instant};

use donder_editor::dto::{AudioTransportSnapshot, AudioTransportState};
use donder_elaboration::{PreparationCache, Retain};
use donder_language::sample_time_from_seconds_f64;
use donder_model::{Controller, ControllerId, DonderProject, ShowId};
use donder_output::OutputFrame;
use donder_runtime::{PlaybackRate, PreparedShow, ShowPlayback};
use donder_runtime_types::{
    ConsoleEvent, SampleDuration, SampleTime, SampleTimeError, sample_time_seconds_f32,
};

use crate::audio::{AudioEngine, Timeline};

/// A show that plays, and the clock it plays on.
#[derive(Clone)]
pub(crate) struct Playing {
    pub(crate) id: ShowId,
    pub(crate) show: PreparedShow,
    pub(crate) clock: PlayClock,
    /// A performance's inputs so far, in order; none for a sequence.
    pub(crate) events: Vec<ConsoleEvent>,
    /// The controllers of the project the show was prepared on, in the order
    /// its outputs index them.
    pub(crate) controllers: Vec<(ControllerId, Controller)>,
    /// Numbers what plays, so a follower tells one show from the next.
    pub(crate) generation: u32,
}

/// The clock a show plays on.
#[derive(Clone, Copy, Debug)]
pub(crate) enum PlayClock {
    /// The audio transport's, which the sequence editor drives.
    Audio,
    /// A performance's, from the moment it started.
    Session(SessionClock),
}

/// Wall time since a performance started, as session time. It stops at the
/// end of the session.
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

impl PlayClock {
    /// Where the clock is now and how it moves, as Donder controllers take
    /// it up: the transport's timeline, or a session playing at normal speed
    /// with no window end. `None` while the transport has no audio.
    pub(crate) fn anchor(&self, audio: &mut AudioEngine) -> Option<(Timeline, PlaybackRate)> {
        match self {
            Self::Audio => Some((audio.timeline()?, audio.playback_rate())),
            Self::Session(clock) => Some((
                Timeline::Playing {
                    at: Instant::now(),
                    position: f64::from(sample_time_seconds_f32(clock.now())),
                    rate: PlaybackRate::NORMAL,
                    end: None,
                },
                PlaybackRate::NORMAL,
            )),
        }
    }
}

impl Playing {
    /// The Preview's clock: the transport's, or the session's playing from
    /// its start.
    pub(crate) fn preview_clock(
        &self,
        audio: AudioTransportSnapshot,
    ) -> crate::preview::PreviewClock {
        match self.clock {
            PlayClock::Audio => audio.into(),
            PlayClock::Session(clock) => crate::preview::PreviewClock {
                generation: self.generation,
                state: AudioTransportState::Playing,
                position_seconds: f64::from(sample_time_seconds_f32(clock.now())),
                playback_speed: PlaybackRate::NORMAL.into(),
            },
        }
    }

    /// Each fixture's id and pixel count, in prepared order.
    pub(crate) fn fixtures(&self) -> Vec<(u32, usize)> {
        self.show
            .fixtures()
            .iter()
            .map(|fixture| (fixture.id, fixture.pixel_count as usize))
            .collect()
    }
}

/// `id` prepared on `project` for `retain`: a sequence, or an armed console.
pub(crate) fn prepare_show(
    project: &DonderProject,
    id: ShowId,
    retain: Retain<'_>,
    cache: &mut PreparationCache,
) -> Option<PreparedShow> {
    match id {
        ShowId::Sequence(id) => donder_elaboration::prepare_cached(project, &id, retain, cache),
        ShowId::Console(id) => {
            donder_elaboration::arm(project, &id, retain, cache).map(|armed| armed.prepared)
        }
    }
}

/// A show played to controllers: its playback, the inputs applied so far and
/// the frame last rendered.
pub(crate) struct PlayingOutput {
    playing: Playing,
    playback: ShowPlayback,
    /// How many of the inputs the playback applied.
    applied: usize,
    rendered: Option<RenderedFrame>,
}

/// A frame, by the clock generation, inputs applied and frame index it
/// renders.
struct RenderedFrame {
    key: (u32, usize, u32),
    controller_frames: Vec<OutputFrame>,
}

/// A frame to send, and when the next one is due.
pub(crate) struct OutputTick<'a> {
    pub(crate) frames: &'a [OutputFrame],
    pub(crate) interval: Duration,
    /// Whether the clock moves; a paused or stopped transport holds.
    pub(crate) moving: bool,
}

impl PlayingOutput {
    pub(crate) fn new(playing: Playing) -> Self {
        let mut output = Self {
            playback: playing.show.clone().into_playback(),
            playing,
            applied: 0,
            rendered: None,
        };
        output.apply(&[]);
        output
    }

    /// Whether the show plays on a performance's session clock, whose inputs
    /// follow it.
    pub(crate) fn is_session(&self) -> bool {
        matches!(self.playing.clock, PlayClock::Session(_))
    }

    /// A performance's new inputs, after the ones it started with.
    pub(crate) fn apply(&mut self, events: &[ConsoleEvent]) {
        self.playing.events.extend_from_slice(events);
        for event in &self.playing.events[self.applied..] {
            self.playback.apply(*event);
        }
        self.applied = self.playing.events.len();
    }

    /// The controller frames at the clock's current place: the transport's
    /// `audio`, or the session's.
    pub(crate) fn render(
        &mut self,
        audio: &AudioTransportSnapshot,
    ) -> Result<OutputTick<'_>, PlayingRenderError> {
        let frame_rate = self.playback.prepared().frame_rate();
        let (generation, time, frame, interval, moving) = match self.playing.clock {
            PlayClock::Audio => {
                if matches!(
                    audio.state,
                    AudioTransportState::Unloaded | AudioTransportState::Error
                ) {
                    return Err(PlayingRenderError::ClockUnavailable {
                        state: audio.state.clone(),
                    });
                }
                let rate = PlaybackRate::try_from(audio.playback_speed)
                    .map_err(PlayingRenderError::InvalidPlaybackSpeed)?;
                let time = sample_time_from_seconds_f64(audio.position_seconds)
                    .map_err(PlayingRenderError::InvalidClock)?;
                let interval = Duration::from_micros(u64::from(
                    rate.wall_elapsed(rate.frame_start(1, frame_rate)),
                ));
                (
                    audio.generation,
                    time,
                    rate.frame_at(time.as_ticks(), frame_rate),
                    interval,
                    matches!(audio.state, AudioTransportState::Playing),
                )
            }
            PlayClock::Session(clock) => {
                let time = clock.now();
                let frame = u64::from(time.as_ticks()) * u64::from(frame_rate) / 1_000_000;
                (
                    self.playing.generation,
                    time,
                    frame as u32,
                    Duration::from_secs(1) / frame_rate,
                    true,
                )
            }
        };
        let key = (generation, self.applied, frame);
        let rendered = match self.rendered.take() {
            Some(rendered) if rendered.key == key => rendered,
            _ => RenderedFrame {
                key,
                controller_frames: controller_frames(
                    &self.playback.evaluate(time),
                    &self.playing.controllers,
                ),
            },
        };
        Ok(OutputTick {
            frames: &self.rendered.insert(rendered).controller_frames,
            interval,
            moving,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum PlayingRenderError {
    ClockUnavailable { state: AudioTransportState },
    InvalidClock(SampleTimeError),
    InvalidPlaybackSpeed(String),
}

impl std::fmt::Display for PlayingRenderError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClockUnavailable {
                state: AudioTransportState::Unloaded,
            } => formatter.write_str("the sequence audio is not loaded"),
            Self::ClockUnavailable { .. } => formatter.write_str("the sequence audio failed"),
            Self::InvalidClock(error) => {
                write!(formatter, "the playback position is invalid: {error}")
            }
            Self::InvalidPlaybackSpeed(message) => formatter.write_str(message),
        }
    }
}

/// A frame's controller output: each output's controller index names a
/// controller of the project it was prepared on, in the project's order. Only
/// E1.31 and Art-Net controllers stream, each as one output of all its
/// universes; Donder controllers' outputs have no transport and are dropped
/// there.
fn controller_frames(
    frame: &donder_runtime::SequenceFrame<'_>,
    controllers: &[(ControllerId, Controller)],
) -> Vec<OutputFrame> {
    frame
        .outputs()
        .map(|output| OutputFrame {
            controller: controllers[output.output.controller_index as usize].0,
            slots: output.bytes.to_vec(),
        })
        .collect()
}

/// The project's controllers, in the order prepared output indexes them.
pub(crate) fn project_controllers(project: &DonderProject) -> Vec<(ControllerId, Controller)> {
    project
        .objects::<Controller>()
        .map(|(id, controller)| (id, controller.clone()))
        .collect()
}
