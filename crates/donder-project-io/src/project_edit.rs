use crate::serialization::{document_text, write_source_documents};
use crate::{
    ExportProjectError, ExportReport, ProjectSession, SaveReport, SourceDocument,
    SourceDocumentKind, SourceObjectId, SourceObjectKind, ensure_document_can_reference_source,
    serialization,
};
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::dsl::Identifier;
use donder_language::identity::SourceIdentity;
use donder_language::sequence::{
    CompositionGraphNode, CompositionGraphNodeId, CompositionGraphNodeKind, EffectGraphEdge,
    GraphNodePosition, GraphPortId, MarkCollection, MarkCollectionKey, Sequence, SequenceAudio,
    SequenceCompositionGraph, SequenceId, SequenceLayer, SequenceLayerId,
};
use donder_language::values::{Color, DonderDuration};
use std::fs;
use yaml_serde::{Mapping, Value};

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

pub fn insert_sequence(
    session: &mut ProjectSession,
    path: Utf8PathBuf,
    object_key: String,
    duration: DonderDuration,
    frame_rate: u32,
) -> Result<SequenceId, ExportProjectError> {
    if !is_module_relative_path(&path)
        || !path.starts_with("sequences")
        || !path
            .file_name()
            .is_some_and(|name| name.ends_with(".sequence.donder"))
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence path must be an owned .sequence.donder document under sequences/"
                .to_string(),
        });
    }
    if Identifier::new(object_key.clone()).is_err()
        || !duration.as_seconds_f32().is_finite()
        || duration.as_seconds_f32() <= 0.0
        || frame_rate == 0
    {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence identity, duration, or frame rate is invalid".to_string(),
        });
    }
    let document_id = session.source.project_document(path.clone());
    let project_root = session.source.project_root();
    if session.source.documents.contains_key(&document_id) || project_root.join(&path).exists() {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "source document already exists".to_string(),
        });
    }
    let identity = SourceIdentity::from_document(document_id.clone(), object_key.clone());
    let id = SequenceId(identity.clone());
    if session.project.sequences.contains_key(&id) {
        return Err(ExportProjectError::InvalidReference {
            path,
            reference: object_key,
            message: "sequence already exists".to_string(),
        });
    }
    let layer_id = SequenceLayerId(0);
    let sequence = Sequence {
        id: id.clone(),
        duration,
        frame_rate,
        audio: SequenceAudio::None,
        mark_collections: vec![MarkCollection {
            key: MarkCollectionKey {
                name: "marks".to_string(),
            },
            name: "Marks".to_string(),
            display_color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            marks: Vec::new(),
        }],
        layers: vec![SequenceLayer {
            id: layer_id.clone(),
            name: "Default".to_string(),
            color: Color {
                red: 56,
                green: 189,
                blue: 248,
            },
            enabled: true,
        }],
        effects: Vec::new(),
        composition_graph: SequenceCompositionGraph {
            nodes: vec![
                CompositionGraphNode {
                    id: CompositionGraphNodeId(1),
                    position: GraphNodePosition { x: 80.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Layer { layer_id },
                },
                CompositionGraphNode {
                    id: CompositionGraphNodeId(2),
                    position: GraphNodePosition { x: 420.0, y: 80.0 },
                    kind: CompositionGraphNodeKind::Output,
                },
            ],
            edges: vec![EffectGraphEdge {
                from: CompositionGraphNodeId(1),
                from_port: GraphPortId("output".to_string()),
                to: CompositionGraphNodeId(2),
                to_port: GraphPortId("input".to_string()),
            }],
        },
        automation_clips: Vec::new(),
    };
    let source_document = SourceDocument::new(
        Vec::new(),
        vec![SourceObjectId {
            kind: SourceObjectKind::Sequence,
            id: identity.object().to_string(),
        }],
        SourceDocumentKind::Donder {
            original_value: Value::Mapping(Mapping::new()),
        },
    )
    .map_err(|message| ExportProjectError::InvalidReference {
        path: path.clone(),
        reference: object_key.clone(),
        message,
    })?;
    session
        .source
        .documents
        .insert(document_id, source_document);
    session.project.sequences.insert(id.clone(), sequence);
    session.project.root.sequences.push(id.clone());
    let entrypoint =
        session
            .source
            .entrypoint
            .clone()
            .ok_or_else(|| ExportProjectError::InvalidReference {
                path: path.clone(),
                reference: object_key.clone(),
                message: "active project has no manifest entrypoint".to_string(),
            })?;
    ensure_document_can_reference_source(
        session,
        &entrypoint,
        SourceObjectKind::Sequence,
        &identity,
    )?;
    Ok(id)
}

fn is_module_relative_path(path: &Utf8Path) -> bool {
    !path.as_str().is_empty()
        && !path.is_absolute()
        && !path.as_str().contains('\\')
        && path
            .components()
            .all(|component| matches!(component, camino::Utf8Component::Normal(_)))
}
