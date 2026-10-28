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
pub use address::{
    DeclaredObject, DocumentId, ObjectAddress, OwnedObjectSlot, SourceIdentity, validation_message,
};
mod analysis;
mod check_cache;
pub use check_cache::ProjectCheckCache;
mod diagnostics;
mod document;
mod errors;
mod imports;
mod inclusion;
mod index;
mod loader;
mod ownership_edit;
pub use ownership_edit::maintain_ownership_sources;
mod project_config;
mod project_loading;
pub use project_config::{
    PROJECT_FORMAT_VERSION, PROJECT_ROOT_FILE, ProjectMetadata, ProjectWorkspace, atomic_write,
    validate_document_path, validate_relative_path,
};
mod path_refactor;
mod project_edit;
mod serialization;
mod source;
mod source_copy;

pub use analysis::{ProjectRecovery, RecoveryDocument, RecoveryObject};
pub use diagnostics::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoFix, IoRelatedLocation,
    ProjectCheckReport, TextPosition, TextRange,
};
pub use document::{DECLARATION_TYPE_NAMES, document_schema};
pub use errors::ExportProjectError;
pub use imports::{
    available_reusable_sources, ensure_document_can_reference_object, link_reusable_source,
};
pub use inclusion::{Inclusion, include_document};
pub use index::{Link, LinkTarget, ProjectIndex, ScriptMember};
pub use path_refactor::{
    PathChangeImpact, PathChangePlan, PathChangeSourceKind, apply_path_change, plan_path_change,
};
pub use project_edit::{
    DocumentTextCache, export_project, save_project, source_document_text, source_document_texts,
};
pub use project_loading::{SourceOverrides, check_project, project_source_texts};
pub use serialization::{SourceTextWrite, write_source_texts};
pub use source::{
    ExportReport, ImportEdge, ProjectSession, ReferencedAsset, SaveReport, SourceDocument,
    SourceDocumentFormat, SourceProject, source_document_format,
};
pub use source_copy::export_editable_project;
