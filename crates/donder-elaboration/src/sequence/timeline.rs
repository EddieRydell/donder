use donder_language::sequence::Sequence;
use donder_language::values::SampleDuration;

pub(crate) struct PreparedTiming {
    pub(crate) duration: SampleDuration,
    pub(crate) frame_count: u32,
}

/// Convert validated authoring timing into the portable runtime representation.
pub(crate) fn prepare_timing(sequence: &Sequence) -> PreparedTiming {
    // validate_sequence establishes both clock representability and frame count
    // before the project reaches elaboration. Keep the same rounded time here.
    PreparedTiming {
        duration: SampleDuration::from_ticks(sequence.duration.as_micros_rounded() as u32),
        frame_count: sequence.frame_count() as u32,
    }
}
