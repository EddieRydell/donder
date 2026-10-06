//! Execution capabilities for samples and signal operators.
use super::RunContext;
use crate::values::sample_duration_seconds_f32;

/// Query and sequence time in seconds, converted once per invocation. On ESP32
/// each conversion is a software division; keeping it out of the interpreter
/// also prevents it from being hoisted into every pixel.
#[derive(Clone, Copy)]
pub(super) struct Clock {
    pub(super) seconds: f32,
    pub(super) duration: f32,
}

impl Clock {
    pub(super) fn new(context: &RunContext) -> Self {
        Self {
            seconds: sample_duration_seconds_f32(context.time),
            duration: sample_duration_seconds_f32(context.duration),
        }
    }
}

/// Effects never query signals; sample admission rejects every signal
/// instruction, including trusted-archive conversion.
pub(crate) struct NoSignals;
