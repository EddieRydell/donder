use crate::diagnostics::{
    effect_diagnostics, load_error_diagnostic, operator_diagnostics, parse_yaml_value,
    push_load_error_diagnostics,
};
use crate::loader::Loader;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, LoadProjectError, PROJECT_CONFIG_FILE,
    ProjectCheckReport, ProjectConfig, ProjectSession, ProjectWorkspace, SourceDocumentFormat,
    analysis, source_document_format,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexSet;
use std::{fs, io};

#[derive(Debug)]
pub struct ProjectLoadError(pub Vec<IoDiagnostic>);
impl std::fmt::Display for ProjectLoadError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let messages = self
            .0
            .iter()
            .map(|diagnostic| {
                let location = diagnostic
                    .range
                    .as_ref()
                    .map(|range| format!(":{}:{}", range.start.line + 1, range.start.character + 1))
                    .unwrap_or_default();
                format!("{}{location}: {}", diagnostic.path, diagnostic.message)
            })
            .collect::<Vec<_>>();
        write!(formatter, "{}", messages.join("\n"))
    }
}
impl std::error::Error for ProjectLoadError {}

pub type SourceOverrides = std::collections::BTreeMap<Utf8PathBuf, String>;

pub fn project_source_texts(root: &Utf8Path) -> io::Result<SourceOverrides> {
    analysis::project_file_inventory(root)
        .into_iter()
        .filter(|path| path.extension() == Some("donder") || path.as_str() == PROJECT_CONFIG_FILE)
        .map(|path| fs::read_to_string(root.join(&path)).map(|text| (path, text)))
        .collect()
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
    let mut diagnostics = Vec::new();
    let config = overrides
        .get(Utf8Path::new(PROJECT_CONFIG_FILE))
        .map_or_else(
            || ProjectConfig::read(root),
            |text| ProjectConfig::parse(text),
        );
    let config = match config {
        Ok(config) => Some(config),
        Err(message) => {
            diagnostics.push(IoDiagnostic {
                path: PROJECT_CONFIG_FILE.into(),
                range: None,
                severity: IoDiagnosticSeverity::Error,
                code: IoDiagnosticCode::ProjectConfiguration,
                message,
                detail: None,
                related: Vec::new(),
            });
            None
        }
    };
    let mut checked_dsl_documents = IndexSet::new();
    let compiled = config.as_ref().and_then(|config| {
        let workspace = match ProjectWorkspace::new(root, config.clone()) {
            Ok(workspace) => workspace,
            Err(message) => {
                diagnostics.push(IoDiagnostic {
                    path: PROJECT_CONFIG_FILE.into(),
                    range: None,
                    severity: IoDiagnosticSeverity::Error,
                    code: IoDiagnosticCode::DonderLoad,
                    message,
                    detail: None,
                    related: Vec::new(),
                });
                return None;
            }
        };
        let result = Loader::new(workspace).and_then(|mut loader| {
            for (path, text) in overrides {
                if path.extension() == Some("donder") {
                    loader.source_overrides.insert(
                        donder_language::identity::DocumentId::new(config.project_id, path.clone()),
                        text.clone(),
                    );
                }
            }
            let result = loader.load();
            checked_dsl_documents = loader.checked_dsl_documents;
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
        config,
        overrides,
        &checked_dsl_documents,
        active_documents.as_ref(),
        &mut diagnostics,
    );
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
    }
}

pub fn check_document_text(path: &Utf8Path, text: &str) -> Vec<IoDiagnostic> {
    match source_document_format(path) {
        SourceDocumentFormat::Effect => return effect_diagnostics(path, text),
        SourceDocumentFormat::Operator => return operator_diagnostics(path, text),
        SourceDocumentFormat::Donder => return analysis::check_donder_document_text(path, text),
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
    document: &donder_language::identity::DocumentId,
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
