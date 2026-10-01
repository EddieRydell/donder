use std::time::{Duration, Instant};

use donder_runtime::sequence::PreparedSequence;
use donder_runtime::signal::EvaluationWorkspace;
use donder_runtime::values::sample_time_from_seconds_f32;
use donder_runtime::wire::{LoadError, LoadLimits, decode_sequence};

use crate::PreviewColor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewPlaybackState {
    Unavailable,
    Playing,
    Paused,
    Stopped,
    Ended,
}

#[derive(Clone, Copy, Debug)]
pub struct PreviewClockSnapshot {
    pub generation: u32,
    pub state: PreviewPlaybackState,
    pub position_seconds: f32,
    pub start_delay_seconds: f32,
}

#[derive(Clone, Copy, Debug)]
struct ClockAnchor {
    snapshot: PreviewClockSnapshot,
    received_at: Instant,
}

impl ClockAnchor {
    fn position_at(self, now: Instant) -> f32 {
        if self.snapshot.state == PreviewPlaybackState::Playing {
            self.snapshot.position_seconds
                + (now
                    .saturating_duration_since(self.received_at)
                    .as_secs_f32()
                    - self.snapshot.start_delay_seconds)
                    .max(0.0)
        } else {
            self.snapshot.position_seconds
        }
    }
}

pub struct PreviewPlayback {
    sequence: Option<PreparedSequence>,
    workspace: Option<EvaluationWorkspace>,
    colors: Vec<PreviewColor>,
    unlit: PreviewColor,
    clock: ClockAnchor,
    last_frame: Option<(u32, u32)>,
}

impl PreviewPlayback {
    pub fn new(unlit: PreviewColor) -> Self {
        Self {
            sequence: None,
            workspace: None,
            colors: Vec::new(),
            unlit,
            clock: ClockAnchor {
                snapshot: PreviewClockSnapshot {
                    generation: 0,
                    state: PreviewPlaybackState::Unavailable,
                    position_seconds: 0.0,
                    start_delay_seconds: 0.0,
                },
                received_at: Instant::now(),
            },
            last_frame: None,
        }
    }

    pub fn set_unlit(&mut self, unlit: PreviewColor) {
        if self.unlit == unlit {
            return;
        }
        self.unlit = unlit;
        if self.sequence.is_none() {
            self.colors.fill(unlit);
        }
    }

    pub fn replace_content(
        &mut self,
        sequence_bytes: Option<&[u8]>,
        instance_count: usize,
    ) -> Result<(), PreviewPlaybackError> {
        let sequence = sequence_bytes
            .map(|bytes| decode_sequence(bytes, preview_load_limits()))
            .transpose()
            .map_err(PreviewPlaybackError::Decode)?;
        if let Some(sequence) = sequence.as_ref()
            && sequence.signals.pixel_count() != instance_count
        {
            return Err(PreviewPlaybackError::PixelCount {
                sequence: sequence.signals.pixel_count(),
                scene: instance_count,
            });
        }
        self.workspace = sequence
            .as_ref()
            .map(|sequence| sequence.signals.workspace());
        self.sequence = sequence;
        self.colors.clear();
        self.colors.resize(instance_count, self.unlit);
        self.last_frame = None;
        Ok(())
    }

    pub fn set_clock(&mut self, snapshot: PreviewClockSnapshot, received_at: Instant) {
        self.clock = ClockAnchor {
            snapshot,
            received_at,
        };
        self.last_frame = None;
    }

    pub fn evaluate(&mut self, now: Instant) -> Result<bool, PreviewPlaybackError> {
        let Some(sequence) = self.sequence.as_ref() else {
            return Ok(self.last_frame.take().is_some());
        };
        if self.clock.snapshot.state == PreviewPlaybackState::Unavailable {
            let changed = self.colors.iter().any(|color| *color != self.unlit);
            self.colors.fill(self.unlit);
            self.last_frame = None;
            return Ok(changed);
        }
        let position = self.clock.position_at(now).max(0.0);
        let frame_rate = sequence.signals.frame_rate();
        let frame_count = sequence.signals.frame_count();
        let frame = frame_at_position(position, frame_rate, frame_count);
        let key = (self.clock.snapshot.generation, frame);
        if self.last_frame == Some(key) {
            return Ok(false);
        }
        let sample_time = sample_time_from_seconds_f32(position)
            .map_err(|_| PreviewPlaybackError::ClockPosition)?;
        let workspace = self
            .workspace
            .as_mut()
            .ok_or(PreviewPlaybackError::Workspace)?;
        let evaluated = sequence
            .signals
            .evaluate(sample_time, workspace)
            .map_err(|_| PreviewPlaybackError::Evaluation)?;
        for (target, color) in self.colors.iter_mut().zip(evaluated) {
            *target = PreviewColor::opaque([color.red, color.green, color.blue]);
        }
        self.last_frame = Some(key);
        Ok(true)
    }

    pub fn colors(&self) -> &[PreviewColor] {
        &self.colors
    }

    pub fn next_deadline(&self, now: Instant) -> Option<Instant> {
        let sequence = self.sequence.as_ref()?;
        if self.clock.snapshot.state != PreviewPlaybackState::Playing {
            return None;
        }
        let frame_rate = sequence.signals.frame_rate();
        if frame_rate == 0 {
            return None;
        }
        let position = self.clock.position_at(now).max(0.0);
        let next_position = (position * frame_rate as f32)
            .floor()
            .mul_add(1.0 / frame_rate as f32, 1.0 / frame_rate as f32);
        let delay = Duration::from_secs_f32((next_position - position).max(0.001));
        Some(now + delay)
    }
}

fn frame_at_position(position: f32, frame_rate: u32, frame_count: u32) -> u32 {
    let frame = (position * frame_rate as f32).floor();
    if !frame.is_finite() || frame <= 0.0 {
        0
    } else {
        (frame as u32).min(frame_count.saturating_sub(1))
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
pub enum PreviewPlaybackError {
    Decode(LoadError),
    PixelCount { sequence: usize, scene: usize },
    ClockPosition,
    Workspace,
    Evaluation,
}
