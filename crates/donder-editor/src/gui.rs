use std::collections::BTreeSet;

use camino::Utf8Path;
use donder_language::{DonderDuration, DonderTime};
use donder_model::EffectInst;
use donder_model::SequenceId;
use donder_model::SourceIdentity;
use donder_project_io::{ProjectSession, SourceObjectKind};

use crate::dto::{
    DiagnosticSeverity, DocumentViewId, GuiDocument, GuiDocumentRequest, GuiEditCommand,
    GuiObjectRef, ObjectKind, ProjectDiagnostic, SequenceSelection, SequenceSelectionEdit,
};

mod controller;
mod description;
mod dispatch;
mod document;
mod edit;
mod fixture;
mod graph;
mod layout;
mod library;
pub mod model;
mod ownership;
mod patch;
mod project;
mod projection;
mod selection;
mod setup;

use edit::edit_sequence;
use fixture::edit_fixture;
use layout::edit_layout;
pub use project::create_sequence;
use project::project_root;
use projection::{project_fixture, project_layout, project_sequence};
pub use selection::copy_sequence_selection;
use selection::{
    delete_sequence_selection, edit_effect_selection, move_clip_selection, move_mark_selection,
    paste_sequence_clipboard, resize_clip_selection,
};
use setup::{edit_setup, project_setup};

pub use dispatch::apply_edit;
pub use dispatch::{
    ClipboardAutomation, ClipboardEffect, ClipboardMark, SequenceClipboard,
    SequenceSelectionMutation, apply_sequence_selection_edit,
};
pub use document::{GuiMutationError, blocked, project_gui_document};
pub use document::{
    ResolvedGuiObject, affected_paths, ensure_owned_gui_document, gui_diagnostic, resolve_request,
};

fn checked_gui_time(seconds: f32) -> Result<DonderTime, GuiMutationError> {
    DonderTime::try_from_seconds_f32(seconds)
        .map_err(|_| GuiMutationError::Invalid("Time is outside the supported range.".into()))
}

fn checked_gui_duration(seconds: f32) -> Result<DonderDuration, GuiMutationError> {
    DonderDuration::try_from_seconds_f32(seconds)
        .map_err(|_| GuiMutationError::Invalid("Duration is outside the supported range.".into()))
}

#[cfg(test)]
mod time_conversion_tests {
    use super::{checked_gui_duration, checked_gui_time};

    #[test]
    fn finite_but_unrepresentable_gui_times_are_rejected_without_panicking() {
        assert!(checked_gui_time(f32::MAX).is_err());
        assert!(checked_gui_duration(f32::MAX).is_err());
        assert!(checked_gui_time(1.0).is_ok());
        assert!(checked_gui_duration(1.0).is_ok());
    }
}
