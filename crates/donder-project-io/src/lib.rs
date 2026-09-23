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

mod analysis;
mod diagnostics;
mod errors;
mod imports;
mod loader;
mod package_artifact;
mod package_loading;
mod package_update;
mod path_refactor;
mod project_edit;
mod schema;
mod serialization;
mod source;
mod source_copy;

pub use analysis::{ProjectRecovery, RecoveryDocument, RecoveryDocumentKind, RecoveryObject};
pub use diagnostics::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoRelatedLocation, ProjectCheckReport,
    TextPosition, TextRange,
};
pub use errors::{ExportProjectError, LoadProjectError};
pub use imports::ensure_document_can_reference_source;
pub use package_artifact::{pack_package, validate_registry_package_artifact};
pub use package_loading::{
    CompiledPackage, CompiledSourceGraph, LoadedPackageProject, PackageLoadError, SourceOverrides,
    check_document_text, check_package, check_package_with_cache, check_package_with_overrides,
    check_project_document_text, check_source_graph, compile_package, compile_package_with_cache,
    compile_source_graph, load_package, load_package_with_cache, load_source_graph,
    project_source_texts,
};
pub use package_update::{
    PackageCompatibilityIssue, PackageCompatibilityIssueKind, PackageCompatibilityReport,
    analyze_package_candidate,
};
pub use path_refactor::{
    PathChangeImpact, PathChangeOwnership, PathChangePlan, PathChangeSourceKind, apply_path_change,
    plan_path_change,
};
pub use project_edit::{export_project, insert_sequence, save_project, source_document_text};
pub use serialization::{SourceTextWrite, write_source_texts};
pub use source::{
    ExportReport, ImportEdge, ImportSource, ProjectSession, ReferencedAsset, SaveReport,
    SourceDocument, SourceDocumentFormat, SourceDocumentKind, SourceObjectId, SourceObjectKind,
    SourceOwnership, SourceProject, source_document_format, source_file_list,
};
pub use source_copy::export_editable_project;
