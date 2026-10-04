use super::*;
use camino::Utf8PathBuf;
use donder_language::operator::{OperatorDefinitionId, custom_operator_definition};
use donder_project_io::{
    ProjectMetadata, ProjectWorkspace, SourceDocument, SourceDocumentKind, SourceObjectId,
    SourceObjectKind, SourceProject,
};
use donder_sequence_api::{BrowserSourceDocument, BrowserSourceKind};

pub(super) fn initial_session(
    project: DonderProject,
    root: &SourceIdentity,
    fixture: &FixtureDefinitionId,
    effect: &EffectDefinitionId,
    effect_source: &str,
) -> Result<ProjectSession, JsValue> {
    let mut source = SourceProject {
        workspace: ProjectWorkspace {
            root: Utf8PathBuf::from("/browser-demo"),
            metadata: ProjectMetadata {
                format_version: donder_project_io::PROJECT_FORMAT_VERSION,
                project_id: root.module_id(),
            },
        },
        entrypoint: Some(root.document_id().clone()),
        documents: IndexMap::new(),
        referenced_assets: Vec::new(),
    };
    let root_document = SourceDocument::new(
        Vec::new(),
        vec![
            SourceObjectId::new(SourceObjectKind::Project, root.object().into())
                .map_err(|error| JsValue::from_str(&error))?,
            SourceObjectId::new(
                SourceObjectKind::FixtureDefinition,
                fixture.0.object().into(),
            )
            .map_err(|error| JsValue::from_str(&error))?,
        ],
        SourceDocumentKind::Donder {
            original_value: yaml_serde::Value::Mapping(yaml_serde::Mapping::new()),
        },
    )
    .map_err(|error| JsValue::from_str(&error))?;
    source
        .documents
        .insert(root.document_id().clone(), root_document);
    register_document(
        &mut source,
        &effect.0,
        BrowserSourceKind::Effect,
        effect_source,
    )?;
    let mut session = ProjectSession { project, source };
    donder_project_io::ensure_document_can_reference_source(
        &mut session,
        root.document_id(),
        SourceObjectKind::EffectDefinition,
        &effect.0,
    )
    .map_err(|error| JsValue::from_str(&error.to_string()))?;
    Ok(session)
}

fn register_document(
    source: &mut SourceProject,
    identity: &SourceIdentity,
    kind: BrowserSourceKind,
    text: &str,
) -> Result<(), JsValue> {
    let (object_kind, document_kind) = match kind {
        BrowserSourceKind::Effect => (
            SourceObjectKind::EffectDefinition,
            SourceDocumentKind::Effect {
                source: text.into(),
            },
        ),
        BrowserSourceKind::Operator => (
            SourceObjectKind::OperatorDefinition,
            SourceDocumentKind::Operator {
                source: text.into(),
            },
        ),
    };
    let object = SourceObjectId::new(object_kind, identity.object().into())
        .map_err(|error| JsValue::from_str(&error))?;
    let document = SourceDocument::new(Vec::new(), vec![object], document_kind)
        .map_err(|error| JsValue::from_str(&error))?;
    source
        .documents
        .insert(identity.document_id().clone(), document);
    Ok(())
}

impl BrowserSession {
    fn update_source(
        &mut self,
        existing_path: Option<&str>,
        kind: BrowserSourceKind,
        text: &str,
    ) -> Result<JsValue, JsValue> {
        let (name, project_edit, identity) = match kind {
            BrowserSourceKind::Effect => {
                let mut definitions = match compile_effects(text) {
                    Ok(definitions) => definitions,
                    Err(diagnostics) => {
                        return js_value(&CompileView {
                            definitions: Vec::new(),
                            diagnostics: diagnostics_view(diagnostics)?,
                        });
                    }
                };
                if definitions.len() != 1 {
                    return Err(JsValue::from_str(
                        "A demo source document must contain one declaration.",
                    ));
                }
                let compiled = definitions.remove(0);
                let name = compiled.name().as_str().to_owned();
                let identity = self.source_identity(existing_path, &name, "effect")?;
                let id = EffectDefinitionId(identity.clone());
                let edit = ProjectEdit::SetEffectDefinition {
                    id: id.clone(),
                    value: EffectDefinition::custom(id, compiled),
                };
                (name, edit, identity)
            }
            BrowserSourceKind::Operator => {
                let mut definitions = match compile_operators(text) {
                    Ok(definitions) => definitions,
                    Err(diagnostics) => {
                        return js_value(&CompileView {
                            definitions: Vec::new(),
                            diagnostics: diagnostics_view(diagnostics)?,
                        });
                    }
                };
                if definitions.len() != 1 {
                    return Err(JsValue::from_str(
                        "A demo source document must contain one declaration.",
                    ));
                }
                let compiled = definitions.remove(0);
                let name = compiled.name().as_str().to_owned();
                let identity = self.source_identity(existing_path, &name, "operator")?;
                let id = OperatorDefinitionId(identity.clone());
                let edit = ProjectEdit::SetOperatorDefinition {
                    id: id.clone(),
                    value: custom_operator_definition(id, compiled),
                };
                (name, edit, identity)
            }
        };
        let mut candidate = (*self.session).clone();
        candidate
            .project
            .apply_edits([project_edit])
            .map_err(|error| JsValue::from_str(&error))?;
        register_document(&mut candidate.source, &identity, kind.clone(), text)?;
        let object_kind = match kind {
            BrowserSourceKind::Effect => SourceObjectKind::EffectDefinition,
            BrowserSourceKind::Operator => SourceObjectKind::OperatorDefinition,
        };
        donder_project_io::ensure_document_can_reference_source(
            &mut candidate,
            self.sequence_id.0.document_id(),
            object_kind,
            &identity,
        )
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.accept(candidate)?;
        js_value(&CompileView {
            definitions: vec![name],
            diagnostics: Vec::new(),
        })
    }

    fn source_identity(
        &self,
        existing_path: Option<&str>,
        name: &str,
        suffix: &str,
    ) -> Result<SourceIdentity, JsValue> {
        let document = DocumentId::new(
            self.session.source.project_module_id(),
            existing_path
                .map(Utf8PathBuf::from)
                .unwrap_or_else(|| Utf8PathBuf::from(format!("{name}.{suffix}.donder"))),
        );
        if let Some(source) = self.session.source.documents.get(&document) {
            if existing_path.is_none() {
                return Err(JsValue::from_str(
                    "A source document with that name already exists.",
                ));
            }
            if !source.objects().iter().any(|object| object.id() == name) {
                return Err(JsValue::from_str(
                    "Keep the source document's original declaration name.",
                ));
            }
        } else if existing_path.is_some() {
            return Err(JsValue::from_str("The source document was not found."));
        }
        Ok(SourceIdentity::from_document(document, name.into()))
    }
}

#[wasm_bindgen]
impl BrowserSession {
    #[wasm_bindgen(js_name = sourceDocuments)]
    pub fn source_documents(&self) -> Result<JsValue, JsValue> {
        let sources: Vec<_> = self
            .session
            .source
            .documents
            .iter()
            .filter_map(|(id, document)| {
                let (kind, source) = match document.kind() {
                    SourceDocumentKind::Effect { source } => (BrowserSourceKind::Effect, source),
                    SourceDocumentKind::Operator { source } => {
                        (BrowserSourceKind::Operator, source)
                    }
                    SourceDocumentKind::Donder { .. } => return None,
                };
                Some(BrowserSourceDocument {
                    path: id.path().to_string(),
                    kind,
                    source: source.clone(),
                })
            })
            .collect();
        js_value(&sources)
    }

    #[wasm_bindgen(js_name = addSource)]
    pub fn add_source(&mut self, kind: JsValue, source: &str) -> Result<JsValue, JsValue> {
        let kind: BrowserSourceKind = serde_wasm_bindgen::from_value(kind)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        self.update_source(None, kind, source)
    }

    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(&mut self, path: &str, source: &str) -> Result<JsValue, JsValue> {
        let document = self
            .session
            .source
            .documents
            .get(&self.session.source.project_document(path.into()))
            .ok_or_else(|| JsValue::from_str("The source document was not found."))?;
        let kind = match document.kind() {
            SourceDocumentKind::Effect { .. } => BrowserSourceKind::Effect,
            SourceDocumentKind::Operator { .. } => BrowserSourceKind::Operator,
            SourceDocumentKind::Donder { .. } => {
                return Err(JsValue::from_str(
                    "This document is not an effect or operator source.",
                ));
            }
        };
        self.update_source(Some(path), kind, source)
    }
}
