//! Documents the project never reaches: a warning on each, with the root
//! import that brings it in.
use crate::address::DocumentId;
use camino::Utf8PathBuf;
use donder_language::compiler::TextSpan;
use donder_language::data::{DataImport, Spanned};
use donder_model::ObjectKind;
use donder_runtime_types::Identifier;
use indexmap::IndexSet;

use crate::imports::canonical_reference_alias;
use crate::{
    IoDiagnostic, IoDiagnosticCode, IoDiagnosticSeverity, IoFix, PROJECT_ROOT_FILE,
    ProjectRecovery, ProjectSession, RecoveryObject, SourceDocumentFormat,
};

/// Bringing an unreferenced document into the project from the root document.
#[derive(Clone, Debug, Eq, PartialEq, Hash, specta::Type)]
pub struct Inclusion {
    document: Utf8PathBuf,
    /// The root import that takes the document: an existing alias it extends
    /// without a name clash, or a new one.
    #[specta(type = String)]
    alias: Identifier,
}

/// An inclusion as text: names read back must be valid identifiers.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InclusionText {
    document: Utf8PathBuf,
    alias: String,
}

impl serde::Serialize for Inclusion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        InclusionText {
            document: self.document.clone(),
            alias: self.alias.as_str().to_string(),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Inclusion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = InclusionText::deserialize(deserializer)?;
        Ok(Self {
            document: text.document,
            alias: Identifier::new(text.alias)
                .map_err(|error| serde::de::Error::custom(format!("{error:?}")))?,
        })
    }
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
    Ok(donder_language::data::print(&document))
}

/// The root alias for a document's declarations, by what they are.
fn alias_base(objects: &[RecoveryObject]) -> Option<&'static str> {
    let kinds = objects
        .iter()
        .map(|object| &object.kind)
        .collect::<IndexSet<_>>();
    if kinds.contains(&ObjectKind::Project) {
        return None;
    }
    kinds.first().map(|kind| canonical_reference_alias(**kind))
}

/// The root import that can take `objects`: `base`, if its documents declare
/// none of them, or the first free `base_2`, `base_3`, ...
fn inclusion_alias(
    session: &ProjectSession,
    root: &DocumentId,
    base: &str,
    objects: &[RecoveryObject],
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
            .all(|declared| objects.iter().all(|object| object.name != declared.name))
    };
    std::iter::once(base.to_string())
        .chain((2_u32..).map(|suffix| format!("{base}_{suffix}")))
        .find(|alias| fits(alias))
        .and_then(|alias| Identifier::new(alias).ok())
}

/// A warning on every Donder document the loaded project does not reach.
pub(crate) fn unreferenced_documents(
    session: &ProjectSession,
    recovery: &ProjectRecovery,
    diagnostics: &mut Vec<IoDiagnostic>,
) {
    let root = DocumentId::new(Utf8PathBuf::from(PROJECT_ROOT_FILE));
    for (path, document) in &recovery.documents {
        if document.format == SourceDocumentFormat::Other
            || session
                .source
                .documents
                .contains_key(&DocumentId::new(path.clone()))
        {
            continue;
        }
        let objects = &document.objects;
        let fix = alias_base(objects)
            .and_then(|base| inclusion_alias(session, &root, base, objects))
            .map(|alias| {
                IoFix::Include(Inclusion {
                    document: path.clone(),
                    alias,
                })
            });
        diagnostics.push(IoDiagnostic {
            severity: IoDiagnosticSeverity::Warning,
            fix,
            ..IoDiagnostic::error(
                path.clone(),
                document.first_name.clone(),
                IoDiagnosticCode::UnreferencedDocument,
                "Nothing imports this document, so the project does not use it.",
            )
        });
    }
}
