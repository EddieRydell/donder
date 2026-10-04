use donder_project_io::SourceObjectKind;
use serde::{Deserialize, Serialize};
use specta::Type;

pub use donder_sequence_api::*;

mod app;
mod audio;
mod diagnostics;
mod output;
mod patch;
mod preview;
mod sequence;
mod setup;
mod synchronization;
mod workspace;

pub use app::*;
pub use audio::*;
pub use diagnostics::*;
pub use output::*;
pub use patch::*;
pub use preview::*;
pub use sequence::*;
pub use setup::*;
pub use synchronization::*;
pub use workspace::*;
