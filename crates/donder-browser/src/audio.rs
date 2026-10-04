use super::*;
use camino::Utf8PathBuf;
use donder_language::sequence::{AssetId, SequenceAudio};
use donder_project_io::ReferencedAsset;
use std::collections::BTreeSet;

/// Attach website-served audio to the sequence. The URL stands in for the
/// asset's resolved path; the browser host plays and decodes it.
pub(super) fn set_audio(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    url: &str,
) -> Result<(), JsValue> {
    let file_name = url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .ok_or_else(|| JsValue::from_str("Audio URL must end with a file name."))?;
    let document = sequence_id.0.document_id().clone();
    let id = AssetId(1);
    session.source.referenced_assets.push(ReferencedAsset {
        id: id.clone(),
        module_id: document.module_id(),
        relative_path: Utf8PathBuf::from(file_name),
        absolute_path: Utf8PathBuf::from(url),
        referenced_by: BTreeSet::from([document]),
    });
    let mut sequence = session
        .project
        .sequence(sequence_id)
        .cloned()
        .ok_or_else(|| JsValue::from_str("The demo sequence was not found."))?;
    sequence.audio = SequenceAudio::Asset(id);
    session
        .project
        .replace_sequence(sequence_id, sequence)
        .map_err(|error| JsValue::from_str(&error))
}
