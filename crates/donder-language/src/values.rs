pub use donder_runtime::values::*;

use core::time::Duration;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SecondsError {
    NotFinite,
    Negative,
}

pub const NANOS_PER_SECOND: u64 = 1_000_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DonderTime(pub Duration);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DonderDuration(pub Duration);

impl DonderTime {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f32(seconds)))
    }

    pub fn from_seconds_f32(seconds: f32) -> Self {
        Self(Duration::from_secs_f32(seconds))
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

impl DonderDuration {
    pub fn try_from_seconds_f32(seconds: f32) -> Result<Self, SecondsError> {
        if !seconds.is_finite() {
            return Err(SecondsError::NotFinite);
        }
        if seconds < 0.0 {
            return Err(SecondsError::Negative);
        }
        Ok(Self(Duration::from_secs_f32(seconds)))
    }

    pub fn from_seconds_f32(seconds: f32) -> Self {
        Self(Duration::from_secs_f32(seconds))
    }

    pub const fn from_nanos(nanos: u64) -> Self {
        Self(Duration::from_nanos(nanos))
    }

    pub const fn from_micros(micros: u64) -> Self {
        Self(Duration::from_micros(micros))
    }

    pub fn as_nanos(&self) -> u128 {
        self.0.as_nanos()
    }

    pub fn as_micros_rounded(&self) -> u128 {
        (self.as_nanos() + 500) / 1_000
    }

    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }

    pub fn as_seconds_f32(&self) -> f32 {
        self.0.as_secs_f32()
    }
}

pub fn sample_time_from_donder_time(time: &DonderTime) -> Result<SampleTime, SampleTimeError> {
    Ok(SampleTime::from_ticks(
        u32::try_from(time.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}

pub fn sample_duration_from_donder_duration(
    duration: &DonderDuration,
) -> Result<SampleDuration, SampleTimeError> {
    Ok(SampleDuration::from_ticks(
        u32::try_from(duration.as_micros_rounded()).map_err(|_| SampleTimeError::OutOfRange)?,
    ))
}
