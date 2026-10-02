// Host elaboration lowers compiled Donder definitions into one immutable
// PreparedSignalGraph. Evaluation consumes only that sequence, a SampleTime, and
// reusable workspace; it does not depend on authored project state.
pub(crate) mod composition;
pub(crate) mod effects;
pub(crate) mod elaboration;
pub(crate) mod fixtures;
pub(crate) mod targets;
pub(crate) mod timeline;
