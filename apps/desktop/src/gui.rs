use std::collections::BTreeSet;

use camino::Utf8Path;
use dawn_language::effect::EffectInst;
use dawn_language::identity::SourceIdentity;
use dawn_language::sequence::SequenceId;
use dawn_project_io::{ProjectSession, SourceObjectKind};

use crate::dto::{
    DiagnosticSeverity, DocumentViewId, GuiDocument, GuiDocumentRequest, GuiEditCommand,
    GuiObjectRef, ObjectKind, ProjectDiagnostic, SequenceSelection, SequenceSelectionEdit,
};

mod controller;
mod controls;
mod dispatch;
mod document;
mod edit;
mod elements;
mod fixture;
mod fixture_profile;
mod layout;
mod library;
pub(crate) mod model;
mod patch;
mod project;
mod projection;
mod selection;
mod setup;

use edit::edit_sequence;
use fixture::edit_fixture;
use layout::edit_layout;
use project::project_root;
use projection::{project_fixture, project_layout, project_sequence};
pub(crate) use selection::copy_sequence_selection;
use selection::{
    delete_sequence_selection, move_effect_selection, move_mark_selection,
    paste_sequence_clipboard, resize_effect_selection,
};
use setup::{edit_setup, project_setup};

pub use dispatch::apply_edit;
pub(crate) use dispatch::{
    ClipboardEffect, ClipboardMark, SequenceClipboard, SequenceSelectionMutation,
    apply_sequence_selection_edit,
};
pub use document::{GuiMutationError, blocked, project_gui_document};
pub(crate) use document::{
    ResolvedGuiObject, affected_paths, ensure_owned_gui_document, gui_diagnostic, resolve_request,
};
