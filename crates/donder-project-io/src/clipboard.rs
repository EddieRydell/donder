//! Sequence items on the system clipboard, as project-language text: a data
//! document beside the project's that declares one `Sequence` holding them,
//! with the imports its references need. Copying prints it with the
//! canonical printer; pasting reads it as the loader reads any document.
use crate::address::DocumentId;
use crate::loader::Loader;
use crate::{DocumentTextCache, ProjectCheckCache, ProjectSession, SessionEditError};
use camino::Utf8PathBuf;
use donder_model::{Sequence, SequenceId};
use donder_runtime_types::Identifier;

/// The name the clipboard's sequence is declared under.
const DECLARED: &str = "clipboard";

/// `sequence` as the text of a document of its own, which refers to the
/// project's objects through imports.
pub fn write_sequence_clipboard(
    session: &ProjectSession,
    mut sequence: Sequence,
) -> Result<String, String> {
    let mut session = session.clone();
    let document = clipboard_document(&session);
    let name = Identifier::new(DECLARED.to_string()).map_err(|error| format!("{error:?}"))?;
    session
        .edit(|edit| {
            sequence.id = SequenceId(edit.project_mut().new_id());
            edit.add_data_document(
                document.path().to_path_buf(),
                vec![(sequence.id.into(), name)],
            )?;
            edit.project_mut().put(sequence)?;
            Ok::<_, SessionEditError>(())
        })
        .map_err(|error| error.to_string())?;
    crate::source_document_text(&session, &document)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| unreachable!("the clipboard document was just added"))
}

/// Whether `text` is sequence clipboard text: a document declaring one
/// sequence and nothing else.
pub fn holds_sequence_clipboard(text: &str) -> bool {
    let (parsed, diagnostics) = crate::document::read(text);
    diagnostics.is_empty()
        && matches!(
            parsed.declarations.as_slice(),
            [(_, _, crate::document::Declaration::Sequence(_))]
        )
}

/// The sequence clipboard `text` declares, its references resolved in
/// `session`: every object it names has the id it has in `session`. `texts`
/// keeps printed clips between calls. The caller checks
/// [`holds_sequence_clipboard`] first.
pub fn read_sequence_clipboard(
    session: &ProjectSession,
    text: &str,
    texts: &mut DocumentTextCache,
) -> Result<Sequence, String> {
    if !holds_sequence_clipboard(text) {
        return Err("Nothing to paste.".to_string());
    }
    let documents = session.source.documents.keys().cloned().collect::<Vec<_>>();
    let mut overrides = crate::source_document_texts(session, &documents, texts)
        .map_err(|error| error.to_string())?
        .into_iter()
        .zip(&documents)
        .filter_map(|(text, document)| Some((document.path().to_path_buf(), text?)))
        .collect::<crate::SourceOverrides>();
    let document = clipboard_document(session);
    overrides.insert(document.path().to_path_buf(), text.to_string());
    let mut loader = Loader::new(
        session.source.workspace.clone(),
        crate::project_loading::SourceTexts::new(session.source.project_root(), &overrides),
        ProjectCheckCache::default(),
    );
    loader
        .detached_sequence(session, &document)
        .map_err(|errors| {
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        })
}

/// A path beside the project's documents that none of them, and no file,
/// has.
fn clipboard_document(session: &ProjectSession) -> DocumentId {
    let path = |stem: &str| {
        Utf8PathBuf::from(format!(
            "{stem}{}",
            donder_language::data::DATA_DOCUMENT_SUFFIX
        ))
    };
    let stem = donder_language::unique_name(DECLARED, |stem| {
        let path = path(stem);
        session
            .source
            .documents
            .contains_key(&DocumentId::new(path.clone()))
            || session.source.project_root().join(&path).exists()
    });
    DocumentId::new(path(stem.as_str()))
}
