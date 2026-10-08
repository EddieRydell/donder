//! Documents the project never reaches: a warning on each, with the root
//! import that brings it in.
use camino::{Utf8Path, Utf8PathBuf};
use donder_language::compiler::TextSpan;
use donder_language::compiler::{DeclarationKind, partial_declaration_spans};
use donder_language::data::{DataImport, DataValue, Spanned};
use donder_model::DocumentId;
use donder_runtime_types::Identifier;
use indexmap::IndexSet;

use crate::diagnostics::byte_range;
use crate::imports::canonical_reference_alias;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoFix, PROJECT_ROOT_FILE, ProjectSession,
    SourceDocumentFormat, SourceObjectKind, source_document_format,
};

/// Bringing an unreferenced document into the project from the root document.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct Inclusion {
    pub document: Utf8PathBuf,
    /// The root import that takes the document: an existing alias it extends
    /// without a name clash, or a new one.
    pub alias: Identifier,
    /// The sequences the document declares, added to the project's `sequences`.
    pub sequences: Vec<Identifier>,
}

/// The root document's text with `inclusion` applied.
pub fn include_document(root_text: &str, inclusion: &Inclusion) -> Result<String, String> {
    let (mut document, diagnostics) = donder_language::data::parse(root_text);
    if !diagnostics.is_empty() {
        return Err(format!("Fix the errors in {PROJECT_ROOT_FILE} first."));
    }
    let nowhere = TextSpan { start: 0, end: 0 };
    let path = Spanned::new(inclusion.document.to_string(), nowhere);
    match document
        .imports
        .iter_mut()
        .find(|import| import.alias.value == inclusion.alias)
    {
        Some(import) => import.paths.push(path),
        None => document.imports.push(DataImport {
            alias: Spanned::new(inclusion.alias.clone(), nowhere),
            paths: vec![path],
        }),
    }
    if !inclusion.sequences.is_empty() {
        let sequences = document
            .declarations
            .iter_mut()
            .filter(|declaration| declaration.ty.value.as_str() == "Project")
            .flat_map(|declaration| declaration.fields.value.iter_mut())
            .find(|field| field.name.value.as_str() == "sequences")
            .map(|field| &mut field.value.value);
        let Some(DataValue::List(items)) = sequences else {
            return Err(format!("{PROJECT_ROOT_FILE} has no project sequence list."));
        };
        for sequence in &inclusion.sequences {
            items.push(Spanned::new(
                DataValue::Reference(vec![
                    Spanned::new(inclusion.alias.clone(), nowhere),
                    Spanned::new(sequence.clone(), nowhere),
                ]),
                nowhere,
            ));
        }
    }
    Ok(donder_language::data::print(&document))
}

/// What an unreferenced document declares.
struct Declared {
    names: Vec<(SourceObjectKind, Identifier)>,
    /// The first declaration's name, where the warning goes.
    first: Option<TextSpan>,
}

fn declared(path: &Utf8Path, text: &str) -> Declared {
    match source_document_format(path) {
        SourceDocumentFormat::Script => {
            let names = partial_declaration_spans(text)
                .into_iter()
                .map(|declaration| {
                    let kind = match declaration.kind {
                        DeclarationKind::Effect => SourceObjectKind::EffectDefinition,
                        DeclarationKind::Operator => SourceObjectKind::OperatorDefinition,
                    };
                    (kind, declaration.name, declaration.name_span)
                })
                .collect::<Vec<_>>();
            Declared {
                first: names.first().map(|(_, _, span)| *span),
                names: names
                    .into_iter()
                    .map(|(kind, name, _)| (kind, name))
                    .collect(),
            }
        }
        SourceDocumentFormat::Data => {
            let (document, _) = crate::document::read(text);
            Declared {
                first: document.declarations.first().map(|(name, _, _)| name.span),
                names: document
                    .declarations
                    .iter()
                    .map(|(name, _, declaration)| (declaration.kind(), name.value.clone()))
                    .collect(),
            }
        }
        SourceDocumentFormat::Other => Declared {
            names: Vec::new(),
            first: None,
        },
    }
}

/// The root alias for a document's declarations, by what they are.
fn alias_base(names: &[(SourceObjectKind, Identifier)]) -> Option<&'static str> {
    let kinds = names.iter().map(|(kind, _)| kind).collect::<IndexSet<_>>();
    if kinds.contains(&SourceObjectKind::Project) || kinds.contains(&SourceObjectKind::Setup) {
        return None;
    }
    let script = |kind: &&SourceObjectKind| {
        matches!(
            kind,
            SourceObjectKind::EffectDefinition | SourceObjectKind::OperatorDefinition
        )
    };
    if kinds.len() > 1 && kinds.iter().all(script) {
        return Some("scripts");
    }
    canonical_reference_alias(kinds.first()?)
}

/// The root import that can take `names`: `base`, if its documents declare
/// none of them, or the first free `base_2`, `base_3`, ...
fn inclusion_alias(
    session: &ProjectSession,
    root: &DocumentId,
    base: &str,
    names: &[(SourceObjectKind, Identifier)],
) -> Option<Identifier> {
    let document = session.source.documents.get(root)?;
    let fits = |alias: &str| {
        document
            .imports
            .iter()
            .filter(|import| import.alias() == alias)
            .flat_map(|import| &import.targets)
            .filter_map(|target| session.source.documents.get(target))
            .flat_map(|target| &target.objects)
            .all(|object| names.iter().all(|(_, name)| name.as_str() != object.id))
    };
    std::iter::once(base.to_string())
        .chain((2_u32..).map(|suffix| format!("{base}_{suffix}")))
        .find(|alias| fits(alias))
        .and_then(|alias| Identifier::new(alias).ok())
}

/// A warning on every Donder document the loaded project does not reach.
pub(crate) fn unreferenced_documents(
    session: &ProjectSession,
    project_id: uuid::Uuid,
    paths: impl IntoIterator<Item = Utf8PathBuf>,
    text: impl Fn(&Utf8Path) -> Option<String>,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let root = DocumentId::new(project_id, Utf8PathBuf::from(PROJECT_ROOT_FILE));
    for path in paths {
        if source_document_format(&path) == SourceDocumentFormat::Other
            || session
                .source
                .documents
                .contains_key(&DocumentId::new(project_id, path.clone()))
        {
            continue;
        }
        let Some(text) = text(&path) else {
            continue;
        };
        let declared = declared(&path, &text);
        let is_setup = declared
            .names
            .iter()
            .any(|(kind, _)| *kind == SourceObjectKind::Setup);
        let fix = alias_base(&declared.names)
            .and_then(|base| inclusion_alias(session, &root, base, &declared.names))
            .map(|alias| {
                IoFix::Include(Inclusion {
                    document: path.clone(),
                    alias,
                    sequences: declared
                        .names
                        .iter()
                        .filter(|(kind, _)| *kind == SourceObjectKind::Sequence)
                        .map(|(_, name)| name.clone())
                        .collect(),
                })
            });
        diagnostics.push(IoDiagnostic {
            range: declared
                .first
                .map(|span| byte_range(&text, span.start, span.end)),
            severity: IoDiagnosticSeverity::Warning,
            code: IoDiagnosticCode::UnreferencedDocument,
            message: "Nothing imports this document, so the project does not use it.".into(),
            detail: is_setup.then(|| {
                format!(
                    "A project has one setup; name this one in {PROJECT_ROOT_FILE}'s `setup` \
                     to use it."
                )
            }),
            fix,
            related: Vec::new(),
            path,
        });
    }
}
