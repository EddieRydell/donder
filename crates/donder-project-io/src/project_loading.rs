use crate::diagnostics::{push_diagnostic, read_error};
use crate::loader::Loader;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, PROJECT_ROOT_FILE, ProjectCheckCache,
    ProjectCheckReport, ProjectMetadata, ProjectWorkspace, analysis,
};
use camino::{Utf8Path, Utf8PathBuf};
use indexmap::IndexSet;
use std::collections::HashMap;
use std::{fs, io};

pub type SourceOverrides = std::collections::BTreeMap<Utf8PathBuf, String>;

pub fn project_source_texts(root: &Utf8Path) -> io::Result<SourceOverrides> {
    analysis::project_file_inventory(root)
        .into_iter()
        .filter(|path| path.extension() == Some("donder"))
        .map(|path| fs::read_to_string(root.join(&path)).map(|text| (path, text)))
        .collect()
}

/// Each document's text during one check, read once: its override, or the
/// file under the project root.
pub(crate) struct SourceTexts {
    root: Utf8PathBuf,
    texts: HashMap<Utf8PathBuf, io::Result<String>>,
}

impl SourceTexts {
    fn new(root: &Utf8Path, overrides: &SourceOverrides) -> Self {
        Self {
            root: root.to_path_buf(),
            texts: overrides
                .iter()
                .map(|(path, text)| (path.clone(), Ok(text.clone())))
                .collect(),
        }
    }

    /// The text of the document at project-relative `path`.
    pub(crate) fn text(&mut self, path: &Utf8Path) -> Result<&str, &io::Error> {
        let root = &self.root;
        self.texts
            .entry(path.to_path_buf())
            .or_insert_with(|| fs::read_to_string(root.join(path)))
            .as_deref()
    }

    /// Whether a document exists at `path`: overridden, read, or on disk.
    pub(crate) fn exists(&self, path: &Utf8Path) -> bool {
        self.texts
            .get(path)
            .map_or_else(|| self.root.join(path).is_file(), |text| text.is_ok())
    }

    /// The paths with text that is not a file's: overrides of documents.
    fn overridden(&self) -> impl Iterator<Item = &Utf8PathBuf> {
        self.texts.keys()
    }
}

/// Check the project at `root`: load it, and diagnose every document. Texts in
/// `overrides` stand in for the files at their paths. `cache` keeps the
/// documents read in earlier checks, so a caller that checks repeatedly, as an
/// editor does after each edit, parses only the documents that changed.
pub fn check_project(
    root: &Utf8Path,
    overrides: &SourceOverrides,
    cache: &mut ProjectCheckCache,
) -> ProjectCheckReport {
    let mut texts = SourceTexts::new(root, overrides);
    let mut diagnostics = Vec::new();
    let mut checked_scripts = IndexSet::new();
    let mut index = crate::ProjectIndex::default();
    let metadata = match texts.text(Utf8Path::new(PROJECT_ROOT_FILE)) {
        Ok(text) => ProjectMetadata::parse(text),
        Err(error) => Err(read_error(PROJECT_ROOT_FILE, error)),
    };
    let workspace = metadata.and_then(|metadata| {
        ProjectWorkspace::new(root, metadata).map_err(|message| {
            vec![IoDiagnostic::error(
                PROJECT_ROOT_FILE,
                None,
                IoDiagnosticCode::DonderLoad,
                message,
            )]
        })
    });
    let compiled = match workspace {
        Ok(workspace) => {
            let mut loader = Loader::new(workspace, texts, std::mem::take(cache));
            let result = loader.load();
            checked_scripts = std::mem::take(&mut loader.checked_scripts);
            index.links = loader.links.take().into_iter().collect();
            (texts, *cache) = loader.into_texts_and_cache();
            result
        }
        Err(errors) => Err(errors),
    };
    let compiled = compiled
        .map_err(|errors| {
            for diagnostic in errors {
                push_diagnostic(&mut diagnostics, diagnostic);
            }
        })
        .ok();
    let active_documents = compiled.as_ref().map(|compiled| {
        compiled
            .source
            .documents
            .keys()
            .map(|id| id.path().to_path_buf())
            .collect::<IndexSet<_>>()
    });
    let paths = analysis::project_file_inventory(root)
        .into_iter()
        .chain(texts.overridden().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let recovery = analysis::analyze_project_documents(
        root,
        paths,
        &mut texts,
        &checked_scripts,
        active_documents.as_ref(),
        cache,
        &mut diagnostics,
    );
    if let Some(compiled) = &compiled {
        crate::inclusion::unreferenced_documents(compiled, &recovery, &mut diagnostics);
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
