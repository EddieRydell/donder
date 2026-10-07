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
        ],
        SourceDocumentKind::Donder {
            original_value: yaml_serde::Value::Mapping(yaml_serde::Mapping::new()),
        },
    )
    .map_err(|error| JsValue::from_str(&error))?;
    source
        .documents
        .insert(root.document_id().clone(), root_document);
    Ok(ProjectSession { project, source })
}

#[derive(Clone, Copy)]
pub(super) enum SourceInstall {
    Create,
    Replace,
}

pub(super) enum SourceOutcome {
    Compiled(Vec<String>),
    Rejected(Vec<DiagnosticView>),
}

impl SourceOutcome {
    pub(super) fn into_result(self, path: &str) -> Result<Vec<String>, JsValue> {
        match self {
            Self::Compiled(names) => Ok(names),
            Self::Rejected(diagnostics) => Err(JsValue::from_str(&format!(
                "{path} does not compile:\n{}",
                diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.message)
                    .collect::<Vec<_>>()
                    .join("\n")
            ))),
        }
    }

    fn view(self) -> CompileView {
        match self {
            Self::Compiled(definitions) => CompileView {
                definitions,
                diagnostics: Vec::new(),
            },
            Self::Rejected(diagnostics) => CompileView {
                definitions: Vec::new(),
                diagnostics,
            },
        }
    }
}

fn object_kind(kind: &BrowserSourceKind) -> SourceObjectKind {
    match kind {
        BrowserSourceKind::Effect => SourceObjectKind::EffectDefinition,
        BrowserSourceKind::Operator => SourceObjectKind::OperatorDefinition,
    }
}

fn source_kind(document: &SourceDocument) -> Option<BrowserSourceKind> {
    match document.kind() {
        SourceDocumentKind::Effect { .. } => Some(BrowserSourceKind::Effect),
        SourceDocumentKind::Operator { .. } => Some(BrowserSourceKind::Operator),
        SourceDocumentKind::Donder { .. } => None,
    }
}

/// Compile a source document and replace every declaration it owns. Declarations
/// removed from the text are removed from the project; the project check rejects
/// removals that the sequence still uses.
pub(super) fn install_source(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    path: &str,
    kind: BrowserSourceKind,
    text: &str,
    install: SourceInstall,
) -> Result<SourceOutcome, JsValue> {
    let suffix = match kind {
        BrowserSourceKind::Effect => ".effect.donder",
        BrowserSourceKind::Operator => ".operator.donder",
    };
    if !path.ends_with(suffix) {
        return Err(JsValue::from_str(&format!(
            "Source path {path} must end with {suffix}."
        )));
    }
    donder_project_io::validate_relative_path(path).map_err(|error| JsValue::from_str(&error))?;
    let document = session.source.project_document(path.into());
    let previous = match (install, session.source.documents.get(&document)) {
        (SourceInstall::Create, Some(_)) => {
            return Err(JsValue::from_str(&format!("{path} already exists.")));
        }
        (SourceInstall::Replace, None) => {
            return Err(JsValue::from_str(&format!("{path} was not found.")));
        }
        (SourceInstall::Create, None) => Vec::new(),
        (SourceInstall::Replace, Some(existing)) => existing
            .objects()
            .iter()
            .map(|object| object.id().to_owned())
            .collect(),
    };
    let identity = |name: &str| SourceIdentity::from_document(document.clone(), name.into());
    let (names, mut edits) = match kind {
        BrowserSourceKind::Effect => match compile_effects(text) {
            Err(diagnostics) => return Ok(SourceOutcome::Rejected(diagnostics_view(diagnostics)?)),
            Ok(compiled) => {
                let names = compiled
                    .iter()
                    .map(|effect| effect.name().as_str().to_owned())
                    .collect::<Vec<_>>();
                let edits = compiled
                    .into_iter()
                    .map(|effect| {
                        let id = EffectDefinitionId(identity(effect.name().as_str()));
                        ProjectEdit::SetEffectDefinition {
                            id: id.clone(),
                            value: EffectDefinition::custom(id, effect),
                        }
                    })
                    .collect::<Vec<_>>();
                (names, edits)
            }
        },
        BrowserSourceKind::Operator => match compile_operators(text) {
            Err(diagnostics) => return Ok(SourceOutcome::Rejected(diagnostics_view(diagnostics)?)),
            Ok(compiled) => {
                let names = compiled
                    .iter()
                    .map(|operator| operator.name().as_str().to_owned())
                    .collect::<Vec<_>>();
                let edits = compiled
                    .into_iter()
                    .map(|operator| {
                        let id = OperatorDefinitionId(identity(operator.name().as_str()));
                        ProjectEdit::SetOperatorDefinition {
                            id: id.clone(),
                            value: custom_operator_definition(id, operator),
                        }
                    })
                    .collect::<Vec<_>>();
                (names, edits)
            }
        },
    };
    edits.extend(
        previous
            .iter()
            .filter(|name| !names.contains(name))
            .map(|name| match kind {
                BrowserSourceKind::Effect => {
                    ProjectEdit::RemoveEffectDefinition(EffectDefinitionId(identity(name)))
                }
                BrowserSourceKind::Operator => {
                    ProjectEdit::RemoveOperatorDefinition(OperatorDefinitionId(identity(name)))
                }
            }),
    );
    if let Err(message) = session.project.apply_edits(edits) {
        return Ok(SourceOutcome::Rejected(vec![DiagnosticView {
            start: 0,
            end: 0,
            message,
        }]));
    }
    let objects = names
        .iter()
        .map(|name| SourceObjectId::new(object_kind(&kind), name.as_str().into()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| JsValue::from_str(&error))?;
    let document_kind = match kind {
        BrowserSourceKind::Effect => SourceDocumentKind::Effect {
            source: text.into(),
        },
        BrowserSourceKind::Operator => SourceDocumentKind::Operator {
            source: text.into(),
        },
    };
    let source_document = SourceDocument::new(Vec::new(), objects, document_kind)
        .map_err(|error| JsValue::from_str(&error))?;
    session
        .source
        .documents
        .insert(document.clone(), source_document);
    for name in &names {
        donder_project_io::ensure_document_can_reference_source(
            session,
            sequence_id.0.document_id(),
            object_kind(&kind),
            &identity(name),
        )
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    }
    Ok(SourceOutcome::Compiled(names))
}

impl BrowserSession {
    /// Replace an existing document's text. Invalid source keeps the last
    /// accepted project and playback.
    pub(super) fn replace_source(
        &mut self,
        path: &str,
        text: &str,
    ) -> Result<CompileView, JsValue> {
        let kind = self
            .session
            .source
            .documents
            .get(&self.session.source.project_document(path.into()))
            .ok_or_else(|| JsValue::from_str(&format!("{path} was not found.")))
            .map(source_kind)?
            .ok_or_else(|| JsValue::from_str(&format!("{path} is not DSL source.")))?;
        let mut candidate = (*self.session).clone();
        let outcome = install_source(
            &mut candidate,
            &self.sequence_id,
            path,
            kind,
            text,
            SourceInstall::Replace,
        )?;
        if matches!(outcome, SourceOutcome::Compiled(_)) {
            self.accept(candidate)?;
        }
        Ok(outcome.view())
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
                let source = match document.kind() {
                    SourceDocumentKind::Effect { source }
                    | SourceDocumentKind::Operator { source } => source,
                    SourceDocumentKind::Donder { .. } => return None,
                };
                Some(BrowserSourceDocument {
                    path: id.path().to_string(),
                    kind: source_kind(document)?,
                    source: source.clone(),
                })
            })
            .collect();
        js_value(&sources)
    }

    /// Replace an existing document's text. Invalid source returns diagnostics
    /// and keeps the last accepted project and playback.
    #[wasm_bindgen(js_name = setSource)]
    pub fn set_source(&mut self, path: &str, source: &str) -> Result<JsValue, JsValue> {
        js_value(&self.replace_source(path, source)?)
    }
}

/// Split a DSL source into one document per declaration, named
/// `<Name>.effect.donder` or `<Name>.operator.donder`. Every document carries
/// the source's functions, which any declaration may call. Other text between
/// declarations (comments) is not part of any document.
#[wasm_bindgen(js_name = declarationSources)]
pub fn declaration_sources(source: &str) -> Result<JsValue, JsValue> {
    let messages = |diagnostics: Vec<donder_language::dsl::Diagnostic>| {
        JsValue::from_str(
            &diagnostics
                .into_iter()
                .map(|diagnostic| diagnostic.message)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    };
    let declarations = donder_language::dsl::declaration_spans(source).map_err(messages)?;
    let functions = donder_language::dsl::function_spans(source)
        .map_err(messages)?
        .into_iter()
        .map(|span| {
            source
                .get(span.start..span.end)
                .map(|text| format!("{text}\n\n"))
                .ok_or_else(|| JsValue::from_str("Function span is outside its source."))
        })
        .collect::<Result<String, JsValue>>()?;
    let documents = declarations
        .into_iter()
        .map(|declaration| {
            let (kind, suffix) = match declaration.kind {
                donder_language::dsl::DeclarationKind::Effect => {
                    (BrowserSourceKind::Effect, "effect")
                }
                donder_language::dsl::DeclarationKind::Operator => {
                    (BrowserSourceKind::Operator, "operator")
                }
            };
            let text = source
                .get(declaration.span.start..declaration.span.end)
                .ok_or_else(|| JsValue::from_str("Declaration span is outside its source."))?;
            Ok(BrowserSourceDocument {
                path: format!("{}.{suffix}.donder", declaration.name.as_str()),
                kind,
                source: format!("{functions}{text}\n"),
            })
        })
        .collect::<Result<Vec<_>, JsValue>>()?;
    js_value(&documents)
}
