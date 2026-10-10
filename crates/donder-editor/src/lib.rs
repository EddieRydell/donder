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

mod gui;
mod source_documents;

pub use gui::model::source_identity_from_gui;
pub use gui::{
    ClipboardAutomation, ClipboardEffect, ClipboardMark, GuiMutationError, ResolvedGuiObject,
    SequenceClipboard, SequenceSelectionMutation, affected_paths, apply_edit,
    apply_sequence_selection_edit, blocked, copy_sequence_selection, create_sequence,
    ensure_owned_gui_document, project_gui_document, project_gui_document_change, resolve_request,
    sequence_effect_details,
};
pub use source_documents::{document_for_editor_path, editor_path};
