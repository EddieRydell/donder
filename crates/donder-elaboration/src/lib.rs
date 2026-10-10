//! Turn one selected sequence from a loaded project into portable playback data.
//!
//! The active setup belongs to the project. Selection is the only optional part:
//! the preparation implementation is private, and playback belongs to runtime.
#![deny(unsafe_code)]
#![deny(unreachable_pub)]
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

mod selection;
mod sequence;

use donder_model::DonderProject;
use donder_model::SequenceId;
use donder_model::{ControllerId, ControllerPortId};
pub use donder_runtime::PreparedSequence;
pub use sequence::PreparationCache;

/// Which physical outputs to retain. Explicit lists preserve first-occurrence
/// order and treat repeated entries as a set. An empty list selects no outputs.
#[derive(Clone, Copy, Debug)]
pub enum PrepareOutputs<'a> {
    All,
    Controllers(&'a [ControllerId]),
    Ports(&'a [(ControllerId, ControllerPortId)]),
}

/// Prepare a sequence against the project's active setup.
///
/// `None` means the requested sequence or output selection cannot be resolved.
/// Internal preparation failures must never be translated into a missing selection.
/// The project is the loaded/accepted authoring model; preparation does not repeat
/// source validation or accept a separately supplied setup or sequence object.
pub fn prepare(
    project: &DonderProject,
    sequence: &SequenceId,
    outputs: PrepareOutputs<'_>,
) -> Option<PreparedSequence> {
    prepare_cached(project, sequence, outputs, &mut PreparationCache::default())
}

/// [`prepare`], reusing the clip programs `cache` holds from earlier
/// preparations. A caller that prepares repeatedly, as an editor does after
/// each edit, keeps one cache so unchanged clips are not lowered again.
pub fn prepare_cached(
    project: &DonderProject,
    sequence: &SequenceId,
    outputs: PrepareOutputs<'_>,
    cache: &mut PreparationCache,
) -> Option<PreparedSequence> {
    let selected = selection::resolve(project, sequence, outputs)?;
    Some(sequence::prepare(
        selected,
        !matches!(outputs, PrepareOutputs::All),
        cache,
    ))
}

#[cfg(test)]
mod tests;
