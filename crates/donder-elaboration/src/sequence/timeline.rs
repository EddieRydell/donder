use crate::RenderError;
use donder_language::sequence::Sequence;
use donder_language::values::{
    SampleDuration, SampleTime, sample_duration_from_donder_duration, sample_time_from_frame,
};

fn invalid_timing(reason: impl Into<String>) -> RenderError {
    RenderError::InvalidTiming {
        reason: reason.into(),
    }
}

pub(crate) struct PreparedTiming {
    pub(crate) duration: SampleDuration,
    pub(crate) frame_count: u32,
}

/// Convert validated authoring timing into the portable runtime representation.
pub(crate) fn prepare_timing(sequence: &Sequence) -> Result<PreparedTiming, RenderError> {
    let duration = sample_duration_from_donder_duration(&sequence.duration)
        .map_err(|_| invalid_timing("sequence duration exceeds the runtime clock range"))?;
    let frame_count = u32::try_from(sequence.frame_count())
        .map_err(|_| invalid_timing("frame count exceeds the runtime range"))?;
    Ok(PreparedTiming {
        duration,
        frame_count,
    })
}

pub(crate) fn sample_time_for_frame(
    frame_index: u32,
    frame_rate: u32,
) -> Result<SampleTime, RenderError> {
    sample_time_from_frame(frame_index, frame_rate)
        .map_err(|_| invalid_timing("frame time exceeds the runtime clock range"))
}

pub(crate) fn first_frame_at_or_after(time: SampleTime, frame_rate: u32) -> u32 {
    let whole = time.as_ticks() / donder_language::values::MICROS_PER_SECOND;
    let partial = time.as_ticks() % donder_language::values::MICROS_PER_SECOND;
    whole * frame_rate + (partial * frame_rate).div_ceil(donder_language::values::MICROS_PER_SECOND)
}

pub(crate) fn frame_at_or_before(time: SampleTime, frame_rate: u32) -> u32 {
    let whole = time.as_ticks() / donder_language::values::MICROS_PER_SECOND;
    let partial = time.as_ticks() % donder_language::values::MICROS_PER_SECOND;
    whole * frame_rate + partial * frame_rate / donder_language::values::MICROS_PER_SECOND
}
