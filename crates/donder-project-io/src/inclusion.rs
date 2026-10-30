//! Documents the project never reaches: a warning on each, with the root
//! import that brings it in.
use crate::address::DocumentId;
use camino::Utf8PathBuf;
use donder_language::compiler::TextSpan;
use donder_language::data::{DataImport, DataValue, Spanned};
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
    /// The sequences the document declares, added to the project's `sequences`.
    #[specta(type = Vec<String>)]
    sequences: Vec<Identifier>,
    /// The rigs the document declares, added to the project's `rigs`.
    #[specta(type = Vec<String>)]
    rigs: Vec<Identifier>,
}

/// An inclusion as text: names read back must be valid identifiers.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct InclusionText {
    document: Utf8PathBuf,
    alias: String,
    sequences: Vec<String>,
    rigs: Vec<String>,
}

impl serde::Serialize for Inclusion {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let names =
            |names: &[Identifier]| names.iter().map(|name| name.as_str().to_string()).collect();
        InclusionText {
            document: self.document.clone(),
            alias: self.alias.as_str().to_string(),
            sequences: names(&self.sequences),
            rigs: names(&self.rigs),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Inclusion {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = InclusionText::deserialize(deserializer)?;
        let name = |name: String| {
            Identifier::new(name).map_err(|error| serde::de::Error::custom(format!("{error:?}")))
        };
        let names = |names: Vec<String>| names.into_iter().map(name).collect::<Result<_, _>>();
        Ok(Self {
            document: text.document,
            alias: name(text.alias)?,
            sequences: names(text.sequences)?,
            rigs: names(text.rigs)?,
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
    for (field, names) in [
        ("sequences", &inclusion.sequences),
        ("rigs", &inclusion.rigs),
    ] {
        if names.is_empty() {
            continue;
        }
        let list = document
            .declarations
            .iter_mut()
            .filter(|declaration| declaration.ty.value.as_str() == "Project")
            .flat_map(|declaration| declaration.fields.value.iter_mut())
            .find(|candidate| candidate.name.value.as_str() == field)
            .map(|candidate| &mut candidate.value.value);
        let Some(DataValue::List(items)) = list else {
            return Err(format!(
                "{PROJECT_ROOT_FILE} has no project `{field}` list."
            ));
        };
        for name in names {
            items.push(Spanned::new(
                DataValue::Reference(vec![
                    Spanned::new(inclusion.alias.clone(), nowhere),
                    Spanned::new(name.clone(), nowhere),
                ]),
                nowhere,
            ));
        }
    }
    Ok(donder_language::data::print(&document))
}

/// The root alias for a document's declarations, by what they are.
fn alias_base(objects: &[RecoveryObject]) -> Option<&'static str> {
    let kinds = objects
        .iter()
        .map(|object| &object.kind)
        .collect::<IndexSet<_>>();
    if kinds.contains(&ObjectKind::Project) || kinds.contains(&ObjectKind::Setup) {
        return None;
    }
    let script = |kind: &&ObjectKind| {
        matches!(
            kind,
            ObjectKind::EffectDefinition | ObjectKind::OperatorDefinition
        )
    };
    if kinds.len() > 1 && kinds.iter().all(script) {
        return Some("scripts");
    }
    canonical_reference_alias(kinds.first()?)
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
        let is_setup = objects
            .iter()
            .any(|object| object.kind == ObjectKind::Setup);
        let declared_of = |wanted: ObjectKind| {
            objects
                .iter()
                .filter(|object| object.kind == wanted)
                .map(|object| object.name.clone())
                .collect()
        };
        let fix = alias_base(objects)
            .and_then(|base| inclusion_alias(session, &root, base, objects))
            .map(|alias| {
                IoFix::Include(Inclusion {
                    document: path.clone(),
                    alias,
                    sequences: declared_of(ObjectKind::Sequence),
                    rigs: declared_of(ObjectKind::Rig),
                })
            });
        diagnostics.push(IoDiagnostic {
            severity: IoDiagnosticSeverity::Warning,
            detail: is_setup.then(|| {
                format!(
                    "A project has one setup; name this one in {PROJECT_ROOT_FILE}'s `setup` \
                     to use it."
                )
            }),
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
