// Host elaboration lowers compiled Dawn definitions into one immutable
// PreparedSignalGraph. Evaluation consumes only that sequence, a SampleTime, and
// reusable workspace; it does not depend on authored project state.
pub(crate) mod color;
pub(crate) mod composition;
pub(crate) mod effects;
pub(crate) mod elaboration;
pub(crate) mod fixtures;
pub(crate) mod raster;
pub(crate) mod renderer;
pub(crate) mod targets;
pub(crate) mod timeline;

pub use elaboration::elaborate_sequence;
pub use targets::resolve_effect_target_pixel_addresses;
