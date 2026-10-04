use donder_project_io::SourceObjectKind;
use serde::{Deserialize, Serialize};
use specta::Type;

pub use crate::*;

mod app;
mod audio;
mod browser;
mod diagnostics;
mod output;
mod patch;
mod preview;
mod sequence;
mod setup;
mod synchronization;
mod view_state;
mod workspace;

pub use app::*;
pub use audio::*;
pub use browser::*;
pub use diagnostics::*;
pub use output::*;
pub use patch::*;
pub use preview::*;
pub use sequence::*;
pub use setup::*;
pub use synchronization::*;
pub use view_state::*;
pub use workspace::*;
