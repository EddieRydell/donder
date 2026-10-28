use std::collections::BTreeSet;

use camino::Utf8Path;
use donder_language::{DonderDuration, DonderTime};
use donder_model::Clip;
use donder_model::SequenceId;
use donder_project_io::SourceIdentity;
use donder_project_io::{DocumentId, ProjectSession};

use crate::dto::{
    GuiDocument, GuiDocumentChange, GuiDocumentRequest, GuiEditCommand, GuiObjectRef, IoDiagnostic,
    SequenceClipDetails, SequenceSelection,
};

mod arrange;
mod clip_match;
mod controller;
mod description;
mod dispatch;
mod document;
mod edit;
mod fixture;
mod graph;
mod input_device;
mod item;
mod layout;
mod library;
pub(crate) mod model;
mod ownership;
mod patch;
mod project;
mod projection;
mod rig;
mod selection;
mod setup;

use arrange::arrange_clip_selection;
pub use clip_match::matching_sequence_clips;
use edit::edit_sequence;
use fixture::edit_fixture;
use layout::edit_layout;
pub use project::create_sequence;
use project::project_root;
use projection::{project_fixture, project_layout, project_sequence};
use selection::copy_sequence_selection;
use selection::{
    delete_sequence_selection, edit_clip_selection, move_clip_selection, move_mark_selection,
    paste_sequence_clipboard, resize_clip_selection,
};
use setup::{edit_setup, project_setup};

pub use dispatch::{
    ClipboardAutomation, ClipboardClip, ClipboardMark, GuiEditOutcome, SequenceClipboard,
    apply_edit, copy_selection,
};
pub use document::{
    GuiMutationError, blocked, project_gui_document, project_gui_document_change,
    sequence_clip_details,
};
pub use document::{ResolvedGuiObject, affected_paths, gui_editor_order, resolve_request};

fn checked_gui_time(seconds: f64) -> Result<DonderTime, GuiMutationError> {
    DonderTime::try_from_seconds_f64(seconds)
        .map_err(|_| GuiMutationError::Invalid("Time is outside the supported range.".into()))
}

fn checked_gui_duration(seconds: f64) -> Result<DonderDuration, GuiMutationError> {
    DonderDuration::try_from_seconds_f64(seconds)
        .map_err(|_| GuiMutationError::Invalid("Duration is outside the supported range.".into()))
}

#[cfg(test)]
mod time_conversion_tests {
    use super::{checked_gui_duration, checked_gui_time};

    #[test]
    fn finite_but_unrepresentable_gui_times_are_rejected_without_panicking() {
        assert!(checked_gui_time(f64::MAX).is_err());
        assert!(checked_gui_duration(f64::MAX).is_err());
        assert!(checked_gui_time(1.0).is_ok());
        assert!(checked_gui_duration(1.0).is_ok());
    }
}
