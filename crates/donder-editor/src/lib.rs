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
    GuiEditOutcome, GuiMutationError, ResolvedGuiObject, affected_paths, apply_edit, blocked,
    check_edit, copy_selection, cut_selection, edit_site, gui_editor_order,
    matching_sequence_clips, paste, project_gui_document, project_gui_document_change, refusal,
    repair_changes, resolve_request, sequence_clip_details,
};
pub use source_documents::document_for_editor_path;
