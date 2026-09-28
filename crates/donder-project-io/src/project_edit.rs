use crate::serialization::{document_text, write_source_documents};
use crate::{ExportProjectError, ExportReport, ProjectSession, SaveReport, serialization};
use camino::{Utf8Path, Utf8PathBuf};
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

    // Export alone clones the session because external asset paths are rewritten
    // for the destination. Normal saves serialize the shared session directly.
    let mut synced = session.clone();
    let project_module_id = synced.source.project_module_id();
    for asset in &mut synced.source.referenced_assets {
        if asset.module_id != project_module_id {
            let file_name = asset.absolute_path.file_name().ok_or_else(|| {
                ExportProjectError::InvalidReference {
                    path: asset.absolute_path.clone(),
                    reference: asset.absolute_path.to_string(),
                    message: "external asset has no file name".to_string(),
                }
            })?;
            asset.relative_path = Utf8PathBuf::from("assets")
                .join(asset.id.0.to_string())
                .join(file_name);
            asset.module_id = project_module_id;
        }
    }
    let written_files = write_source_documents(&synced, output_root)?;

    let mut copied_assets = Vec::new();
    for (source_asset, exported_asset) in session
        .source
        .referenced_assets
        .iter()
        .zip(&synced.source.referenced_assets)
    {
        let output_path = output_root.join(&exported_asset.relative_path);
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
        copied_assets.push(exported_asset.relative_path.clone());
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
    document_id: &donder_language::identity::DocumentId,
) -> Result<Option<String>, ExportProjectError> {
    serialization::validate_source_inventory(session)?;
    let Some(document) = session.source.documents.get(document_id) else {
        return Ok(None);
    };
    document_text(session, document_id, document).map(Some)
}
