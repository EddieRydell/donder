//! Data documents read by earlier project checks. A check reads each data
//! document once, and a later check reuses the reading of every document whose
//! text is unchanged, so an edit to one document parses only that document.
use crate::document::{self, ParsedDocument};
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::Diagnostic;
use std::collections::HashMap;
use std::sync::Arc;

/// A data document's declarations and every syntax and schema error in it.
#[derive(Debug)]
pub(crate) struct ReadDocument {
    pub(crate) document: ParsedDocument,
    pub(crate) diagnostics: Vec<Diagnostic>,
}

/// Readings kept between checks. Entries a check did not use are dropped when
/// it finishes.
#[derive(Debug, Default)]
pub struct ProjectCheckCache {
    documents: HashMap<Utf8PathBuf, CachedDocument>,
}

#[derive(Debug)]
struct CachedDocument {
    text: String,
    read: Arc<ReadDocument>,
    used: bool,
}

impl ProjectCheckCache {
    /// The reading of `text`, the current text of the document at `path`.
    pub(crate) fn read(&mut self, path: &Utf8Path, text: &str) -> Arc<ReadDocument> {
        if let Some(cached) = self.documents.get_mut(path)
            && cached.text == text
        {
            cached.used = true;
            return Arc::clone(&cached.read);
        }
        let (document, diagnostics) = document::read(text);
        let read = Arc::new(ReadDocument {
            document,
            diagnostics,
        });
        self.documents.insert(
            path.to_path_buf(),
            CachedDocument {
                text: text.to_string(),
                read: Arc::clone(&read),
                used: true,
            },
        );
        read
    }

    pub(crate) fn finish(&mut self) {
        self.documents
            .retain(|_, cached| std::mem::take(&mut cached.used));
    }
}
