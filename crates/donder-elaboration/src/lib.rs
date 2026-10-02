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

mod output;
mod selection;
mod sequence;

use donder_language::controller::{ControllerId, ControllerPortId};
use donder_language::model::DonderProject;
use donder_language::sequence::SequenceId;
pub use donder_runtime::sequence::PreparedSequence;

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
    let selected = selection::resolve(project, sequence, outputs)?;
    let mut signals =
        sequence::elaboration::prepare_sequence(project, selected.layout, selected.sequence);
    let mut patch =
        output::patch::prepare_patch(selected.layout, selected.patch, &signals, &selected.ports);
    if !matches!(outputs, PrepareOutputs::All) {
        output::fragment::compact(&mut signals, &mut patch);
    }
    Some(PreparedSequence::new(
        signals,
        patch,
        selected
            .ports
            .iter()
            .map(|port| donder_runtime::sequence::PreparedOutput {
                controller_index: port.controller_index,
                port: port.port.id.0,
                width: u32::from(port.port.slot_count),
            })
            .collect(),
    ))
}

#[cfg(test)]
mod tests;
