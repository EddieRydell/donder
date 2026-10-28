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

pub mod dto;
mod gui;
mod source_documents;

pub use gui::model::source_identity_from_gui;
pub use gui::{
    ClipboardAutomation, ClipboardEffect, ClipboardMark, GuiEditOutcome, GuiMutationError,
    ResolvedGuiObject, SequenceClipboard, affected_paths, apply_edit, blocked, copy_selection,
    create_sequence, gui_editor_order, matching_sequence_clips, project_gui_document,
    project_gui_document_change, resolve_request, sequence_effect_details,
};
pub use source_documents::document_for_editor_path;
