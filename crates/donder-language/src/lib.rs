#![deny(unsafe_code)]
#![cfg_attr(
    not(test),
    deny(
        clippy::expect_used,
        clippy::panic,
        clippy::todo,
        clippy::unimplemented,
        clippy::unwrap_used
    )
)]

pub mod analysis;
pub mod compiler;
pub mod data;
mod imports;
mod names;
pub use imports::{
    ImportAlias, ImportDeclaration, ImportSource, SourceReference, is_valid_import_alias,
};
pub use names::{is_object_name, name_from_text, object_name, unique_name};
mod quantities;
pub use quantities::{
    Distance, DistanceSpan, DonderDuration, DonderTime, NANOS_PER_SECOND, Point3, Rotation3,
    Scale3, SecondsError, sample_duration_from_donder_duration, sample_time_from_donder_time,
};
