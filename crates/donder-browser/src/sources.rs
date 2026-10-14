use super::*;
use donder_model::{OperatorDefinitionId, custom_operator_definition};
use donder_project_io::{SourceDocument, SourceDocumentKind, SourceObjectId, SourceObjectKind};
use donder_sequence_api::{BrowserSourceDocument, BrowserSourceKind};

pub(super) enum SourceOutcome {
    Compiled(Vec<String>),
    Rejected(Vec<DiagnosticView>),
}

impl SourceOutcome {
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

/// Each browser script holds one kind of declaration.
fn source_kind(document: &SourceDocument) -> Option<BrowserSourceKind> {
    match document.kind() {
        SourceDocumentKind::Script { .. } => Some(
            if document
                .objects()
                .iter()
                .all(|object| object.kind() == &SourceObjectKind::OperatorDefinition)
            {
                BrowserSourceKind::Operator
            } else {
                BrowserSourceKind::Effect
            },
        ),
        SourceDocumentKind::Data => None,
    }
}

/// Compile a source document and replace every declaration it owns. Declarations
/// removed from the text are removed from the project; the project check rejects
/// removals that the sequence still uses.
fn install_source(
    session: &mut ProjectSession,
    sequence_id: &SequenceId,
    path: &str,
    kind: BrowserSourceKind,
    text: &str,
) -> Result<SourceOutcome, JsValue> {
    if donder_project_io::source_document_format(path.into())
        != donder_project_io::SourceDocumentFormat::Script
    {
        return Err(JsValue::from_str(&format!(
            "Source path {path} must be a script, ending in {}.",
            donder_language::data::SCRIPT_SUFFIX
        )));
    }
    donder_project_io::validate_relative_path(path).map_err(|error| JsValue::from_str(&error))?;
    let document = session.source.project_document(path.into());
    let previous = session
        .source
        .documents
        .get(&document)
        .ok_or_else(|| JsValue::from_str(&format!("{path} was not found.")))?
        .objects()
        .iter()
        .map(|object| object.id().to_owned())
        .collect::<Vec<_>>();
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
    let document_kind = SourceDocumentKind::Script {
        source: text.into(),
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
        let outcome = install_source(&mut candidate, &self.sequence_id, path, kind, text)?;
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
                let SourceDocumentKind::Script { source } = document.kind() else {
                    return None;
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
