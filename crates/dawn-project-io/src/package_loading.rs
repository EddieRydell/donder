use crate::diagnostics::{
    effect_diagnostics, load_error_diagnostic, operator_diagnostics, parse_yaml_value,
    push_diagnostic, push_load_error_diagnostics,
};
use crate::loader::Loader;
use crate::source;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, LoadProjectError, ProjectCheckReport,
    ProjectRecovery, ProjectSession, SourceDocumentFormat, SourceProject, analysis,
    source_document_format,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexSet;
use std::{fs, io};

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
    let source_graph = dawn_package::ResolvedSourceGraph::from_lock(
        root,
        manifest.clone(),
        &lockfile,
        Some(cache),
    )?;
    let graph = compile_source_graph(source_graph)?;
    Ok(CompiledPackage {
        manifest,
        lockfile,
        graph,
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
                match package_cache_for_lock(lockfile).and_then(|cache| {
                    dawn_package::ResolvedSourceGraph::from_lock(
                        root,
                        manifest.clone(),
                        lockfile,
                        cache.as_ref(),
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
    let source_graph = match dawn_package::ResolvedSourceGraph::from_lock(
        root,
        manifest,
        &lockfile,
        Some(cache),
    ) {
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
    lockfile: &dawn_package::Lockfile,
) -> Result<Option<dawn_package::CacheStore>, dawn_package::PackageError> {
    if lockfile.packages.is_empty() {
        return Ok(None);
    }
    Ok(Some(
        dawn_package::DawnDirectories::discover()?.package_cache(),
    ))
}

pub fn check_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match source_document_format(path) {
        SourceDocumentFormat::Effect => return effect_diagnostics(path, text),
        SourceDocumentFormat::Operator => return operator_diagnostics(path, text),
        SourceDocumentFormat::Dawn => return analysis::check_dawn_document_text(path, text),
        SourceDocumentFormat::Other => {}
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
