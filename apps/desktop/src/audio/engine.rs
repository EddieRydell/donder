use donder_editor::dto::{
    AudioTransportSnapshot, AudioTransportState, PlaybackRange, SequenceAudio,
};
use donder_runtime::PlaybackRate;
use std::time::{Duration, Instant};

use super::backend::{
    AudioDriver, AudioHandle, BackendPlaybackState, KiraAudioDriver, LoadedAudio, PlaybackBounds,
    canonical_audio_path,
};

/// Where the transport is, for Donder controllers that follow it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Timeline {
    /// Playing from `position` at `at`.
    Playing {
        at: Instant,
        position: f64,
        rate: PlaybackRate,
        end: Option<WindowEnd>,
    },
    Paused(f64),
    Stopped(f64),
    /// Played to the end of the sequence, where followers end by themselves.
    Ended,
}

/// Where playback wraps to the loop start, or stops and returns Home, before
/// the end of the sequence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum WindowEnd {
    Wrap { start: f64, end: f64 },
    Stop { end: f64, home: f64 },
}

impl Timeline {
    /// The position playing reaches at `now`, wrapped into the loop.
    pub(crate) fn position_at(&self, now: Instant) -> f64 {
        match *self {
            Self::Playing {
                at,
                position,
                rate,
                end,
            } => {
                let reached = position + show_seconds_between(rate, at, now);
                match end {
                    Some(WindowEnd::Wrap { start, end }) if reached >= end => {
                        start + (reached - start) % (end - start)
                    }
                    _ => reached,
                }
            }
            Self::Paused(position) | Self::Stopped(position) => position,
            Self::Ended => 0.0,
        }
    }

    /// When playing next reaches its window end after `now`.
    pub(crate) fn next_end(&self, now: Instant) -> Option<(Instant, WindowEnd)> {
        let Self::Playing { rate, end, .. } = *self else {
            return None;
        };
        let end = end?;
        let edge = match end {
            WindowEnd::Wrap { end, .. } | WindowEnd::Stop { end, .. } => end,
        };
        let remaining = micros(Duration::from_secs_f64(
            (edge - self.position_at(now)).max(0.0),
        ));
        Some((
            now + Duration::from_micros(u64::from(rate.wall_elapsed(remaining))),
            end,
        ))
    }
}

enum TransportClock {
    Audio(LoadedAudio),
    Silent { duration_seconds: f64 },
}

pub(crate) struct AudioEngine {
    driver: Option<Box<dyn AudioDriver>>,
    handle: Option<Box<dyn AudioHandle>>,
    clock: Option<TransportClock>,
    home_seconds: f64,
    position_seconds: f64,
    state: AudioTransportState,
    generation: u32,
    last_error: Option<String>,
    can_resume_handle: bool,
    /// When and where playing last started at the current rate.
    anchor: Option<(Instant, f64)>,
    rate: PlaybackRate,
    range: Option<PlaybackRange>,
    looping: bool,
}

impl AudioEngine {
    pub(crate) fn new() -> Self {
        match KiraAudioDriver::new() {
            Ok(driver) => Self::with_optional_driver(Some(Box::new(driver)), None),
            Err(error) => Self::with_optional_driver(None, Some(error)),
        }
    }

    fn with_optional_driver(driver: Option<Box<dyn AudioDriver>>, error: Option<String>) -> Self {
        Self {
            driver,
            handle: None,
            clock: None,
            home_seconds: 0.0,
            position_seconds: 0.0,
            state: if error.is_some() {
                AudioTransportState::Error
            } else {
                AudioTransportState::Unloaded
            },
            generation: 0,
            last_error: error,
            can_resume_handle: false,
            anchor: None,
            rate: PlaybackRate::NORMAL,
            range: None,
            looping: false,
        }
    }

    pub(crate) fn empty_snapshot() -> AudioTransportSnapshot {
        AudioTransportSnapshot {
            state: AudioTransportState::Unloaded,
            audio: None,
            generation: 0,
            position_seconds: 0.0,
            home_seconds: 0.0,
            duration_seconds: 0.0,
            playback_speed: PlaybackRate::NORMAL.into(),
            range: None,
            looping: false,
            last_error: None,
        }
    }

    pub(crate) fn snapshot(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        self.current_snapshot()
    }

    pub(crate) fn load_silent_sequence(&mut self, duration_seconds: f64) -> AudioTransportSnapshot {
        self.observe_backend();
        if let Some(TransportClock::Silent {
            duration_seconds: current,
            ..
        }) = &self.clock
            && *current == duration_seconds
        {
            return self.current_snapshot();
        }
        self.reset_loaded_audio();
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            self.state = AudioTransportState::Error;
            self.last_error = Some("Sequence duration must be positive and finite.".into());
        } else {
            self.clock = Some(TransportClock::Silent { duration_seconds });
            self.state = AudioTransportState::Stopped;
            self.last_error = None;
        }
        self.bump_generation();
        self.current_snapshot()
    }

    pub(crate) fn load(&mut self, audio: Option<SequenceAudio>) -> AudioTransportSnapshot {
        self.observe_backend();
        let Some(audio) = audio else {
            return self.unload();
        };
        if !audio.exists {
            self.reset_loaded_audio();
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio file does not exist.".to_string());
            self.bump_generation();
            return self.current_snapshot();
        }
        let canonical_path = match canonical_audio_path(&audio.resolved_path) {
            Ok(canonical_path) => canonical_path,
            Err(error) => {
                self.reset_loaded_audio();
                self.state = AudioTransportState::Error;
                self.last_error = Some(error.to_string());
                self.bump_generation();
                return self.current_snapshot();
            }
        };
        if self
            .clock
            .as_ref()
            .is_some_and(|clock| matches!(clock, TransportClock::Audio(loaded) if loaded.canonical_path == canonical_path))
        {
            return self.current_snapshot();
        }
        let Some(driver) = self.driver.as_mut() else {
            self.reset_loaded_audio();
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio manager is not available.".to_string());
            self.bump_generation();
            return self.current_snapshot();
        };
        match driver.load_metadata(&audio.resolved_path) {
            Ok(metadata) => {
                self.reset_loaded_audio();
                self.clock = Some(TransportClock::Audio(LoadedAudio {
                    audio,
                    canonical_path,
                    duration_seconds: metadata.duration_seconds,
                }));
                self.home_seconds = 0.0;
                self.position_seconds = 0.0;
                self.state = AudioTransportState::Stopped;
                self.last_error = None;
                self.can_resume_handle = false;
                self.bump_generation();
            }
            Err(error) => {
                self.reset_loaded_audio();
                self.state = AudioTransportState::Error;
                self.last_error = Some(error);
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
        self.current_snapshot()
    }

    pub(crate) fn unload(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.clock.is_none()
            && self.handle.is_none()
            && matches!(self.state, AudioTransportState::Unloaded)
            && self.last_error.is_none()
        {
            return self.current_snapshot();
        }
        self.reset_loaded_audio();
        self.state = AudioTransportState::Unloaded;
        self.last_error = None;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    pub(crate) fn play(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.clock.is_none() {
            return self.current_snapshot();
        }
        if matches!(self.state, AudioTransportState::Playing) {
            return self.current_snapshot();
        }
        let position = self.play_position();
        if position != self.position_seconds {
            self.position_seconds = position;
            self.can_resume_handle = false;
        }
        if let Some(TransportClock::Silent { .. }) = &self.clock {
            self.anchor = Some((Instant::now(), self.position_seconds));
            self.state = AudioTransportState::Playing;
            self.last_error = None;
            self.bump_generation();
            return self.current_snapshot();
        }
        if matches!(self.state, AudioTransportState::Paused)
            && self.can_resume_handle
            && let Some(handle) = self.handle.as_mut()
        {
            handle.resume();
            self.anchor = Some((Instant::now(), self.position_seconds));
            self.state = AudioTransportState::Playing;
            self.last_error = None;
            self.can_resume_handle = false;
            self.bump_generation();
            return self.current_snapshot();
        }
        self.lifecycle_stop_handle();
        self.start_stream_at_position();
        self.current_snapshot()
    }

    pub(crate) fn pause(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if !matches!(self.state, AudioTransportState::Playing) {
            return self.current_snapshot();
        }
        self.sample_handle_position();
        self.anchor = None;
        if let Some(handle) = self.handle.as_mut() {
            handle.pause();
        }
        self.state = AudioTransportState::Paused;
        self.can_resume_handle = true;
        self.bump_generation();
        self.current_snapshot()
    }

    pub(crate) fn stop(&mut self) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.clock.is_none() {
            return self.current_snapshot();
        }
        self.position_seconds = self.home_seconds;
        self.lifecycle_stop_handle();
        self.state = AudioTransportState::Stopped;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    pub(crate) fn rewind_to_zero(&mut self) -> AudioTransportSnapshot {
        self.seek_to(0.0, AudioTransportState::Paused)
    }

    pub(crate) fn seek(&mut self, position_seconds: f64) -> AudioTransportSnapshot {
        let position_seconds = self.clamp_position(position_seconds);
        self.seek_to(position_seconds, AudioTransportState::Paused)
    }

    pub(crate) fn playback_rate(&self) -> PlaybackRate {
        self.rate
    }

    /// Keeps the current position; playing sources continue from it at the new rate.
    pub(crate) fn set_playback_rate(&mut self, rate: PlaybackRate) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.rate == rate {
            return self.current_snapshot();
        }
        if let Some(timeline) = self.playing_timeline() {
            let now = Instant::now();
            self.anchor = Some((now, timeline.position_at(now)));
        }
        if let Some(handle) = self.handle.as_mut() {
            handle.set_playback_rate(rate);
        }
        self.rate = rate;
        self.bump_generation();
        self.current_snapshot()
    }

    /// Play starts from Home after the end, and at the range start from outside the range.
    pub(crate) fn play_position(&self) -> f64 {
        let position = if matches!(self.state, AudioTransportState::Ended) {
            self.home_seconds
        } else {
            self.position_seconds
        };
        match self.range {
            Some(range) if position < range.start_seconds || position >= range.end_seconds => {
                range.start_seconds
            }
            _ => position,
        }
    }

    /// A new range becomes Home; a stopped or paused playhead moves to its start.
    pub(crate) fn set_range(
        &mut self,
        range: Option<PlaybackRange>,
    ) -> Result<AudioTransportSnapshot, String> {
        self.observe_backend();
        if self.clock.is_none() {
            return Ok(self.current_snapshot());
        }
        let range = range.map(|range| self.clamped_range(range)).transpose()?;
        self.range = range;
        if let Some(range) = range {
            self.home_seconds = range.start_seconds;
            if !matches!(self.state, AudioTransportState::Playing) {
                self.position_seconds = range.start_seconds;
            }
        }
        self.restart_bounded_stream();
        self.bump_generation();
        Ok(self.current_snapshot())
    }

    pub(crate) fn set_looping(&mut self, looping: bool) -> AudioTransportSnapshot {
        self.observe_backend();
        self.looping = looping;
        self.restart_bounded_stream();
        self.bump_generation();
        self.current_snapshot()
    }

    /// Where the transport is, for controllers that follow it; nothing while
    /// no sequence is loaded.
    pub(crate) fn timeline(&mut self) -> Option<Timeline> {
        self.observe_backend();
        match self.state {
            AudioTransportState::Playing => self.playing_timeline(),
            AudioTransportState::Paused => Some(Timeline::Paused(self.position_seconds)),
            AudioTransportState::Stopped => Some(Timeline::Stopped(self.position_seconds)),
            AudioTransportState::Ended => Some(Timeline::Ended),
            AudioTransportState::Unloaded | AudioTransportState::Error => None,
        }
    }

    fn playing_timeline(&self) -> Option<Timeline> {
        let (at, position) = self.anchor?;
        Some(Timeline::Playing {
            at,
            position,
            rate: self.rate,
            end: self.window_end(),
        })
    }

    fn clamped_range(&self, range: PlaybackRange) -> Result<PlaybackRange, String> {
        if !range.start_seconds.is_finite() || !range.end_seconds.is_finite() {
            return Err("Playback range must be finite.".into());
        }
        let start_seconds = self.clamp_position(range.start_seconds);
        let end_seconds = self.clamp_position(range.end_seconds);
        if start_seconds >= end_seconds {
            return Err("Playback range must cover part of the sequence.".into());
        }
        Ok(PlaybackRange {
            start_seconds,
            end_seconds,
        })
    }

    /// Looping repeats the range, or the whole sequence without one.
    fn loop_region(&self) -> Option<(f64, f64)> {
        self.looping.then(|| {
            self.range.map_or((0.0, self.duration_seconds()), |range| {
                (range.start_seconds, range.end_seconds)
            })
        })
    }

    /// Where playback wraps or stops before the natural end of the sequence.
    fn window_end(&self) -> Option<WindowEnd> {
        match (self.loop_region(), self.range) {
            (Some((start, end)), _) => Some(WindowEnd::Wrap { start, end }),
            (None, Some(range)) => Some(WindowEnd::Stop {
                end: range.end_seconds,
                home: self.home_seconds,
            }),
            (None, None) => None,
        }
    }

    fn bounds(&self) -> PlaybackBounds {
        PlaybackBounds {
            end_seconds: self.range.map(|range| range.end_seconds),
            loop_region: self.loop_region(),
        }
    }

    /// Streams carry their bounds, so a local stream restarts under new ones.
    fn restart_bounded_stream(&mut self) {
        self.can_resume_handle = false;
        if !matches!(self.state, AudioTransportState::Playing) {
            return;
        }
        self.sample_handle_position();
        self.position_seconds = self.play_position();
        if let Some(TransportClock::Silent { .. }) = &self.clock {
            self.anchor = Some((Instant::now(), self.position_seconds));
            return;
        }
        self.lifecycle_stop_handle();
        self.start_stream_at_position();
    }

    fn seek_to(
        &mut self,
        position_seconds: f64,
        final_state: AudioTransportState,
    ) -> AudioTransportSnapshot {
        self.observe_backend();
        if self.clock.is_none() {
            return self.current_snapshot();
        }
        let position_seconds = self.clamp_position(position_seconds);
        self.home_seconds = position_seconds;
        self.position_seconds = position_seconds;
        self.anchor = None;
        if let Some(handle) = self.handle.as_mut() {
            handle.pause();
            handle.seek_to(position_seconds);
        }
        self.state = final_state;
        self.can_resume_handle = false;
        self.bump_generation();
        self.current_snapshot()
    }

    fn start_stream_at_position(&mut self) {
        let bounds = self.bounds();
        let Some(driver) = self.driver.as_mut() else {
            self.handle = None;
            self.state = AudioTransportState::Error;
            self.last_error = Some("Audio manager is not available.".to_string());
            self.bump_generation();
            return;
        };
        let Some(TransportClock::Audio(loaded)) = self.clock.as_ref() else {
            return;
        };
        match driver.play(
            &loaded.audio.resolved_path,
            self.position_seconds,
            self.rate,
            bounds,
        ) {
            Ok(handle) => {
                self.handle = Some(handle);
                self.anchor = Some((Instant::now(), self.position_seconds));
                self.state = AudioTransportState::Playing;
                self.last_error = None;
                self.can_resume_handle = false;
                self.bump_generation();
            }
            Err(error) => {
                self.handle = None;
                self.state = AudioTransportState::Error;
                self.last_error = Some(error);
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
    }

    fn observe_backend(&mut self) {
        if let Some(TransportClock::Silent { duration_seconds }) = &self.clock {
            if matches!(self.state, AudioTransportState::Playing)
                && let Some((started, position)) = self.anchor
            {
                let duration_seconds = *duration_seconds;
                let elapsed = position + show_seconds_between(self.rate, started, Instant::now());
                match (self.loop_region(), self.range) {
                    (Some((start, end)), _) if elapsed >= end => {
                        self.position_seconds = start + (elapsed - start) % (end - start);
                    }
                    (None, Some(range)) if elapsed >= range.end_seconds => {
                        self.position_seconds = self.home_seconds;
                        self.state = AudioTransportState::Stopped;
                        self.anchor = None;
                        self.bump_generation();
                    }
                    _ => {
                        self.position_seconds = elapsed.min(duration_seconds);
                        if self.position_seconds >= duration_seconds {
                            self.state = AudioTransportState::Ended;
                            self.anchor = None;
                            self.bump_generation();
                        }
                    }
                }
            }
            return;
        }
        let Some(handle) = self.handle.as_mut() else {
            return;
        };
        let observation = handle.observe();
        if let Some(error) = observation.error {
            self.state = AudioTransportState::Error;
            self.last_error = Some(error);
            self.can_resume_handle = false;
            self.bump_generation();
            return;
        }
        if !matches!(self.state, AudioTransportState::Playing) {
            return;
        }
        match observation.state {
            BackendPlaybackState::Advancing => {
                self.position_seconds = self.clamp_position(observation.position_seconds);
            }
            BackendPlaybackState::Paused => {}
            // A stream sliced to the range stops at its end and returns Home.
            BackendPlaybackState::Stopped if self.range.is_some() && !self.looping => {
                self.position_seconds = self.home_seconds;
                self.anchor = None;
                self.handle = None;
                self.state = AudioTransportState::Stopped;
                self.can_resume_handle = false;
                self.bump_generation();
            }
            BackendPlaybackState::Stopped => {
                self.position_seconds = self.duration_seconds();
                self.anchor = None;
                self.handle = None;
                self.state = AudioTransportState::Ended;
                self.can_resume_handle = false;
                self.bump_generation();
            }
        }
    }

    fn sample_handle_position(&mut self) {
        if let Some(handle) = self.handle.as_mut() {
            let position_seconds = handle.observe().position_seconds;
            self.position_seconds = self.clamp_position(position_seconds);
        }
    }

    fn reset_loaded_audio(&mut self) {
        self.lifecycle_stop_handle();
        self.clock = None;
        self.home_seconds = 0.0;
        self.position_seconds = 0.0;
        self.range = None;
        self.looping = false;
        self.can_resume_handle = false;
    }

    fn lifecycle_stop_handle(&mut self) {
        self.anchor = None;
        if let Some(mut handle) = self.handle.take() {
            handle.stop();
        }
    }

    fn current_snapshot(&self) -> AudioTransportSnapshot {
        AudioTransportSnapshot {
            state: self.state.clone(),
            audio: match &self.clock {
                Some(TransportClock::Audio(loaded)) => Some(loaded.audio.clone()),
                Some(TransportClock::Silent { .. }) | None => None,
            },
            generation: self.generation,
            position_seconds: self.position_seconds,
            home_seconds: self.home_seconds,
            duration_seconds: self.duration_seconds(),
            playback_speed: self.rate.into(),
            range: self.range,
            looping: self.looping,
            last_error: self.last_error.clone(),
        }
    }

    fn clamp_position(&self, position_seconds: f64) -> f64 {
        if !position_seconds.is_finite() {
            return 0.0;
        }
        position_seconds.clamp(0.0, self.duration_seconds())
    }

    fn duration_seconds(&self) -> f64 {
        match &self.clock {
            Some(TransportClock::Audio(loaded)) => loaded.duration_seconds,
            Some(TransportClock::Silent { duration_seconds }) => *duration_seconds,
            None => 0.0,
        }
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.saturating_add(1);
    }
}

/// A wall duration in the 32-bit microseconds the show clock uses, saturating.
fn micros(duration: Duration) -> u32 {
    u32::try_from(duration.as_micros()).unwrap_or(u32::MAX)
}

fn show_seconds_between(rate: PlaybackRate, start: Instant, now: Instant) -> f64 {
    rate.show_elapsed(micros(now.saturating_duration_since(start))) as f64 / 1_000_000.0
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.lifecycle_stop_handle();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::super::backend::{
        AudioDriver, AudioHandle, AudioMetadata, BackendObservation, BackendPlaybackState,
    };
    use super::*;

    #[test]
    fn load_valid_source_initializes_stopped_without_handle() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = fixture.engine();

        let snapshot = engine.load(Some(test_audio()));

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        assert_eq!(snapshot.home_seconds, 0.0);
        assert_eq!(snapshot.position_seconds, 0.0);
        assert_eq!(snapshot.duration_seconds, 12.0);
        assert_eq!(snapshot.generation, 1);
        assert!(snapshot.audio.is_some());
        fixture.assert_actions(&[DriverAction::LoadMetadata]);
    }

    #[test]
    fn pause_while_playing_samples_position_and_preserves_home() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_position(3.25);

        let snapshot = engine.pause();

        assert_eq!(snapshot.state, AudioTransportState::Paused);
        assert_eq!(snapshot.position_seconds, 3.25);
        assert_eq!(snapshot.home_seconds, 0.0);
        assert_eq!(snapshot.generation, 3);
        fixture.assert_handle_actions(&[HandleAction::Pause]);
    }

    #[test]
    fn play_after_pause_resumes_existing_handle_without_seek() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_position(3.25);
        engine.pause();
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 3.25);
        fixture.assert_actions(&[]);
        fixture.assert_handle_actions(&[HandleAction::Resume]);
    }

    #[test]
    fn stop_while_playing_releases_backend_stream_and_returns_home() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        fixture.clear_handle_actions();

        let snapshot = engine.stop();

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        assert_eq!(snapshot.position_seconds, 4.0);
        assert_eq!(snapshot.home_seconds, 4.0);
        fixture.assert_handle_actions(&[HandleAction::Stop]);
    }

    #[test]
    fn play_after_stop_recreates_stream_at_home_instead_of_seek_then_resume() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        engine.stop();
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 4.0);
        fixture.assert_actions(&[DriverAction::Play(4.0)]);
        fixture.assert_handle_actions(&[]);
    }

    #[test]
    fn seek_while_playing_pauses_sets_home_and_seeks_handle() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();

        let snapshot = engine.seek(5.5);

        assert_eq!(snapshot.state, AudioTransportState::Paused);
        assert_eq!(snapshot.position_seconds, 5.5);
        assert_eq!(snapshot.home_seconds, 5.5);
        fixture.assert_handle_actions(&[HandleAction::Pause, HandleAction::Seek(5.5)]);
    }

    #[test]
    fn play_after_seek_recreates_stream_at_seek_position_instead_of_resuming() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(5.5);
        fixture.clear_driver_actions();
        fixture.clear_handle_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 5.5);
        fixture.assert_actions(&[DriverAction::Play(5.5)]);
        fixture.assert_handle_actions(&[HandleAction::Stop]);
    }

    #[test]
    fn natural_end_while_playing_reports_ended_at_duration() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.set_handle_state(BackendPlaybackState::Stopped);

        let snapshot = engine.snapshot();

        assert_eq!(snapshot.state, AudioTransportState::Ended);
        assert_eq!(snapshot.position_seconds, 12.0);
        assert_eq!(snapshot.generation, 3);
    }

    #[test]
    fn play_from_ended_restarts_from_home() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        engine.seek(4.0);
        engine.play();
        fixture.set_handle_state(BackendPlaybackState::Stopped);
        let ended = engine.snapshot();
        assert_eq!(ended.state, AudioTransportState::Ended);
        fixture.clear_driver_actions();

        let snapshot = engine.play();

        assert_eq!(snapshot.state, AudioTransportState::Playing);
        assert_eq!(snapshot.position_seconds, 4.0);
        fixture.assert_actions(&[DriverAction::Play(4.0)]);
    }

    #[test]
    fn unload_and_source_replacement_call_lifecycle_stop() {
        let fixture = AudioFixture::new(12.0);
        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();

        let unloaded = engine.unload();

        assert_eq!(unloaded.state, AudioTransportState::Unloaded);
        fixture.assert_handle_actions(&[HandleAction::Stop]);

        let mut engine = playing_engine(&fixture);
        fixture.clear_handle_actions();
        let mut replacement = test_audio();
        replacement.resolved_path = std::env::current_dir()
            .expect("test working directory")
            .to_string_lossy()
            .into_owned();
        let snapshot = engine.load(Some(replacement));

        assert_eq!(snapshot.state, AudioTransportState::Stopped);
        fixture.assert_handle_actions(&[HandleAction::Stop]);
    }

    fn loaded_engine(fixture: &AudioFixture) -> AudioEngine {
        let mut engine = fixture.engine();
        engine.load(Some(test_audio()));
        engine
    }

    fn playing_engine(fixture: &AudioFixture) -> AudioEngine {
        let mut engine = loaded_engine(fixture);
        engine.play();
        engine
    }

    fn test_audio() -> SequenceAudio {
        SequenceAudio {
            import_path: "audio.wav".to_string(),
            resolved_path: std::env::current_exe()
                .expect("test executable path")
                .to_string_lossy()
                .into_owned(),
            file_name: "audio.wav".to_string(),
            exists: true,
        }
    }

    struct AudioFixture {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioFixture {
        fn new(duration_seconds: f64) -> Self {
            Self {
                shared: Arc::new(Mutex::new(FakeShared {
                    duration_seconds,
                    driver_actions: Vec::new(),
                    handle_actions: Vec::new(),
                    handle_state: BackendPlaybackState::Advancing,
                    handle_position: 0.0,
                })),
            }
        }

        fn engine(&self) -> AudioEngine {
            AudioEngine::with_optional_driver(
                Some(Box::new(FakeAudioDriver {
                    shared: Arc::clone(&self.shared),
                })),
                None,
            )
        }

        fn set_handle_state(&self, state: BackendPlaybackState) {
            self.shared.lock().expect("fake shared").handle_state = state;
        }

        fn set_handle_position(&self, position_seconds: f64) {
            self.shared.lock().expect("fake shared").handle_position = position_seconds;
        }

        fn clear_driver_actions(&self) {
            self.shared
                .lock()
                .expect("fake shared")
                .driver_actions
                .clear();
        }

        fn clear_handle_actions(&self) {
            self.shared
                .lock()
                .expect("fake shared")
                .handle_actions
                .clear();
        }

        fn assert_actions(&self, expected: &[DriverAction]) {
            assert_eq!(
                self.shared.lock().expect("fake shared").driver_actions,
                expected
            );
        }

        fn assert_handle_actions(&self, expected: &[HandleAction]) {
            assert_eq!(
                self.shared.lock().expect("fake shared").handle_actions,
                expected
            );
        }
    }

    struct FakeShared {
        duration_seconds: f64,
        driver_actions: Vec<DriverAction>,
        handle_actions: Vec<HandleAction>,
        handle_state: BackendPlaybackState,
        handle_position: f64,
    }

    #[derive(Debug, PartialEq)]
    enum DriverAction {
        LoadMetadata,
        Play(f64),
    }

    #[derive(Debug, PartialEq)]
    enum HandleAction {
        Pause,
        Resume,
        Seek(f64),
        Stop,
    }

    struct FakeAudioDriver {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioDriver for FakeAudioDriver {
        fn load_metadata(&mut self, _path: &str) -> Result<AudioMetadata, String> {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.driver_actions.push(DriverAction::LoadMetadata);
            Ok(AudioMetadata {
                duration_seconds: shared.duration_seconds,
            })
        }

        fn play(
            &mut self,
            _path: &str,
            position_seconds: f64,
            _rate: PlaybackRate,
            _bounds: PlaybackBounds,
        ) -> Result<Box<dyn AudioHandle>, String> {
            let mut shared = self.shared.lock().expect("fake shared");
            shared
                .driver_actions
                .push(DriverAction::Play(position_seconds));
            shared.handle_position = position_seconds;
            shared.handle_state = BackendPlaybackState::Advancing;
            Ok(Box::new(FakeAudioHandle {
                shared: Arc::clone(&self.shared),
            }))
        }
    }

    struct FakeAudioHandle {
        shared: Arc<Mutex<FakeShared>>,
    }

    impl AudioHandle for FakeAudioHandle {
        fn observe(&mut self) -> BackendObservation {
            let shared = self.shared.lock().expect("fake shared");
            BackendObservation {
                state: shared.handle_state,
                position_seconds: shared.handle_position,
                error: None,
            }
        }

        fn pause(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Pause);
            shared.handle_state = BackendPlaybackState::Paused;
        }

        fn resume(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Resume);
            shared.handle_state = BackendPlaybackState::Advancing;
        }

        fn seek_to(&mut self, position_seconds: f64) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared
                .handle_actions
                .push(HandleAction::Seek(position_seconds));
            shared.handle_position = position_seconds;
        }

        fn set_playback_rate(&mut self, _rate: PlaybackRate) {}

        fn stop(&mut self) {
            let mut shared = self.shared.lock().expect("fake shared");
            shared.handle_actions.push(HandleAction::Stop);
            shared.handle_state = BackendPlaybackState::Stopped;
        }
    }
}
