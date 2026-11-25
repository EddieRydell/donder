//! Turn one selected sequence or console from a loaded project into portable playback data.
//!
//! Selection is the only optional part:
//! the preparation implementation is private, and playback belongs to runtime.
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

mod console;
mod derive;
mod selection;
mod sequence;
mod stage;

pub use console::{ArmedConsole, arm};
pub use derive::derive;
use donder_model::DonderProject;
use donder_model::SequenceId;
use donder_model::{ControllerId, ControllerOutput};
pub use donder_runtime::PreparedShow;
pub use sequence::PreparationCache;

/// Which hardware outputs a show keeps. Explicit lists preserve first-occurrence
/// order and treat repeated entries as a set. An empty list selects no outputs.
/// Each output is one record of the prepared show: an E1.31 or Art-Net
/// controller's channels across all its universes, or one Donder port.
#[derive(Clone, Copy, Debug)]
pub enum Retain<'a> {
    All,
    Controllers(&'a [ControllerId]),
    Outputs(&'a [(ControllerId, ControllerOutput)]),
}

/// Prepare a sequence against the project.
///
/// `None` means the requested sequence or output selection cannot be resolved.
/// Internal preparation failures must never be translated into a missing selection.
/// The project is the loaded/accepted authoring model; preparation does not repeat
/// source validation or accept a separately supplied sequence object.
pub fn prepare(
    project: &DonderProject,
    sequence: &SequenceId,
    retain: Retain<'_>,
) -> Option<PreparedShow> {
    prepare_cached(project, sequence, retain, &mut PreparationCache::default())
}

/// [`prepare`], reusing the clip programs `cache` holds from earlier
/// preparations. A caller that prepares repeatedly, as an editor does after
/// each edit, keeps one cache so unchanged clips are not lowered again.
pub fn prepare_cached(
    project: &DonderProject,
    sequence: &SequenceId,
    retain: Retain<'_>,
    cache: &mut PreparationCache,
) -> Option<PreparedShow> {
    let selected = selection::resolve(project, donder_model::ShowId::Sequence(*sequence), retain)?;
    Some(sequence::prepare(
        selected,
        !matches!(retain, Retain::All),
        cache,
    ))
}

#[cfg(test)]
mod tests;
