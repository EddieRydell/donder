use crate::diagnostics::{
    data_diagnostics, load_error_diagnostic, push_load_error_diagnostics, script_diagnostics,
};
use crate::loader::Loader;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, PROJECT_ROOT_FILE, ProjectCheckCache,
    ProjectCheckReport, ProjectMetadata, ProjectSession, ProjectWorkspace, SourceDocumentFormat,
    analysis, source_document_format,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexSet;
use std::collections::BTreeMap;
use std::{fs, io};

#[derive(Debug)]
pub struct ProjectLoadError(pub Vec<IoDiagnostic>);
impl std::fmt::Display for ProjectLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let messages = self.0.iter().map(ToString::to_string).collect::<Vec<_>>();
        write!(formatter, "{}", messages.join("\n"))
    }
}
impl std::error::Error for ProjectLoadError {}

pub type SourceOverrides = std::collections::BTreeMap<Utf8PathBuf, String>;

pub fn project_source_texts(root: &Utf8Path) -> io::Result<SourceOverrides> {
    analysis::project_file_inventory(root)
        .into_iter()
        .filter(|path| path.extension() == Some("donder"))
        .map(|path| fs::read_to_string(root.join(&path)).map(|text| (path, text)))
        .collect()
}

/// A project held in memory instead of a directory: its documents by
/// project-relative path, and where each asset it references is found.
#[derive(Clone, Debug, Default)]
pub struct MemoryProject {
    pub documents: SourceOverrides,
    pub assets: BTreeMap<Utf8PathBuf, Utf8PathBuf>,
}

/// Load a project from memory. `root` stands in for the project directory;
/// nothing is read from disk.
pub fn load_memory_project(
    root: &Utf8Path,
    project: &MemoryProject,
) -> Result<ProjectSession, ProjectLoadError> {
    let mut diagnostics = Vec::new();
    let loaded = project
        .documents
        .get(Utf8Path::new(PROJECT_ROOT_FILE))
        .ok_or_else(|| crate::LoadProjectError::InvalidDocument {
            path: PROJECT_ROOT_FILE.into(),
            range: None,
            message: "the project has no root document".into(),
        })
        .and_then(|text| ProjectMetadata::parse(text))
        .and_then(|metadata| {
            let workspace = ProjectWorkspace {
                root: root.to_path_buf(),
                metadata,
            };
            let mut loader = Loader::new(workspace)?;
            for (path, text) in &project.documents {
                loader.source_overrides.insert(
                    donder_model::DocumentId::new(
                        loader.workspace.metadata.project_id,
                        path.clone(),
                    ),
                    text.clone(),
                );
            }
            loader.asset_locations = Some(project.assets.clone());
            loader.load()
        });
    match loaded {
        Ok(session) => Ok(session),
        Err(error) => {
            push_load_error_diagnostics(&mut diagnostics, error);
            Err(ProjectLoadError(diagnostics))
        }
    }
}

pub fn load_project(root: &Utf8Path) -> Result<ProjectSession, ProjectLoadError> {
    let report = check_project(root);
    report.session.ok_or(ProjectLoadError(report.diagnostics))
}

pub fn check_project(root: &Utf8Path) -> ProjectCheckReport {
    check_project_with_overrides(root, &SourceOverrides::new())
}

pub fn check_project_with_overrides(
    root: &Utf8Path,
    overrides: &SourceOverrides,
) -> ProjectCheckReport {
    check_project_cached(root, overrides, &mut ProjectCheckCache::default())
}

/// [`check_project_with_overrides`], reusing the documents `cache` read in
/// earlier checks. A caller that checks repeatedly, as an editor does after
/// each edit, keeps one cache so unchanged documents are not parsed again.
pub fn check_project_cached(
    root: &Utf8Path,
    overrides: &SourceOverrides,
    cache: &mut ProjectCheckCache,
) -> ProjectCheckReport {
    let mut diagnostics = Vec::new();
    let config = overrides.get(Utf8Path::new(PROJECT_ROOT_FILE)).map_or_else(
        || ProjectMetadata::read(root),
        |text| ProjectMetadata::parse(text),
    );
    let config = match config {
        Ok(config) => Some(config),
        Err(error) => {
            push_load_error_diagnostics(&mut diagnostics, error);
            None
        }
    };
    let mut checked_scripts = IndexSet::new();
    let mut index = crate::ProjectIndex::default();
    let compiled = config.as_ref().and_then(|config| {
        let workspace = match ProjectWorkspace::new(root, config.clone()) {
            Ok(workspace) => workspace,
            Err(message) => {
                diagnostics.push(IoDiagnostic {
                    path: PROJECT_ROOT_FILE.into(),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::DonderLoad,
                    message,
                    detail: None,
                    fix: None,
                    related: Vec::new(),
                });
                return None;
            }
        };
        let result = Loader::new(workspace).and_then(|mut loader| {
            loader.cache = std::mem::take(cache);
            for (path, text) in overrides {
                if path.extension() == Some("donder") {
                    loader.source_overrides.insert(
                        donder_model::DocumentId::new(config.project_id, path.clone()),
                        text.clone(),
                    );
                }
            }
            let result = loader.load();
            *cache = std::mem::take(&mut loader.cache);
            checked_scripts = loader.checked_scripts;
            index.links = loader.links.into_inner().into_iter().collect();
            result
        });
        match result {
            Ok(compiled) => Some(compiled),
            Err(error) => {
                push_load_error_diagnostics(&mut diagnostics, error);
                None
            }
        }
    });
    let active_documents = compiled.as_ref().map(|compiled| {
        compiled
            .source
            .documents
            .keys()
            .map(|id| id.path().to_path_buf())
            .collect::<IndexSet<_>>()
    });
    let recovery = analysis::analyze_project_documents(
        root,
        overrides,
        &checked_scripts,
        active_documents.as_ref(),
        cache,
        &mut diagnostics,
    );
    if let (Some(compiled), Some(config)) = (&compiled, &config) {
        crate::inclusion::unreferenced_documents(
            compiled,
            config.project_id,
            recovery.documents.keys().cloned(),
            |path| {
                overrides
                    .get(path)
                    .cloned()
                    .or_else(|| fs::read_to_string(root.join(path)).ok())
            },
            cache,
            &mut diagnostics,
        );
    }
    cache.finish();
    analysis::sort_diagnostics(&mut diagnostics);
    let session = compiled.filter(|_| {
        !diagnostics
            .iter()
            .any(|d| d.severity == IoDiagnosticSeverity::Error)
    });
    ProjectCheckReport {
        session,
        recovery,
        diagnostics,
        index,
    }
}

pub fn check_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match source_document_format(path) {
        SourceDocumentFormat::Script => script_diagnostics(path, text),
        SourceDocumentFormat::Data => data_diagnostics(path, text),
        SourceDocumentFormat::Other => Vec::new(),
    }
}

pub fn check_project_document_text(
    session: &ProjectSession,
    document: &donder_model::DocumentId,
    text: &str,
) -> Vec<IoDiagnostic> {
    let local_diagnostics = check_document_text(document.path(), text);
    if local_diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.code,
            IoDiagnosticCode::DataSyntax | IoDiagnosticCode::ScriptCompile
        )
    }) {
        return local_diagnostics;
    }

    let mut loader = match Loader::new(session.source.workspace.clone()) {
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
