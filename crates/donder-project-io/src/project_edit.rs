pub use crate::document::text_cache::DocumentTextCache;
use crate::serialization::{document_text, write_source_documents};
use crate::{ExportProjectError, ExportReport, ProjectSession, SaveReport, serialization};
use camino::Utf8Path;
use std::fs;

pub fn export_project(
    session: &ProjectSession,
    output_root: &Utf8Path,
) -> Result<ExportReport, ExportProjectError> {
    if output_root.exists() && !output_root.is_dir() {
        return Err(ExportProjectError::OutputRootIsFile {
            path: output_root.to_path_buf(),
        });
    }
    fs::create_dir_all(output_root).map_err(|source| ExportProjectError::Io {
        path: output_root.to_path_buf(),
        source,
    })?;

    let written_files = write_source_documents(session, output_root)?;
    let mut copied_assets = Vec::new();
    for source_asset in &session.source.referenced_assets {
        let output_path = output_root.join(&source_asset.relative_path);
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).map_err(|source| ExportProjectError::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        fs::copy(&source_asset.absolute_path, &output_path).map_err(|source| {
            ExportProjectError::Io {
                path: output_path.clone(),
                source,
            }
        })?;
        copied_assets.push(source_asset.relative_path.clone());
    }

    Ok(ExportReport {
        written_files,
        copied_assets,
    })
}

pub fn save_project(session: &ProjectSession) -> Result<SaveReport, ExportProjectError> {
    let project_root = session.source.project_root();
    let written_files = write_source_documents(session, project_root)?;
    Ok(SaveReport { written_files })
}

pub fn source_document_text(
    session: &ProjectSession,
    document_id: &donder_model::DocumentId,
) -> Result<Option<String>, ExportProjectError> {
    Ok(source_document_texts(
        session,
        std::slice::from_ref(document_id),
        &mut DocumentTextCache::default(),
    )?
    .remove(0))
}

/// The canonical text of each of `documents`, `None` for one the project
/// does not have. `cache` keeps printed clips between calls, so a caller that
/// prints after every edit reprints only the clips that changed.
pub fn source_document_texts(
    session: &ProjectSession,
    documents: &[donder_model::DocumentId],
    cache: &mut DocumentTextCache,
) -> Result<Vec<Option<String>>, ExportProjectError> {
    serialization::validate_source_inventory(session)?;
    documents
        .iter()
        .map(|document_id| {
            session
                .source
                .documents
                .get(document_id)
                .map(|document| document_text(session, document_id, document, cache))
                .transpose()
        })
        .collect()
}
