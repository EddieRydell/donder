//! The files objects of their own are saved in: a folder per kind, and a
//! file named after the one object it declares.
use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{ObjectId, ObjectKind};
use donder_runtime_types::Identifier;

use crate::{DocumentId, SourceProject};

/// The folder an object of `kind` is saved in, and the name a new one starts
/// from.
fn folder(kind: ObjectKind) -> (&'static str, &'static str) {
    match kind {
        ObjectKind::Project => ("projects", "project"),
        ObjectKind::Layout => ("layouts", "layout"),
        ObjectKind::Controller => ("controllers", "controller"),
        ObjectKind::InputDevice => ("input_devices", "input_device"),
        ObjectKind::Sequence => ("sequences", "sequence"),
        ObjectKind::Console => ("consoles", "console"),
        ObjectKind::FixtureDefinition => ("fixtures", "fixture"),
        ObjectKind::Curve => ("curves", "curve"),
        ObjectKind::Gradient => ("gradients", "gradient"),
        ObjectKind::Script => ("scripts", "script"),
    }
}

/// The file an object of `kind` named `name` has of its own.
pub fn object_document_path(kind: ObjectKind, name: &Identifier) -> Utf8PathBuf {
    Utf8PathBuf::from(format!(
        "{}/{}{}",
        folder(kind).0,
        name.as_str(),
        donder_language::data::DATA_DOCUMENT_SUFFIX
    ))
}

impl SourceProject {
    /// Whether the project has, or the disk holds, a file at `path`.
    pub fn has_document_at(&self, path: &Utf8Path) -> bool {
        self.documents
            .contains_key(&self.project_document(path.to_path_buf()))
            || self.project_root().join(path).exists()
    }

    /// The name a new object of `kind` is offered: its kind, numbered when a
    /// file of that name exists.
    pub fn new_object_name(&self, kind: ObjectKind) -> Identifier {
        donder_language::unique_name(folder(kind).1, |name| {
            Identifier::new(name.to_string())
                .is_ok_and(|name| self.has_document_at(&object_document_path(kind, &name)))
        })
    }

    /// A file of its own declaring `id` as `name`; refused when that file
    /// exists. The session edit inserts its typed value beside it.
    pub(crate) fn add_object_document(
        &mut self,
        id: ObjectId,
        name: Identifier,
    ) -> Result<DocumentId, String> {
        let path = object_document_path(id.kind(), &name);
        if self.has_document_at(&path) {
            return Err(format!("The file {path} already exists."));
        }
        self.add_data_document(path, vec![(id, name)])
    }
}
