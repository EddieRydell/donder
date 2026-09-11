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

use camino::{Utf8Path, Utf8PathBuf};
use dawn_language::dsl::Identifier;
use dawn_language::identity::SourceIdentity;
use dawn_language::sequence::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceId, SequenceLayer, SequenceLayerId,
};
use dawn_language::values::{Color, DawnDuration};
use indexmap::{IndexMap, IndexSet};
use marked_yaml::Node;
use std::cell::RefCell;
use std::fmt;
use std::fs;
use std::io;
use yaml_serde::{Mapping, Value};

thread_local! {
    static YAML_SOURCE_INDICES: RefCell<IndexMap<Utf8PathBuf, YamlSourceIndex>> =
        RefCell::new(IndexMap::new());
}

mod analysis;
mod imports;
mod schema;
pub use imports::ensure_document_can_reference_source;
mod package_update;
mod path_refactor;
mod source;
mod source_copy;
pub use analysis::{ProjectRecovery, RecoveryDocument, RecoveryDocumentKind, RecoveryObject};
pub use package_update::{
    PackageCompatibilityIssue, PackageCompatibilityIssueKind, PackageCompatibilityReport,
    analyze_package_candidate,
};
pub use path_refactor::{
    PathChangeImpact, PathChangeOwnership, PathChangePlan, PathChangeSourceKind, apply_path_change,
    plan_path_change,
};
pub use source::{
    ExportReport, ImportEdge, ImportSource, ProjectSession, ReferencedAsset, SaveReport,
    SourceDocument, SourceDocumentKind, SourceObjectId, SourceObjectKind, SourceOwnership,
    SourceProject, source_file_list,
};
pub use source_copy::export_editable_project;

/// A package-resolved project. The compiler still receives the same typed
/// `ProjectSession`; package metadata and lock validation stay at the IO
/// boundary and never leak into runtime rendering.
#[derive(Clone, Debug, PartialEq)]
pub struct LoadedPackageProject {
    pub manifest: dawn_package::PackageManifest,
    pub lockfile: dawn_package::Lockfile,
    pub session: ProjectSession,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledSourceGraph {
    pub source: SourceProject,
    pub project: Option<dawn_language::model::DawnProject>,
    pub definitions: dawn_language::model::ProjectDefinitionStores,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CompiledPackage {
    pub manifest: dawn_package::PackageManifest,
    pub lockfile: dawn_package::Lockfile,
    pub graph: CompiledSourceGraph,
}

#[derive(Debug)]
pub enum PackageLoadError {
    Analysis(Vec<IoDiagnostic>),
    Package(dawn_package::PackageError),
    Project(LoadProjectError),
}

impl std::fmt::Display for PackageLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Analysis(diagnostics) => write!(
                formatter,
                "project analysis failed: {}",
                diagnostics
                    .iter()
                    .map(|diagnostic| {
                        let location = diagnostic
                            .range
                            .as_ref()
                            .map(|range| format!(":{}", range.start.line + 1))
                            .unwrap_or_default();
                        format!("{}{location}: {}", diagnostic.path, diagnostic.message)
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Self::Package(error) => write!(formatter, "package error: {error}"),
            Self::Project(error) => write!(formatter, "project error: {error:?}"),
        }
    }
}

impl std::error::Error for PackageLoadError {}

impl From<dawn_package::PackageError> for PackageLoadError {
    fn from(error: dawn_package::PackageError) -> Self {
        Self::Package(error)
    }
}

impl From<LoadProjectError> for PackageLoadError {
    fn from(error: LoadProjectError) -> Self {
        Self::Project(error)
    }
}

pub fn load_package(root: &Utf8Path) -> Result<LoadedPackageProject, PackageLoadError> {
    let compiled = compile_package(root)?;
    let project = compiled
        .graph
        .project
        .ok_or_else(|| LoadProjectError::InvalidEntrypoint {
            path: root.join(dawn_package::MANIFEST_FILE),
        })?;
    Ok(LoadedPackageProject {
        manifest: compiled.manifest,
        lockfile: compiled.lockfile,
        session: ProjectSession {
            project,
            source: compiled.graph.source,
        },
    })
}

pub fn compile_package(root: &Utf8Path) -> Result<CompiledPackage, PackageLoadError> {
    let (analysis, lockfile) = analyze_package(root);
    let graph = analysis
        .compiled
        .ok_or(PackageLoadError::Analysis(analysis.diagnostics))?;
    let manifest = graph.source.source_graph.project_module().manifest.clone();
    let lockfile = lockfile.ok_or_else(|| {
        dawn_package::PackageError::Invalid("package analysis produced no lockfile".to_string())
    })?;
    Ok(CompiledPackage {
        manifest,
        lockfile,
        graph,
    })
}

pub fn compile_package_with_cache(
    root: &Utf8Path,
    manifest: dawn_package::PackageManifest,
    lockfile: dawn_package::Lockfile,
    cache: &dawn_package::CacheStore,
) -> Result<CompiledPackage, PackageLoadError> {
    let source_graph =
        dawn_package::ResolvedSourceGraph::from_lock(root, manifest.clone(), &lockfile, cache)?;
    let graph = compile_source_graph(source_graph)?;
    Ok(CompiledPackage {
        manifest,
        lockfile,
        graph,
    })
}

pub fn validate_registry_package_artifact(
    package_root: &Utf8Path,
    package: &dawn_package::PackageId,
    locked: &dawn_package::LockedPackage,
    global_lock: &dawn_package::Lockfile,
    cache: &dawn_package::CacheStore,
) -> Result<(), dawn_package::PackageError> {
    if global_lock.packages.get(package) != Some(locked) {
        return Err(dawn_package::PackageError::Invalid(format!(
            "artifact validation received a lock entry that does not match `{package}`"
        )));
    }

    let manifest = dawn_package::PackageManifest::read(package_root)?;
    let publication = manifest.publication.as_ref().ok_or_else(|| {
        dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` has no publication identity",
            locked.version
        ))
    })?;
    if publication.package != *package
        || publication.version != locked.version
        || manifest.module_id != locked.module_id
    {
        return Err(dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` does not match dawn.lock",
            locked.version
        )));
    }

    let declared_dependencies = manifest
        .dependencies
        .iter()
        .map(|(alias, dependency)| match dependency {
            dawn_package::Dependency::Registry {
                package: dependency,
                version,
            } => {
                let selected = global_lock.packages.get(dependency).ok_or_else(|| {
                    dawn_package::PackageError::Invalid(format!(
                        "cached package `{package}` points to unlocked dependency `{dependency}`"
                    ))
                })?;
                if !version.matches(&selected.version) {
                    return Err(dawn_package::PackageError::Invalid(format!(
                        "locked `{dependency}@{}` does not satisfy `{version}` required by `{package}`",
                        selected.version
                    )));
                }
                Ok((alias.clone(), dependency.clone()))
            }
            dawn_package::Dependency::Path { .. } => {
                Err(dawn_package::PackageError::Invalid(format!(
                    "cached registry package `{package}` contains a path dependency"
                )))
            }
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    if declared_dependencies != locked.dependencies {
        return Err(dawn_package::PackageError::Invalid(format!(
            "cached package `{package}@{}` dependency edges do not match dawn.lock",
            locked.version
        )));
    }

    let mut visiting = vec![package.clone()];
    let mut closure = std::collections::BTreeSet::new();
    for dependency in locked.dependencies.values() {
        collect_registry_package_closure(dependency, global_lock, &mut visiting, &mut closure)?;
    }
    let packages = closure
        .into_iter()
        .map(|dependency| {
            let locked_dependency =
                global_lock
                    .packages
                    .get(&dependency)
                    .cloned()
                    .ok_or_else(|| {
                        dawn_package::PackageError::Invalid(format!(
                            "locked package graph points to missing package `{dependency}`"
                        ))
                    })?;
            Ok((dependency, locked_dependency))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, dawn_package::PackageError>>()?;
    let artifact_lock = dawn_package::Lockfile {
        lock_version: global_lock.lock_version,
        manifest_sha256: dawn_package::manifest_hash(&manifest)?,
        registry: global_lock.registry.clone(),
        packages,
        path_dependencies: std::collections::BTreeMap::new(),
    };

    let compiled = compile_package_with_cache(package_root, manifest, artifact_lock, cache)
        .map_err(|error| {
            dawn_package::PackageError::Invalid(format!(
                "package `{package}@{}` failed compiler validation: {error}",
                locked.version
            ))
        })?;
    let receipt = fs::read(package_root.join("dawn-release.json"))?;
    let receipt = serde_json::from_slice::<dawn_package::ReleaseReceipt>(&receipt)?;
    let compiled_exports = release_export_index(&compiled).map_err(|error| {
        dawn_package::PackageError::Invalid(format!(
            "package `{package}@{}` failed compiler export validation: {error}",
            locked.version
        ))
    })?;
    if receipt.exports != compiled_exports {
        return Err(dawn_package::PackageError::Invalid(format!(
            "package `{package}@{}` release export index does not match compiler output",
            locked.version
        )));
    }
    Ok(())
}

fn collect_registry_package_closure(
    package: &dawn_package::PackageId,
    lockfile: &dawn_package::Lockfile,
    visiting: &mut Vec<dawn_package::PackageId>,
    closure: &mut std::collections::BTreeSet<dawn_package::PackageId>,
) -> Result<(), dawn_package::PackageError> {
    if closure.contains(package) {
        return Ok(());
    }
    if let Some(index) = visiting.iter().position(|entry| entry == package) {
        let mut cycle = visiting[index..]
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        cycle.push(package.to_string());
        return Err(dawn_package::PackageError::Invalid(format!(
            "package dependency cycle while validating artifacts: {}",
            cycle.join(" -> ")
        )));
    }
    let locked = lockfile.packages.get(package).ok_or_else(|| {
        dawn_package::PackageError::Invalid(format!(
            "locked package graph points to missing package `{package}`"
        ))
    })?;
    visiting.push(package.clone());
    for dependency in locked.dependencies.values() {
        collect_registry_package_closure(dependency, lockfile, visiting, closure)?;
    }
    let _ = visiting.pop();
    closure.insert(package.clone());
    Ok(())
}

pub fn pack_package(root: &Utf8Path) -> Result<dawn_package::PackedRelease, PackageLoadError> {
    let compiled = compile_package(root)?;
    if compiled
        .graph
        .source
        .source_graph
        .modules()
        .values()
        .any(|module| {
            module
                .manifest
                .dependencies
                .values()
                .any(|dependency| matches!(dependency, dawn_package::Dependency::Path { .. }))
        })
    {
        return Err(dawn_package::PackageError::Invalid(
            "published dependency closure cannot contain path dependencies".to_string(),
        )
        .into());
    }
    let plan = release_archive_plan(&compiled)?;
    dawn_package::pack_directory_with_plan(root, &plan).map_err(Into::into)
}

fn release_archive_plan(
    compiled: &CompiledPackage,
) -> Result<dawn_package::ReleaseArchivePlan, PackageLoadError> {
    let project_module_id = compiled.manifest.module_id;
    let mut pending = std::collections::BTreeSet::new();
    for export in compiled.manifest.exports.values() {
        for document in &export.documents {
            pending.insert(dawn_language::identity::DocumentId::new(
                project_module_id,
                Utf8PathBuf::from(document),
            ));
        }
    }
    if let Some(project) = &compiled.manifest.project {
        pending.insert(dawn_language::identity::DocumentId::new(
            project_module_id,
            Utf8PathBuf::from(&project.entrypoint),
        ));
    }
    let mut reachable = std::collections::BTreeSet::new();
    while let Some(document_id) = pending.pop_first() {
        if document_id.module_id() != project_module_id
            || !reachable.insert(document_id.path().to_string())
        {
            continue;
        }
        let document = compiled
            .graph
            .source
            .documents
            .get(&document_id)
            .ok_or_else(|| {
                dawn_package::PackageError::Invalid(format!(
                    "release root `{}` was not compiled",
                    document_id.path()
                ))
            })?;
        for import in document.imports() {
            for target in import.targets() {
                if target.module_id() == project_module_id
                    && !reachable.contains(target.path().as_str())
                {
                    pending.insert(target.clone());
                }
            }
        }
    }
    reachable.insert(dawn_package::MANIFEST_FILE.to_string());
    reachable.extend(compiled.manifest.assets.keys().cloned());
    for metadata_path in ["README.md", "RELEASE_NOTES.md", "LICENSE"] {
        if compiled
            .graph
            .source
            .project_root()
            .join(metadata_path)
            .is_file()
        {
            reachable.insert(metadata_path.to_string());
        }
    }
    if compiled
        .manifest
        .publication
        .as_ref()
        .is_some_and(|publication| publication.license == "LicenseRef-Custom")
        && !reachable.contains("LICENSE")
    {
        return Err(dawn_package::PackageError::Invalid(
            "LicenseRef-Custom requires a root LICENSE file".to_string(),
        )
        .into());
    }

    Ok(dawn_package::ReleaseArchivePlan {
        files: reachable,
        exports: release_export_index(compiled)?,
    })
}

fn release_export_index(
    compiled: &CompiledPackage,
) -> Result<std::collections::BTreeMap<String, dawn_package::ReleaseExportGroup>, PackageLoadError>
{
    let project_module_id = compiled.manifest.module_id;
    let mut exports = std::collections::BTreeMap::new();
    for (group_name, group) in &compiled.manifest.exports {
        let mut objects = Vec::new();
        let mut object_names = std::collections::BTreeMap::new();
        for document in &group.documents {
            let document_id = dawn_language::identity::DocumentId::new(
                project_module_id,
                Utf8PathBuf::from(document),
            );
            let source = compiled
                .graph
                .source
                .documents
                .get(&document_id)
                .ok_or_else(|| {
                    dawn_package::PackageError::Invalid(format!(
                        "export document `{document}` was not compiled"
                    ))
                })?;
            for object in source.objects() {
                if let Some(previous_document) =
                    object_names.insert(object.id().to_string(), document.clone())
                {
                    return Err(dawn_package::PackageError::Invalid(format!(
                        "export group `{group_name}` exposes object `{}` from both `{previous_document}` and `{document}`",
                        object.id()
                    ))
                    .into());
                }
                objects.push(dawn_package::ReleaseExportObject {
                    document: document.clone(),
                    name: object.id().to_string(),
                    kind: release_object_kind(object.kind())?,
                });
            }
        }
        objects.sort();
        exports.insert(
            group_name.clone(),
            dawn_package::ReleaseExportGroup {
                documents: group.documents.clone(),
                objects,
            },
        );
    }
    Ok(exports)
}

fn release_object_kind(
    kind: &SourceObjectKind,
) -> Result<dawn_package::ExportObjectKind, PackageLoadError> {
    Ok(match kind {
        SourceObjectKind::Project => dawn_package::ExportObjectKind::Project,
        SourceObjectKind::Setup => dawn_package::ExportObjectKind::Setup,
        SourceObjectKind::Controller => dawn_package::ExportObjectKind::Controller,
        SourceObjectKind::Layout => dawn_package::ExportObjectKind::Layout,
        SourceObjectKind::Patch => dawn_package::ExportObjectKind::Patch,
        SourceObjectKind::FixtureDefinition => dawn_package::ExportObjectKind::FixtureDefinition,
        SourceObjectKind::Curve => dawn_package::ExportObjectKind::Curve,
        SourceObjectKind::Gradient => dawn_package::ExportObjectKind::Gradient,
        SourceObjectKind::Sequence => dawn_package::ExportObjectKind::Sequence,
        SourceObjectKind::EffectDefinition => dawn_package::ExportObjectKind::EffectDefinition,
        SourceObjectKind::OperatorDefinition => dawn_package::ExportObjectKind::OperatorDefinition,
        SourceObjectKind::EffectInstance => {
            return Err(dawn_package::PackageError::Invalid(
                "effect instances cannot be package exports".to_string(),
            )
            .into());
        }
    })
}

pub fn load_package_with_cache(
    root: &Utf8Path,
    manifest: dawn_package::PackageManifest,
    lockfile: dawn_package::Lockfile,
    cache: &dawn_package::CacheStore,
) -> Result<LoadedPackageProject, PackageLoadError> {
    let report = check_package_with_cache(root, manifest.clone(), lockfile.clone(), cache);
    let session = report
        .session
        .ok_or(PackageLoadError::Analysis(report.diagnostics))?;
    Ok(LoadedPackageProject {
        manifest,
        lockfile,
        session,
    })
}

pub fn load_source_graph(
    source_graph: dawn_package::ResolvedSourceGraph,
) -> Result<ProjectSession, LoadProjectError> {
    Loader::new(source_graph)?.load()
}

pub fn compile_source_graph(
    source_graph: dawn_package::ResolvedSourceGraph,
) -> Result<CompiledSourceGraph, LoadProjectError> {
    Loader::new(source_graph)?.compile()
}

struct ProjectAnalysis {
    compiled: Option<CompiledSourceGraph>,
    recovery: ProjectRecovery,
    diagnostics: Vec<IoDiagnostic>,
}

impl ProjectAnalysis {
    fn into_report(mut self) -> ProjectCheckReport {
        let session = self.compiled.and_then(|compiled| match compiled.project {
            Some(project) => Some(ProjectSession {
                project,
                source: compiled.source,
            }),
            None => {
                push_load_error_diagnostics(
                    &mut self.diagnostics,
                    LoadProjectError::InvalidEntrypoint {
                        path: self.recovery.root.join(dawn_package::MANIFEST_FILE),
                    },
                );
                None
            }
        });
        analysis::sort_diagnostics(&mut self.diagnostics);
        ProjectCheckReport {
            session,
            recovery: self.recovery,
            diagnostics: self.diagnostics,
        }
    }
}

fn compile_for_analysis(
    overrides: &SourceOverrides,
    source_graph: dawn_package::ResolvedSourceGraph,
    checked_dsl_documents: &mut IndexSet<Utf8PathBuf>,
    diagnostics: &mut Vec<IoDiagnostic>,
) -> Option<CompiledSourceGraph> {
    let result = Loader::new(source_graph).and_then(|mut loader| {
        for (path, text) in overrides {
            let Some((module, path)) =
                source::workspace_module_for_path(&loader.source_graph, path)
            else {
                continue;
            };
            loader.source_overrides.insert(
                dawn_language::identity::DocumentId::new(module, path),
                text.clone(),
            );
        }
        let result = loader.compile();
        *checked_dsl_documents = loader.checked_dsl_documents;
        result
    });
    match result {
        Ok(compiled) => Some(compiled),
        Err(error) => {
            push_load_error_diagnostics(diagnostics, error);
            None
        }
    }
}

fn finish_analysis(
    overrides: &SourceOverrides,
    root: &Utf8Path,
    manifest: Option<dawn_package::PackageManifest>,
    compiled: Option<CompiledSourceGraph>,
    checked_dsl_documents: &IndexSet<Utf8PathBuf>,
    mut diagnostics: Vec<IoDiagnostic>,
) -> ProjectAnalysis {
    let recovery = analysis::analyze_project_documents(
        root,
        manifest,
        overrides,
        checked_dsl_documents,
        &mut diagnostics,
    );
    analysis::sort_diagnostics(&mut diagnostics);
    let compiled = compiled.filter(|_| {
        !diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == IoDiagnosticSeverity::Error)
    });
    ProjectAnalysis {
        compiled,
        recovery,
        diagnostics,
    }
}

pub fn check_source_graph(source_graph: dawn_package::ResolvedSourceGraph) -> ProjectCheckReport {
    let project_module = source_graph.project_module();
    let root = project_module.root.clone();
    let manifest = project_module.manifest.clone();
    let mut diagnostics = Vec::new();
    let mut checked_dsl_documents = IndexSet::new();
    let compiled = compile_for_analysis(
        &SourceOverrides::new(),
        source_graph,
        &mut checked_dsl_documents,
        &mut diagnostics,
    );
    finish_analysis(
        &SourceOverrides::new(),
        &root,
        Some(manifest),
        compiled,
        &checked_dsl_documents,
        diagnostics,
    )
    .into_report()
}

pub fn check_package(root: &Utf8Path) -> ProjectCheckReport {
    check_package_with_overrides(root, &SourceOverrides::new())
}

pub type SourceOverrides = std::collections::BTreeMap<Utf8PathBuf, String>;

pub fn project_source_texts(root: &Utf8Path) -> io::Result<SourceOverrides> {
    analysis::project_file_inventory(root)
        .into_iter()
        .filter(|path| {
            path.extension() == Some("dawn")
                || matches!(
                    path.file_name(),
                    Some(dawn_package::MANIFEST_FILE | dawn_package::LOCK_FILE)
                )
        })
        .map(|path| fs::read_to_string(root.join(&path)).map(|text| (path, text)))
        .collect()
}

/// Analyze the exact working sources without writing any input to disk.
pub fn check_package_with_overrides(
    root: &Utf8Path,
    overrides: &SourceOverrides,
) -> ProjectCheckReport {
    analyze_package_overrides(root, overrides).0.into_report()
}

fn analyze_package(root: &Utf8Path) -> (ProjectAnalysis, Option<dawn_package::Lockfile>) {
    analyze_package_overrides(root, &SourceOverrides::new())
}

fn analyze_package_overrides(
    root: &Utf8Path,
    overrides: &SourceOverrides,
) -> (ProjectAnalysis, Option<dawn_package::Lockfile>) {
    let mut diagnostics = Vec::new();
    let manifest = match overrides
        .get(Utf8Path::new(dawn_package::MANIFEST_FILE))
        .map_or_else(
            || dawn_package::PackageManifest::read_for_analysis(root),
            |text| dawn_package::PackageManifest::parse_for_analysis(text.as_bytes()),
        ) {
        Ok(manifest) => {
            diagnostics.extend(analysis::package_validation_diagnostics(
                dawn_package::MANIFEST_FILE,
                manifest.validation_issues(root),
                IoDiagnosticCode::ManifestField,
            ));
            Some(manifest)
        }
        Err(error) => {
            if root.join(dawn_package::MANIFEST_FILE).is_file() {
                diagnostics.push(analysis::package_parse_diagnostic(
                    dawn_package::MANIFEST_FILE,
                    error,
                    IoDiagnosticCode::ManifestSyntax,
                ));
            } else {
                diagnostics.push(IoDiagnostic {
                    path: Utf8PathBuf::from(dawn_package::MANIFEST_FILE),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::DawnLoad,
                    message: error.to_string(),
                    detail: None,
                    related: Vec::new(),
                });
            }
            None
        }
    };
    let lockfile = match overrides
        .get(Utf8Path::new(dawn_package::LOCK_FILE))
        .map_or_else(
            || dawn_package::Lockfile::read_for_analysis(root),
            |text| dawn_package::Lockfile::parse_for_analysis(text.as_bytes()),
        ) {
        Ok(lockfile) => {
            if let Some(manifest) = &manifest {
                diagnostics.extend(analysis::package_validation_diagnostics(
                    dawn_package::LOCK_FILE,
                    lockfile.validation_issues(manifest),
                    IoDiagnosticCode::LockField,
                ));
            }
            Some(lockfile)
        }
        Err(error) => {
            diagnostics.push(analysis::package_parse_diagnostic(
                dawn_package::LOCK_FILE,
                error,
                IoDiagnosticCode::LockSyntax,
            ));
            None
        }
    };

    let mut checked_dsl_documents = IndexSet::new();
    let package_files_valid = !diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            IoDiagnosticCode::ManifestField
                | IoDiagnosticCode::ManifestSyntax
                | IoDiagnosticCode::LockField
                | IoDiagnosticCode::LockSyntax
        )
    });
    let compiled = if package_files_valid {
        match (manifest.as_ref(), lockfile.as_ref()) {
            (Some(manifest), Some(lockfile)) => {
                match package_cache_for_lock(root, lockfile).and_then(|cache| {
                    dawn_package::ResolvedSourceGraph::from_lock(
                        root,
                        manifest.clone(),
                        lockfile,
                        &cache,
                    )
                }) {
                    Ok(source_graph) => compile_for_analysis(
                        overrides,
                        source_graph,
                        &mut checked_dsl_documents,
                        &mut diagnostics,
                    ),
                    Err(error) => {
                        push_diagnostic(
                            &mut diagnostics,
                            IoDiagnostic {
                                path: Utf8PathBuf::from(dawn_package::LOCK_FILE),
                                range: None,
                                severity: IoDiagnosticSeverity::Error,
                                code: IoDiagnosticCode::LockField,
                                message: error.to_string(),
                                detail: None,
                                related: Vec::new(),
                            },
                        );
                        None
                    }
                }
            }
            _ => None,
        }
    } else {
        None
    };
    (
        finish_analysis(
            overrides,
            root,
            manifest,
            compiled,
            &checked_dsl_documents,
            diagnostics,
        ),
        lockfile,
    )
}

pub fn check_package_with_cache(
    root: &Utf8Path,
    manifest: dawn_package::PackageManifest,
    lockfile: dawn_package::Lockfile,
    cache: &dawn_package::CacheStore,
) -> ProjectCheckReport {
    let recovery_manifest = manifest.clone();
    let source_graph =
        match dawn_package::ResolvedSourceGraph::from_lock(root, manifest, &lockfile, cache) {
            Ok(source_graph) => source_graph,
            Err(error) => {
                let mut diagnostics = Vec::new();
                let recovery = analysis::analyze_project_documents(
                    root,
                    Some(recovery_manifest),
                    &SourceOverrides::new(),
                    &IndexSet::new(),
                    &mut diagnostics,
                );
                diagnostics.push(IoDiagnostic {
                    path: Utf8PathBuf::from(dawn_package::LOCK_FILE),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::LockField,
                    message: error.to_string(),
                    detail: None,
                    related: Vec::new(),
                });
                analysis::sort_diagnostics(&mut diagnostics);
                return ProjectCheckReport {
                    session: None,
                    recovery,
                    diagnostics,
                };
            }
        };
    check_source_graph(source_graph)
}

fn package_cache_for_lock(
    root: &Utf8Path,
    lockfile: &dawn_package::Lockfile,
) -> Result<dawn_package::CacheStore, dawn_package::PackageError> {
    if lockfile.packages.is_empty() {
        return Ok(dawn_package::CacheStore::new(
            root.join(".dawn-unused-cache"),
        ));
    }
    Ok(dawn_package::DawnDirectories::discover()?.package_cache())
}

pub fn check_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    if path
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".effect.dawn"))
    {
        return effect_diagnostics(path, text);
    }
    if path
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".operator.dawn"))
    {
        return operator_diagnostics(path, text);
    }
    if path
        .file_name()
        .is_some_and(|file_name| file_name.ends_with(".dawn"))
    {
        return analysis::check_dawn_document_text(path, text);
    }

    match parse_yaml_value(path, text) {
        Ok(_) => Vec::new(),
        Err(LoadProjectError::ParseYaml { message, range, .. }) => vec![IoDiagnostic {
            path: path.to_path_buf(),
            range,
            severity: IoDiagnosticSeverity::Error,
            code: IoDiagnosticCode::YamlParse,
            message,
            detail: None,
            related: Vec::new(),
        }],
        Err(error) => vec![load_error_diagnostic(error)],
    }
}

pub fn check_project_document_text(
    session: &ProjectSession,
    document: &dawn_language::identity::DocumentId,
    text: &str,
) -> Vec<IoDiagnostic> {
    let local_diagnostics = check_document_text(document.path(), text);
    if local_diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            IoDiagnosticCode::YamlParse
                | IoDiagnosticCode::EffectCompile
                | IoDiagnosticCode::OperatorCompile
        )
    }) {
        return local_diagnostics;
    }

    let loader = match Loader::new(session.source.source_graph.clone()) {
        Ok(mut loader) => {
            loader
                .source_overrides
                .insert(document.clone(), text.to_string());
            loader
        }
        Err(error) => return vec![load_error_diagnostic(error)],
    };
    match loader.load() {
        Ok(_) => local_diagnostics,
        Err(error) => {
            let mut diagnostics = Vec::new();
            push_load_error_diagnostics(&mut diagnostics, error);
            let additional = local_diagnostics
                .into_iter()
                .filter(|local| {
                    !diagnostics.iter().any(|canonical| {
                        canonical.path == local.path
                            && canonical.range == local.range
                            && canonical.message == local.message
                    })
                })
                .collect::<Vec<_>>();
            diagnostics.extend(additional);
            analysis::sort_diagnostics(&mut diagnostics);
            diagnostics
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProjectCheckReport {
    pub session: Option<ProjectSession>,
    pub recovery: ProjectRecovery,
    pub diagnostics: Vec<IoDiagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoDiagnostic {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub severity: IoDiagnosticSeverity,
    pub code: IoDiagnosticCode,
    pub message: String,
    pub detail: Option<String>,
    pub related: Vec<IoRelatedLocation>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct IoRelatedLocation {
    pub path: Utf8PathBuf,
    pub range: Option<TextRange>,
    pub message: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub enum IoDiagnosticCode {
    DawnLoad,
    DawnReference,
    EffectCompile,
    OperatorCompile,
    IoRead,
    ManifestField,
    ManifestSyntax,
    LockField,
    LockSyntax,
    SequenceField,
    SequenceItem,
    YamlParse,
}

impl IoDiagnosticCode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::DawnLoad => "dawn.load",
            Self::DawnReference => "dawn.reference",
            Self::EffectCompile => "effect.compile",
            Self::OperatorCompile => "operator.compile",
            Self::IoRead => "io.read",
            Self::ManifestField => "manifest.field",
            Self::ManifestSyntax => "manifest.syntax",
            Self::LockField => "lock.field",
            Self::LockSyntax => "lock.syntax",
            Self::SequenceField => "sequence.field",
            Self::SequenceItem => "sequence.item",
            Self::YamlParse => "yaml.parse",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextRange {
    pub start: TextPosition,
    pub end: TextPosition,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TextPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
enum YamlPathSegment {
    Key(String),
    Index(usize),
}

#[derive(Clone, Debug, Default)]
struct YamlSourceIndex {
    entries: Vec<YamlSourceEntry>,
    value_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_value_paths: IndexSet<Vec<YamlPathSegment>>,
    scalar_bindings: IndexMap<usize, Vec<YamlPathSegment>>,
    claimed_scalar_paths: IndexSet<Vec<YamlPathSegment>>,
}

#[derive(Clone, Debug)]
struct YamlSourceEntry {
    path: Vec<YamlPathSegment>,
    value: Value,
    range: Option<TextRange>,
}

impl YamlSourceIndex {
    fn from_value_and_node(value: &Value, node: &Node) -> Self {
        let mut index = Self::default();
        let mut path = Vec::new();
        index.push(value, node, &mut path);
        index
    }

    fn push(&mut self, value: &Value, node: &Node, path: &mut Vec<YamlPathSegment>) {
        self.entries.push(YamlSourceEntry {
            path: path.clone(),
            value: value.clone(),
            range: node_range(node),
        });

        match (value, node) {
            (Value::Mapping(mapping), Node::Mapping(marked_mapping)) => {
                for (key, child_value) in mapping {
                    let Some(key) = key.as_str() else {
                        continue;
                    };
                    let Some(child_node) = marked_mapping.get_node(key) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Key(key.to_string()));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            (Value::Sequence(sequence), Node::Sequence(marked_sequence)) => {
                for (index, child_value) in sequence.iter().enumerate() {
                    let Some(child_node) = marked_sequence.get_node(index) else {
                        continue;
                    };
                    path.push(YamlPathSegment::Index(index));
                    self.push(child_value, child_node, path);
                    let _ = path.pop();
                }
            }
            _ => {}
        }
    }

    fn bound_value_path(&mut self, value: &Value) -> Option<Vec<YamlPathSegment>> {
        let pointer = std::ptr::from_ref(value).addr();
        if let Some(path) = self.value_bindings.get(&pointer) {
            return Some(path.clone());
        }
        let path = self
            .entries
            .iter()
            .filter(|entry| &entry.value == value)
            .map(|entry| &entry.path)
            .find(|path| !self.claimed_value_paths.contains(*path))?
            .clone();
        self.claimed_value_paths.insert(path.clone());
        self.value_bindings.insert(pointer, path.clone());
        Some(path)
    }

    fn range_for_value(&mut self, value: &Value) -> Option<TextRange> {
        let path = self.bound_value_path(value)?;
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_field_value(&mut self, parent: &Value, key: &str) -> Option<TextRange> {
        let parent_path = self.bound_value_path(parent)?;
        let mut field_path = parent_path;
        field_path.push(YamlPathSegment::Key(key.to_string()));
        self.entries
            .iter()
            .find(|entry| entry.path == field_path)
            .and_then(|entry| entry.range.clone())
    }

    fn range_for_scalar(&mut self, value: &str) -> Option<TextRange> {
        let pointer = value.as_ptr().addr();
        let path = if let Some(path) = self.scalar_bindings.get(&pointer) {
            path.clone()
        } else {
            let path = self
                .entries
                .iter()
                .filter(|entry| entry.value.as_str() == Some(value))
                .map(|entry| &entry.path)
                .find(|path| !self.claimed_scalar_paths.contains(*path))?
                .clone();
            self.claimed_scalar_paths.insert(path.clone());
            self.scalar_bindings.insert(pointer, path.clone());
            path
        };
        self.entries
            .iter()
            .find(|entry| entry.path == path)
            .and_then(|entry| entry.range.clone())
    }
}

pub fn export_project(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<ExportReport, ExportProjectError> {
    if output_root.exists() && !output_root.is_dir() {
        return Err(ExportProjectError::OutputRootIsFile {
            path: output_root.to_path_buf(),
        });
    }
    fs::create_dir_all(output_root).map_err(|source| ExportProjectError::Io {
        path: output_root.to_path_buf(),
        source,
    })?;

    // Export alone clones the session because external asset paths are rewritten
    // for the destination. Normal saves serialize the shared session directly.
    let mut synced = session.clone();
    let project_module_id = synced.source.project_module_id();
    for asset in &mut synced.source.referenced_assets {
        if asset.module_id != project_module_id {
            let file_name = asset.absolute_path.file_name().ok_or_else(|| {
                ExportProjectError::InvalidReference {
                    path: asset.absolute_path.clone(),
                    reference: asset.absolute_path.to_string(),
                    message: "external asset has no file name".to_string(),
                }
            })?;
            asset.relative_path = Utf8PathBuf::from("assets")
                .join(asset.id.0.to_string())
                .join(file_name);
            asset.module_id = project_module_id;
        }
    }
    let written_files = write_source_documents(&synced, output_root)?;

    let mut copied_assets = Vec::new();
    for (source_asset, exported_asset) in session
        .source
        .referenced_assets
        .iter()
        .zip(&synced.source.referenced_assets)
    {
        let output_path = output_root.join(&exported_asset.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_asset.absolute_path, &output_path).map_err(|source| {
            ExportProjectError::Io {
                path: output_path.clone(),
                source,
            }
        })?;
        copied_assets.push(exported_asset.relative_path.clone());
    }

    Ok(ExportReport {
        written_files,
        copied_assets,
    })
}

pub fn save_project(session: &ProjectSession) -> Result<SaveReport, ExportProjectError> {
    let project_root = session.source.project_root();
    let written_files = write_source_documents(session, project_root)?;
    Ok(SaveReport { written_files })
}

pub fn source_document_text(
    session: &ProjectSession,
    document_id: &dawn_language::identity::DocumentId,
) -> Result<Option<String>, ExportProjectError> {
    serialization::validate_source_inventory(session)?;
    let Some(document) = session.source.documents.get(document_id) else {
        return Ok(None);
    };
    document_text(session, document_id, document).map(Some)
}

pub fn insert_sequence(
    session: &mut ProjectSession,
    path: Utf8PathBuf,
    object_key: String,
    duration: DawnDuration,
    frame_rate: u32,
) -> Result<SequenceId, ExportProjectError> {
    if !is_module_relative_path(&path)
        || !path.starts_with("sequences")
        || !path
            .file_name()
            .is_some_and(|name| name.ends_with(".sequence.dawn"))
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence path must be an owned .sequence.dawn document under sequences/"
                .to_string(),
        });
    }
    if Identifier::new(object_key.clone()).is_err()
        || !duration.as_seconds_f32().is_finite()
        || duration.as_seconds_f32() <= 0.0
        || frame_rate == 0
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence identity, duration, or frame rate is invalid".to_string(),
        });
    }
    let document_id = session.source.project_document(path.clone());
    let project_root = session.source.project_root();
    if session.source.documents.contains_key(&document_id) || project_root.join(&path).exists() {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "source document already exists".to_string(),
        });
    }
    let identity = SourceIdentity::from_document(document_id.clone(), object_key.clone());
    let id = SequenceId(identity.clone());
    if session.project.sequences.contains_key(&id) {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence already exists".to_string(),
        });
    }
    let layer_id = SequenceLayerId(0);
    let sequence = Sequence {
        id: id.clone(),
        duration,
        frame_rate,
        audio: SequenceAudio::None,
        mark_collections: vec![MarkCollection {
            key: MarkCollectionKey {
                name: "marks".to_string(),
            },
            name: "Marks".to_string(),
            display_color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            marks: Vec::new(),
        }],
        layers: vec![SequenceLayer {
            id: layer_id.clone(),
            name: "Default".to_string(),
            color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            enabled: true,
        }],
        effects: Vec::new(),
        composition_graph: SequenceCompositionGraph {
            nodes: vec![
                CompositionGraphNode {
                    id: CompositionGraphNodeId(1),
                    position: GraphNodePosition { x: 80.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Layer { layer_id },
                },
                CompositionGraphNode {
                    id: CompositionGraphNodeId(2),
                    position: GraphNodePosition { x: 420.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Output,
                },
            ],
            edges: vec![EffectGraphEdge {
                from: CompositionGraphNodeId(1),
                from_port: GraphPortId("output".to_string()),
                to: CompositionGraphNodeId(2),
                to_port: GraphPortId("input".to_string()),
            }],
        },
        automation_clips: Vec::new(),
    };
    let source_document = SourceDocument::new(
        Vec::new(),
        vec![SourceObjectId {
            kind: SourceObjectKind::Sequence,
            id: identity.object().to_string(),
        }],
        SourceDocumentKind::Dawn {
            original_value: Value::Mapping(Mapping::new()),
        },
    )
    .map_err(|message| ExportProjectError::InvalidReference {
        path: path.clone(),
        reference: object_key.clone(),
        message,
    })?;
    session
        .source
        .documents
        .insert(document_id, source_document);
    session.project.sequences.insert(id.clone(), sequence);
    session.project.root.sequences.push(id.clone());
    let entrypoint =
        session
            .source
            .entrypoint
            .clone()
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: path.clone(),
                reference: object_key.clone(),
                message: "active project has no manifest entrypoint".to_string(),
            })?;
    ensure_document_can_reference_source(
        session,
        &entrypoint,
        SourceObjectKind::Sequence,
        &identity,
    )?;
    Ok(id)
}

fn is_module_relative_path(path: &Utf8Path) -> bool {
    !path.as_str().is_empty()
        && !path.is_absolute()
        && !path.as_str().contains('\\')
        && path
            .components()
            .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
}

#[derive(Debug)]
pub enum LoadProjectError {
    InvalidImports {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
    InvalidEntrypoint {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    ParseYaml {
        path: Utf8PathBuf,
        message: String,
        range: Option<TextRange>,
    },
    InvalidDocument {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        message: String,
    },
    InvalidReference {
        path: Utf8PathBuf,
        range: Option<TextRange>,
        reference: String,
    },
    InvalidEffect {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
    InvalidOperator {
        path: Utf8PathBuf,
        diagnostics: Vec<IoDiagnostic>,
    },
}

#[derive(Debug)]
pub enum ExportProjectError {
    OutputRootIsFile {
        path: Utf8PathBuf,
    },
    Io {
        path: Utf8PathBuf,
        source: io::Error,
    },
    Serialize {
        path: Utf8PathBuf,
        source: yaml_serde::Error,
    },
    InvalidReference {
        path: Utf8PathBuf,
        reference: String,
        message: String,
    },
}

impl fmt::Display for LoadProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEntrypoint { path } => write!(formatter, "invalid entrypoint {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::ParseYaml { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidDocument { path, message, .. } => write!(formatter, "{path}: {message}"),
            Self::InvalidReference {
                path, reference, ..
            } => {
                write!(formatter, "{path}: invalid reference {reference}")
            }
            Self::InvalidEffect { path, diagnostics }
            | Self::InvalidImports { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid document: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::InvalidOperator { path, diagnostics } => {
                write!(
                    formatter,
                    "{path}: invalid operator: {}",
                    diagnostics
                        .iter()
                        .map(|diagnostic| diagnostic.message.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        }
    }
}

impl std::error::Error for LoadProjectError {}

mod diagnostics;
use diagnostics::{
    effect_diagnostics, load_error_diagnostic, node_range, operator_diagnostics, parse_yaml_value,
    push_diagnostic, push_load_error_diagnostics,
};

impl fmt::Display for ExportProjectError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputRootIsFile { path } => write!(formatter, "output root is a file: {path}"),
            Self::Io { path, source } => write!(formatter, "{path}: {source}"),
            Self::Serialize { path, source } => write!(formatter, "{path}: {source}"),
            Self::InvalidReference {
                path,
                reference,
                message,
            } => write!(
                formatter,
                "{path}: invalid reference {reference}: {message}"
            ),
        }
    }
}

impl std::error::Error for ExportProjectError {}

mod serialization;
pub use serialization::{SourceTextWrite, write_source_texts};
use serialization::{document_text, write_source_documents};

mod loader;
use loader::Loader;
