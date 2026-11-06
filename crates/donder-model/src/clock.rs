//! Validated times on the playback clock.
use donder_language::{
    DonderDuration, DonderTime, sample_duration_from_donder_duration, sample_time_from_donder_time,
};
use donder_runtime_types::{SampleDuration, SampleTime};

/// A time of a validated project on the playback clock; validation proved it fits.
pub fn sample_time(time: &DonderTime) -> SampleTime {
    sample_time_from_donder_time(time)
        .unwrap_or_else(|_| unreachable!("validation proves a time fits the playback clock"))
}

/// A duration of a validated project on the playback clock; validation proved it fits.
pub fn sample_duration(duration: &DonderDuration) -> SampleDuration {
    sample_duration_from_donder_duration(duration)
        .unwrap_or_else(|_| unreachable!("validation proves a duration fits the playback clock"))
}
