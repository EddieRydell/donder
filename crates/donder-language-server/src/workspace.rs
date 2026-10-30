//! The documents the server knows: the client's open texts over the project's
//! files on disk, and the last check of the project they form.
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use camino::{Utf8Path, Utf8PathBuf};
use donder_project_io::DocumentId;
use donder_project_io::{
    IoDiagnostic, PROJECT_ROOT_FILE, ProjectCheckCache, ProjectCheckReport, ProjectIndex,
    SourceOverrides, check_project,
};
use indexmap::IndexMap;

use crate::document::Document;
use crate::text::{file_uri, uri_path};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    Data,
    Script,
    Other,
}

pub(crate) fn kind(uri: &str) -> Kind {
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

/// A host that keeps its own copies of project documents and checks the
/// project itself, as the desktop does with its working copies. The server
/// then never checks: it publishes the host's latest check.
pub trait DocumentSource: Send {
    /// A document's text, by project-relative path.
    fn text(&self, relative: &Utf8Path) -> Option<String>;
    /// The host's latest project check.
    fn check(&self) -> Option<Arc<ProjectCheck>>;
}

/// A project check and the document texts it read over disk.
pub struct ProjectCheck {
    root: Utf8PathBuf,
    pub(crate) diagnostics: Vec<IoDiagnostic>,
    pub(crate) index: ProjectIndex,
    texts: Arc<SourceOverrides>,
}

impl ProjectCheck {
    /// The check `report` of the project at `root` with `texts` over disk.
    pub fn new(
        root: &Utf8Path,
        texts: Arc<SourceOverrides>,
        diagnostics: Vec<IoDiagnostic>,
        index: ProjectIndex,
    ) -> Self {
        Self {
            root: root.to_path_buf(),
            diagnostics,
            index,
            texts,
        }
    }
}

/// Who checks the project.
pub(crate) enum Checker {
    /// The server, after edits, reusing what earlier checks read.
    Own(ProjectCheckCache),
    /// The host, which also supplies the documents the client has not opened.
    Host(Arc<dyn DocumentSource + Sync>),
}

impl Default for Checker {
    fn default() -> Self {
        Self::Own(ProjectCheckCache::default())
    }
}

#[derive(Default)]
pub(crate) struct Workspace {
    pub checker: Checker,
    pub root: Option<Utf8PathBuf>,
    /// Open documents by URI.
    pub open: IndexMap<String, Arc<Document>>,
    /// Documents read for requests, by URI, until the files may have changed.
    read: RefCell<HashMap<String, Arc<Document>>>,
    pub check: Option<Arc<ProjectCheck>>,
    pub stale: bool,
    /// Documents with published diagnostics, to clear when they have none.
    pub published: BTreeSet<String>,
}

impl Workspace {
    pub(crate) fn with_checker(checker: Checker) -> Self {
        Self {
            checker,
            ..Self::default()
        }
    }

    /// The project-relative path of a URI inside the project.
    pub(crate) fn relative(&self, uri: &str) -> Option<Utf8PathBuf> {
        let root = normalized(self.root.as_ref()?);
        let path = normalized(&uri_path(uri)?);
        let relative = path.strip_prefix(&root)?.strip_prefix('/')?;
        Some(Utf8PathBuf::from(relative))
    }

    /// The URI of a project-relative path, as the client spelled it when the
    /// document is open.
    pub(crate) fn uri(&self, relative: &Utf8Path) -> Option<String> {
        if let Some(uri) = self
            .open
            .keys()
            .find(|uri| self.relative(uri).as_deref() == Some(relative))
        {
            return Some(uri.clone());
        }
        Some(file_uri(&self.root.as_ref()?.join(relative)))
    }

    pub(crate) fn document_uri(&self, document: &DocumentId) -> Option<String> {
        self.uri(document.path())
    }

    pub(crate) fn document_id(&self, uri: &str) -> Option<DocumentId> {
        self.check.as_ref()?;
        Some(DocumentId::new(self.relative(uri)?))
    }

    /// A document: the client's when open, otherwise the host's or the disk's.
    pub(crate) fn document(&self, uri: &str) -> Option<Arc<Document>> {
        if let Some(document) = self.open.get(uri) {
            return Some(Arc::clone(document));
        }
        if let Some(document) = self.read.borrow().get(uri) {
            return Some(Arc::clone(document));
        }
        let host = match &self.checker {
            Checker::Host(source) => self
                .relative(uri)
                .and_then(|relative| source.text(&relative)),
            Checker::Own(_) => None,
        };
        let text = host.or_else(|| std::fs::read_to_string(uri_path(uri)?).ok())?;
        let document = Arc::new(Document::new(kind(uri), text));
        self.read
            .borrow_mut()
            .insert(uri.to_string(), Arc::clone(&document));
        Some(document)
    }

    pub(crate) fn project_document(&self, document: &DocumentId) -> Option<Arc<Document>> {
        self.document(&self.document_uri(document)?)
    }

    /// Files may have changed outside the client.
    pub(crate) fn files_changed(&mut self) {
        self.read.get_mut().clear();
        self.stale = true;
    }

    /// Take the latest check: the host's, or a new one of the open texts
    /// when edits made the last one stale. Returns whether it changed.
    pub(crate) fn refresh(&mut self) -> bool {
        let texts = matches!(self.checker, Checker::Own(_))
            .then(|| self.open_texts())
            .filter(|_| self.stale);
        match &mut self.checker {
            Checker::Host(source) => {
                // A check of another project, such as the one being closed,
                // is not this session's.
                let latest = source.check().filter(|check| {
                    self.root
                        .as_deref()
                        .is_some_and(|root| normalized(root) == normalized(&check.root))
                });
                let changed = match (&latest, &self.check) {
                    (Some(latest), Some(current)) => !Arc::ptr_eq(latest, current),
                    (latest, current) => latest.is_some() != current.is_some(),
                };
                self.check = latest;
                if changed {
                    self.read.get_mut().clear();
                }
                changed
            }
            Checker::Own(cache) => {
                let Some(texts) = texts else {
                    return false;
                };
                self.stale = false;
                self.read.get_mut().clear();
                let Some(root) = self.root.clone() else {
                    self.check = None;
                    return true;
                };
                let ProjectCheckReport {
                    diagnostics, index, ..
                } = check_project(&root, &texts, cache);
                self.check = Some(Arc::new(ProjectCheck::new(
                    &root,
                    Arc::new(texts),
                    diagnostics,
                    index,
                )));
                true
            }
        }
    }

    /// The open documents' texts, by project-relative path.
    fn open_texts(&self) -> SourceOverrides {
        self.open
            .iter()
            .filter_map(|(uri, document)| {
                let relative = self.relative(uri)?;
                (relative.extension() == Some("donder"))
                    .then(|| (relative, document.text().to_string()))
            })
            .collect()
    }

    /// The last check, when every open document still has the text it
    /// checked, so its index locates names in the current texts.
    pub(crate) fn current_check(&self) -> Option<&ProjectCheck> {
        let check = self.check.as_deref()?;
        self.open
            .iter()
            .all(|(uri, document)| match self.relative(uri) {
                Some(relative) if relative.extension() == Some("donder") => check
                    .texts
                    .get(&relative)
                    .is_some_and(|text| text == document.text()),
                _ => true,
            })
            .then_some(check)
    }

    /// The project's diagnostics, by URI.
    pub(crate) fn project_diagnostics(&self) -> IndexMap<String, Vec<IoDiagnostic>> {
        let mut grouped = IndexMap::<String, Vec<IoDiagnostic>>::new();
        if let Some(check) = &self.check {
            for diagnostic in &check.diagnostics {
                if let Some(uri) = self.uri(&diagnostic.path) {
                    grouped.entry(uri).or_default().push(diagnostic.clone());
                }
            }
        }
        grouped
    }

    /// Whether a URI belongs to the checked project.
    pub(crate) fn in_project(&self, uri: &str) -> bool {
        self.relative(uri).is_some()
    }

    /// The project root holding `path`: its nearest ancestor with a root
    /// document.
    pub(crate) fn find_root(path: &Utf8Path) -> Option<Utf8PathBuf> {
        path.ancestors()
            .skip(1)
            .find(|directory| directory.join(PROJECT_ROOT_FILE).is_file())
            .map(Utf8Path::to_path_buf)
    }
}
