//! The documents the server knows: the client's open texts over the project's
//! files on disk, and the last check of the project they form.
use std::collections::BTreeSet;

use camino::{Utf8Path, Utf8PathBuf};
use donder_language::identity::DocumentId;
use donder_project_io::{
    IoDiagnostic, PROJECT_ROOT_FILE, ProjectCheckReport, ProjectMetadata, SourceOverrides,
    check_project_with_overrides,
};
use indexmap::IndexMap;

use crate::text::{file_uri, uri_path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Data,
    Script,
    Other,
}

pub fn kind(uri: &str) -> Kind {
    let path = uri.split(['?', '#']).next().unwrap_or(uri);
    if path.ends_with(donder_language::data::DATA_DOCUMENT_SUFFIX) {
        Kind::Data
    } else if path.ends_with(donder_language::data::SCRIPT_SUFFIX) {
        Kind::Script
    } else {
        Kind::Other
    }
}

/// A path as compared across clients: `/` separators, no verbatim prefix,
/// and a lowercase drive letter.
fn normalized(path: &Utf8Path) -> String {
    let text = path.as_str().replace('\\', "/");
    let text = text.strip_prefix("//?/").unwrap_or(&text).to_string();
    let mut characters = text.chars();
    match (characters.next(), characters.next()) {
        (Some(drive), Some(':')) => format!("{}{}", drive.to_ascii_lowercase(), &text[1..]),
        _ => text,
    }
}

/// The host's own copies of project documents, such as the desktop's unsaved
/// buffers. Documents open in the client take precedence; the rest of the
/// project is read from disk.
pub trait DocumentSource: Send {
    /// A document's text, by project-relative path.
    fn text(&self, relative: &Utf8Path) -> Option<String>;
    /// Every document whose text differs from disk.
    fn overrides(&self) -> Vec<(Utf8PathBuf, String)>;
}

pub struct Check {
    pub report: ProjectCheckReport,
    pub module: Option<uuid::Uuid>,
}

#[derive(Default)]
pub struct Workspace {
    pub source: Option<std::sync::Arc<dyn DocumentSource + Sync>>,
    pub root: Option<Utf8PathBuf>,
    /// Open documents by URI.
    pub open: IndexMap<String, String>,
    pub check: Option<Check>,
    pub stale: bool,
    /// Documents with published diagnostics, to clear when they have none.
    pub published: BTreeSet<String>,
}

impl Workspace {
    /// The project-relative path of a URI inside the project.
    pub fn relative(&self, uri: &str) -> Option<Utf8PathBuf> {
        let root = normalized(self.root.as_ref()?);
        let path = normalized(&uri_path(uri)?);
        let relative = path.strip_prefix(&root)?.strip_prefix('/')?;
        Some(Utf8PathBuf::from(relative))
    }

    /// The URI of a project-relative path, as the client spelled it when the
    /// document is open.
    pub fn uri(&self, relative: &Utf8Path) -> Option<String> {
        if let Some(uri) = self
            .open
            .keys()
            .find(|uri| self.relative(uri).as_deref() == Some(relative))
        {
            return Some(uri.clone());
        }
        Some(file_uri(&self.root.as_ref()?.join(relative)))
    }

    pub fn document_uri(&self, document: &DocumentId) -> Option<String> {
        self.uri(document.path())
    }

    pub fn document_id(&self, uri: &str) -> Option<DocumentId> {
        Some(DocumentId::new(
            self.check.as_ref()?.module?,
            self.relative(uri)?,
        ))
    }

    /// A document's text: the client's when open, otherwise from disk.
    pub fn text(&self, uri: &str) -> Option<String> {
        if let Some(text) = self.open.get(uri) {
            return Some(text.clone());
        }
        if let (Some(source), Some(relative)) = (&self.source, self.relative(uri))
            && let Some(text) = source.text(&relative)
        {
            return Some(text);
        }
        std::fs::read_to_string(uri_path(uri)?).ok()
    }

    pub fn document_text(&self, document: &DocumentId) -> Option<String> {
        self.text(&self.document_uri(document)?)
    }

    /// Recheck the project with the open texts.
    pub fn refresh(&mut self) {
        self.stale = false;
        let Some(root) = self.root.clone() else {
            self.check = None;
            return;
        };
        let mut overrides: SourceOverrides = self
            .source
            .as_ref()
            .map(|source| source.overrides().into_iter().collect())
            .unwrap_or_default();
        overrides.extend(self.open.iter().filter_map(|(uri, text)| {
            let relative = self.relative(uri)?;
            (relative.extension() == Some("donder")).then(|| (relative, text.clone()))
        }));
        overrides.retain(|relative, _| relative.extension() == Some("donder"));
        let report = check_project_with_overrides(&root, &overrides);
        let root_text = overrides
            .get(Utf8Path::new(PROJECT_ROOT_FILE))
            .cloned()
            .or_else(|| std::fs::read_to_string(root.join(PROJECT_ROOT_FILE)).ok());
        let module = root_text
            .and_then(|text| ProjectMetadata::parse(&text).ok())
            .map(|metadata| metadata.project_id);
        self.check = Some(Check { report, module });
    }

    /// The project's diagnostics, by URI.
    pub fn project_diagnostics(&self) -> IndexMap<String, Vec<IoDiagnostic>> {
        let mut grouped = IndexMap::<String, Vec<IoDiagnostic>>::new();
        if let Some(check) = &self.check {
            for diagnostic in &check.report.diagnostics {
                if let Some(uri) = self.uri(&diagnostic.path) {
                    grouped.entry(uri).or_default().push(diagnostic.clone());
                }
            }
        }
        grouped
    }

    /// Whether a URI belongs to the checked project.
    pub fn in_project(&self, uri: &str) -> bool {
        self.relative(uri).is_some()
    }

    /// The project root holding `path`: its nearest ancestor with a root
    /// document.
    pub fn find_root(path: &Utf8Path) -> Option<Utf8PathBuf> {
        path.ancestors()
            .skip(1)
            .find(|directory| directory.join(PROJECT_ROOT_FILE).is_file())
            .map(Utf8Path::to_path_buf)
    }
}
