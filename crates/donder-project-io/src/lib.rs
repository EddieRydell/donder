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

mod address;
pub use address::{DeclaredObject, DocumentId, SourceIdentity, validation_message};
mod analysis;
mod check_cache;
pub use check_cache::ProjectCheckCache;
mod clipboard;
pub use clipboard::{holds_sequence_clipboard, read_sequence_clipboard, write_sequence_clipboard};
mod diagnostics;
mod document;
mod errors;
mod imports;
mod inclusion;
mod index;
mod loader;
mod project_config;
mod project_loading;
pub use project_config::{
    PROJECT_FORMAT_VERSION, PROJECT_ROOT_FILE, ProjectWorkspace, atomic_write,
    validate_document_path,
};
pub(crate) use project_config::{canonicalize, validate_relative_path};
mod object_files;
mod path_refactor;
mod project_edit;
mod serialization;
mod session_edit;
pub use session_edit::{SessionEdit, SessionEditError, remove_documents};
mod source;
mod source_copy;

pub use analysis::{ProjectRecovery, RecoveryDocument, RecoveryObject};
pub use diagnostics::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoFix, IoRelatedLocation,
    ProjectCheckReport, TextPosition, TextRange, dsl_diagnostic,
};
pub use document::{DECLARATION_TYPE_NAMES, document_schema, param_refers, reference_targets};
pub use errors::ExportProjectError;
pub use imports::ensure_document_can_reference_object;
pub use inclusion::{Inclusion, include_document};
pub use index::{Link, LinkTarget, ProjectIndex, ScriptMember};
pub use object_files::object_document_path;
pub use path_refactor::{
    PathChangeImpact, PathChangePlan, PathChangeSourceKind, apply_path_change, plan_path_change,
};
pub use project_edit::{
    DocumentTextCache, save_project, source_document_text, source_document_texts,
};
pub use project_loading::{SourceOverrides, check_project, project_source_texts};
pub use serialization::{SourceTextWrite, write_source_texts};
pub use source::{
    AssetError, ImportEdge, ProjectSession, ReferencedAsset, SourceDocument, SourceDocumentFormat,
    SourceProject, source_document_format,
};
